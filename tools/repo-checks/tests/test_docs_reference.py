"""Each reference document is complete against the thing it inventories.

The positive half drives the committed check over the committed tree. Each
negative hands one of the check's own rules a document's text with one defect in
it — the ways a document goes wrong that a reader would not notice: it does not
say what it covers, it says it covers something it does not, it is missing a
section, or one of its inventories has drifted from the thing it inventories, in
either direction. The tree they are read beside is this repository's own.
"""

from __future__ import annotations

from repo_checks.checks_docs import (
    Surface,
    _api_and_clients,
    _architecture,
    _command_surface,
    _common_operations,
    _document_frame,
    _intervention_policy,
    _read_surface,
    _testing,
    reference,
)
from repo_checks.docs import Document, docs_policy, entry_body
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo

COMMANDS = "docs/reference/command-surface.md"
API = "docs/reference/api-and-clients.md"
ARCHITECTURE = "docs/reference/architecture.md"
OPERATIONS = "docs/reference/common-operations.md"
TESTING = "docs/reference/testing.md"
POLICY = "docs/reference/intervention-policy.md"


def _surface(committed: Repo) -> Surface:
    """The generated command surface every inventory is read beside."""
    surface = _read_surface(committed, docs_policy(committed))
    if isinstance(surface, str):
        raise AssertionError(surface)
    return surface


def _declared(committed: Repo, path: str) -> Document:
    """One declared document, with the headings this repository declares for it."""
    for document in docs_policy(committed).documents:
        if document.path == path:
            return document
    msg = f"`{path}` is not a declared reference document"
    raise AssertionError(msg)


def _headings(committed: Repo) -> set[str]:
    """Every heading any declared document owes."""
    return {
        heading for document in docs_policy(committed).documents for heading in document.headings
    }


def _edited(committed: Repo, path: str, old: str, new: str) -> str:
    """One document's text with one exact fragment replaced, refusing a no-op."""
    text = committed.read(path)
    if old not in text:
        msg = f"`{path}` does not contain {old!r}; the fixture is stale"
        raise AssertionError(msg)
    return text.replace(old, new, 1)


def _without_in_entry(committed: Repo, path: str, entry: str, needle: str) -> str:
    """One document's text with the one line of one `###` entry carrying a fragment gone."""
    text = committed.read(path)
    body = entry_body(text, entry)
    dropped = "\n".join(line for line in body.splitlines() if needle not in line)
    if dropped == body:
        msg = f"{path}'s `{entry}` entry carries no {needle!r}; the fixture is stale"
        raise AssertionError(msg)
    return text.replace(body, dropped, 1)


def test_the_committed_documents_are_accepted(committed: Repo) -> None:
    """The documents this repository ships pass every rule below."""
    accepted(reference(committed))


def test_a_document_with_no_opening_coverage_statement_is_refused(committed: Repo) -> None:
    """A document that begins with its first heading never says what it is."""
    text = committed.read(POLICY)
    title, _, rest = text.partition("\n")
    opened = title + "\n\n" + rest[rest.index("## ") :]

    refused(
        _document_frame(_declared(committed, POLICY), opened, _headings(committed)),
        "carries no opening coverage statement",
    )


def test_an_opening_statement_naming_coverage_the_document_lacks_is_refused(
    committed: Repo,
) -> None:
    """Saying it covers a section it does not carry is a false statement."""
    text = _edited(
        committed,
        COMMANDS,
        "and the commands — each with its arguments",
        "the commands, and Where the bounds come from — each with its arguments",
    )

    refused(
        _document_frame(_declared(committed, COMMANDS), text, _headings(committed)),
        "says it covers `Where the bounds come from`",
    )


def test_an_opening_statement_omitting_a_section_it_carries_is_refused(committed: Repo) -> None:
    """Leaving a section out of the statement is a false statement too."""
    text = _edited(committed, COMMANDS, "the exit statuses,\nand the commands", "and the commands")

    refused(
        _document_frame(_declared(committed, COMMANDS), text, _headings(committed)),
        "carries `## The exit statuses` and its opening statement",
    )


