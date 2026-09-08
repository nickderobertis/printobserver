"""Every gate target of the two client projects fails on the defect it is meant to catch.

A target that passed over its own defect would be a no-op wearing a target's
name. Each journey assembles a copy of the project carrying that one defect and
runs the real Nx target over it.
"""

from __future__ import annotations

import json
from collections.abc import Callable

import pytest
from journey import REPO_ROOT, GateCopy

PYTHON_PROJECT = "printobserver-sdk-python"
NODE_PROJECT = "printobserver-sdk-node"
GATE_TARGETS = ("format-check", "lint", "typecheck", "test")

PY_SOURCE = "python/printobserver-sdk/src/printobserver_sdk/defect.py"
PY_TESTS = "python/printobserver-sdk/tests/test_defect.py"
TS_SOURCE = "npm/printobserver-sdk/src/defect.ts"
TS_TESTS = "npm/printobserver-sdk/test/defect.test.ts"


def _run_target(copy: GateCopy, project: str, target: str) -> tuple[int, str]:
    import os
    import subprocess

    environment = dict(os.environ)
    environment.pop("VIRTUAL_ENV", None)
    environment["UV_PROJECT_ENVIRONMENT"] = str(copy.shared_venv)
    result = subprocess.run(
        ["bunx", "nx", "run", f"{project}:{target}", "--output-style=stream", "--skip-nx-cache"],
        cwd=copy.root,
        capture_output=True,
        text=True,
        check=False,
        timeout=600,
        env=environment,
    )
    return result.returncode, result.stdout + result.stderr


@pytest.mark.parametrize("project", [PYTHON_PROJECT, NODE_PROJECT])
def test_the_project_declares_every_gate_target(project: str) -> None:
    """A formatting, lint, type-check and test target, in that project's own language."""
    roots = {
        PYTHON_PROJECT: "python/printobserver-sdk",
        NODE_PROJECT: "npm/printobserver-sdk",
    }
    declared = json.loads(
        (REPO_ROOT / roots[project] / "project.json").read_text(encoding="utf-8")
    )["targets"]

    for target in GATE_TARGETS:
        assert target in declared, target


@pytest.mark.parametrize("target", GATE_TARGETS)
@pytest.mark.parametrize("project", [PYTHON_PROJECT, NODE_PROJECT])
def test_every_gate_target_is_reached_by_the_check_recipe(project: str, target: str) -> None:
    """A target the gate never reaches is a target that gates nothing."""
    justfile = (REPO_ROOT / "justfile").read_text(encoding="utf-8")

    assert f"nx run-many -t {target}" in justfile


def test_the_committed_project_passes_every_python_target(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The clean project the defect copies are made from passes its own targets."""
    clean = gate_copy()

    for target in GATE_TARGETS:
        code, said = _run_target(clean, PYTHON_PROJECT, target)
        assert code == 0, (target, said)


def test_the_committed_project_passes_every_typescript_target(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The same, for the Node client."""
    clean = gate_copy()

    for target in GATE_TARGETS:
        code, said = _run_target(clean, NODE_PROJECT, target)
        assert code == 0, (target, said)


def test_the_python_format_target_fails_on_an_unformatted_file(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`ruff format --check` over the project, failing on the file it would rewrite."""
    broken = gate_copy()
    broken.write(PY_SOURCE, '"""A module."""\n\n\nx  =  1\n')

    code, said = _run_target(broken, PYTHON_PROJECT, "format-check")

    assert code != 0
    assert "defect.py" in said, said


def test_the_python_lint_target_fails_on_a_lint_finding(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`ruff check` over the project, failing on an unused import."""
    broken = gate_copy()
    broken.write(PY_SOURCE, '"""A module."""\n\nimport json\n')

    code, said = _run_target(broken, PYTHON_PROJECT, "lint")

    assert code != 0
    assert "F401" in said, said


def test_the_python_typecheck_target_fails_on_a_type_error(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`ty check` over the project, failing on a return that is not the declared type."""
    broken = gate_copy()
    broken.write(
        PY_SOURCE,
        '"""A module."""\n\n\ndef count() -> int:\n    """Return a count."""\n'
        '    return "not an integer"\n',
    )

    code, said = _run_target(broken, PYTHON_PROJECT, "typecheck")

    assert code != 0
    assert "invalid-return-type" in said, said


def test_the_python_test_target_fails_on_a_failing_test(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`pytest` over the project, failing on a red assertion."""
    broken = gate_copy()
    broken.write(
        PY_TESTS,
        '"""A failing journey."""\n\n\ndef test_the_target_must_fail() -> None:\n'
        '    """This assertion is meant to fail."""\n    assert 1 + 1 == 3\n',
    )

    code, said = _run_target(broken, PYTHON_PROJECT, "test")

    assert code != 0
    assert "test_the_target_must_fail" in said, said


def test_the_typescript_format_target_fails_on_an_unformatted_file(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`biome format` over the project, failing on the file it would rewrite."""
    broken = gate_copy()
    broken.write(TS_SOURCE, "export  const   defect    =        1;\n")

    code, said = _run_target(broken, NODE_PROJECT, "format-check")

    assert code != 0
    assert "defect.ts" in said, said


def test_the_typescript_lint_target_fails_on_a_lint_finding(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`biome lint` over the project, failing on an unused local."""
    broken = gate_copy()
    broken.write(
        TS_SOURCE, "export function defect(): number {\n  const unused = 1;\n  return 2;\n}\n"
    )

    code, said = _run_target(broken, NODE_PROJECT, "lint")

    assert code != 0
    assert "defect.ts" in said, said


def test_the_typescript_typecheck_target_fails_on_a_type_error(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`tsc --noEmit` over the project, failing on a mismatched type."""
    broken = gate_copy()
    broken.write(TS_SOURCE, 'export const defect: number = "not a number";\n')

    code, said = _run_target(broken, NODE_PROJECT, "typecheck")

    assert code != 0
    assert "defect.ts" in said, said


def test_the_typescript_test_target_fails_on_a_failing_test(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`bun test` over the project, failing on a red assertion."""
    broken = gate_copy()
    broken.write(
        TS_TESTS,
        'import { expect, test } from "bun:test";\n\n'
        'test("the target must fail", () => {\n  expect(1 + 1).toBe(3);\n});\n',
    )

    code, said = _run_target(broken, NODE_PROJECT, "test")

    assert code != 0
    assert "the target must fail" in said, said
