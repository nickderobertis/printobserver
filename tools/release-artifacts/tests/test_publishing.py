"""Putting each built artifact where its own consumers install it from.

The registries are stood in for by `standin.py`, answering every registry's
own protocol on one address, and the publisher under test is driven against
them with the real `uv`, the real `npm` and its own forge upload — over the
real artifacts `build-all` assembles from the committed tree. Nothing here
writes to a real registry or the real forge: `PRINTOBSERVER_PROOF_REGISTRIES`
points every read and every write at the stand-in, and the tokens are made up.

What these prove is the recovery a partial publish has to have: that a run
skips what its registry already serves, attempts everything else whatever an
earlier artifact answered, and fails at the end naming each refusal in the
registry's own words — so that the repair for a publish that stopped partway is
running it again.
"""

from __future__ import annotations

import base64
import json
import os
import shutil
import socket
from collections.abc import Iterator
from pathlib import Path
from urllib.parse import urlsplit

import pytest
from release_artifacts.__main__ import main
from release_artifacts.build import CHECKSUMS, PROGRAM, manifest_of
from release_artifacts.packages import digest_of
from release_artifacts.platforms import host, supported
from release_artifacts.publishing import (
    CREDENTIALS,
    Outcome,
    Package,
    PublishError,
    npmrc_line,
    publish,
    publish_package,
    publish_wheel,
    registries_of,
)
from release_artifacts.registries import (
    PRINTOBSERVER_PROOF_REGISTRIES,
    Bases,
    RegistryError,
    Token,
    exchange,
    npm_versions,
    pypi_files,
    pypi_name,
    release_of,
)
from release_artifacts.standin import (
    FORGE_PREFIX,
    PYPI_PREFIX,
    Authorization,
    Registries,
    StandinError,
    Write,
)
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.model import Repo

#: The tokens this journey publishes under, one per registry, under the real
#: credential names — so that what reaches each registry can be read back.
TOKENS = {name: Token(f"a-{name.lower()}-this-journey-made-up") for name in CREDENTIALS.values()}

#: What the release of 2026-09-11 met: the JavaScript registry refusing a
#: per-platform package because its organization did not exist yet.
SCOPE_NOT_FOUND = b'{"error": "Scope not found"}'


@pytest.fixture
def registries(repo: Repo, tmp_path: Path) -> Iterator[Registries]:
    """The three registries, answering on one address, serving nothing yet."""
    standing_in = Registries(repo, tmp_path / "served")
    try:
        yield standing_in
    finally:
        standing_in.stop()


@pytest.fixture
def environment(registries: Registries) -> dict[str, str]:
    """The environment a publish runs under: the stand-in named, and every token set."""
    return {**os.environ, PRINTOBSERVER_PROOF_REGISTRIES: registries.base, **TOKENS}


@pytest.fixture
def bases(repo: Repo, environment: dict[str, str]) -> Bases:
    """Where the publisher reads and writes each registry, which is the stand-in."""
    return Bases.read(repo, environment)


def _outcomes(said: list[str] | tuple[str, ...]) -> dict[str, str]:
    """What a publish said of each artifact, by `<registry> <artifact>`."""
    outcomes: dict[str, str] = {}
    for line in said:
        registry, artifact, outcome = line.split("\t")
        outcomes[f"{registry} {artifact}"] = outcome
    return outcomes


def _artifacts(repo: Repo, dist: Path, version: str) -> dict[str, Path]:
    """Every artifact a publish of `dist` sends, keyed as its answer names it."""
    named: dict[str, Path] = {}
    for path in sorted(dist.iterdir()):
        if path.name.endswith(".whl"):
            named[f"pypi {path.name}"] = path
        elif path.name.endswith(".tgz"):
            manifest = manifest_of(path)
            named[f"npm {manifest['name']}@{manifest['version']}"] = path
        elif path.name.startswith(f"{PROGRAM}-") and path.name.endswith(".tar.gz"):
            named[f"release {path.name}"] = path
    named[f"release {CHECKSUMS}"] = dist / CHECKSUMS
    return named


def _served(bases: Bases, artifact: str, path: Path, version: str) -> bool:
    """Whether the registry serves one artifact, asked through the publisher's own reads."""
    registry, _, name = artifact.partition(" ")
    match registry:
        case "pypi":
            return path.name in pypi_files(bases, pypi_name(name.partition("-")[0]), version)
        case "npm":
            package, _, published = name.rpartition("@")
            return published in npm_versions(bases, package)
        case _:
            try:
                listed = release_of(bases, version).named(name)
            except RegistryError:
                return False
            return listed is not None and listed.size == path.stat().st_size


