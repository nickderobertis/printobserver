"""The install-path section is complete, pasteable, and the only source of itself."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks import install_path as ip
from repo_checks.checks_ci import install_path_section
from repo_checks.expect import accepted, contains, equal, refused
from repo_checks.model import Repo
from repo_checks.platforms import supported
from treecopy import Tree

FETCH = (
    "curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install.sh | sh"
)
PINNED = FETCH + " -s -- --version v0.1.0 --to ~/.local/bin"
INSTALLER = (
    "curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install-service.sh | sudo sh"
)
LAUNCHD_START = (
    "sudo launchctl bootstrap system "
    "/Library/LaunchDaemons/io.github.nickderobertis.printobserver.plist"
)
WINDOWS_INSTALLER = (
    "irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/"
    "scripts/install-service.ps1 | iex"
)
WINDOWS_ACTIVATION = "Set-Service -Name printobserver -StartupType Automatic -Status Running"
#: The systemd pair with its installer replaced by the command that starts the
#: service, which leaves a pair whose first command starts a 3D printer's
#: supervisor.
INSTALLER_REPLACED = "```console\nsudo systemctl enable --now printobserver.service\n```"
SKILL_HEADING = "\n### Between the two commands, install the agent's skill"
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


def test_the_section_states_three_routes_and_two_commands_per_manager(committed: Repo) -> None:
    """Three alternatives to the program, then two commands in order per service manager."""
    path = ip.parse(committed.agents_md)

    equal(len(path.routes), 3)
    equal(
        [route.command for route in path.routes],
        ["pip install printobserver-cli", "npm install -g printobserver-cli", FETCH],
    )
    equal(path.routes[2].commands[1], PINNED)
    equal(len(path.commands), 6)
    equal(path.commands[1], "sudo systemctl enable --now printobserver.service")
    equal(path.commands[3], LAUNCHD_START)
    equal(path.commands[5], WINDOWS_ACTIVATION)


def test_the_two_commands_are_read_through_the_platforms_own_service_manager(
    committed: Repo,
) -> None:
    """One pair per service manager, and a platform's pair is its own manager's.

    The install path targets `systemd` and `launchd` platforms, so the section
    states one pair for each — and what a consumer asks for is the pair belonging
    to a platform, not whichever pair came first. The `windows-service` pair is
    stated ahead of the routes reaching Windows — the service is delivered before
    the routes are — so a Windows platform is held to that pair and not to
    another's, whatever its `install path` answer.
    """
    path = ip.parse(committed.agents_md)

    equal(
        sorted(path.service_commands),
        ["launchd", "systemd", "windows-service"],
        describing="the managers stated",
    )
    equal(
        path.commands_for("systemd"),
        (INSTALLER, "sudo systemctl enable --now printobserver.service"),
        describing="the systemd pair, in installer-then-start order",
    )
    equal(
        path.commands_for("launchd"),
        (INSTALLER, LAUNCHD_START),
        describing="the launchd pair, in installer-then-start order",
    )
    equal(
        path.commands_for("windows-service"),
        (WINDOWS_INSTALLER, WINDOWS_ACTIVATION),
        describing="the windows-service pair, in installer-then-start order",
    )
    for platform in supported(committed):
        pair = path.commands_for(platform.service_manager)
        if platform.install_path:
            equal(len(pair), 2, describing=f"the pair {platform.id} is held to")
        equal(
            bool(pair),
            True,
            describing=f"whether {platform.id} has a pair to be held to",
        )


def test_a_manager_whose_platform_the_install_path_comes_to_target_owes_its_pair(
    tree: Callable[[], Tree],
) -> None:
    """Flipping a platform to `install path: yes` is what makes its manager's pair owed.

    A pair is permitted before the routes reach a platform of its manager and
    owed once they do. So the `windows-service` pair is taken out of the copy
    and both Windows platforms are answered `no`: with no pair and no platform
    the install path targets under that manager, nothing is owed and the
    section is accepted — and flipping one of those platforms back is what
    changes that.
    """
    broken = tree()
    text = broken.read("AGENTS.md")
    pair_start = text.index("\n#### windows-service\n")
    pair_end = text.index("\n### Between the two commands", pair_start)
    broken.edit("AGENTS.md", text[pair_start:pair_end], "")
    for platform in ("windows-x86_64", "windows-aarch64"):
        text = broken.read("AGENTS.md")
        start = text.index(f"- `{platform}` — ")
        end = text.index("\n", start)
        entry = text[start:end]
        broken.edit(
            "AGENTS.md",
            entry,
            entry[: entry.index("install path: yes")] + "install path: no — the routes are owed",
        )
    accepted(install_path_section(broken.repo))

    text = broken.read("AGENTS.md")
    start = text.index("- `windows-x86_64` — ")
    end = text.index("\n", start)
    entry = text[start:end]
    broken.edit("AGENTS.md", entry, entry[: entry.index("install path: no")] + "install path: yes")

    findings = install_path_section(broken.repo)

    refused(findings, "states no pair of commands for the `windows-service` service manager")


def test_a_service_manager_the_list_names_and_the_section_states_no_pair_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A platform whose pair is not stated is a platform nobody can put the service on."""
    broken = tree()
    broken.edit("AGENTS.md", "\n#### systemd\n", "\n#### openrc\n")

    findings = install_path_section(broken.repo)

    refused(findings, "states no pair of commands for the `systemd` service manager")
    refused(findings, "states a pair of commands for the `openrc` service manager")


