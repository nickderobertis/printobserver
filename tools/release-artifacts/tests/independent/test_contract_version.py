"""The published clients state the version they were built at, and need nothing else.

Used from a checkout, the Python and Node clients read `CONTRACT_VERSION` out
of the workspace manifest of the tree they sit in. An installed package has no
such tree, so the build stamps the release version over that computation. These
journeys build both packages from a copy of the tree whose version was moved the
way a release pull request moves it, open what was built, and run each module
from a directory with no workspace anywhere above it.
"""

from __future__ import annotations

import re
import sys
import tarfile
import tomllib
import zipfile
from pathlib import Path

import pytest
from conftest import REPO_ROOT
from contract_codegen.version import BEGIN, END, NODE_MODULE, PYTHON_MODULE
from release_artifacts import bumping
from release_artifacts.build import (
    CLIENT_REQUIRES_PYTHON,
    CONTRACT_FIELD,
    CONTRACT_FILE,
    NODE_OUTPUT,
    BuildError,
    build,
    floor,
    manifest_of,
    recorded,
)
from release_artifacts.installing import interpreter_in
from repo_checks import scratch
from repo_checks.expect import Run, absent, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run

#: Every spelling by which a module could go looking for a workspace manifest
#: or read a file at all. A published module carries none of them.
LOOKING = (
    "Cargo.toml",
    "tomllib",
    "smol-toml",
    "open(",
    "Path",
    "node:fs",
    "readFileSync",
    "import.meta",
)

#: What each built module is asked, printing the constant and nothing else.
PYTHON_PROBE = (
    "import sys; sys.path.insert(0, sys.argv[1]); "
    "from printobserver_sdk.contract import CONTRACT_VERSION; print(CONTRACT_VERSION)"
)
WHEEL_PROBE = (
    "import sys, printobserver_sdk; "
    "print(f'{sys.version_info.major}.{sys.version_info.minor}', "
    "printobserver_sdk.CONTRACT_VERSION)"
)
NODE_PROBE = (
    "const { CONTRACT_VERSION } = await import(process.argv[1]); console.log(CONTRACT_VERSION);"
)


@pytest.fixture
def copy(tmp_path: Path) -> Path:
    """A fresh copy of the committed tree the build may write into."""
    return scratch.copy_tree(REPO_ROOT, tmp_path / "tree")


@pytest.fixture
def bumped(copy: Path) -> tuple[Repo, str]:
    """That copy, its version moved as a release pull request moves it."""
    version = bumping.next_minor(bumping.workspace_version(copy))
    bumping.bump_workspace_version(copy, version)
    return Repo(copy), version


def _wheel_module(wheel: Path, into: Path) -> tuple[str, str]:
    """The wheel's own contract module and metadata entry, the wheel unpacked into `into`."""
    with zipfile.ZipFile(wheel) as opened:
        # The package's own files, each written where an installer puts it;
        # read by name rather than extracted wholesale.
        for name in opened.namelist():
            if name.startswith("printobserver_sdk/"):
                (into / name).parent.mkdir(parents=True, exist_ok=True)
                (into / name).write_bytes(opened.read(name))
        stated = next(name for name in opened.namelist() if name.endswith(CONTRACT_FILE))
        metadata = opened.read(stated).decode().strip()
    return (into / "printobserver_sdk" / "contract.py").read_text(encoding="utf-8"), metadata


def _package_module(tarball: Path, into: Path) -> str:
    """The package's own compiled contract module, the package unpacked into `into`."""
    with tarfile.open(tarball, "r:gz") as archive:
        archive.extractall(into, filter="data")
    return (into / "package" / "dist" / "contract.js").read_text(encoding="utf-8")


def _stating_only(module: str, version: str, *, describing: str) -> None:
    """Fail unless `module` states `version` as a literal and looks for nothing."""
    equal(recorded(module), version, describing=f"the literal {describing} states")
    for spelling in (*LOOKING, BEGIN, END):
        absent(module, spelling, describing=describing)


