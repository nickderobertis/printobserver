"""Every artifact this repository ships, built and taken the way a consumer takes it.

Nothing is mocked. The six real recipes run, the real tool builds each artifact
from the committed tree, and each is installed into a throwaway environment
holding no copy of these sources — a virtual environment, a package directory,
an unpacked crate — and then proved where it was installed. A client is proved
by its own committed smoke check against a **real supervisor**; a route by the
program it put on a path reporting its own version.

The three routes are installed with a PATH holding no Rust toolchain at all,
which is the whole reason they carry a program already built for the platform:
the host they are for is the small machine beside the printer.

This tier runs the recipes in this repository's own tree rather than in a copy
of it, and deliberately: each recipe builds the `printobserver` program, and six
copies with no build products between them would be six release builds of a Rust
workspace. What each recipe installs into is still a directory holding none of
these sources, which is what the assertions are about.
"""

from __future__ import annotations

import tomllib

import pytest
from journey import REPO_ROOT, clean_environment, run
from repo_checks.expect import contains, failing, passing

#: How long one recipe is given: the first pays for a release build of the
#: workspace, and every one after it reuses what that left.
RECIPE_TIMEOUT_SECONDS = 2400

#: Every artifact this repository ships, the recipe that takes it, and what a
#: reader of that recipe's output should see: a client says what its smoke
#: check found, and a route says the version of the program it installed.
CLIENTS = [
    ("crate:printobserver-sdk", "prove-client-rust"),
    ("pypi:printobserver-sdk", "prove-client-python"),
    ("npm:@printobserver/sdk", "prove-client-node"),
]
ROUTES = [
    ("pypi:printobserver-cli", "prove-route-pypi"),
    ("npm:printobserver-cli", "prove-route-npm"),
    ("release:printobserver", "prove-route-script"),
]
SHIPPED = CLIENTS + ROUTES


def _version() -> str:
    """The version release automation wrote into the workspace."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def _pythonpath() -> str:
    """The packages this repository's own tools live in.

    Read out of the justfile's own export rather than repeated here, so a
    package this repository grows is one this tier finds without being told.
    """
    for line in (REPO_ROOT / "justfile").read_text(encoding="utf-8").splitlines():
        if line.startswith("export PYTHONPATH :="):
            return line.partition(":=")[2].strip().strip('"')
    message = "the justfile exports no PYTHONPATH, and these tools live on it"
    raise AssertionError(message)


def _tool(*arguments: str) -> tuple[int, str]:
    """Run the artifact tool the way its own recipes run it."""
    from repo_checks.shell import run as shell_run

    result = shell_run(
        ["uv", "run", "-q", "python", "-m", "release_artifacts", *arguments],
        cwd=REPO_ROOT,
        env=clean_environment(PYTHONPATH=_pythonpath()),
        timeout=RECIPE_TIMEOUT_SECONDS,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def _recipe(name: str) -> tuple[int, str]:
    """Run one recipe of this repository's own command surface."""
    result = run(["just", name], REPO_ROOT, timeout=RECIPE_TIMEOUT_SECONDS)
    return result.returncode, result.stdout + result.stderr


def test_the_declaration_names_every_artifact_this_tier_takes() -> None:
    """A shipped artifact this walk does not drive is one nothing proves."""
    code, said = _tool("list")
    passing((code, said), describing="listing the declared targets")
    declared = {line.split("\t")[0] for line in said.splitlines() if line.strip()}

    for identifier, _ in SHIPPED:
        contains(declared, identifier, describing="the declared targets")


@pytest.mark.parametrize(("identifier", "recipe"), CLIENTS, ids=[name for _, name in CLIENTS])
def test_each_client_is_installed_and_proved_against_a_real_server(
    identifier: str, recipe: str
) -> None:
    """A client is resolvable and usable where a consumer put it.

    Its own committed smoke check runs there — reading a status and
    materializing an image whose bytes it checks against the digest the record
    declares — and says which server contract the installed package records.
    """
    code, said = _recipe(recipe)

    passing((code, said), describing=f"`just {recipe}` over `{identifier}`")
    contains(said, "smoke: contract", describing=f"what `just {recipe}` said")
    contains(said, f"contract {_version()}", describing=f"what `just {recipe}` said")


@pytest.mark.parametrize(("identifier", "recipe"), ROUTES, ids=[name for _, name in ROUTES])
def test_each_route_leaves_a_runnable_program_on_the_path(identifier: str, recipe: str) -> None:
    """A route installs a program that runs and reports its own version.

    And the recipe says what the path that program was installed under carried:
    a route that reached a Rust toolchain on this host would name it here
    instead, which is the property that makes these three routes rather than a
    source install what the end user gets.
    """
    code, said = _recipe(recipe)

    passing((code, said), describing=f"`just {recipe}` over `{identifier}`")
    contains(said, f"printobserver {_version()}", describing=f"what `just {recipe}` said")
    contains(
        said,
        "Rust toolchain on the install path: none",
        describing=f"what `just {recipe}` said",
    )


@pytest.mark.parametrize(
    ("recipe", "expected"),
    [("prove-client-python", "smoke: contract"), ("prove-route-pypi", "printobserver")],
)
def test_python_proofs_can_be_repeated(recipe: str, expected: str) -> None:
    """A second proof installs and runs the artifact in its disposable environment."""
    for attempt in range(2):
        code, said = _recipe(recipe)
        passing((code, said), describing=f"`just {recipe}`, attempt {attempt + 1}")
        contains(said, f"{expected} {_version()}", describing=f"what `just {recipe}` said")


def test_the_tool_refuses_an_artifact_nothing_here_builds() -> None:
    """A target the declaration does not name is a stop rather than a guess."""
    code, said = _tool(
        "build", "--target", "pypi:no-such-distribution", "--into", "dist/proof/refused"
    )

    failing((code, said), naming="is not a target release-targets.toml declares")
