"""The readers every check is built on, driven over the shapes they have to survive."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

import pytest
from repo_checks.expect import contains, equal
from repo_checks.parsing import (
    MarkerBlockMissingError,
    jobs_of,
    load_workflow,
    marker_block,
    programs_in,
    recipes,
    run_commands,
    section,
    steps_of,
)


def test_a_missing_marker_block_is_an_error_rather_than_an_empty_list() -> None:
    """A block a check needs and cannot find is a refusal, not silence."""
    with pytest.raises(MarkerBlockMissingError):
        marker_block("# nothing here\n", "supported-platforms")


def test_a_missing_section_reads_as_empty() -> None:
    """A section that is not there yields nothing to compare against."""
    equal(section("# a document\n", "Not A Section"), "")


def test_environment_assignments_are_not_programs() -> None:
    """`FOO=bar cmd` invokes `cmd`."""
    equal(programs_in("RUST_LOG=debug cargo test"), ["cargo"])


def test_a_line_of_only_assignments_invokes_nothing() -> None:
    """An assignment on its own runs no program."""
    equal(programs_in("FOO=bar"), [])


def test_each_side_of_a_pipe_is_a_program() -> None:
    """A fetch piped into a shell runs two programs, and both are named."""
    equal(programs_in("curl -fsSL https://example.test/x.sh | sh"), ["curl", "sh"])


def test_shell_keywords_are_not_programs() -> None:
    """`cd` is not something an allowlist has anything to say about."""
    equal(programs_in("cd /tmp"), [])


def test_a_recipe_carries_its_dependencies_and_its_body() -> None:
    """Both halves of a recipe are readable, because both can carry a tier."""
    parsed = recipes("check: lint\n    just test\n\nlint:\n    cargo clippy\n")

    equal(parsed["check"].dependencies, ("lint",))
    equal(parsed["check"].body, ("just test",))
    equal(parsed["lint"].body, ("cargo clippy",))


def test_a_workflow_that_is_not_a_mapping_reads_as_empty(tmp_path: Path) -> None:
    """A file that parses but is not a workflow declares no jobs."""
    path = tmp_path / "not-a-workflow.yml"
    path.write_text("- just a list\n", encoding="utf-8")

    equal(load_workflow(path), {})


def test_yamls_on_is_true_surprise_is_normalized(tmp_path: Path) -> None:
    """`on:` parses as the boolean `True`, which every trigger check would miss."""
    path = tmp_path / "workflow.yml"
    path.write_text("on:\n  push:\n    branches: [main]\njobs: {}\n", encoding="utf-8")

    contains(load_workflow(path)["on"], "push")


def test_a_workflow_without_jobs_declares_none() -> None:
    """A malformed `jobs:` is no jobs rather than a crash."""
    equal(jobs_of({"jobs": "not a mapping"}), {})


def test_a_job_without_steps_runs_nothing() -> None:
    """A malformed `steps:` is no steps rather than a crash."""
    equal(steps_of({"steps": "not a list"}), [])
    equal(run_commands({"steps": [{"uses": "actions/checkout@v5"}]}), [])


def test_a_multi_line_run_step_is_read_line_by_line() -> None:
    """A block scalar carries more than one command and each one counts."""
    equal(
        run_commands({"steps": [{"run": "just bootstrap\njust check\n"}]}),
        ["just bootstrap", "just check"],
    )


def test_a_recipe_taking_a_parameter_is_still_a_recipe() -> None:
    """A reader that passed over one would leave a recipe no check could see."""
    parsed = recipes("run *flags:\n    uv run -q python thing.py {{flags}}\n")

    contains(parsed, "run", describing="the parsed recipes")
    equal(parsed["run"].body, ("uv run -q python thing.py {{flags}}",))
    equal(parsed["run"].dependencies, ())


def test_a_setting_is_not_read_as_a_recipe() -> None:
    """`set` and `export` lines carry a colon and declare no recipe."""
    parsed = recipes('set shell := ["bash", "-c"]\nexport PYTHONPATH := "src"\n')

    equal(sorted(parsed), [], describing="the recipes a settings-only justfile declares")