# llmlint: ignore[test_tiers_split_by_project_not_by_marker] suppressions.toml has the reason.
def test_the_built_clients_state_the_bumped_version_and_read_no_manifest(
    bumped: tuple[Repo, str], program: Path, tmp_path: Path
) -> None:
    """Both packages from a bumped tree carry that version, and run where no workspace is."""
    repo, version = bumped

    wheel = build(repo, "pypi:printobserver-sdk", tmp_path / "python", program).paths[0]
    unpacked = tmp_path / "wheel"
    module, metadata = _wheel_module(wheel, unpacked)
    _stating_only(module, version, describing="the wheel's contract.py")
    equal(metadata, version, describing=f"the wheel's {CONTRACT_FILE}")
    ran = run([sys.executable, "-I", "-c", PYTHON_PROBE, str(unpacked)], cwd=tmp_path)
    passing(ran, describing="importing the wheel's module with no workspace above it")
    equal(ran.stdout.strip(), version, describing="what the wheel's module reports")

    tarball = build(repo, "npm:@printobserver/sdk", tmp_path / "node", program).paths[0]
    unpacked = tmp_path / "package"
    compiled = _package_module(tarball, unpacked)
    _stating_only(compiled, version, describing="the package's contract.js")
    equal(manifest_of(tarball)[CONTRACT_FIELD], version, describing=CONTRACT_FIELD)
    module_url = (unpacked / "package" / "dist" / "contract.js").as_uri()
    ran = run(["bun", "--eval", NODE_PROBE, module_url], cwd=tmp_path)
    passing(ran, describing="importing the package's module with no workspace above it")
    equal(ran.stdout.strip(), version, describing="what the package's module reports")


def _without_computation(root: Path, module: str, statement: str) -> None:
    """Replace the generated computation in one client module by a hand-written literal."""
    path = root / module
    text = path.read_text(encoding="utf-8")
    edited = re.sub(
        rf"^[^\n]*{re.escape(BEGIN)}\n.*?^[^\n]*{re.escape(END)}\n",
        statement + "\n",
        text,
        flags=re.MULTILINE | re.DOTALL,
    )
    truth(edited != text, describing=f"{module} to have carried the computation")
    path.write_text(edited, encoding="utf-8")


def test_a_python_client_carrying_no_computation_refuses_the_build(
    copy: Path, program: Path, tmp_path: Path
) -> None:
    """A stamp that cannot find what it replaces stops the build naming the module."""
    _without_computation(copy, PYTHON_MODULE, 'CONTRACT_VERSION = "0.0.1"')

    with pytest.raises(BuildError, match=re.escape(PYTHON_MODULE)):
        build(Repo(copy), "pypi:printobserver-sdk", tmp_path / "python", program)


def test_a_node_client_carrying_no_computation_refuses_the_build(
    copy: Path, program: Path, tmp_path: Path
) -> None:
    """The compiled module the stamp could not find a computation in is named."""
    _without_computation(copy, NODE_MODULE, 'export const CONTRACT_VERSION = "0.0.1";')

    with pytest.raises(BuildError, match=re.escape(f"{NODE_OUTPUT}/contract.js")):
        build(Repo(copy), "npm:@printobserver/sdk", tmp_path / "node", program)


@pytest.mark.parametrize("declared", ["latest", "01.2.3"])
@pytest.mark.parametrize("target", ["pypi:printobserver-sdk", "npm:@printobserver/sdk"])
def test_a_workspace_declaring_no_release_version_refuses_the_client_build(
    copy: Path, program: Path, tmp_path: Path, target: str, declared: str
) -> None:
    """What the source clients refuse to report, no package is stamped with."""
    manifest = copy / "Cargo.toml"
    was = bumping.workspace_version(copy)
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            f'version = "{was}"', f'version = "{declared}"', 1
        ),
        encoding="utf-8",
    )

    with pytest.raises(BuildError, match=re.escape(repr(declared))):
        build(Repo(copy), target, tmp_path / "built", program)


