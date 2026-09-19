"""Every artifact this repository ships is assembled the way its registry reads it.

Real wheels and real packages, written by the real tool and opened again as a
registry and an installer open them. What stands in for the `printobserver`
program is a small one of this suite's own: what these journeys are about is
the artifact around it.
"""

from __future__ import annotations

import json
import os
import platform as host_platform
import struct
import tarfile
import zipfile
from collections.abc import Callable
from pathlib import Path
from typing import cast

import pytest
from release_artifacts import targets, wheels
from release_artifacts.build import (
    CONTRACT_FIELD,
    CONTRACT_FILE,
    BuildError,
    assembled,
    build,
    contract_version,
    manifest_of,
    staged_release,
)
from release_artifacts.installing import InstallError, install
from release_artifacts.packages import checksums, digest_of
from release_artifacts.publishing import PublishError, publish
from release_artifacts.targets import TargetError
from repo_checks import platforms
from repo_checks.expect import contains, equal, failing, truth
from repo_checks.model import Repo
from route_proof import ROUTE_PROOF


def test_the_declaration_names_the_six_this_tool_assembles(repo: Repo) -> None:
    """Three clients a dependent takes, and three routes an end user takes."""
    shipped = {target.id for target in assembled(repo)}

    equal(len(shipped), 6, describing="the artifacts this tool assembles")
    for identifier in ("pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver"):
        contains(shipped, identifier, describing="the artifacts this tool assembles")


def test_a_target_the_declaration_does_not_name_is_refused(repo: Repo) -> None:
    """A name nothing declares is a stop rather than a guess."""
    with pytest.raises(TargetError, match="is not a target"):
        targets.named(repo.root, "pypi:no-such-distribution")


