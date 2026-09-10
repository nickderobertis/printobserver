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
    cut_from,
    install_path_not_narrowed,
    install_script_path,
)
from repo_checks.checks_release import publish_credentials, release_automation, release_targets
from repo_checks.expect import accepted, equal, refused, refused_naming
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


#: The base branch every copy below is laid down on, and the branch a change
#: is cut onto — the two the check reads a merge base out of.
BASE = "main"
WORK = "a-change-of-its-own"

#: The file a change writes to have a commit of its own. A branch carrying none
#: *is* the base branch, and merging the base into it fast-forwards rather than
#: merging — so a journey about what a merge does needs a change to merge into.
WORKED_ON = "the-work-this-change-is.md"


def _commit(tree: Tree, message: str) -> None:
    """Commit everything in the copy, under an identity of this suite's own."""
    run(["git", "add", "-A"], cwd=tree.root, check=True)
    run(
        [
            "git",
            "-c",
            "user.email=checks@printobserver.test",
            "-c",
            "user.name=checks",
            "commit",
            "-q",
            "-m",
            message,
        ],
        cwd=tree.root,
        check=True,
    )


def _committed(tree: Tree) -> None:
    """Make the copy a repository whose base branch carries what was copied."""
    run(["git", "init", "-q", "-b", BASE], cwd=tree.root, check=True)
    _commit(tree, "chore: the committed tree, copied")


def _head(tree: Tree) -> str:
    """Whatever commit the copy is on."""
    return run(["git", "rev-parse", "HEAD"], cwd=tree.root, check=True).stdout.strip()


def _a_commit_of_its_own(tree: Tree) -> None:
    """Commit some work on the change's own branch, which is what a change is."""
    tree.write(WORKED_ON, "Whatever this change is for.\n")
    _commit(tree, "chore: the change does some work of its own")


def _merge_the_base_into_the_change(tree: Tree) -> None:
    """Merge the base branch into the change, which is what publishing one does first.

    It is also what moves a merge base: after this the newest commit the two
    branches share is the base branch's own tip, so a check reading that reads
    the tree the base branch narrowed rather than the one this work was cut
    with.
    """
    run(
        [
            "git",
            "-c",
            "user.email=checks@printobserver.test",
            "-c",
            "user.name=checks",
            "merge",
            "--no-edit",
            "-q",
            BASE,
        ],
        cwd=tree.root,
        check=True,
    )


def _advance_the_base(tree: Tree, moved: Callable[[str], str]) -> None:
    """Move the base branch on by one commit, and come back to the change.

    What is left is the state every journey below is about: a base branch whose
    **tip** is a commit this change was never cut from. A check reading that tip
    reads whatever landed there since, which is not what this work was cut with.
    """
    run(["git", "checkout", "-q", BASE], cwd=tree.root, check=True)
    tree.write(AGENTS, moved(tree.read(AGENTS)))
    _commit(tree, "chore: the base branch moves on")
    run(["git", "checkout", "-q", WORK], cwd=tree.root, check=True)


def _cut_a_change_and_advance_the_base(tree: Tree, moved: Callable[[str], str]) -> None:
    """Cut a change from what was committed, then move the base branch on past it."""
    run(["git", "checkout", "-q", "-b", WORK], cwd=tree.root, check=True)
    _advance_the_base(tree, moved)


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


#: The platform line one of the two narrowings deletes.
DELETED_PLATFORM = (
    "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
    "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
)


def _without_the_platform(text: str) -> str:
    """The install path with one of its two platforms deleted."""
    return text.replace(DELETED_PLATFORM, "")


def _without_the_route(text: str) -> str:
    """The install path with the second of its three routes deleted."""
    return text[: text.index("#### Route 2")] + text[text.index("#### Route 3") :]


def test_a_platform_deleted_from_the_install_path_is_refused(tmp_path: Path) -> None:
    """Every artifact, matrix and route here is derived from that one list."""
    narrowed = Tree(copy_tree(tmp_path / "narrowed-platform"))
    _committed(narrowed)
    narrowed.write(AGENTS, _without_the_platform(narrowed.read(AGENTS)))

    refused(install_path_not_narrowed(narrowed.repo), "linux-aarch64")


def test_a_route_deleted_from_the_install_path_is_refused(tmp_path: Path) -> None:
    """A route deleted here is a way to the program nobody has any more."""
    narrowed = Tree(copy_tree(tmp_path / "narrowed-route"))
    _committed(narrowed)
    narrowed.write(AGENTS, _without_the_route(narrowed.read(AGENTS)))

    refused(install_path_not_narrowed(narrowed.repo), "Route 2")


