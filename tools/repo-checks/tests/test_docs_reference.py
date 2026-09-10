"""Each reference document is complete against the thing it inventories.

Every test here drives the committed check against a real copy of the committed
tree with one defect in it. The defects are the ways a document goes wrong that
a reader would not notice: it is gone, it does not say what it covers, it says
it covers something it does not, it is missing a section, or one of its
inventories has drifted from the thing it inventories — in either direction.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_docs import reference
from repo_checks.docs import entry_body
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

COMMANDS = "docs/reference/command-surface.md"
API = "docs/reference/api-and-clients.md"
ARCHITECTURE = "docs/reference/architecture.md"
OPERATIONS = "docs/reference/common-operations.md"
TESTING = "docs/reference/testing.md"
POLICY = "docs/reference/intervention-policy.md"


def _drop_in_entry(copy: Tree, document: str, entry: str, needle: str) -> None:
    """Drop the one line of one `###` entry that carries a fragment."""
    text = copy.read(document)
    body = entry_body(text, entry)
    dropped = "\n".join(line for line in body.splitlines() if needle not in line)
    if dropped == body:
        msg = f"{document}'s `{entry}` entry carries no {needle!r}; the fixture is stale"
        raise AssertionError(msg)
    copy.write(document, text.replace(body, dropped, 1))


def test_the_committed_documents_are_accepted(committed: Repo) -> None:
    """The documents this repository ships pass every rule below."""
    accepted(reference(committed))


def test_a_declared_document_that_is_absent_is_refused(tree: Callable[[], Tree]) -> None:
    """The skill links to it, so a reader reaches nothing."""
    copy = tree()
    copy.remove(TESTING)

    refused(reference(copy.repo), f"`{TESTING}` is absent")


def test_a_document_with_no_opening_coverage_statement_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A document that begins with its first heading never says what it is."""
    copy = tree()
    text = copy.read(POLICY)
    title, _, rest = text.partition("\n")
    copy.write(POLICY, title + "\n\n" + rest[rest.index("## ") :])

    refused(reference(copy.repo), "carries no opening coverage statement")


def test_an_opening_statement_naming_coverage_the_document_lacks_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Saying it covers a section it does not carry is a false statement."""
    copy = tree()
    copy.edit(
        COMMANDS,
        "and the commands — each with its arguments",
        "the commands, and Where the bounds come from — each with its arguments",
    )

    refused(reference(copy.repo), "says it covers `Where the bounds come from`")


def test_an_opening_statement_omitting_a_section_it_carries_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Leaving a section out of the statement is a false statement too."""
    copy = tree()
    copy.edit(COMMANDS, "the exit statuses,\nand the commands", "and the commands")

    refused(reference(copy.repo), "carries `## The exit statuses` and its opening statement")


def test_a_document_missing_a_declared_heading_is_refused(tree: Callable[[], Tree]) -> None:
    """A section the reader was promised is a failing check, not a surprise."""
    copy = tree()
    copy.edit(COMMANDS, "## The exit statuses", "## What comes back")

    refused(reference(copy.repo), "does not carry the declared heading `## The exit statuses`")


def test_an_entry_with_no_command_behind_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A command entry the program has no command for is documentation of nothing."""
    copy = tree()
    copy.append(COMMANDS, "\n### reboot\n\nTurn the printer off and on again.\n")

    refused(reference(copy.repo), "carries an entry `reboot`, and there is no such command")


def test_a_command_with_no_entry_is_refused(tree: Callable[[], Tree]) -> None:
    """A document cannot be complete by listing less than the program has."""
    copy = tree()
    copy.edit(COMMANDS, "### cancel\n", "### cancelling\n")

    refused(reference(copy.repo), "carries no entry for the command `cancel`")


def test_a_command_entry_omitting_an_argument_is_refused(tree: Callable[[], Tree]) -> None:
    """Every argument the operation declares is one the reader has to supply."""
    copy = tree()
    _drop_in_entry(copy, COMMANDS, "set-fan-percent", "`--percent`")

    refused(reference(copy.repo), "does not name the argument `--percent`")


def test_a_command_entry_omitting_its_output_is_refused(tree: Callable[[], Tree]) -> None:
    """What a command answers is half of what a reader came for."""
    copy = tree()
    _drop_in_entry(copy, COMMANDS, "status", "**Output.**")

    refused(reference(copy.repo), "`status` entry states no output")


def test_a_command_entry_omitting_its_failures_is_refused(tree: Callable[[], Tree]) -> None:
    """So is what it does when it does not work."""
    copy = tree()
    _drop_in_entry(copy, COMMANDS, "status", "**Failures.**")

    refused(reference(copy.repo), "`status` entry states no failures")


def test_an_operation_entry_with_no_operation_behind_it_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The API document is held to the operations the server serves."""
    copy = tree()
    copy.edit(
        API, "## The clients", "### reboot\n\nA route this server does not serve.\n\n## The clients"
    )

    refused(reference(copy.repo), "carries an entry `reboot`, and there is no such operation")


def test_a_client_method_the_client_does_not_export_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """One entry per method the client exports, read off the client's own sources."""
    copy = tree()
    copy.append(API, "\n#### connect\n\nA method this client does not export.\n")

    refused(reference(copy.repo), "carries an entry `connect`")