def test_a_target_release_automation_publishes_is_not_built_here(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A crate is published straight from the workspace, by one tool."""
    with pytest.raises(BuildError, match="release-plz"):
        build(repo, "crate:printobserver-types", into("not-built-here"), program)


@ROUTE_PROOF
def test_the_python_route_carries_a_platform_tag_and_a_runnable_program(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A wheel with a pure tag would install on a machine it cannot run on."""
    dist = into("python-route")

    built = build(repo, "pypi:printobserver-cli", dist, program)

    wheel = built.paths[0]
    platform = platforms.host(repo)
    truth(
        wheel.name.endswith(
            f"-{wheels.INTERPRETER}-{platform.wheel_tag(platforms.host_baseline(program))}.whl"
        )
        and f"-{platform.naming.wheel_family}_" in wheel.name,
        describing=f"{wheel.name} to state the platform it was built for, `{platform.id}`",
    )
    truth("none-any" not in wheel.name, describing=f"{wheel.name} not to carry a pure tag")
    with zipfile.ZipFile(wheel) as opened:
        carried = opened.namelist()
        script = next(
            name for name in carried if name.endswith(f"/scripts/{platforms.host(repo).program}")
        )
        equal(opened.read(script), program.read_bytes(), describing="the program the wheel carries")


@pytest.mark.parametrize(
    ("machine", "tag"),
    [("AMD64", "win_amd64"), ("ARM64", "win_arm64")],
    ids=["windows-x86_64", "windows-aarch64"],
)
def test_a_wheel_built_on_windows_carries_the_program_under_the_name_windows_runs(
    machine: str,
    tag: str,
    repo: Repo,
    program: Path,
    into: Callable[[str], Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An installer copies a wheel's scripts verbatim, so the `.exe` has to be in the wheel.

    Carried as a bare `printobserver`, the program lands in the environment's
    `Scripts` under a name a Windows host does not run, and the route proves a
    program missing. And the tag is the platform's own — `win_amd64` or
    `win_arm64`, the two that carry no version component — read off the
    descriptor rather than composed. Built with this host answering as each
    Windows platform in turn, so both answers are asked for on every host.
    """
    monkeypatch.setattr(host_platform, "system", lambda: "Windows")
    monkeypatch.setattr(host_platform, "machine", lambda: machine)

    built = build(repo, "pypi:printobserver-cli", into(f"windows-python-route-{tag}"), program)

    wheel = built.paths[0]
    truth(wheel.name.endswith(f"-{tag}.whl"), describing=f"{wheel.name} to be a `{tag}` wheel")
    truth("none-any" not in wheel.name, describing=f"{wheel.name} not to carry a pure tag")
    with zipfile.ZipFile(wheel) as opened:
        scripts = [name.rsplit("/", 1)[1] for name in opened.namelist() if "/scripts/" in name]
        carried = next(
            name for name in opened.namelist() if name.endswith("/scripts/printobserver.exe")
        )
        equal(
            opened.read(carried), program.read_bytes(), describing="the program the wheel carries"
        )
    equal(scripts, ["printobserver.exe"], describing="the programs the Windows wheel carries")


@ROUTE_PROOF
def test_the_node_route_resolves_a_per_platform_package(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A launcher and the package the caller's own install resolves through it."""
    dist = into("node-route")

    built = build(repo, "npm:printobserver-cli", dist, program)

    manifests = {manifest_of(path)["name"]: manifest_of(path) for path in built.paths}
    platform = platforms.host(repo)
    contains(manifests, "printobserver-cli", describing="the packages this route publishes")
    contains(manifests, platform.npm_package, describing="the packages this route publishes")
    carried = manifests[platform.npm_package]
    equal(carried["printobserverPlatform"], platform.id)
    optional = manifests["printobserver-cli"]["optionalDependencies"]
    truth(isinstance(optional, dict), describing="the launcher to declare optional packages")
    contains(
        cast("dict[str, str]", optional),
        platform.npm_package,
        describing="what the launcher resolves the program through",
    )


@ROUTE_PROOF
def test_the_script_route_publishes_an_artifact_and_the_digest_it_is_verified_by(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """Nothing reaches a path that was not checked against what the release said."""
    dist = into("script-route")

    built = build(repo, "release:printobserver", dist, program)

    archive = next(path for path in built.paths if path.name.endswith(".tar.gz"))
    digests = next(path for path in built.paths if path.name.endswith("SHA256SUMS"))
    contains(digests.read_text(encoding="utf-8"), digest_of(archive), describing="the digests")
    with tarfile.open(archive, "r:gz") as opened:
        equal(
            opened.getnames(),
            [platforms.host(repo).program],
            describing="what the release artifact carries: the program under its own name here",
        )


def test_a_staged_release_has_the_shape_the_forge_serves(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A script proven against this is proven against the layout it will meet."""
    staged = staged_release(repo, into("staged"), program)

    version = targets.workspace(repo.root)["version"]
    truth(
        (staged / "latest/download" / platforms.host(repo).asset).is_file(),
        describing="the newest release to be where the forge serves one",
    )
    truth(
        (staged / "download" / f"v{version}" / "SHA256SUMS").is_file(),
        describing="a pinned release to be where the forge serves one",
    )


def test_the_client_packages_record_the_contract_they_were_generated_from(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A consumer reads it off the installed package rather than off this tree."""
    recorded = contract_version(repo, "python/printobserver-sdk/src/printobserver_sdk/contract.py")
    equal(recorded, targets.workspace(repo.root)["version"])

    wheel = build(repo, "pypi:printobserver-sdk", into("python-client"), program).paths[0]
    with zipfile.ZipFile(wheel) as opened:
        stated = next(name for name in opened.namelist() if name.endswith(CONTRACT_FILE))
        equal(opened.read(stated).decode().strip(), recorded)

    tarball = build(repo, "npm:@printobserver/sdk", into("node-client"), program).paths[0]
    equal(manifest_of(tarball)[CONTRACT_FIELD], recorded)


def test_a_generated_module_recording_no_contract_is_refused(repo: Repo) -> None:
    """A package that recorded none would leave a consumer unable to tell."""
    with pytest.raises(BuildError, match="records no"):
        contract_version(repo, "justfile")


def test_an_artifact_nothing_here_takes_is_refused(repo: Repo, into: Callable[[str], Path]) -> None:
    """A consumer nothing knows how to be is a stop rather than a guess."""
    with pytest.raises(InstallError, match="nothing here takes"):
        install(repo, "crate:printobserver-types", into("refused"))


def test_a_publish_with_no_credential_is_refused(repo: Repo, into: Callable[[str], Path]) -> None:
    """A publish that failed after a merge for want of a token is a release lost."""
    with pytest.raises(PublishError, match="repository secret"):
        publish(repo, into("nothing-to-publish"), {})


def test_a_checksum_file_is_what_the_tool_a_machine_already_has_reads(
    tmp_path: Path,
) -> None:
    """A caller with no script at all can verify a download with `sha256sum`."""
    one = tmp_path / "one.tar.gz"
    one.write_bytes(b"an artifact")

    said = checksums([one]).decode()

    equal(said, f"{digest_of(one)}  one.tar.gz\n")


def test_a_platform_nothing_here_names_is_refused(repo: Repo) -> None:
    """A supported platform no artifact knows how to name is a stop, not a guess."""
    unknown = platforms.Platform(
        id="linux-riscv64",
        runner="a-runner",
        target="a-target",
        service_manager="systemd",
        install_path=True,
    )

    with pytest.raises(platforms.PlatformError, match="NAMING"):
        _ = unknown.npm
    with pytest.raises(platforms.PlatformError, match="NAMING"):
        unknown.wheel_tag((2, 39))


def host_of(identifier: str) -> tuple[str, str]:
    """What `platform.system()` and `platform.machine()` report on a host of `identifier`."""
    return next(reported for reported, named in platforms.HOSTS.items() if named == identifier)


def pretend_host(monkeypatch: pytest.MonkeyPatch, identifier: str) -> None:
    """Make this interpreter report a host of `identifier`, through what the builders read.

    A Linux host reports a C library, and a macOS host a system release far newer
    than any program below was built for — so a tag read off the host rather than
    off the program would carry it.
    """
    system, machine = host_of(identifier)
    monkeypatch.setattr(host_platform, "system", lambda: system)
    monkeypatch.setattr(host_platform, "machine", lambda: machine)
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("15.7", ("", "", ""), machine))
    # `raising=False`: a Windows interpreter carries no `confstr` to replace, and
    # the host this answers for is the one `system` reports.
    monkeypatch.setattr(os, "confstr", lambda _name: "glibc 2.39", raising=False)


#: The Mach-O processor type of each macOS platform, as the format's header spells it.
MACHO_CPU = {"macos-aarch64": 0x0100000C}


def macos_program(path: Path, identifier: str, minimum: tuple[int, int]) -> Path:
    """A 64-bit Mach-O program for `identifier` whose `LC_BUILD_VERSION` records `minimum`.

    The header, an `LC_UUID` a reader has to walk past, and one
    `LC_BUILD_VERSION` (0x32) for macOS carrying `minimum` as its `minos` — the
    shape a linker writes, and all a wheel's tag is read from.
    """
    major, minor = minimum
    commands = struct.pack("<II", 0x1B, 24) + bytes(16)
    commands += struct.pack("<IIIIII", 0x32, 24, 1, major << 16 | minor << 8, 15 << 16, 0)
    header = struct.pack(
        "<IiiIIIII", 0xFEEDFACF, MACHO_CPU[identifier], 0, 2, 2, len(commands), 0, 0
    )
    path.write_bytes(header + commands)
    return path


@ROUTE_PROOF
@pytest.mark.parametrize(
    ("identifier", "minimum", "tag"),
    [
        ("linux-x86_64", None, "manylinux_2_39_x86_64"),
        ("linux-aarch64", None, "manylinux_2_39_aarch64"),
        ("macos-aarch64", (11, 0), "macosx_11_0_arm64"),
        ("macos-aarch64", (13, 4), "macosx_14_0_arm64"),
        ("windows-x86_64", None, "win_amd64"),
        ("windows-aarch64", None, "win_arm64"),
    ],
    ids=[
        "linux-x86_64",
        "linux-aarch64",
        "macos-aarch64",
        "macos-minor-rounds-up",
        "windows-x86_64",
        "windows-aarch64",
    ],
)
def test_the_wheel_tag_states_the_floor_the_program_was_built_against(
    repo: Repo,
    program: Path,
    into: Callable[[str], Path],
    monkeypatch: pytest.MonkeyPatch,
    identifier: str,
    minimum: tuple[int, int] | None,
    tag: str,
) -> None:
    """A wheel claiming an older floor would install where it cannot run.

    The real builder, on a host of each platform: Linux states the C library the
    host built against, and macOS the minimum release recorded in the program the
    wheel carries — spelled as an installer generates it, so a minor release
    from 11 on rounds up rather than claiming a host older than the program
    needs, and never the release of the host that happened to build it. The two
    Windows tags carry no version component, and a Windows host states none.
    """
    pretend_host(monkeypatch, identifier)
    carried = program if minimum is None else macos_program(program, identifier, minimum)

    built = build(repo, "pypi:printobserver-cli", into("tagged"), carried)

    equal(
        built.paths[0].name.removesuffix(".whl").split("-")[-1],
        tag,
        describing=f"the platform tag of the wheel built on a `{identifier}` host",
    )


def test_a_macos_program_recording_no_floor_is_refused_naming_it(
    repo: Repo, program: Path, into: Callable[[str], Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    """A tag stating a floor nothing recorded is a guess put on a wheel; it is a stop."""
    pretend_host(monkeypatch, "macos-aarch64")
    header = struct.pack("<IiiIIIII", 0xFEEDFACF, MACHO_CPU["macos-aarch64"], 0, 2, 0, 0, 0, 0)
    program.write_bytes(header)

    with pytest.raises(platforms.PlatformError) as refusal:
        build(repo, "pypi:printobserver-cli", into("refused"), program)

    contains(str(refusal.value), str(program), describing="the refusal")
    contains(str(refusal.value), "records no minimum macOS release", describing="the refusal")


@pytest.mark.parametrize("identifier", sorted(platforms.NAMING))
def test_every_install_platforms_routes_are_named_by_its_descriptor(
    repo: Repo,
    program: Path,
    into: Callable[[str], Path],
    monkeypatch: pytest.MonkeyPatch,
    identifier: str,
) -> None:
    """On a host of each platform the install path targets, the routes name that platform.

    The per-platform package is the one the launcher resolves on that host,
    selected by the operating system and processor its descriptor states; the
    launcher declares every install platform's; and the release asset carries
    that platform's name and program file. A platform the install path does not
    target builds nothing to check, and says so by being skipped.
    """
    supported = {platform.id: platform for platform in platforms.install_platforms(repo)}
    if identifier not in supported:
        pytest.skip(f"`{identifier}` is not a platform the install path targets")
    platform = supported[identifier]
    pretend_host(monkeypatch, identifier)

    node = build(repo, "npm:printobserver-cli", into("node"), program)
    script = build(repo, "release:printobserver", into("script"), program)

    manifests = {manifest_of(path)["name"]: manifest_of(path) for path in node.paths}
    carried = manifests[platform.npm_package]
    system, processor = platform.npm
    equal(carried["os"], [system], describing=f"the operating system `{identifier}` selects by")
    equal(carried["cpu"], [processor], describing=f"the processor `{identifier}` selects by")
    equal(carried["printobserverPlatform"], identifier, describing="the platform it carries")
    equal(
        manifests["printobserver-cli"]["optionalDependencies"],
        {one.npm_package: targets.workspace(repo.root)["version"] for one in supported.values()},
        describing="the packages the launcher resolves the program through",
    )
    equal(
        sorted(path.name for path in script.paths),
        sorted([platform.asset, "SHA256SUMS"]),
        describing=f"what the script route publishes on a `{identifier}` host",
    )
    archive = next(path for path in script.paths if path.name == platform.asset)
    with tarfile.open(archive, "r:gz") as opened:
        equal(opened.getnames(), [platform.program], describing="what the asset carries")


def test_the_tool_says_what_it_refused_rather_than_stopping_silently(
    repo: Repo, into: Callable[[str], Path]
) -> None:
    """Its own command surface answers a refusal as a message and a status."""
    from release_artifacts.__main__ import main

    equal(main(["list", "--root", str(repo.root)]), 0)
    equal(main(["build", "--root", str(repo.root)]), 2)
    equal(
        main(
            [
                "build",
                "--target",
                "pypi:no-such-distribution",
                "--root",
                str(repo.root),
                "--into",
                str(into("refused-by-the-surface")),
            ]
        ),
        1,
    )


def test_a_built_manifest_that_is_not_one_is_refused(tmp_path: Path) -> None:
    """A package carrying no manifest is not one a registry could serve."""
    empty = tmp_path / "empty.tgz"
    with tarfile.open(empty, "w:gz") as archive:
        info = tarfile.TarInfo("package/nothing")
        info.size = 0
        archive.addfile(info)

    with pytest.raises(BuildError, match="carries no package manifest"):
        manifest_of(empty)


def test_a_program_that_is_not_there_is_refused(repo: Repo) -> None:
    """A build carrying no program would ship an artifact with nothing in it."""
    from release_artifacts.build import program as built_program

    with pytest.raises(BuildError, match="not a program"):
        built_program(repo, Path("/no/such/program"))


def test_what_a_registry_reads_off_a_wheel_is_what_the_declaration_said(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """The metadata a registry shows comes from the declaration and the workspace."""
    wheel = build(repo, "pypi:printobserver-sdk", into("metadata"), program).paths[0]

    with zipfile.ZipFile(wheel) as opened:
        metadata = next(name for name in opened.namelist() if name.endswith("METADATA"))
        said = opened.read(metadata).decode()
    declared = targets.named(repo.root, "pypi:printobserver-sdk")
    contains(said, f"Name: {declared.name}", describing="what the wheel says about itself")
    contains(said, declared.description, describing="what the wheel says about itself")
    contains(
        said,
        f"Version: {targets.workspace(repo.root)['version']}",
        describing="what the wheel says about itself",
    )


def test_the_declaration_is_read_rather_than_repeated(repo: Repo, tmp_path: Path) -> None:
    """A target with no identifier of the form every consumer reads is refused."""
    broken = tmp_path / "broken"
    broken.mkdir()
    (broken / "release-targets.toml").write_text(
        'schema_version = 2\n\n[[target]]\nid = "nameless"\n', encoding="utf-8"
    )

    with pytest.raises(TargetError, match="<registry>:<name>"):
        targets.declared(broken)


def test_a_workspace_declaring_no_version_is_refused(tmp_path: Path) -> None:
    """Every artifact takes the version release automation wrote there."""
    broken = tmp_path / "no-version"
    broken.mkdir()
    (broken / "Cargo.toml").write_text("[workspace]\nmembers = []\n", encoding="utf-8")

    with pytest.raises(TargetError, match="declares no"):
        targets.workspace(broken)


def test_what_the_tool_wrote_is_a_document_a_registry_can_read(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """Every package it writes carries a manifest that parses."""
    built = build(repo, "npm:printobserver-cli", into("parses"), program)

    for path in built.paths:
        equal(json.loads(json.dumps(manifest_of(path)))["license"], "MIT")


def test_a_failure_says_what_it_was_doing(repo: Repo, into: Callable[[str], Path]) -> None:
    """A stop nobody can act on is worse than none."""
    failing((1, str(BuildError("`cargo package` failed"))), naming="cargo package")
