"""The enforcement path: the committed hook, in the clone a publication is cut from.

There is no gate between this repository and `main` other than `.githooks/pre-push`
and the jobs a pull request runs, and every publication is made from a clone cut
fresh for it. So the hook runs the whole gate in a tree that has never run `just
bootstrap`, and it runs it in the environment git gives a hook.

Two defects live only on that path and on no path a person takes by hand. The
dependency tree is absent, so anything reaching Nx dies before judging anything.
And git exports `GIT_DIR` into the hook, which every check and suite the gate
starts inherits — pointing the repositories they build in temporary directories
at the repository being pushed instead.

This journey drives the real hook over a real copy carrying neither. Nothing is
mocked: the hook is the committed file, the gate is the whole gate, and the
assertion is on what it said.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from journey import GateCopy, output
from repo_checks.expect import absent, passing, truth

MISSING_MODULES = "Could not find Nx modules"
HIJACKED_REPOSITORY = "returned non-zero exit status"


def test_the_pre_push_hook_passes_in_a_fresh_publication_clone(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The one thing standing between this repository and `main` runs where it is run."""
    fresh = gate_copy(node_modules=False)
    truth(
        not (fresh.root / "node_modules").exists(),
        describing="a publication clone to carry no installed dependencies",
    )

    result = fresh.hook("pre-push")

    passing(result, describing="the committed pre-push hook")
    absent(output(result), MISSING_MODULES)
    absent(output(result), HIJACKED_REPOSITORY)
