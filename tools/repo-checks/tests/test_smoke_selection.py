"""Nothing selects the real-printer smoke test, and the check that keeps it that way.

The smoke drives a machine capable of destroying itself, and a print is hours
of filament. So the check driven here is refused by a tree in which the gate,
a graph target, a continuous-integration job or a scheduled workflow could
reach it — each of those being an unattended run that starts a print nobody was
watching.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_smoke import smoke_selection
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

RECIPE = "test-printer-smoke"
SCRIPT = "tools/printer-smoke/printer_smoke.py"


def test_the_committed_tree_selects_it_from_nothing(committed: Repo) -> None:
    """As this repository stands, one recipe reaches it and nothing else does."""
    accepted(smoke_selection(committed))


def test_a_gate_tier_naming_the_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """Declaring it a tier would drive the machine on every gate run."""
    copy = tree()
    copy.edit("repo-policy.toml", '    "test-e2e",\n]', f'    "test-e2e",\n    "{RECIPE}",\n]')

    refused_naming(smoke_selection(copy.repo), "gate.tiers", RECIPE)


def test_the_check_recipe_invoking_it_is_refused(tree: Callable[[], Tree]) -> None:
    """`just check` is what every change runs, and it runs beside no printer."""
    copy = tree()
    copy.edit("justfile", "    just test-e2e\n", f"    just test-e2e\n    just {RECIPE}\n")

    refused_naming(smoke_selection(copy.repo), "the `check` recipe invokes", RECIPE)


def test_a_graph_target_running_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A target is what a fan-out tier selects, and nothing may select this one."""
    copy = tree()
    copy.edit(
        "tools/printer-smoke/project.json",
        '"command": "uv run -q pytest tools/printer-smoke/tests"',
        f'"command": "uv run -q python {SCRIPT} --run"',
    )

    refused_naming(smoke_selection(copy.repo), "printer-smoke:test", "runs the real-printer")


def test_a_workflow_running_it_is_refused(tree: Callable[[], Tree]) -> None:
    """Continuous integration runs beside no printer, on a change or on a schedule."""
    copy = tree()
    copy.edit(
        ".github/workflows/obico.yml",
        "      - run: just obico-up",
        f"      - run: just {RECIPE} --run\n      - run: just obico-up",
    )

    refused_naming(smoke_selection(copy.repo), "obico.yml", "runs the real-printer smoke test")


def test_a_recipe_that_swallows_its_arguments_is_refused(tree: Callable[[], Tree]) -> None:
    """A recipe that dropped the flag would leave one input selecting it alone."""
    copy = tree()
    copy.edit("justfile", " {{flags}}\n", "\n")

    refused_naming(smoke_selection(copy.repo), "passes no argument through")


def test_a_recipe_that_runs_something_else_is_refused(tree: Callable[[], Tree]) -> None:
    """The recipe the rules name runs the smoke, or it is not that recipe."""
    copy = tree()
    copy.edit("justfile", f"uv run -q python {SCRIPT} {{{{flags}}}}", "echo {{flags}}")

    refused_naming(smoke_selection(copy.repo), "does not run", SCRIPT)


def test_a_tree_declaring_no_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """A smoke test nothing can run is one nobody runs beside the machine."""
    copy = tree()
    copy.edit("justfile", f"{RECIPE} *flags:", "something-else *flags:")

    refused_naming(smoke_selection(copy.repo), "which the recipe set does not declare")


def test_a_tree_without_the_smoke_itself_is_refused(tree: Callable[[], Tree]) -> None:
    """A rule guarding a file nothing carries has stopped being a rule."""
    copy = tree()
    copy.remove(SCRIPT)

    refused(smoke_selection(copy.repo), "which is not there")


def test_a_smoke_reading_no_device_variable_is_refused(tree: Callable[[], Tree]) -> None:
    """One of the two inputs that select it is the variable naming the device."""
    copy = tree()
    copy.write(SCRIPT, '"""A smoke test that selects itself from a flag alone."""\n')

    refused_naming(smoke_selection(copy.repo), "PRINTOBSERVER_SMOKE_DEVICE")


def test_a_tree_declaring_no_smoke_section_is_refused(tree: Callable[[], Tree]) -> None:
    """The check reads its own anchor, and refuses a tree carrying none."""
    copy = tree()
    copy.write("repo-policy.toml", copy.read("repo-policy.toml").replace("[smoke]", "[unsmoke]"))

    refused(smoke_selection(copy.repo), "declares no `[smoke]` section")


def test_a_tree_whose_prose_says_nothing_about_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A test that drives a real machine is one a person stays next to."""
    copy = tree()
    copy.edit("AGENTS.md", "## The real-printer smoke test", "## Something else entirely")

    refused(smoke_selection(copy.repo), "carries no `The real-printer smoke test` section")


def test_prose_that_does_not_name_the_two_inputs_is_refused(tree: Callable[[], Tree]) -> None:
    """A reader who cannot see both inputs cannot run it at all."""
    copy = tree()
    copy.edit(
        "AGENTS.md",
        "PRINTOBSERVER_SMOKE_DEVICE=/dev/ttyACM0 just test-printer-smoke --run",
        "run it",
    )
    copy.write(
        "AGENTS.md",
        copy.read("AGENTS.md").replace("PRINTOBSERVER_SMOKE_DEVICE", "the device variable"),
    )

    refused_naming(smoke_selection(copy.repo), "does not name `PRINTOBSERVER_SMOKE_DEVICE`")
