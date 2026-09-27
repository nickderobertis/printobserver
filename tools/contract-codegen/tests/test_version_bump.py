"""The committed clients survive a release pull request untouched.

Release automation moves the workspace version and regenerates nothing, so a
client that restated that version as text would be stale on every release
pull request and the correspondence gate would refuse the only change a
release comes from. Both halves are driven over a copy of the tree moved
exactly as that pull request moves it: regenerating over it changes no file,
and each client used from that copy's sources reports the version it moved to.
"""

from __future__ import annotations

import os
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import REPO_ROOT
from contract_codegen.generate import OUTPUTS, write
from contract_codegen.version import NODE_MODULE, PYTHON_MODULE, RUST_MODULE, WORKSPACE_MANIFEST
from release_artifacts.build import CLIENT_REQUIRES_PYTHON, floor
from repo_checks.expect import absent, contains, equal, failing, passing
from repo_checks.shell import run


def test_regenerating_over_a_bumped_tree_changes_no_generated_file(
    bumped: tuple[Path, str],
) -> None:
    """The generator writes the same bytes whatever version the workspace declares."""
    copy, version = bumped

    equal(write(copy), [], describing="the files regeneration changed after the bump")
    for output in OUTPUTS:
        absent(
            (copy / output.path).read_text(encoding="utf-8"),
            version,
            describing=f"{output.path}, which must not state the workspace version",
        )


#: What running one from-source client answers.
Completed = subprocess.CompletedProcess[str]

#: The interpreters the Python client is imported on from source: the lowest
#: the published wheel declares, and the one this suite itself runs on.
PYTHONS = (
    floor(CLIENT_REQUIRES_PYTHON),
    f"{sys.version_info.major}.{sys.version_info.minor}",
)

#: What each from-source client is asked, printing the constant and nothing else.
PYTHON_PROBE = (
    "import sys; sys.path.insert(0, sys.argv[1]); "
    "from printobserver_sdk import CONTRACT_VERSION; print(CONTRACT_VERSION)"
)
NODE_PROBE = (
    "const { CONTRACT_VERSION } = await import(process.argv[1]); console.log(CONTRACT_VERSION);"
)
RUST_PROBE = """//! Prints the contract version the client reports.

fn main() {
    println!("{}", printobserver_sdk::CONTRACT_VERSION);
}
"""

#: Where the Rust client is compiled from a copy. Under the committed tree's own
#: `target`, which nothing tracks, so a second run reuses the dependencies the
#: first compiled rather than building them all again from nothing.
RUST_TARGET = REPO_ROOT / "target" / "contract-version-probe"

#: How long one from-source client is given, compiling included.
TIMEOUT_SECONDS = 1800


def python_from_source(root: Path, interpreter: str) -> Completed:
    """Import the Python client from the sources of `root`, on one interpreter.

    Isolated (`-I`), so that nothing but the copy's own sources can answer the
    import: not this suite's environment and not a `PYTHONPATH`.
    """
    sources = root / Path(PYTHON_MODULE).parents[1]
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
        cwd=root,
        timeout=TIMEOUT_SECONDS,
    )


def node_from_source(root: Path) -> Completed:
    """Import the Node client's module from the sources of `root`, under bun."""
    module = (root / NODE_MODULE).resolve().as_uri()
    return run(["bun", "--eval", NODE_PROBE, module], cwd=root, timeout=TIMEOUT_SECONDS)


def reported(result: Completed, *, describing: str) -> str:
    """The one line a probe printed, or a failure carrying the whole run."""
    passing(result, describing=describing)
    return result.stdout.strip()


@pytest.mark.parametrize("interpreter", PYTHONS)
def test_the_python_client_from_a_bumped_trees_sources_reports_the_bumped_version(
    bumped: tuple[Path, str], interpreter: str
) -> None:
    """Imported from a checkout, it reports that checkout's workspace version."""
    copy, version = bumped

    said = reported(
        python_from_source(copy, interpreter),
        describing=f"importing the Python client on Python {interpreter}",
    )

    equal(said, version, describing=f"CONTRACT_VERSION on Python {interpreter}")


def test_the_node_client_from_a_bumped_trees_sources_reports_the_bumped_version(
    bumped: tuple[Path, str],
) -> None:
    """Imported from a checkout under the runtime its tests use, the same."""
    copy, version = bumped

    said = reported(node_from_source(copy), describing="importing the Node client")

    equal(said, version, describing="CONTRACT_VERSION from the Node client")


# llmlint: ignore[test_tiers_split_by_project_not_by_marker] suppressions.toml has the reason.
def test_the_rust_client_compiled_in_a_bumped_tree_reports_the_bumped_version(
    bumped: tuple[Path, str],
) -> None:
    """Compiled from a checkout, it reports the version that checkout's crate is."""
    copy, version = bumped
    example = copy / Path(RUST_MODULE).parents[1] / "examples" / "contract_version.rs"
    example.parent.mkdir(parents=True, exist_ok=True)
    example.write_text(RUST_PROBE, encoding="utf-8")

    compiled = run(
        [
            "cargo",
            "run",
            "-q",
            "--locked",
            "-p",
            "printobserver-sdk",
            "--example",
            "contract_version",
        ],
        cwd=copy,
        env={**os.environ, "CARGO_TARGET_DIR": str(RUST_TARGET)},
        timeout=TIMEOUT_SECONDS,
    )

    equal(
        reported(compiled, describing="compiling the Rust client"),
        version,
        describing="CONTRACT_VERSION from the Rust client",
    )


#: Workspace manifests from which no client may report a version, by what is
#: wrong with each; `None` is no manifest at all.
REFUSED_MANIFESTS = {
    "missing": None,
    "not-toml": '[workspace.package\nversion = "0.3.0"\n',
    "versionless": '[workspace]\nmembers = ["crates/*"]\n',
    "not-a-version": '[workspace.package]\nversion = "latest"\n',
    "leading-zero": '[workspace.package]\nversion = "01.2.3"\n',
    "unquoted": "[workspace.package]\nversion = 0.3\n",
    "not-a-table": '[workspace]\npackage = "0.3.0"\n',
}

#: The two clients that read the manifest at import, by how each is imported
#: and what its refusal is raised as.
FROM_SOURCE: dict[str, tuple[Callable[[Path], Completed], str]] = {
    "python": (lambda root: python_from_source(root, PYTHONS[-1]), "ImportError"),
    "node": (node_from_source, "Error"),
}


@pytest.mark.parametrize("client", FROM_SOURCE)
@pytest.mark.parametrize("manifest", REFUSED_MANIFESTS)
def test_a_from_source_client_without_a_workspace_version_refuses_to_import(
    scratch: Callable[[], Path], manifest: str, client: str
) -> None:
    """No manifest, or one declaring no release version, is a refusal naming it — never a guess."""
    copy = scratch()
    path = copy / WORKSPACE_MANIFEST
    text = REFUSED_MANIFESTS[manifest]
    if text is None:
        path.unlink()
    else:
        path.write_text(text, encoding="utf-8")
    reason = "could not be read" if text is None or manifest == "not-toml" else "declares no"
    importing, raised = FROM_SOURCE[client]

    result = importing(copy)

    failing(result, naming=raised)
    contains(result.stderr, str(path.resolve()), describing=f"the {client} client's refusal")
    contains(result.stderr, reason, describing=f"the {client} client's refusal")
    equal(result.stdout, "", describing=f"what the {client} client reported")
