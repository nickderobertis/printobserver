"""Putting each built artifact where its own consumers install it from.

Every publish here authenticates with an **API token carried in a repository
secret**. Not a keyless trusted publisher: this repository's own manifest,
`gh-secrets.json`, is the authoritative list of what it holds, and a publish
that authenticated by something outside it would be a credential nobody
declared. The names below are the environment those tokens arrive in, and
`just check-repo` refuses a workflow that names a secret that manifest does not
declare.

**A publish that failed partway is finished by running it again**, with nothing
cleaned up and nothing moved by hand. Three things make that safe, and each is
per artifact rather than per registry. Before an artifact is sent, its own
registry is asked — through the same documents the install-path proof reads —
whether it already serves exactly that artifact at the workspace version, and
one it serves is reported as already published rather than sent again: a
wheel by its file name, because a distribution published as one wheel per
platform is served the moment the first lands; a package by its name and
version; a release asset by its name and size, and one present under the name
that differs — another size, or an upload the forge lists as never finished —
is replaced. A refusal is recorded and every remaining artifact, of the same
registry and of the others, is still attempted, so one registry's `404` does
not leave the artifacts after it unpublished. And the run fails at the end
naming every refusal in the registry's own words, so that a `Scope not found`
reads as that.

**Every address a registry is read at or written to comes from `Bases`.** So
with `PRINTOBSERVER_PROOF_REGISTRIES` set the whole publish lands on the
stand-in `standin.py` stands up, and a journey can drive this real publisher
over real artifacts against it — which is the only proof of the forge upload
there is until a release runs it.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from urllib.parse import quote, urlsplit

from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import packages
from release_artifacts.build import ASSEMBLED_HERE, CHECKSUMS, PROGRAM, manifest_of
from release_artifacts.registries import (
    FORGE_ACCEPT,
    OCTET_STREAM,
    UPLOADED,
    Bases,
    RegistryError,
    Release,
    exchange,
    npm_versions,
    pypi_files,
    pypi_name,
    release_of,
)
from release_artifacts.targets import Target, declared, workspace

#: The environment each registry's own credential arrives in, which is also the
#: repository secret it is carried by. One place spells them; `gh-secrets.json`
#: is where a check confirms the repository holds them.
CREDENTIALS = {
    "pypi": "PYPI_TOKEN",
    "npm": "NPM_TOKEN",
    "release": "RELEASE_PLZ_TOKEN",
}

#: How long any one publish is given.
PUBLISH_TIMEOUT_SECONDS = 900


class Outcome(StrEnum):
    """What one line of the answer says happened to one artifact."""

    PUBLISHED = "published"
    ALREADY_PUBLISHED = "already published"
    REFUSED = "refused"


class PublishError(RuntimeError):
    """An artifact could not be published.

    Raised at the END of a run that met a refusal, carrying what the run said
    of every artifact before it, so that a caller can still report what was
    published beside what was not.
    """

    def __init__(self, message: str, said: tuple[str, ...] = ()) -> None:
        """Record the refusal, and everything the run said before it."""
        super().__init__(message)
        self.said = said


@dataclass(frozen=True, slots=True)
class Refusal:
    """One artifact a registry refused, with the registry's own words about it."""

    registry: str
    artifact: str
    reason: str


@dataclass(frozen=True, slots=True)
class Package:
    """One packed package of the JavaScript registry, as its own manifest names it."""

    name: str
    version: str

    @property
    def id(self) -> str:
        """`<name>@<version>`, which is how the registry and a publish name one."""
        return f"{self.name}@{self.version}"

    @classmethod
    def of(cls, tarball: Path) -> Package:
        """What one tarball says it is, read from the `package.json` inside it.

        Raises:
            PublishError: If that manifest names no package, which is a
                tarball nothing here can ask a registry about.
        """
        manifest = manifest_of(tarball)
        name, version = manifest.get("name"), manifest.get("version")
        if not isinstance(name, str) or not name or not isinstance(version, str) or not version:
            msg = f"{tarball} carries a manifest naming no package and version to publish"
            raise PublishError(msg)
        return cls(name, version)


