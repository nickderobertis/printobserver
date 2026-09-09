"""The installed service, and the ingress answer bound, held to what prose states.

Every journey below drives the committed check against a real copy of the
committed tree with one defect in it, beside the tree as it stands.

The two defects that matter most are the ones nothing else here would catch. An
installer that writes a differently named unit is a tree that installs
successfully and then cannot be started by the command the install path states,
which is a failure the continuous-integration job that installs the service
finds hours later. And an installer that enables the service is one that starts
a process which can move a 3D printer as a side effect of installing a package,
which is the one thing that section says twice must never happen.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_service import ingress_answer_bound, service_install
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

INSTALLER = "scripts/install-service.sh"
AGENTS = "AGENTS.md"
SOURCE = "crates/printobserver-server/src/config.rs"


def test_the_committed_installer_is_accepted(committed: Repo) -> None:
    """The installer this repository ships is the one the install path names."""
    accepted(service_install(committed))


def test_the_committed_answer_bound_is_accepted(committed: Repo) -> None:
    """The bound this repository ships is below the timeout it records."""
    accepted(ingress_answer_bound(committed))


def test_an_installer_shipping_another_unit_name_is_refused(tree: Callable[[], Tree]) -> None:
    """A unit nothing can enable by the command the install path states."""
    broken = tree()
    broken.write(
        INSTALLER,
        broken.read(INSTALLER).replace(
            'UNIT_NAME="printobserver.service"', 'UNIT_NAME="observer.service"'
        ),
    )

    findings = service_install(broken.repo)

    refused(findings, "observer.service")


def test_an_absent_installer_is_refused(tree: Callable[[], Tree]) -> None:
    """The path the install path fetches has to be a file this repository holds."""
    broken = tree()
    broken.remove(INSTALLER)

    findings = service_install(broken.repo)

    refused(findings, "the tree holds no such file")


def test_an_installer_that_starts_the_service_is_refused(tree: Callable[[], Tree]) -> None:
    """Installing must not start a process that can move a machine."""
    broken = tree()
    broken.write(
        INSTALLER,
        broken.read(INSTALLER) + '\nsystemctl enable --now "$UNIT_NAME"\n',
    )

    findings = service_install(broken.repo)

    refused(findings, "starts nothing")


def test_an_installer_that_writes_the_unit_elsewhere_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A unit outside the directory the service manager reads is a unit nothing loads."""
    broken = tree()
    broken.write(
        INSTALLER,
        broken.read(INSTALLER).replace("/etc/systemd/system", "/opt/units"),
    )

    findings = service_install(broken.repo)

    refused(findings, "which is where the service manager reads units from")


def test_an_installer_granting_an_action_the_contracts_do_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The names the installed configuration grants are the contracts' vocabulary."""
    broken = tree()
    broken.write(
        INSTALLER,
        broken.read(INSTALLER).replace('"pause", "resume"', '"pause", "reboot"'),
    )

    findings = service_install(broken.repo)

    refused(findings, "grants `reboot`")


def test_an_installer_that_prints_the_next_command_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """Saying which command starts the service is not running it.

    The whole point of the separation is that the installer *names* the command
    an operator runs next, so a check that could not tell saying from running
    would refuse the one correct script.
    """
    printing = tree()
    printing.write(
        INSTALLER,
        printing.read(INSTALLER) + '\necho "run: systemctl enable --now $UNIT_NAME" >&2\n',
    )

    accepted(service_install(printing.repo))


def test_a_shipped_bound_at_the_producers_timeout_is_refused(tree: Callable[[], Tree]) -> None:
    """A bound Obico has already given up on is no bound at all."""
    broken = tree()
    broken.write(
        SOURCE,
        broken.read(SOURCE).replace(
            "pub const DEFAULT_INGRESS_ANSWER_BOUND_MS: u64 = 1_000;",
            "pub const DEFAULT_INGRESS_ANSWER_BOUND_MS: u64 = 5_000;",
        ),
    )

    findings = ingress_answer_bound(broken.repo)

    refused(findings, "is not below the")


def test_a_recorded_timeout_the_server_disagrees_with_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """One claim about an external producer, written twice, disagreeing with itself."""
    broken = tree()
    broken.write(
        SOURCE,
        broken.read(SOURCE).replace(
            "pub const OBICO_POSTING_TIMEOUT_MS: u64 = 5_000;",
            "pub const OBICO_POSTING_TIMEOUT_MS: u64 = 9_000;",
        ),
    )

    findings = ingress_answer_bound(broken.repo)

    refused(findings, "disagreeing with itself")


def test_a_block_recording_no_timeout_is_refused(tree: Callable[[], Tree]) -> None:
    """Nothing for the shipped default to be below."""
    broken = tree()
    broken.write(
        AGENTS,
        broken.read(AGENTS).replace("- posting timeout: `5000` ms\n", ""),
    )

    findings = ingress_answer_bound(broken.repo)

    refused(findings, "records no posting timeout")


def test_a_block_that_lost_its_provenance_is_refused(tree: Callable[[], Tree]) -> None:
    """A number with no source is a number nobody can re-read."""
    broken = tree()
    text = broken.read(AGENTS)
    start = text.index("- source: `backend/notifications")
    end = text.index("\n", start) + 1
    broken.write(AGENTS, text[:start] + text[end:])

    findings = ingress_answer_bound(broken.repo)

    refused(findings, "records no `source`")