def test_a_document_missing_a_declared_heading_is_refused(committed: Repo) -> None:
    """A section the reader was promised is a failing check, not a surprise."""
    text = _edited(committed, COMMANDS, "## The exit statuses", "## What comes back")

    refused(
        _document_frame(_declared(committed, COMMANDS), text, _headings(committed)),
        "does not carry the declared heading `## The exit statuses`",
    )


def test_an_entry_that_says_nothing_is_refused(committed: Repo) -> None:
    """A complete inventory with empty prose beside it is a list, not a document."""
    text = committed.read(COMMANDS)
    emptied = text.replace(entry_body(text, "resume"), "\n", 1)

    refused(
        _document_frame(_declared(committed, COMMANDS), emptied, _headings(committed)),
        "`resume` entry says nothing",
    )


def test_an_entry_with_no_command_behind_it_is_refused(committed: Repo) -> None:
    """A command entry the program has no command for is documentation of nothing."""
    text = f"{committed.read(COMMANDS)}\n### reboot\n\nTurn the printer off and on again.\n"

    refused(
        _command_surface(committed, COMMANDS, text, _surface(committed)),
        "carries an entry `reboot`, and there is no such command",
    )


def test_a_command_with_no_entry_is_refused(committed: Repo) -> None:
    """A document cannot be complete by listing less than the program has."""
    text = _edited(committed, COMMANDS, "### cancel\n", "### cancelling\n")

    refused(
        _command_surface(committed, COMMANDS, text, _surface(committed)),
        "carries no entry for the command `cancel`",
    )


def test_a_command_entry_omitting_an_argument_is_refused(committed: Repo) -> None:
    """Every argument the operation declares is one the reader has to supply."""
    text = _without_in_entry(committed, COMMANDS, "set-fan-percent", "`--percent`")

    refused(
        _command_surface(committed, COMMANDS, text, _surface(committed)),
        "does not name the argument `--percent`",
    )


def test_a_command_entry_omitting_its_output_is_refused(committed: Repo) -> None:
    """What a command answers is half of what a reader came for."""
    text = _without_in_entry(committed, COMMANDS, "status", "**Output.**")

    refused(
        _command_surface(committed, COMMANDS, text, _surface(committed)),
        "`status` entry states no output",
    )


def test_a_command_entry_omitting_its_failures_is_refused(committed: Repo) -> None:
    """So is what it does when it does not work."""
    text = _without_in_entry(committed, COMMANDS, "status", "**Failures.**")

    refused(
        _command_surface(committed, COMMANDS, text, _surface(committed)),
        "`status` entry states no failures",
    )


def test_an_operation_entry_with_no_operation_behind_it_is_refused(committed: Repo) -> None:
    """The API document is held to the operations the server serves."""
    text = _edited(
        committed,
        API,
        "## The clients",
        "### reboot\n\nA route this server does not serve.\n\n## The clients",
    )

    refused(
        _api_and_clients(committed, API, text, _surface(committed)),
        "carries an entry `reboot`, and there is no such operation",
    )


def test_a_client_method_the_client_does_not_export_is_refused(committed: Repo) -> None:
    """One entry per method the client exports, read off the client's own sources."""
    text = f"{committed.read(API)}\n#### connect\n\nA method this client does not export.\n"

    refused(
        _api_and_clients(committed, API, text, _surface(committed)),
        "carries an entry `connect`",
    )


def test_an_operation_with_no_worked_example_is_refused(committed: Repo) -> None:
    """A worked example a reader can follow is what this document is for."""
    text = _edited(committed, OPERATIONS, "$ printobserver status --print-id PRINT_ID\n", "")

    refused(
        _common_operations(committed, OPERATIONS, text, _surface(committed)),
        "`status` entry carries no worked example",
    )


def test_a_testing_entry_with_the_wrong_schedule_is_refused(committed: Repo) -> None:
    """A schedule written in prose cannot differ from the one that fires."""
    text = _edited(committed, TESTING, "`17 4 * * 1`", "`0 3 * * 0`")

    refused(
        _testing(committed, TESTING, text, _surface(committed)),
        "does not give the schedule the committed configuration",
    )