def test_a_pair_for_a_service_manager_the_list_does_not_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A pair nothing runs is two commands nobody is held to."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        SKILL_HEADING,
        "\n#### openrc\n\n```console\nsudo rc-update add printobserver default\n```\n\n"
        "```console\nsudo rc-service printobserver start\n```\n" + SKILL_HEADING,
    )

    findings = install_path_section(broken.repo)

    refused(findings, "states a pair of commands for the `openrc` service manager")


def test_a_pair_whose_first_command_starts_the_service_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Installing must not start a process that can move a printer, in any pair."""
    broken = tree()
    broken.edit("AGENTS.md", f"```console\n{INSTALLER}\n```", INSTALLER_REPLACED)

    findings = install_path_section(broken.repo)

    refused(findings, "starts or enables the service")


def test_a_launchd_pair_whose_first_command_loads_the_service_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Loading a property list starts what it defines, so it may not come first either."""
    broken = tree()
    text = broken.read("AGENTS.md")
    launchd = text.index("\n#### launchd\n")
    installer = text.index(f"```console\n{INSTALLER}\n```", launchd)
    broken.write(
        "AGENTS.md",
        text[:installer]
        + f"```console\n{LAUNCHD_START}\n```"
        + text[installer + len(f"```console\n{INSTALLER}\n```") :],
    )

    findings = install_path_section(broken.repo)

    refused(findings, "the first command of the `launchd` pair")


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

    refused(findings, "states 3 commands for the `systemd` service manager")


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


SIGN_IN = "sudo -u printobserver /usr/local/lib/printobserver/printobserver sign-in"
WINDOWS_SIGN_IN = "& 'C:\\Program Files\\printobserver\\printobserver.exe' sign-in"
SKILL_INSTALL = (
    "sudo gh skill install nickderobertis/printobserver printobserver "
    "--dir /var/lib/printobserver/skills"
)
WINDOWS_SKILL_INSTALL = (
    "gh skill install nickderobertis/printobserver printobserver "
    "--dir C:\\ProgramData\\printobserver\\state\\skills"
)


def test_the_section_states_how_the_harness_is_signed_in(committed: Repo) -> None:
    """The skill install, the harness installs and the service-user sign-in are one source."""
    path = ip.parse(committed.agents_md)

    equal(
        path.sign_in,
        (
            SKILL_INSTALL,
            WINDOWS_SKILL_INSTALL,
            "sudo npm install -g @anthropic-ai/claude-code",
            "sudo npm install -g @openai/codex",
            SIGN_IN,
            WINDOWS_SIGN_IN,
        ),
    )
    for command in path.sign_in:
        contains(path.canonical, command, describing="every command the section states")


def test_a_section_stating_no_sign_in_is_refused(tree: Callable[[], Tree]) -> None:
    """A path that installs the service and never signs its harness in supervises nothing."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("### Between the two commands, sign in the agent's harness")
    end = text.index("## The registry install-path proof")
    broken.write("AGENTS.md", text[:start] + text[end:])

    findings = install_path_section(broken.repo)

    refused(findings, "states no `printobserver sign-in`")


def test_a_section_stating_no_skill_install_is_refused(tree: Callable[[], Tree]) -> None:
    """The program carries no skill, so a path that never installs one supervises nothing."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index(SKILL_HEADING)
    end = text.index("\n### Between the two commands, sign in the agent's harness")
    broken.write("AGENTS.md", text[:start] + text[end:])

    findings = install_path_section(broken.repo)

    refused(findings, "states no `gh skill install nickderobertis/printobserver printobserver")


def test_a_skill_install_restatement_that_differs_is_refused(tree: Callable[[], Tree]) -> None:
    """The README's skill install is derived from the section like every other command."""
    broken = tree()
    broken.edit(
        "README.md",
        SKILL_INSTALL,
        SKILL_INSTALL.replace("/var/lib/printobserver/skills", "/root/.agents/skills"),
    )

    findings = install_path_section(broken.repo)

    refused(findings, "README.md states")


def test_telling_an_operator_to_sign_in_to_github_is_refused(tree: Callable[[], Tree]) -> None:
    """The skill is read from a public repository: no GitHub credential belongs on the host."""
    for place in ("AGENTS.md", "README.md"):
        broken = tree()
        broken.edit(
            place,
            "Its one prerequisite is GitHub CLI 2.100.0 or later"
            if place == "AGENTS.md"
            else "It needs GitHub CLI 2.100.0 or later",
            "First run gh auth login. Its one prerequisite is GitHub CLI 2.100.0 or later"
            if place == "AGENTS.md"
            else "First run gh auth login. It needs GitHub CLI 2.100.0 or later",
        )

        findings = install_path_section(broken.repo)

        refused(findings, "tells an operator to run `gh auth login`")


def test_a_sign_in_restatement_that_differs_is_refused(tree: Callable[[], Tree]) -> None:
    """The README's sign-in is derived from the section like every other command."""
    broken = tree()
    broken.edit("README.md", SIGN_IN, SIGN_IN.replace("-u printobserver", "-u root"))

    findings = install_path_section(broken.repo)

    refused(findings, "README.md states")


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


def test_a_second_pair_for_one_service_manager_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The second replaces the first, leaving a pair nobody wrote as the one that is read."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        SKILL_HEADING,
        "\n#### systemd\n\n```console\ncurl -fsSL https://example.invalid/other.sh | sudo sh\n"
        "```\n\n```console\nsudo systemctl enable --now printobserver.service\n```\n"
        + SKILL_HEADING,
    )

    findings = install_path_section(broken.repo)

    refused(findings, "states more than one pair of commands for the `systemd` service manager")
