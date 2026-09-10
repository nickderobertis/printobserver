"""What this repository ships, and the checks that keep the three routes real.

Every journey here drives the committed check functions over a real copy of the
committed tree with one defect in it. The install-path section is the authority
for the routes and the platforms, so most of these break that section rather
than the configuration derived from it — a check reading the derived matrix
would be satisfied by narrowing that matrix.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from repo_checks.checks_ci import (
    artifact_jobs,
    install_path_not_narrowed,
    install_script_path,
)
from repo_checks.checks_release import publish_credentials, release_automation, release_targets
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import Tree, copy_tree

AGENTS = "AGENTS.md"
TARGETS = "release-targets.toml"
ARTIFACTS = ".github/workflows/artifacts.yml"
RELEASE = ".github/workflows/release-plz.yml"

#: The platform line one journey adds to the supported-platform list, and the
#: route heading another adds. Both are what a *widened* section looks like:
#: the check is against the derived configuration, so the section growing is
#: what a narrowed configuration looks like from the section's side.
GAINED_PLATFORM = (
    "- `linux-riscv64` — runner `ubuntu-24.04-riscv`, Rust target "
    "`riscv64gc-unknown-linux-gnu`, service manager `systemd`, install path: yes"
)

GAINED_ROUTE = """
#### Route 4 — the operating system's own package manager

An ordinary install of this repository's system package.

```console
apt install printobserver-cli
```
"""


def _committed(tree: Tree) -> None:
    """Make the copy a repository whose base branch carries what was copied."""
    for argv in (
        ["init", "-q", "-b", "main"],
        ["add", "-A"],
        [
            "-c",
            "user.email=checks@printobserver.test",
            "-c",
            "user.name=checks",
            "commit",
            "-q",
            "-m",
            "chore: the committed tree, copied",
        ],
    ):
        run(["git", *argv], cwd=tree.root, check=True)


def test_the_committed_tree_ships_what_it_says_it_ships(committed: Repo) -> None:
    """Every check over what this repository publishes accepts the tree it ships."""
    for check in (release_targets, release_automation, publish_credentials, artifact_jobs):
        accepted(check(committed), describing=f"the committed tree under {check.__name__}")
    accepted(install_script_path(committed))


def test_a_route_with_no_target_behind_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A route the section names and nothing publishes is a way to no program."""
    widened = tree()
    widened.append(AGENTS, "")
    widened.edit(AGENTS, "#### Route 3", GAINED_ROUTE + "\n#### Route 3")

    refused(release_targets(widened.repo), "for which release-targets.toml declares no target")


def test_a_route_installing_another_name_than_the_declaration_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The command a reader pastes and the artifact published are one name."""
    broken = tree()
    broken.edit(AGENTS, "pip install printobserver-cli", "pip install printobserver-tool")

    refused_naming(release_targets(broken.repo), "printobserver-tool", "two different names")


def test_a_platform_nothing_builds_for_is_refused(tree: Callable[[], Tree]) -> None:
    """A platform the section names and no build covers is a host with no program."""
    widened = tree()
    widened.edit(
        AGENTS,
        "[//]: # (END supported-platforms)",
        f"{GAINED_PLATFORM}\n[//]: # (END supported-platforms)",
    )

    refused(release_automation(widened.repo), "linux-riscv64")


def test_a_route_the_automation_declares_no_build_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A route with no artifact behind it is one release automation never builds."""
    widened = tree()
    widened.edit(AGENTS, "#### Route 3", GAINED_ROUTE + "\n#### Route 3")

    refused(release_automation(widened.repo), "declares no build at all")