def test_a_testing_document_omitting_the_judged_lint_tier_is_refused(committed: Repo) -> None:
    """The judged tier is in no `gate.tiers`, which is how it went undocumented once."""
    text = committed.read(TESTING)
    without = text[: text.index("### lint-llm-diff")] + text[text.index("### test-integration") :]

    refused(
        _testing(committed, TESTING, without, _surface(committed)),
        "carries no entry for the tier `lint-llm-diff`",
    )


def test_an_architecture_entry_with_no_crate_behind_it_is_refused(committed: Repo) -> None:
    """A crate the workspace does not declare is a crate nobody can read."""
    text = _edited(
        committed,
        ARCHITECTURE,
        "## Why core names no implementation crate",
        "### printobserver-telemetry\n\nA crate this tree has not got.\n\n"
        "## Why core names no implementation crate",
    )

    refused(
        _architecture(committed, ARCHITECTURE, text, _surface(committed)),
        "carries an entry `printobserver-telemetry`, and there is no such crate",
    )


def test_an_architecture_document_omitting_a_crate_is_refused(committed: Repo) -> None:
    """Every crate the workspace declares has an entry saying what it owns."""
    text = _edited(committed, ARCHITECTURE, "### printobserver-sdk\n", "### the client crate\n")

    refused(
        _architecture(committed, ARCHITECTURE, text, _surface(committed)),
        "carries no entry for the crate `printobserver-sdk`",
    )


def test_an_architecture_document_the_manifests_contradict_is_refused(committed: Repo) -> None:
    """The dependency direction is read beside the workspace's own manifests."""
    text = _edited(
        committed,
        ARCHITECTURE,
        "on `printobserver-printer-api`, `printobserver-store-api`,",
        "on `printobserver-printer-api`, `printobserver-octoprint`,",
    )

    findings = _architecture(committed, ARCHITECTURE, text, _surface(committed))
    refused(findings, "depends on `printobserver-octoprint`, and its manifest does not")
    refused(findings, "omits `printobserver-store-api`")


def test_an_architecture_document_that_points_at_no_command_surface_is_refused(
    committed: Repo,
) -> None:
    """The surface it says the agent uses is the one that document inventories."""
    text = _edited(
        committed,
        ARCHITECTURE,
        "[the command surface](command-surface.md) is the whole of",
        "the surface described here is the whole of",
    )

    refused(
        _architecture(committed, ARCHITECTURE, text, _surface(committed)),
        "does not point at `docs/reference/command-surface.md`",
    )


def test_a_rejection_field_the_contracts_do_not_declare_is_refused(committed: Repo) -> None:
    """The fields a rejection carries are the contracts' to declare."""
    text = _edited(
        committed, POLICY, "It carries `state`.", "It carries `state` and `printer_message`."
    )

    refused(
        _intervention_policy(committed, POLICY, text, _surface(committed)),
        "a rejection carries `printer_message`",
    )


def test_a_rejection_field_the_document_omits_is_refused(committed: Repo) -> None:
    """And a document cannot be complete by naming fewer of them."""
    text = _edited(committed, POLICY, "`interval_s` and `since_last_s`", "`interval_s`")

    refused(
        _intervention_policy(committed, POLICY, text, _surface(committed)),
        "omits `since_last_s`",
    )


def test_a_restoration_behaviour_naming_no_test_is_refused(committed: Repo) -> None:
    """A behaviour stated against nothing is prose nobody has read."""
    text = _edited(
        committed,
        POLICY,
        "`an_intervention_expires_on_the_clock_alone_and_restores_the_prior_value` in",
        "It is asserted somewhere in",
    )

    refused(
        _intervention_policy(committed, POLICY, text, _surface(committed)),
        "states a behaviour and names no test asserting it",
    )


def test_an_expiry_behaviour_naming_a_test_that_is_not_there_is_refused(committed: Repo) -> None:
    """Naming a test this repository does not carry is worse than naming none."""
    text = _edited(
        committed,
        POLICY,
        "`an_intervention_with_no_prior_value_restores_nothing_and_says_so`",
        "`an_intervention_with_no_prior_value_writes_a_sensible_default`",
    )

    refused(
        _intervention_policy(committed, POLICY, text, _surface(committed)),
        "this repository carries no such test",
    )