def _platform_package(repo: Repo, version: str) -> str:
    """The per-platform package this host's build produced, as a publish names it."""
    return f"{host(repo).npm_package}@{version}"


def test_every_artifact_reaches_the_registry_it_is_declared_for(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """Two wheels to one registry, four packages to another, the assets to the forge.

    Each is then served by the document the publisher itself reads, and each
    write reached its registry carrying the token from the credential that
    registry's secret is named by, in the form that registry takes it.
    """
    registries.release(f"v{version}")

    said = publish(repo, dist, environment)

    artifacts = _artifacts(repo, dist, version)
    equal(_outcomes(said), dict.fromkeys(artifacts, Outcome.PUBLISHED), describing="what was said")
    for artifact, path in artifacts.items():
        truth(_served(bases, artifact, path, version), describing=f"{artifact} to be served")
    basic = base64.b64encode(f"__token__:{TOKENS['PYPI_TOKEN']}".encode()).decode()
    carried = {(write.registry, write.credential) for write in registries.written}
    equal(
        carried,
        {
            ("pypi", f"Basic {basic}"),
            ("npm", f"Bearer {TOKENS['NPM_TOKEN']}"),
            ("release", f"Bearer {TOKENS['RELEASE_PLZ_TOKEN']}"),
        },
        describing="the credential each registry was sent",
    )
    truth(
        not any(write.refused for write in registries.written),
        describing="every write taken",
    )
    absent([path.name for path in dist.iterdir()], ".npmrc", describing="the credential file")


def test_a_publish_that_failed_partway_is_finished_by_running_it_again(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The regression: a registry refusing one artifact must not cost the rest.

    A first publish against a stand-in refusing three artifacts leaves it
    holding the others. With two of the three refusals lifted, the run under
    test then reports what is held as already published, publishes the two,
    uploads the release assets, and fails naming the one still refused with
    the registry's own words. With that lifted too, a third run publishes
    exactly that one and nothing else.
    """
    registries.release(f"v{version}")
    platform_package = _platform_package(repo, version)
    for registry, name in (
        ("npm", platform_package.rpartition("@")[0]),
        ("npm", "@printobserver/sdk"),
        ("release", f"{PROGRAM}-{host(repo).id}.tar.gz"),
    ):
        registries.refuse(registry, name, status=404, body=SCOPE_NOT_FOUND)
    with pytest.raises(PublishError):
        publish(repo, dist, environment)
    registries.accept("npm", "@printobserver/sdk")
    registries.accept("release", f"{PROGRAM}-{host(repo).id}.tar.gz")
    del registries.written[:]

    for name, value in environment.items():
        monkeypatch.setenv(name, value)
    exit_status = main(["publish", "--root", str(repo.root), "--into", str(dist)])

    equal(exit_status, 1, describing="the exit of a publish that was refused one artifact")
    out, err = capsys.readouterr()
    artifacts = _artifacts(repo, dist, version)
    expected = dict.fromkeys(artifacts, Outcome.ALREADY_PUBLISHED)
    expected[f"npm @printobserver/sdk@{version}"] = Outcome.PUBLISHED
    expected[f"release {PROGRAM}-{host(repo).id}.tar.gz"] = Outcome.PUBLISHED
    expected[f"npm {platform_package}"] = Outcome.REFUSED
    equal(_outcomes(out.splitlines()), expected, describing="what the run said")
    contains(err, platform_package.rpartition("@")[0], describing="the refused package named")
    contains(err, "Scope not found", describing="the registry's own reason")
    contains(err, "404", describing="the registry's own status")
    for artifact, path in artifacts.items():
        equal(
            _served(bases, artifact, path, version),
            artifact != f"npm {platform_package}",
            describing=f"whether {artifact} is served after the run",
        )
    absent([path.name for path in dist.iterdir()], ".npmrc", describing="the credential file")

    registries.accept("npm", platform_package.rpartition("@")[0])
    del registries.written[:]
    said = publish(repo, dist, environment)

    expected = dict.fromkeys(artifacts, Outcome.ALREADY_PUBLISHED)
    expected[f"npm {platform_package}"] = Outcome.PUBLISHED
    equal(_outcomes(said), expected, describing="what the second run said")
    equal(
        registries.written,
        [
            Write(
                "npm",
                "PUT",
                platform_package.rpartition("@")[0],
                Authorization(f"Bearer {TOKENS['NPM_TOKEN']}"),
                refused=False,
            )
        ],
        describing="the writes the second run made",
    )
    truth(
        _served(bases, f"npm {platform_package}", artifacts[f"npm {platform_package}"], version),
        describing="the package the second run published",
    )


def test_a_fail_fast_publish_leaves_the_artifacts_after_the_refusal_unserved(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """The shape `main` had: one loop that raises on the first refusal.

    Against the same stand-in and the same refusal, it stops at the refused
    package and everything sorted after it — the client, every release asset —
    is never sent. That is the difference the publisher above makes.
    """
    registries.release(f"v{version}")
    platform_package = _platform_package(repo, version)
    registries.refuse("npm", platform_package.rpartition("@")[0], status=404, body=SCOPE_NOT_FOUND)
    npmrc = dist / ".npmrc"
    npmrc.write_text(npmrc_line(bases, TOKENS["NPM_TOKEN"]), encoding="utf-8")

    with pytest.raises(PublishError, match="Scope not found"):
        for wheel in sorted(dist.glob("*.whl")):
            publish_wheel(repo, bases, TOKENS["PYPI_TOKEN"], wheel, version, environment)
        for tarball in sorted(dist.glob("*.tgz")):
            publish_package(repo, bases, tarball, Package.of(tarball), npmrc, environment)

    artifacts = _artifacts(repo, dist, version)
    unserved = {
        artifact
        for artifact, path in artifacts.items()
        if not _served(bases, artifact, path, version)
    }
    equal(
        unserved,
        {
            f"npm {platform_package}",
            f"npm @printobserver/sdk@{version}",
            f"release {PROGRAM}-{host(repo).id}.tar.gz",
            f"release {CHECKSUMS}",
        },
        describing="what a fail-fast publish left unserved",
    )


def test_the_checksum_file_lists_every_platforms_tarball(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """One checksum file naming both tarballs, replacing the one-platform file.

    Each platform's build writes a checksum file of its own tarball alone, and
    the publish job downloads both builds into one `dist`. The file the
    release carries is composed from what `dist` holds — and one an earlier
    run uploaded listing one platform is replaced, because it differs in size.
    """
    registries.release(f"v{version}")
    publish(repo, dist, environment)
    served = registries.assets_of(f"v{version}")[CHECKSUMS].decode()
    equal(served.count("\n"), 1, describing="the lines of a one-platform checksum file")

    tarballs = {
        platform.id: dist / f"{PROGRAM}-{platform.id}.tar.gz" for platform in supported(repo)
    }
    other = [path for platform, path in tarballs.items() if platform != host(repo).id]
    for path in other:
        shutil.copy2(dist / f"{PROGRAM}-{host(repo).id}.tar.gz", path)
    said = publish(repo, dist, environment)

    outcomes = _outcomes(said)
    equal(
        outcomes[f"release {CHECKSUMS}"], Outcome.PUBLISHED, describing="the checksum file replaced"
    )
    for path in other:
        equal(
            outcomes[f"release {path.name}"], Outcome.PUBLISHED, describing="the other platform's"
        )
    served = registries.assets_of(f"v{version}")[CHECKSUMS].decode()
    for path in tarballs.values():
        contains(served, f"{digest_of(path)}  {path.name}\n", describing="the served checksums")
    equal(served.count("\n"), len(tarballs), describing="one line per tarball")
    equal(
        exchange(f"{bases.releases}/download/v{version}/{CHECKSUMS}").decode(),
        served,
        describing="what the install script downloads",
    )


def test_an_asset_whose_upload_never_finished_is_replaced(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """An asset the forge lists under the name in another state is not one served."""
    registries.release(f"v{version}")
    publish(repo, dist, environment)
    asset = f"{PROGRAM}-{host(repo).id}.tar.gz"
    download = f"{bases.releases}/download/v{version}/{asset}"
    registries.interrupted(f"v{version}", asset)
    with pytest.raises(RegistryError, match="404"):
        exchange(download)
    del registries.written[:]

    said = publish(repo, dist, environment)

    equal(
        _outcomes(said)[f"release {asset}"], Outcome.PUBLISHED, describing="the interrupted asset"
    )
    equal(
        [(write.method, write.name) for write in registries.written],
        [("DELETE", asset), ("POST", asset)],
        describing="the writes: the interrupted one removed, then the upload",
    )
    listed = release_of(bases, version).named(asset)
    truth(listed is not None and listed.state == "uploaded", describing="what the forge lists")
    equal(exchange(download), (dist / asset).read_bytes(), describing="what a download reads now")


def test_a_forge_refusing_to_remove_an_interrupted_asset_uploads_nothing_over_it(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """The deletion is a request of its own, and one the forge refused is the asset's refusal.

    Nothing is uploaded after it: the forge refuses a second upload under a
    taken name, so the interrupted one stays listed as it was, and the run
    fails naming the forge's own words about the deletion.
    """
    registries.release(f"v{version}")
    publish(repo, dist, environment)
    asset = f"{PROGRAM}-{host(repo).id}.tar.gz"
    registries.interrupted(f"v{version}", asset)
    registries.refuse("release", asset, status=403, body=b'{"message": "Resource not accessible"}')
    del registries.written[:]

    with pytest.raises(PublishError) as refused:
        publish(repo, dist, environment)

    contains(str(refused.value), "Resource not accessible", describing="the forge's own words")
    equal(_outcomes(refused.value.said)[f"release {asset}"], Outcome.REFUSED, describing="its line")
    equal(
        [(write.method, write.name) for write in registries.written],
        [("DELETE", asset)],
        describing="the one write: the deletion, and no upload after its refusal",
    )
    listed = release_of(bases, version).named(asset)
    truth(listed is not None and listed.state != "uploaded", describing="what the forge lists")


def test_an_upload_refused_after_the_deletion_landed_is_finished_by_running_it_again(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """A forge that took the deletion and refused the upload leaves the name free.

    The run reports the refusal in the forge's words; what the second run
    meets is a release with nothing under that name, so it uploads with nothing
    to delete first and the release ends up carrying exactly the file.
    """
    registries.release(f"v{version}")
    publish(repo, dist, environment)
    asset = f"{PROGRAM}-{host(repo).id}.tar.gz"
    registries.interrupted(f"v{version}", asset)
    registries.refuse(
        "release", asset, status=502, body=b'{"message": "Bad Gateway"}', method="POST"
    )
    del registries.written[:]

    with pytest.raises(PublishError) as refused:
        publish(repo, dist, environment)

    contains(str(refused.value), "Bad Gateway", describing="the forge's own words")
    equal(
        [(write.method, write.name, write.refused) for write in registries.written],
        [("DELETE", asset, False), ("POST", asset, True)],
        describing="the writes: the deletion taken, the upload refused",
    )
    truth(release_of(bases, version).named(asset) is None, describing="the name left free")

    registries.accept("release", asset)
    del registries.written[:]
    said = publish(repo, dist, environment)

    equal(_outcomes(said)[f"release {asset}"], Outcome.PUBLISHED, describing="the second run")
    equal(
        [(write.method, write.name) for write in registries.written],
        [("POST", asset)],
        describing="the second run's writes: the upload, with nothing to delete first",
    )
    listed = release_of(bases, version).named(asset)
    truth(listed is not None and listed.state == "uploaded", describing="what the forge lists")
    equal(
        registries.assets_of(f"v{version}")[asset],
        (dist / asset).read_bytes(),
        describing="what the release carries",
    )


def test_a_forge_listing_no_release_refuses_the_assets_and_the_rest_still_lands(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """No release to upload to is every asset refused, and no wheel or package lost."""
    with pytest.raises(PublishError) as refused:
        publish(repo, dist, environment)

    contains(str(refused.value), f"lists no release v{version}", describing="what it said")
    for artifact, path in _artifacts(repo, dist, version).items():
        equal(
            _served(bases, artifact, path, version),
            not artifact.startswith("release "),
            describing=f"whether {artifact} is served",
        )


@pytest.mark.parametrize("registry", sorted(CREDENTIALS))
def test_a_publish_with_no_credential_names_the_secret_it_needs(
    registry: str, repo: Repo, dist: Path, registries: Registries, environment: dict[str, str]
) -> None:
    """A publish that failed after a merge for want of a token is a release lost.

    And it is refused before anything is sent to any registry: a token found
    missing after the first registry was written would be exactly the partial
    publish this tool exists to recover from.
    """
    environment[CREDENTIALS[registry]] = "   "

    with pytest.raises(PublishError, match=CREDENTIALS[registry]):
        publish(repo, dist, environment)

    equal(registries.written, [], describing="what reached a registry")


def test_a_registry_that_refuses_what_it_was_sent_is_reported(
    repo: Repo,
    dist: Path,
    version: str,
    registries: Registries,
    environment: dict[str, str],
    bases: Bases,
) -> None:
    """A publish that failed silently would be a release nobody knew was lost.

    The refusal is reported in the registry's own words, at the end, with
    every other artifact still published.
    """
    registries.release(f"v{version}")
    wheel = next(path.name for path in dist.iterdir() if path.name.startswith("printobserver_sdk"))
    registries.refuse(
        "pypi",
        wheel,
        status=403,
        body=b"Invalid or non-existent authentication information.",
        content_type="text/plain",
    )

    with pytest.raises(PublishError) as refused:
        publish(repo, dist, environment)

    contains(str(refused.value), wheel, describing="the refused wheel named")
    contains(str(refused.value), "403", describing="the registry's own status")
    contains(
        str(refused.value), "non-existent authentication", describing="the registry's own words"
    )
    outcomes = _outcomes(refused.value.said)
    equal(outcomes.pop(f"pypi {wheel}"), Outcome.REFUSED, describing="the refused wheel")
    equal(set(outcomes.values()), {Outcome.PUBLISHED}, describing="every other artifact")
    absent([path.name for path in dist.iterdir()], ".npmrc", describing="the credential file")


def test_the_tool_publishes_exactly_what_the_declaration_names(repo: Repo) -> None:
    """Nothing here restates a name, a registry or a route."""
    shipped = {target.id for target in registries_of(repo)}

    equal(len(shipped), 6, describing="the artifacts this tool publishes")
    for registry in CREDENTIALS:
        truth(
            any(target.startswith(f"{registry}:") for target in shipped),
            describing=f"an artifact reaching {registry}",
        )


def test_the_credential_line_for_the_real_registry_is_the_one_always_written(
    repo: Repo,
) -> None:
    """Composed from `Bases`, and byte-identical for the real registry to what it was."""
    equal(
        npmrc_line(Bases.read(repo, {}), Token("a-token")),
        "//registry.npmjs.org/:_authToken=a-token\n",
        describing="the line `npm publish` reads against the real registry",
    )


def test_every_registry_is_written_where_it_is_read(repo: Repo) -> None:
    """One stand-in address covers the writes as it covers the reads."""
    real = Bases.read(repo, {})
    equal(real.pypi_upload, "https://upload.pypi.org/legacy/", describing="the real upload")
    declared = repo.policy["repository"]
    equal(
        real.uploads,
        f"https://uploads.github.com/repos/{declared['owner']}/{declared['name']}/releases",
        describing="the real forge's own upload host",
    )
    standing_in = Bases.read(repo, {PRINTOBSERVER_PROOF_REGISTRIES: "http://127.0.0.1:9"})
    equal(standing_in.pypi_upload, "http://127.0.0.1:9/pypi/legacy/", describing="the stand-in's")
    equal(standing_in.uploads, "http://127.0.0.1:9/forge/releases", describing="its uploads")


def test_a_registry_listing_a_version_as_something_other_than_files_is_refused(
    repo: Repo, version: str, registries: Registries, bases: Bases
) -> None:
    """A malformed document is not a registry serving nothing: read past, it would re-send."""
    registries.answers(
        f"{PYPI_PREFIX}/pypi/printobserver-cli/json",
        f'{{"releases": {{"{version}": "one file"}}}}'.encode(),
    )

    with pytest.raises(RegistryError) as refused:
        pypi_files(bases, "printobserver-cli", version)

    contains(str(refused.value), "other than the list of files", describing="what it said")


def test_a_registry_answering_no_metadata_document_lists_no_files(
    repo: Repo, version: str, registries: Registries, bases: Bases
) -> None:
    """A name the registry does not serve lists no files, and a malformed body is a stop."""
    equal(pypi_files(bases, "printobserver-cli", version), (), describing="an unserved name")
    registries.answers(f"{PYPI_PREFIX}/pypi/printobserver-cli/json", b'{"releases": []}')

    with pytest.raises(RegistryError, match="other than the metadata document"):
        pypi_files(bases, "printobserver-cli", version)


@pytest.mark.parametrize(
    "document",
    [
        b'{"message": "not a release"}',
        b'{"id": 1, "upload_url": "x", "assets": [{"name": "no id or size"}]}',
        # A boolean is an integer in Python, and a document answering one for
        # an id would otherwise read as release number one.
        b'{"id": true, "upload_url": "x", "assets": []}',
        b'{"id": 1, "upload_url": "x",'
        b' "assets": [{"id": 2, "name": "x", "size": -1, "state": "uploaded"}]}',
        b'{"id": 1, "upload_url": "x",'
        b' "assets": [{"id": false, "name": "x", "size": 1, "state": "uploaded"}]}',
    ],
)
def test_a_forge_answering_no_release_document_is_refused(
    document: bytes, repo: Repo, version: str, registries: Registries, bases: Bases
) -> None:
    """A release document the forge's protocol does not describe is a stop naming it."""
    registries.answers(f"{FORGE_PREFIX}/tags/v{version}", document)

    with pytest.raises(RegistryError, match="other than the release document"):
        release_of(bases, version)


def test_a_release_naming_an_upload_address_off_the_forge_is_refused(
    repo: Repo, version: str, registries: Registries, bases: Bases
) -> None:
    """An upload address is sent the release credential, so it is the forge's or nothing.

    Where a release document names somewhere else, the read refuses it naming
    both addresses and nothing is sent; where it names the address `Bases`
    says that forge takes uploads at — which for the real one is a host of its
    own beside the one it is read on — it is taken.
    """
    served = f"{FORGE_PREFIX}/tags/v{version}"
    elsewhere = "https://uploads.example.invalid/repos/x/y/releases/7/assets{?name,label}"
    registries.answers(
        served, json.dumps({"id": 7, "upload_url": elsewhere, "assets": []}).encode()
    )

    with pytest.raises(RegistryError) as refused:
        release_of(bases, version)

    contains(str(refused.value), "uploads.example.invalid", describing="the address refused")
    contains(str(refused.value), bases.uploads, describing="where that forge takes uploads")

    # Under the forge's own uploads base and still not this release's address:
    # one a check of the prefix would pass, and one the forge would resolve
    # to some other repository's releases.
    its_own = f"{bases.uploads}/7/assets"
    for beside in (f"{bases.uploads}/8/assets", f"{bases.uploads}/../elsewhere/releases/7/assets"):
        registries.answers(
            served,
            json.dumps({"id": 7, "upload_url": beside + "{?name,label}", "assets": []}).encode(),
        )
        with pytest.raises(RegistryError) as refused:
            release_of(bases, version)
        contains(str(refused.value), beside, describing="the address refused")
        contains(str(refused.value), its_own, describing="the release's own address")

    registries.answers(
        served,
        json.dumps({"id": 7, "upload_url": its_own + "{?name,label}", "assets": []}).encode(),
    )

    equal(
        release_of(bases, version).upload_url,
        its_own,
        describing="the upload address the forge's own document names",
    )


def test_a_forge_refusing_an_upload_is_reported_in_its_own_words(
    repo: Repo, version: str, registries: Registries, bases: Bases
) -> None:
    """The forge's status and body reach the caller as the forge wrote them."""
    registries.release(f"v{version}")
    registries.refuse("release", "x.txt", status=422, body=b'{"message": "Validation Failed"}')
    release = release_of(bases, version)

    with pytest.raises(RegistryError) as refused:
        exchange(
            f"{release.upload_url}?name=x.txt",
            method="POST",
            body=b"x",
            content_type="text/plain",
            token=TOKENS["RELEASE_PLZ_TOKEN"],
        )

    contains(str(refused.value), "422", describing="the forge's own status")
    contains(str(refused.value), "Validation Failed", describing="the forge's own words")


@pytest.mark.parametrize(
    ("method", "path", "body", "content_type", "naming"),
    [
        (
            "POST",
            "/pypi/legacy/",
            b"--x\r\n\r\nno file\r\n--x--\r\n",
            "multipart/form-data; boundary=x",
            "no file",
        ),
        (
            "POST",
            "/pypi/legacy/",
            b'--x\r\nContent-Disposition: form-data; name="name"\r\n\r\n\xff\xfe\r\n--x--\r\n',
            "multipart/form-data; boundary=x",
            "not text",
        ),
        ("PUT", "/npm/nothing", b"not json", "application/json", "not a publish document"),
        (
            "PUT",
            "/npm/nothing",
            b'{"versions": {"1.0.0": {"name": "nothing", "version": "1.0.0"}},'
            b' "_attachments": {"t.tgz": {"data": "not base64!"}}}',
            "application/json",
            "not an attachment",
        ),
        (
            "PUT",
            "/npm/nothing",
            b'{"versions": {"1.0.0": {"name": "other", "version": "1.0.0"}},'
            b' "_attachments": {"t.tgz": {"data": "eA=="}}}',
            "application/json",
            "not the package named",
        ),
        (
            "PUT",
            "/npm/nothing",
            b'{"versions": {"1.0.0": {"name": "nothing", "version": "2.0.0"}},'
            b' "_attachments": {"t.tgz": {"data": "eA=="}}}',
            "application/json",
            "not the package named",
        ),
        ("PUT", "/npm/nothing", b'{"versions": {}}', "application/json", "not a publish document"),
        (
            "PUT",
            "/npm/nothing",
            b'{"versions": {"1.0.0": 1}, "_attachments": {}}',
            "application/json",
            "not a manifest",
        ),
        (
            "POST",
            "/forge/releases/999/assets?name=x",
            b"x",
            "application/octet-stream",
            "Not Found",
        ),
        ("POST", "/forge/releases/1/assets", b"x", "application/octet-stream", "Not Found"),
        ("DELETE", "/forge/releases/assets/999", None, "", "Not Found"),
        ("PUT", "/pypi/legacy/", b"x", "text/plain", "not served here"),
    ],
)
def test_the_stand_in_answers_a_write_it_cannot_take_as_the_registry_would(
    method: str,
    path: str,
    body: bytes | None,
    content_type: str,
    naming: str,
    version: str,
    registries: Registries,
) -> None:
    """A write no registry protocol describes is refused with a status, not taken."""
    registries.release(f"v{version}")

    with pytest.raises(RegistryError) as refused:
        exchange(f"{registries.base}{path}", method=method, body=body, content_type=content_type)

    contains(str(refused.value), naming, describing="what the stand-in said")


def test_the_stand_in_refuses_a_write_declaring_a_length_it_cannot_read(
    registries: Registries,
) -> None:
    """A body is read by what the request says it is, so what that says is checked.

    The bytes below are what a client sending a malformed `Content-Length`
    puts on the wire, and the stand-in answers them the way a server does —
    with a status — rather than failing inside its own handler.
    """
    where = urlsplit(registries.base)
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    with socket.create_connection((where.hostname, where.port or 80), timeout=30) as connection:
        connection.sendall(
            b"POST /pypi/legacy/ HTTP/1.1\r\n"
            b"Host: standin\r\n"
            b"Content-Type: application/octet-stream\r\n"
            b"Content-Length: as-many-as-it-likes\r\n"
            b"Connection: close\r\n\r\n"
        )
        whole = b""
        while chunk := connection.recv(4096):
            whole += chunk
    answered = whole.decode("utf-8", "replace")

    contains(answered, "400", describing="the status a malformed length is answered with")
    contains(answered, "no length this can read", describing="what the stand-in said")


def test_an_asset_uploaded_under_a_taken_name_is_refused_as_the_forge_refuses_it(
    version: str, registries: Registries, bases: Bases
) -> None:
    """The forge answers `422 already_exists`, which is why an upload deletes first."""
    registries.release(f"v{version}")
    release = release_of(bases, version)
    exchange(
        f"{release.upload_url}?name=x.txt", method="POST", body=b"x", content_type="text/plain"
    )

    with pytest.raises(RegistryError, match="already_exists"):
        exchange(
            f"{release.upload_url}?name=x.txt", method="POST", body=b"y", content_type="text/plain"
        )

    equal(registries.assets_of(f"v{version}")["x.txt"], b"x", describing="the asset kept")


def test_the_stand_in_refuses_what_it_cannot_be_told(version: str, registries: Registries) -> None:
    """A refusal on no registry, or an interruption of no asset, is a caller's mistake."""
    with pytest.raises(StandinError, match="not a registry here"):
        registries.refuse("crate", "x", status=404, body=b"")
    with pytest.raises(StandinError, match="carries no asset"):
        registries.interrupted(f"v{version}", "x")
