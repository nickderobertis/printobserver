"""The install-path section is complete, pasteable, and the only source of itself."""

from __future__ import annotations

from collections.abc import Callable

from conftest import Tree
from repo_checks import install_path as ip
from repo_checks.checks_ci import install_path_section
from repo_checks.model import Repo

FETCH = (
    "curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install.sh | sh"
)
PINNED = FETCH + " -s -- --version v0.1.0 --to ~/.local/bin"
FOURTH_ROUTE = """
#### Route 4 — a distribution channel nobody built

Made executable by nobody.

```console
brew install printobserver
```
"""


def test_the_committed_section_is_accepted(committed: Repo) -> None:
    """The section this repository ships is complete and agrees with every restatement."""
    assert install_path_section(committed) == []


def test_the_section_states_three_routes_and_two_commands(committed: Repo) -> None:
    """Three alternatives to the program, then two commands in order."""
    path = ip.parse(committed.agents_md)

    assert len(path.routes) == 3
    assert [route.command for route in path.routes] == [
        "pip install printobserver-cli",
        "npm install -g printobserver-cli",
        FETCH,
    ]
    assert path.routes[2].commands[1] == PINNED
    assert len(path.commands) == 2
    assert path.commands[1] == "sudo systemctl enable --now printobserver.service"


def test_a_fourth_route_is_refused(tree: Callable[[], Tree]) -> None:
    """The section states exactly three alternatives."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "### Then, in order — two commands",
        FOURTH_ROUTE + "\n### Then, in order — two commands",
    )

    findings = install_path_section(broken.repo)

    assert any("states 4 routes" in finding for finding in findings), findings


def test_a_missing_route_is_refused(tree: Callable[[], Tree]) -> None:
    """A section that states two routes states a path this repository does not offer."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("#### Route 2 — the JavaScript package registry")
    end = text.index("#### Route 3 — the bundled install script")
    broken.write("AGENTS.md", text[:start] + text[end:])

    findings = install_path_section(broken.repo)

    assert any("states 2 routes" in finding for finding in findings), findings


def test_a_third_command_after_the_routes_is_refused(tree: Callable[[], Tree]) -> None:
    """Exactly two commands follow the routes, in order."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "```console\nsudo systemctl enable --now printobserver.service\n```",
        "```console\nsudo systemctl enable --now printobserver.service\n```\n\n"
        "```console\nsudo systemctl status printobserver.service\n```",
    )

    findings = install_path_section(broken.repo)

    assert any("states 3 commands" in finding for finding in findings), findings


def test_presenting_the_routes_as_a_sequence_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A reader takes one route, not all three in turn."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "These three routes are alternatives reaching the same program: take one of them,\n"
        "not all three.",
        "Take these three steps in sequence.",
    )

    findings = install_path_section(broken.repo)

    assert any("does not say the routes are" in finding for finding in findings), findings
    assert any("presents the routes as a sequence" in finding for finding in findings), findings


def test_an_elided_fetch_command_is_refused(tree: Callable[[], Tree]) -> None:
    """A route a reader has to reconstruct is not a route they can take."""
    broken = tree()
    broken.edit("AGENTS.md", FETCH + "\n```", "curl -fsSL https://.../install.sh | sh\n```")

    findings = install_path_section(broken.repo)

    assert any("where a literal value belongs" in finding for finding in findings), findings


def test_a_fetch_url_naming_another_file_is_refused(tree: Callable[[], Tree]) -> None:
    """The URL a reader is given and the file it fetches cannot drift apart."""
    broken = tree()
    broken.edit("AGENTS.md", FETCH + "\n```", FETCH.replace("install.sh", "setup.sh") + "\n```")

    findings = install_path_section(broken.repo)

    assert any("is not a path this section declares" in finding for finding in findings), findings


def test_a_fetch_url_naming_another_repository_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A URL that points somewhere else installs somebody else's program."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        FETCH + "\n```",
        FETCH.replace("nickderobertis/printobserver", "someone/else") + "\n```",
    )

    findings = install_path_section(broken.repo)

    assert any("which does not name" in finding for finding in findings), findings


def test_a_fetch_url_naming_another_branch_is_refused(tree: Callable[[], Tree]) -> None:
    """A branch other than this repository's base branch is a URL that can vanish."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        FETCH + "\n```",
        FETCH.replace("/printobserver/main/", "/printobserver/develop/") + "\n```",
    )

    findings = install_path_section(broken.repo)

    assert any("which does not name" in finding for finding in findings), findings


def test_a_metavariable_release_tag_is_refused(tree: Callable[[], Tree]) -> None:
    """The pinned form is a line a reader can paste, not a template to fill in."""
    broken = tree()
    broken.edit(
        "AGENTS.md", "--version v0.1.0 --to ~/.local/bin", "--version vX.Y.Z --to ~/.local/bin"
    )

    findings = install_path_section(broken.repo)

    assert any("no concrete release tag" in finding for finding in findings), findings


def test_a_metavariable_install_directory_is_refused(tree: Callable[[], Tree]) -> None:
    """So is the directory."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "--version v0.1.0 --to ~/.local/bin",
        "--version v0.1.0 --to DIR",
    )

    findings = install_path_section(broken.repo)

    assert any("no concrete install directory" in finding for finding in findings), findings


def test_a_first_command_that_starts_the_service_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Installing must not start a process that can move a 3D printer."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "```console\ncurl -fsSL https://raw.githubusercontent.com/nickderobertis/"
        "printobserver/main/scripts/install-service.sh | sudo sh\n```",
        "```console\nsudo systemctl enable --now printobserver.service\n```",
    )

    findings = install_path_section(broken.repo)

    assert any("starts or enables the service" in finding for finding in findings), findings


def test_dropping_the_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A later change must not helpfully fold the two steps back together."""
    broken = tree()
    broken.write("AGENTS.md", broken.read("AGENTS.md").replace("commands a 3D printer", "runs"))

    findings = install_path_section(broken.repo)

    assert any("why enabling and" in finding for finding in findings), findings


def test_a_restatement_that_differs_is_refused(tree: Callable[[], Tree]) -> None:
    """Every other statement of one of the five is derived from the section."""
    broken = tree()
    broken.edit(
        "README.md",
        "pip install printobserver-cli",
        "pip install printobserver",
    )

    findings = install_path_section(broken.repo)

    assert any("README.md states" in finding for finding in findings), findings
