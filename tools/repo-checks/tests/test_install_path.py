"""The install-path section is complete, pasteable, and the only source of itself."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks import install_path as ip
from repo_checks.checks_ci import install_path_section
from repo_checks.expect import accepted, contains, equal, refused
from repo_checks.model import Repo
from treecopy import Tree

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
    accepted(install_path_section(committed))


def test_the_section_states_three_routes_and_two_commands(committed: Repo) -> None:
    """Three alternatives to the program, then two commands in order."""
    path = ip.parse(committed.agents_md)

    equal(len(path.routes), 3)
    equal(
        [route.command for route in path.routes],
        ["pip install printobserver-cli", "npm install -g printobserver-cli", FETCH],
    )
    equal(path.routes[2].commands[1], PINNED)
    equal(len(path.commands), 2)
    equal(path.commands[1], "sudo systemctl enable --now printobserver.service")


def test_a_fourth_route_is_refused(tree: Callable[[], Tree]) -> None:
    """The section states exactly three alternatives."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "### Then, in order — two commands",
        FOURTH_ROUTE + "\n### Then, in order — two commands",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "states 4 routes")


def test_a_missing_route_is_refused(tree: Callable[[], Tree]) -> None:
    """A section that states two routes states a path this repository does not offer."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("#### Route 2 — the JavaScript package registry")
    end = text.index("#### Route 3 — the bundled install script")
    broken.write("AGENTS.md", text[:start] + text[end:])

    findings = install_path_section(broken.repo)

    refused(findings, "states 2 routes")


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

    refused(findings, "states 3 commands")


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

    refused(findings, "does not say the routes are")
    refused(findings, "presents the routes as a sequence")


def test_an_elided_fetch_command_is_refused(tree: Callable[[], Tree]) -> None:
    """A route a reader has to reconstruct is not a route they can take."""
    broken = tree()
    broken.edit("AGENTS.md", FETCH + "\n```", "curl -fsSL https://.../install.sh | sh\n```")

    findings = install_path_section(broken.repo)

    refused(findings, "where a literal value belongs")


def test_a_fetch_url_naming_another_file_is_refused(tree: Callable[[], Tree]) -> None:
    """The URL a reader is given and the file it fetches cannot drift apart."""
    broken = tree()
    broken.edit("AGENTS.md", FETCH + "\n```", FETCH.replace("install.sh", "setup.sh") + "\n```")

    findings = install_path_section(broken.repo)

    refused(findings, "is not a path this section declares")


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

    refused(findings, "which does not name")


def test_a_fetch_url_naming_another_branch_is_refused(tree: Callable[[], Tree]) -> None:
    """A branch other than this repository's base branch is a URL that can vanish."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        FETCH + "\n```",
        FETCH.replace("/printobserver/main/", "/printobserver/develop/") + "\n```",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "which does not name")


def test_a_metavariable_release_tag_is_refused(tree: Callable[[], Tree]) -> None:
    """The pinned form is a line a reader can paste, not a template to fill in."""
    broken = tree()
    broken.edit(
        "AGENTS.md", "--version v0.1.0 --to ~/.local/bin", "--version vX.Y.Z --to ~/.local/bin"
    )

    findings = install_path_section(broken.repo)

    refused(findings, "no concrete release tag")


def test_a_metavariable_install_directory_is_refused(tree: Callable[[], Tree]) -> None:
    """So is the directory."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "--version v0.1.0 --to ~/.local/bin",
        "--version v0.1.0 --to DIR",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "no concrete install directory")


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

    refused(findings, "starts or enables the service")


def test_dropping_the_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A later change must not helpfully fold the two steps back together."""
    broken = tree()
    broken.write("AGENTS.md", broken.read("AGENTS.md").replace("commands a 3D printer", "runs"))

    findings = install_path_section(broken.repo)

    refused(findings, "why enabling and")


def test_a_restatement_that_differs_is_refused(tree: Callable[[], Tree]) -> None:
    """Every other statement of one of the five is derived from the section."""
    broken = tree()
    broken.edit(
        "README.md",
        "pip install printobserver-cli",
        "pip install printobserver",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "README.md states")


def test_the_section_states_the_check_on_what_a_route_installed(committed: Repo) -> None:
    """One command, which runs the program and asks which version it is."""
    path = ip.parse(committed.agents_md)

    equal(len(path.verification), 1)
    equal(path.checked, "printobserver --version")
    contains(path.canonical, path.checked, describing="every command the section states")


def test_a_section_stating_no_check_on_what_was_installed_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A path that ends at an install cannot tell a working one from a broken one."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "```console\nprintobserver --version\n```",
        "You will know it worked because it worked.",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "it must state exactly one")


def test_a_check_that_does_not_run_the_program_is_refused(tree: Callable[[], Tree]) -> None:
    """The check has to run the program the route installed, not something beside it."""
    broken = tree()
    broken.edit("AGENTS.md", "printobserver --version\n```", "systemctl --version\n```")

    findings = install_path_section(broken.repo)

    refused(findings, "which is the program every route installs")


def test_a_check_that_does_not_ask_which_version_it_is_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Running the program is not enough: which version ran is what a proof reads."""
    broken = tree()
    broken.edit("AGENTS.md", "printobserver --version\n```", "printobserver --help\n```")

    findings = install_path_section(broken.repo)

    refused(findings, "does not ask the program which version it is")


def test_a_check_carrying_a_placeholder_is_refused(tree: Callable[[], Tree]) -> None:
    """Every command this section states is one a reader pastes."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "printobserver --version\n```",
        "printobserver --version | grep ${EXPECTED}\n```",
    )

    findings = install_path_section(broken.repo)

    refused(findings, "where a literal value belongs")
