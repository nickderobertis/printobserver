"""The end-to-end tier: journeys that drive this repository's own gate.

Each journey copies the committed tree, breaks it in exactly one way, and runs
the real recipes over the copy. Nothing is mocked — `just` runs, `cargo` runs,
`nx` runs, and the assertion is on what the gate said.

A copy's `tests/repo-e2e` is replaced by a single trivial test, because the
suite doing the copying is the suite the copy would otherwise run: a gate that
ran the suite that runs the gate could not terminate. Everything else about the
copy is the committed tree.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

from repo_checks.shell import run as shell_run

REPO_ROOT = Path(__file__).resolve().parents[3]
# Assembled rather than written whole: a real directive in this module's source
# would itself be a suppression the allowlist check would then demand an entry
# for, in the file that exists to write one into a copy.
_S101_DIRECTIVE = "# ruff: " + "noqa: S101"
PLACEHOLDER_SUITE = f'''"""The meta-suite a gate copy runs in place of the one driving the copy."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
{_S101_DIRECTIVE}


def test_the_end_to_end_tier_is_reachable() -> None:
    """The tier runs; the real journeys are the ones driving this copy."""
    assert True
'''
PLACEHOLDER_SUPPRESSION = (
    "\n[[suppression]]\n"
    'rule = "S101"\n'
    'file = "tests/repo-e2e/tests/test_placeholder.py"\n'
    'site = "ruff: noqa: S101"\n'
    'reason = "The meta-suite a gate copy runs in place of the suite driving '
    "the copy. `assert` is pytest's assertion statement, and S101 stays "
    "enabled everywhere else in the copy exactly as it is in the committed "
    'tree."\n'
)
REPLACED_SUITE = "tests/repo-e2e/tests/"


def tracked_files(root: Path) -> list[str]:
    """Every file a clone would carry once this change lands.

    `--others --exclude-standard` includes files this change has added but not
    yet committed, and excludes everything `.gitignore` covers. Reading only the
    index would copy a tree missing exactly the files the change is about.
    """
    listing = shell_run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
    ).stdout
    return [name for name in listing.split("\0") if name]


def copy_tracked(destination: Path) -> Path:
    """Copy the committed tree, symlinks and all, carrying no build products."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in tracked_files(REPO_ROOT):
        source = REPO_ROOT / name
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(source.readlink())
        else:
            shutil.copy2(source, target)
    return destination


def clean_environment(**extra: str) -> dict[str, str]:
    """The caller's environment, minus this checkout's own activated virtualenv."""
    environment = dict(os.environ)
    environment.pop("VIRTUAL_ENV", None)
    environment.update(extra)
    return environment


def run(command: list[str], cwd: Path, *, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    """Run a real command in a real directory and hand back everything it said."""
    return shell_run(command, cwd=cwd, timeout=timeout, env=clean_environment())


def output(result: subprocess.CompletedProcess[str]) -> str:
    """Everything a run said, on either stream."""
    return result.stdout + result.stderr


class GateCopy:
    """A copy of the committed tree the gate can be run over."""

    def __init__(self, root: Path, shared: Path) -> None:
        """Prepare a copy that resolves its dependencies without a fresh install."""
        self.root = copy_tracked(root)
        suite = self.root / "tests" / "repo-e2e" / "tests"
        shutil.rmtree(suite, ignore_errors=True)
        suite.mkdir(parents=True)
        (suite / "test_placeholder.py").write_text(PLACEHOLDER_SUITE, encoding="utf-8")
        self._replace_meta_suite_suppressions()

        for args in (
            ["init", "-q", "-b", "main"],
            ["add", "-A"],
            [
                "-c",
                "user.email=e2e@printobserver.test",
                "-c",
                "user.name=e2e",
                "commit",
                "-q",
                "-m",
                "chore: the committed tree, copied",
            ],
        ):
            shell_run(["git", *args], cwd=self.root, check=True)

        (self.root / "node_modules").symlink_to(REPO_ROOT / "node_modules")
        self.shared_venv = shared

    def _replace_meta_suite_suppressions(self) -> None:
        """Swap the replaced suite's allowlist entries for the placeholder's.

        The copy deletes the modules those entries name, and the copy's own
        `just check-repo` refuses an entry that matches no directive — so the
        entries have to go with the suite they describe.
        """
        allowlist = self.root / "suppressions.toml"
        blocks = allowlist.read_text(encoding="utf-8").split("\n[[suppression]]")
        kept = [blocks[0]] + [
            block for block in blocks[1:] if f'file = "{REPLACED_SUITE}' not in block
        ]
        allowlist.write_text(
            "\n[[suppression]]".join(kept).rstrip() + "\n" + PLACEHOLDER_SUPPRESSION,
            encoding="utf-8",
        )

    def read(self, relative: str) -> str:
        """Read a file of the copy."""
        return (self.root / relative).read_text(encoding="utf-8")

    def write(self, relative: str, text: str) -> None:
        """Replace a file of the copy."""
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def append(self, relative: str, text: str) -> None:
        """Add to a file of the copy."""
        self.write(relative, self.read(relative) + text)

    def edit(self, relative: str, old: str, new: str) -> None:
        """Replace one exact fragment, refusing a no-op edit."""
        text = self.read(relative)
        if old not in text:
            msg = f"{relative} does not contain {old!r}; the fixture is stale"
            raise AssertionError(msg)
        self.write(relative, text.replace(old, new, 1))

    def just(self, recipe: str, *, timeout: int = 900) -> subprocess.CompletedProcess[str]:
        """Run one recipe of the copy's own command surface."""
        return shell_run(
            ["just", recipe],
            cwd=self.root,
            timeout=timeout,
            env=clean_environment(UV_PROJECT_ENVIRONMENT=str(self.shared_venv)),
        )
