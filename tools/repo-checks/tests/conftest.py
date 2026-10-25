"""Fixtures that hand a check a real tree on disk to read.

Every test here drives the committed check functions against a real copy of the
committed tree, with one defect introduced. Nothing is mocked: the check reads
files, and the files are the ones this repository ships.
"""

from __future__ import annotations

from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from repo_checks import gate_shards, workers
from repo_checks.model import Repo
from treecopy import REPO_ROOT, Closure, Tree, copy_tree, strayed

# Each worker measures itself when this suite runs under coverage.
pytest_configure_node = workers.pytest_configure_node
pytest_configure = workers.pytest_configure
pytest_unconfigure = workers.pytest_unconfigure


@pytest.hookimpl(optionalhook=True)
def pytest_xdist_auto_num_workers(config: pytest.Config) -> int:
    """What `-n auto` means for this suite on this host, said once before it starts."""
    del config
    return workers.chosen(workers.Suite("repo-checks", "test"), REPO_ROOT)


@pytest.fixture(autouse=True)
def no_ambient_gate_base(monkeypatch: pytest.MonkeyPatch) -> None:
    """Drop the base and head the gate job exports before any test starts.

    `nx-set-shas` exports `NX_BASE` and `NX_HEAD` into the job, so every test of
    a gate cell inherits commits of *that* checkout — which none of the trees a
    test builds contains, and which the tier rightly refuses as naming no
    commit. A test that means a base names it itself.
    """
    monkeypatch.delenv("NX_BASE", raising=False)
    monkeypatch.delenv("NX_HEAD", raising=False)


@pytest.fixture(autouse=True)
def no_ambient_shard_part(monkeypatch: pytest.MonkeyPatch) -> None:
    """Drop the part of the gate a shard ran this suite under before any test starts.

    A sharded gate runs this suite with `PRINTOBSERVER_GATE_PROJECTS` naming
    its side of the test tier, and every tier a test drives would inherit it;
    a test that means a part names it itself.
    """
    for name in gate_shards.PARTS:
        monkeypatch.delenv(name, raising=False)


@pytest.fixture(autouse=True)
def reads_inside_the_closure() -> Iterator[None]:
    """Fail a test that read a committed file its closure copy does not carry.

    A check reading a file its copy left out reads nothing, and may accept a
    tree only because the file that would refuse it is not there. So the read
    itself fails the test, naming the file its module's `CLOSURE` must add.
    """
    strayed()
    yield
    missed = strayed()
    if missed:
        pytest.fail(
            "read committed files its closure copy does not carry; add them to the "
            f"module's CLOSURE: {', '.join(missed)}"
        )


def closure_of(request: pytest.FixtureRequest) -> Closure | None:
    """The closure a test module's copies are cut to, or none for the whole tree."""
    declared = getattr(request.module, "CLOSURE", None)
    if declared is not None and not isinstance(declared, Closure):
        msg = f"{request.module.__name__}.CLOSURE is {declared!r}, not a Closure"
        raise TypeError(msg)
    return declared


@pytest.fixture
def committed() -> Repo:
    """The committed tree, exactly as this repository leaves it."""
    return Repo(REPO_ROOT)


@pytest.fixture
def tree(tmp_path: Path, request: pytest.FixtureRequest) -> Callable[[], Tree]:
    """A factory for fresh copies of the committed tree, cut to the module's `CLOSURE`."""
    counter = {"n": 0}
    closure = closure_of(request)

    def make() -> Tree:
        counter["n"] += 1
        return Tree(copy_tree(tmp_path / f"tree{counter['n']}", closure=closure))

    return make