@dataclass(slots=True)
class _Run:
    """What one run of the publisher has said and been refused so far."""

    said: list[str] = field(default_factory=list)
    refused: list[Refusal] = field(default_factory=list)

    def attempt(self, registry: str, artifact: str, publishing: Callable[[], bool]) -> None:
        """Try one artifact, recording the outcome and never stopping on a refusal.

        `publishing` answers whether it published the artifact — `False` where
        the registry already served it — and raises where the registry, or
        the read that asks it, refused.
        """
        try:
            outcome = Outcome.PUBLISHED if publishing() else Outcome.ALREADY_PUBLISHED
        except (PublishError, RegistryError) as refused:
            self.refused.append(Refusal(registry, artifact, str(refused)))
            outcome = Outcome.REFUSED
        self.said.append(f"{registry}\t{artifact}\t{outcome}")


def _credential(environment: dict[str, str], registry: str) -> str:
    """The token one registry is published under.

    Raises:
        PublishError: If the environment carries none, which is a publish that
            would otherwise fail after a merge rather than before one.
    """
    name = CREDENTIALS[registry]
    token = environment.get(name, "").strip()
    if not token:
        msg = (
            f"publishing to {registry} needs {name}, and the environment carries none. "
            f"It is a repository secret `gh-secrets.json` declares."
        )
        raise PublishError(msg)
    return token


def _ran(argv: list[str], *, cwd: Path, env: dict[str, str], describing: str) -> str:
    """Run one publish, or stop saying what it said.

    Raises:
        PublishError: If it failed.
    """
    result = run(argv, cwd=cwd, env=env, timeout=PUBLISH_TIMEOUT_SECONDS)
    if result.returncode != 0:
        msg = f"{describing} failed ({result.returncode}):\n{result.stdout}{result.stderr}"
        raise PublishError(msg)
    return result.stdout + result.stderr


def _built(dist: Path, suffixes: tuple[str, ...]) -> list[Path]:
    """Every built artifact of a kind, in a stable order."""
    return sorted(path for path in dist.iterdir() if path.name.endswith(suffixes))


def publish(repo: Repo, dist: Path, environment: dict[str, str]) -> list[str]:
    """Publish every artifact this tool assembles its registry does not already serve.

    Answers one line per artifact — published, already published, or refused
    — in the order they were attempted.

    Raises:
        PublishError: If a credential is absent, which stops the run before
            anything is sent; or, at the end, if any registry refused any
            artifact, naming each one with the registry's own reason.
    """
    registries = {
        target.registry
        for target in declared(repo.root)
        if target.built_by == ASSEMBLED_HERE and target.registry != "crate"
    }
    # Every credential before any write: a token found missing after the
    # first registry was written is a publish stopped partway, which is the
    # state this whole module exists to make recoverable rather than to cause.
    tokens = {
        registry: _credential(environment, registry)
        for registry in CREDENTIALS
        if registry in registries
    }
    bases = Bases.read(repo, environment)
    version = _version(repo)
    publishing = _Run()
    if "pypi" in registries:
        for wheel in _built(dist, (".whl",)):
            publishing.attempt(
                "pypi",
                wheel.name,
                lambda wheel=wheel: publish_wheel(
                    repo, bases, tokens["pypi"], wheel, version, environment
                ),
            )
    if "npm" in registries:
        npmrc = dist / ".npmrc"
        npmrc.write_text(npmrc_line(bases, tokens["npm"]), encoding="utf-8")
        try:
            for tarball in _built(dist, (".tgz",)):
                package = Package.of(tarball)
                publishing.attempt(
                    "npm",
                    package.id,
                    lambda tarball=tarball, package=package: publish_package(
                        repo, bases, tarball, package, npmrc, environment
                    ),
                )
        finally:
            npmrc.unlink()
    if "release" in registries:
        _publish_release(bases, tokens["release"], dist, version, publishing)
    if publishing.refused:
        raise PublishError(_refusals(publishing.refused), tuple(publishing.said))
    return publishing.said


def _refusals(refused: list[Refusal]) -> str:
    """Every refusal a run met, each with the registry's own words, one after another."""
    lines = [f"{len(refused)} artifact(s) were refused; run the publish again once each is fixed:"]
    for refusal in refused:
        lines.append(f"- {refusal.registry}: {refusal.artifact}")
        lines.extend(f"    {line}" for line in refusal.reason.strip().splitlines())
    return "\n".join(lines)