def test_an_operation_with_no_worked_example_is_refused(tree: Callable[[], Tree]) -> None:
    """A worked example a reader can follow is what this document is for."""
    copy = tree()
    copy.edit(OPERATIONS, "$ printobserver status --print-id PRINT_ID\n", "")

    refused(reference(copy.repo), "`status` entry carries no worked example")


def test_a_testing_entry_with_the_wrong_schedule_is_refused(tree: Callable[[], Tree]) -> None:
    """A schedule written in prose cannot differ from the one that fires."""
    copy = tree()
    copy.edit(TESTING, "`17 4 * * 1`", "`0 3 * * 0`")

    refused(reference(copy.repo), "does not give the schedule the committed configuration")


def test_a_testing_document_omitting_the_judged_lint_tier_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The judged tier is in no `gate.tiers`, which is how it went undocumented once."""
    copy = tree()
    text = copy.read(TESTING)
    copy.write(
        TESTING,
        text[: text.index("### lint-llm-diff")] + text[text.index("### test-integration") :],
    )

    refused(reference(copy.repo), "carries no entry for the tier `lint-llm-diff`")


def test_a_tier_declared_after_this_document_was_written_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A tier is owed an entry by being declared, rather than by a check knowing its name."""
    copy = tree()
    copy.edit(
        "repo-policy.toml",
        "\n[llmlint]\n",
        '\n[smoke]\ntier = "test-smoke"\n\n[llmlint]\n',
    )

    refused(reference(copy.repo), "carries no entry for the tier `test-smoke`")


def test_an_architecture_entry_with_no_crate_behind_it_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A crate the workspace does not declare is a crate nobody can read."""
    copy = tree()
    copy.edit(
        ARCHITECTURE,
        "## Why core names no implementation crate",
        "### printobserver-telemetry\n\nA crate this tree has not got.\n\n"
        "## Why core names no implementation crate",
    )

    refused(
        reference(copy.repo),
        "carries an entry `printobserver-telemetry`, and there is no such crate",
    )


def test_an_architecture_document_omitting_a_crate_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Every crate the workspace declares has an entry saying what it owns."""
    copy = tree()
    copy.edit(ARCHITECTURE, "### printobserver-sdk\n", "### the client crate\n")

    refused(reference(copy.repo), "carries no entry for the crate `printobserver-sdk`")


def test_an_architecture_document_the_manifests_contradict_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The dependency direction is read beside the workspace's own manifests."""
    copy = tree()
    copy.edit(
        ARCHITECTURE,
        "on `printobserver-printer-api`, `printobserver-store-api`,",
        "on `printobserver-printer-api`, `printobserver-octoprint`,",
    )

    findings = reference(copy.repo)
    refused(findings, "depends on `printobserver-octoprint`, and its manifest does not")
    refused(findings, "omits `printobserver-store-api`")


def test_a_rejection_field_the_contracts_do_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The fields a rejection carries are the contracts' to declare."""
    copy = tree()
    copy.edit(POLICY, "It carries `state`.", "It carries `state` and `printer_message`.")

    refused(reference(copy.repo), "a rejection carries `printer_message`")


def test_a_rejection_field_the_document_omits_is_refused(tree: Callable[[], Tree]) -> None:
    """And a document cannot be complete by naming fewer of them."""
    copy = tree()
    copy.edit(POLICY, "`interval_s` and `since_last_s`", "`interval_s`")

    refused(reference(copy.repo), "omits `since_last_s`")


def test_a_restoration_behaviour_naming_no_test_is_refused(tree: Callable[[], Tree]) -> None:
    """A behaviour stated against nothing is prose nobody has read."""
    copy = tree()
    copy.edit(
        POLICY,
        "`an_intervention_expires_on_the_clock_alone_and_restores_the_prior_value` in",
        "It is asserted somewhere in",
    )

    refused(reference(copy.repo), "states a behaviour and names no test asserting it")


def test_an_expiry_behaviour_naming_a_test_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Naming a test this repository does not carry is worse than naming none."""
    copy = tree()
    copy.edit(
        POLICY,
        "`an_intervention_with_no_prior_value_restores_nothing_and_says_so`",
        "`an_intervention_with_no_prior_value_writes_a_sensible_default`",
    )

    refused(reference(copy.repo), "this repository carries no such test")


def test_an_architecture_document_that_points_at_no_command_surface_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The surface it says the agent uses is the one that document inventories."""
    copy = tree()
    copy.edit(
        ARCHITECTURE,
        "[the command surface](command-surface.md) is the whole of",
        "the surface described here is the whole of",
    )

    refused(reference(copy.repo), "does not point at `docs/reference/command-surface.md`")


def test_an_entry_that_says_nothing_is_refused(tree: Callable[[], Tree]) -> None:
    """A complete inventory with empty prose beside it is a list, not a document."""
    copy = tree()
    body = entry_body(copy.read(COMMANDS), "resume")
    copy.write(COMMANDS, copy.read(COMMANDS).replace(body, "\n", 1))

    refused(reference(copy.repo), "`resume` entry says nothing")