def test_a_publish_by_a_keyless_trusted_publisher_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Every publish here authenticates with a token this repository declares."""
    broken = tree()
    broken.edit(
        RELEASE,
        "permissions:\n  contents: read",
        "permissions:\n  contents: read\n  id-token: write",
    )

    refused(publish_credentials(broken.repo), "keyless trusted publisher")


def test_a_publish_naming_no_secret_is_refused(tree: Callable[[], Tree]) -> None:
    """A publishing step with no credential is one that fails after a merge."""
    broken = tree()
    broken.edit(
        RELEASE,
        "      - run: just publish-artifacts\n        env:\n"
        "          PYPI_TOKEN: ${{ secrets.PYPI_TOKEN }}\n"
        "          NPM_TOKEN: ${{ secrets.NPM_TOKEN }}\n"
        "          RELEASE_PLZ_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}\n",
        "      - run: just publish-artifacts\n",
    )

    refused(publish_credentials(broken.repo), "names no secret as its credential")


def test_a_publish_naming_an_undeclared_secret_is_refused(tree: Callable[[], Tree]) -> None:
    """`gh-secrets.json` is the authoritative list of what this repository holds."""
    broken = tree()
    broken.edit(RELEASE, "secrets.PYPI_TOKEN", "secrets.SOME_OTHER_TOKEN")

    refused(publish_credentials(broken.repo), "gh-secrets.json does not declare")


def test_a_client_with_no_job_of_its_own_is_refused(tree: Callable[[], Tree]) -> None:
    """An artifact with no job is an artifact nothing builds, installs or proves."""
    broken = tree()
    broken.edit(ARTIFACTS, "      - run: just prove-client-python\n", "      - run: true\n")

    refused_naming(artifact_jobs(broken.repo), "prove-client-python", "declares no job")


def test_a_job_that_does_not_prove_what_it_installed_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A recipe pointed at another artifact stops proving the one it named."""
    broken = tree()
    broken.edit(
        "justfile",
        "prove-client-node:\n    uv run -q python -m release_artifacts prove "
        "--target npm:@printobserver/sdk --into dist/proof/client-node",
        "prove-client-node:\n    echo nothing",
    )

    refused_naming(artifact_jobs(broken.repo), "npm:@printobserver/sdk", "recipe builds it")


def test_a_platform_a_route_job_omits_is_refused(tree: Callable[[], Tree]) -> None:
    """Each route's job runs once per platform the install path names."""
    widened = tree()
    widened.edit(
        AGENTS,
        "[//]: # (END supported-platforms)",
        f"{GAINED_PLATFORM}\n[//]: # (END supported-platforms)",
    )

    refused_naming(artifact_jobs(widened.repo), "linux-riscv64", "release:printobserver")


def test_a_route_with_no_job_of_its_own_is_refused(tree: Callable[[], Tree]) -> None:
    """The three routes are alternatives; a route with no job is one nothing proves."""
    widened = tree()
    widened.edit(AGENTS, "#### Route 3", GAINED_ROUTE + "\n#### Route 3")

    refused(artifact_jobs(widened.repo), "declares no job of its own")


def test_a_fetch_url_naming_no_committed_script_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The section cannot offer a runnable command that fetches nothing."""
    broken = tree()
    broken.edit(AGENTS, "main/scripts/install.sh", "main/scripts/no-such-install.sh")

    refused(install_script_path(broken.repo), "no-such-install.sh")


def test_the_committed_tree_has_narrowed_nothing(tmp_path: Path) -> None:
    """The install path still names every platform and route it was cut with."""
    unchanged = Tree(copy_tree(tmp_path / "unchanged"))
    _committed(unchanged)

    accepted(install_path_not_narrowed(unchanged.repo))


def test_a_platform_deleted_from_the_install_path_is_refused(tmp_path: Path) -> None:
    """Every artifact, matrix and route here is derived from that one list."""
    narrowed = Tree(copy_tree(tmp_path / "narrowed-platform"))
    _committed(narrowed)
    narrowed.edit(
        AGENTS,
        "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
        "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n",
        "",
    )

    refused(install_path_not_narrowed(narrowed.repo), "linux-aarch64")


def test_a_route_deleted_from_the_install_path_is_refused(tmp_path: Path) -> None:
    """A route deleted here is a way to the program nobody has any more."""
    narrowed = Tree(copy_tree(tmp_path / "narrowed-route"))
    _committed(narrowed)
    text = narrowed.read(AGENTS)
    start = text.index("#### Route 2")
    end = text.index("#### Route 3")
    narrowed.write(AGENTS, text[:start] + text[end:])

    refused(install_path_not_narrowed(narrowed.repo), "Route 2")