def publish_wheel(
    repo: Repo, bases: Bases, token: str, wheel: Path, version: str, environment: dict[str, str]
) -> bool:
    """Send one wheel to the Python registry, unless it already serves that file.

    Raises:
        PublishError: If the registry refused it.
        RegistryError: If the registry could not be asked what it serves.
    """
    name = pypi_name(wheel.name.partition("-")[0])
    if wheel.name in pypi_files(bases, name, version):
        return False
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    _ran(
        ["uv", "publish", "--publish-url", bases.pypi_upload, "--token", token, str(wheel)],
        cwd=repo.root,
        env=environment,
        describing=f"publishing {wheel.name} to {bases.pypi_upload}",
    )
    return True


def npmrc_line(bases: Bases, token: str) -> str:
    """The one line `npm publish` reads its credential from, for the registry it is sent to.

    `npm` keys a credential by the registry's host and path, so the line is
    composed from the address `Bases` gives that registry — which for the real
    one is `//registry.npmjs.org/`, byte for byte the line this always wrote.
    """
    where = urlsplit(bases.npm)
    return f"//{where.netloc}{where.path.rstrip('/')}/:_authToken={token}\n"


def publish_package(
    repo: Repo,
    bases: Bases,
    tarball: Path,
    package: Package,
    npmrc: Path,
    environment: dict[str, str],
) -> bool:
    """Send one package to the JavaScript registry, unless it already serves that version.

    Raises:
        PublishError: If the registry refused it.
        RegistryError: If the registry could not be asked what it serves.
    """
    if package.version in npm_versions(bases, package.name):
        return False
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    _ran(
        [
            "npm",
            "publish",
            "--access",
            "public",
            # With its trailing slash, because that is what `npm` keys the
            # credential in `.npmrc` by: the registry's path up to the last
            # `/`, so an address without one would be keyed a path short.
            "--registry",
            f"{bases.npm}/",
            f"--userconfig={npmrc}",
            str(tarball),
        ],
        cwd=repo.root,
        env=environment,
        describing=f"publishing {package.id} to {bases.npm}",
    )
    return True


def _publish_release(bases: Bases, token: str, dist: Path, version: str, publishing: _Run) -> None:
    """Put the per-platform artifacts the install script downloads on the release.

    Every `printobserver-*.tar.gz` in `dist`, and then ONE checksum file listing
    all of them — composed here from what `dist` holds rather than taken from
    the build, because each platform's build writes a checksum file of its own
    tarball alone, and the file the release carries has to name every one.
    """
    tarballs = [path for path in _built(dist, (".tar.gz",)) if path.name.startswith(f"{PROGRAM}-")]
    digests = dist / CHECKSUMS
    digests.write_bytes(packages.checksums(tarballs))
    assets = [*tarballs, digests]
    try:
        release = release_of(bases, version)
    except RegistryError as unread:
        for path in assets:
            publishing.attempt("release", path.name, lambda unread=unread: _raise(unread))
        return
    for path in assets:
        publishing.attempt(
            "release",
            path.name,
            lambda path=path: publish_asset(bases, token, release, path),
        )


def _raise(refused: RegistryError) -> bool:
    """Refuse, with what the forge said when asked for the release."""
    raise refused


def publish_asset(bases: Bases, token: str, release: Release, path: Path) -> bool:
    """Put one file on the release, unless the forge already lists exactly it.

    Exactly it: an asset of the same name and size, in the state a finished
    upload leaves. One present under the name that differs is what an
    interrupted upload or an earlier run's one-platform checksum file leaves,
    and it is replaced — deleted first, because the forge refuses a second
    upload under a taken name, then uploaded to the release's own upload
    address as the forge's API takes it.

    Raises:
        RegistryError: If the forge refused either request, in its own words.
    """
    present = release.named(path.name)
    if present is not None:
        if present.size == path.stat().st_size and present.state == UPLOADED:
            return False
        exchange(
            f"{bases.listing}/assets/{present.id}",
            method="DELETE",
            accept=FORGE_ACCEPT,
            token=token,
        )
    exchange(
        f"{release.upload_url}?name={quote(path.name, safe='')}",
        method="POST",
        body=path.read_bytes(),
        content_type=OCTET_STREAM,
        accept=FORGE_ACCEPT,
        token=token,
    )
    return True


def _version(repo: Repo) -> str:
    """The version release automation wrote into the workspace."""
    return workspace(repo.root)["version"]


def registries_of(repo: Repo) -> list[Target]:
    """Every target this tool publishes, in the order the declaration names them."""
    return [target for target in declared(repo.root) if target.built_by == ASSEMBLED_HERE]