def test_a_base_branch_that_deleted_a_route_first_does_not_excuse_deleting_it(
    tmp_path: Path,
) -> None:
    """What this work was cut from is a commit, and the base branch's tip is not it.

    The base branch deletes the route in a commit of its own, and then this
    change deletes it too. Read against that tip the route was already gone and
    nothing here deleted anything; read against the commit this work was cut
    from — which is what the check reads — the route is one this tree no longer
    has a way to the program by.
    """
    narrowed = Tree(copy_tree(tmp_path / "advanced-route"))
    _committed(narrowed)
    _cut_a_change_and_advance_the_base(narrowed, _without_the_route)
    narrowed.write(AGENTS, _without_the_route(narrowed.read(AGENTS)))

    refused(install_path_not_narrowed(narrowed.repo), "Route 2")


def test_a_base_branch_that_deleted_a_platform_first_does_not_excuse_deleting_it(
    tmp_path: Path,
) -> None:
    """The same, over the one list every artifact, matrix and route is derived from."""
    narrowed = Tree(copy_tree(tmp_path / "advanced-platform"))
    _committed(narrowed)
    _cut_a_change_and_advance_the_base(narrowed, _without_the_platform)
    narrowed.write(AGENTS, _without_the_platform(narrowed.read(AGENTS)))

    refused(install_path_not_narrowed(narrowed.repo), "linux-aarch64")


def test_a_route_the_base_branch_deleted_and_merged_in_is_still_refused(tmp_path: Path) -> None:
    """Publishing merges the base branch in first, and that must change nothing here.

    After the merge the newest commit the two branches share is the base
    branch's own tip — the one that deleted the route — so a check reading a
    merge base reads a tree with no route in it and finds nothing missing. The
    commit this work was cut from is not on that branch's line, and this asserts
    it is the same commit before and after the merge.
    """
    narrowed = Tree(copy_tree(tmp_path / "merged-route"))
    _committed(narrowed)
    cut = _head(narrowed)
    run(["git", "checkout", "-q", "-b", WORK], cwd=narrowed.root, check=True)
    _a_commit_of_its_own(narrowed)
    _advance_the_base(narrowed, _without_the_route)
    equal(cut_from(narrowed.repo, BASE)[0], cut, describing="the commit before the merge")

    _merge_the_base_into_the_change(narrowed)

    equal(cut_from(narrowed.repo, BASE)[0], cut, describing="the commit after the merge")
    refused(install_path_not_narrowed(narrowed.repo), "Route 2")


def test_a_platform_the_base_branch_deleted_and_merged_in_is_still_refused(
    tmp_path: Path,
) -> None:
    """The same, over the one list every artifact, matrix and route is derived from."""
    narrowed = Tree(copy_tree(tmp_path / "merged-platform"))
    _committed(narrowed)
    cut = _head(narrowed)
    run(["git", "checkout", "-q", "-b", WORK], cwd=narrowed.root, check=True)
    _a_commit_of_its_own(narrowed)
    _advance_the_base(narrowed, _without_the_platform)
    _merge_the_base_into_the_change(narrowed)

    equal(cut_from(narrowed.repo, BASE)[0], cut, describing="the commit after the merge")
    refused(install_path_not_narrowed(narrowed.repo), "linux-aarch64")


def test_the_reference_survives_a_base_branch_merged_in_more_than_once(
    tmp_path: Path,
) -> None:
    """A branch open long enough is published more than once, and merged in each time."""
    narrowed = Tree(copy_tree(tmp_path / "merged-twice"))
    _committed(narrowed)
    cut = _head(narrowed)
    run(["git", "checkout", "-q", "-b", WORK], cwd=narrowed.root, check=True)
    _a_commit_of_its_own(narrowed)
    _advance_the_base(narrowed, _without_the_route)
    _merge_the_base_into_the_change(narrowed)
    _advance_the_base(narrowed, _without_the_platform)
    _merge_the_base_into_the_change(narrowed)

    equal(cut_from(narrowed.repo, BASE)[0], cut, describing="the commit after two merges")
    findings = install_path_not_narrowed(narrowed.repo)
    refused(findings, "Route 2")
    refused(findings, "linux-aarch64")


def test_a_route_the_base_branch_gained_after_this_work_is_not_demanded_of_it(
    tmp_path: Path,
) -> None:
    """Advancing the base branch cannot move the reference the other way either.

    A route added to the base branch after this work was cut is not one this
    tree deleted, and a check reading the tip would report every such addition
    as a narrowing — which is a check nobody could keep green by working.
    """
    unchanged = Tree(copy_tree(tmp_path / "advanced-widened"))
    _committed(unchanged)
    _cut_a_change_and_advance_the_base(
        unchanged, lambda text: text.replace("#### Route 3", GAINED_ROUTE + "\n#### Route 3")
    )

    accepted(install_path_not_narrowed(unchanged.repo))