def _importing(interpreter: str) -> Run:
    """Import the committed Python client from its sources on one interpreter."""
    sources = REPO_ROOT / Path(PYTHON_MODULE).parents[1]
    return run(
        [
            "uv",
            "run",
            "-q",
            "--no-project",
            "--python",
            interpreter,
            "python",
            "-I",
            "-c",
            PYTHON_PROBE,
            str(sources),
        ],
        cwd=REPO_ROOT,
        timeout=1800,
    )


def _installed(wheel: Path, interpreter: str, into: Path) -> tuple[Run, Path]:
    """Install `wheel` into a fresh environment on one interpreter, as a consumer does.

    Answered beside that environment's own interpreter, which the install puts
    the package where it imports from.
    """
    passing(
        run(["uv", "venv", "-q", "--python", interpreter, str(into)], cwd=into.parent),
        describing=f"making a Python {interpreter} environment",
    )
    python = interpreter_in(into)
    installed = run(
        ["uv", "pip", "install", "-q", "--python", str(python), str(wheel)],
        cwd=into.parent,
        timeout=1800,
    )
    return installed, python


def test_the_client_wheel_declares_the_lowest_interpreter_the_client_imports_on(
    repo: Repo, program: Path, tmp_path: Path
) -> None:
    """Imported and installed on its declared floor, and refused on the release before it.

    Real interpreters, not a reading of the source: the declaration is too low
    the moment the generator emits syntax the floor cannot parse, and too high
    the moment the release before it could import the client after all.
    """
    lowest = floor(CLIENT_REQUIRES_PYTHON)
    major, minor = lowest.split(".")
    below = f"{major}.{int(minor) - 1}"

    passing(_importing(lowest), describing=f"importing the client on Python {lowest}")
    failing(_importing(below), naming="ImportError")

    wheel = build(repo, "pypi:printobserver-sdk", tmp_path / "python", program).paths[0]
    with zipfile.ZipFile(wheel) as opened:
        metadata = next(name for name in opened.namelist() if name.endswith("/METADATA"))
        truth(
            f"Requires-Python: {CLIENT_REQUIRES_PYTHON}\n" in opened.read(metadata).decode(),
            describing=f"the wheel's metadata to declare {CLIENT_REQUIRES_PYTHON}",
        )

    installed, python = _installed(wheel, lowest, tmp_path / "floor")
    passing(installed, describing=f"installing the wheel on Python {lowest}")
    # Run from a directory with no workspace above it, so only the wheel answers.
    ran = run([str(python), "-I", "-c", WHEEL_PROBE], cwd=tmp_path / "floor")
    passing(ran, describing=f"importing the installed wheel on Python {lowest}")
    equal(
        ran.stdout.strip(),
        f"{lowest} {bumping.workspace_version(repo.root)}",
        describing="the interpreter and the contract the installed wheel reports",
    )

    refused, _ = _installed(wheel, below, tmp_path / "below")
    failing(refused, naming=CLIENT_REQUIRES_PYTHON)


#: The ruff configuration the shipped Python client's sources are linted under.
CLIENT_RUFF = REPO_ROOT / Path(PYTHON_MODULE).parents[1] / "ruff.toml"


def test_the_client_sources_are_linted_at_the_floor_the_wheel_declares() -> None:
    """Ruff's target for the client is the declared floor, so the two cannot drift.

    Linted at a newer target, ruff would ask the generator for syntax the floor
    cannot parse; at an older one, it would let through syntax the floor can.
    """
    with CLIENT_RUFF.open("rb") as handle:
        declared = tomllib.load(handle)

    equal(
        declared.get("target-version"),
        "py" + floor(CLIENT_REQUIRES_PYTHON).replace(".", ""),
        describing=f"the target-version {CLIENT_RUFF.relative_to(REPO_ROOT)} lints at",
    )
