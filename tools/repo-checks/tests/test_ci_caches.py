"""Every cache `ci.yml` keeps is restored by every run and saved by a run on `main` alone.

A cache saved by a pull request's run is scoped to that pull request's merge
ref: no other run restores it, and the repository's quota is one 10 GB pool, so
open pull requests saving theirs evict `main`'s — the only caches every pull
request restores — and every pull request after them runs cold. So each cache
step of the committed workflow is read here as GitHub runs it, for each of its
job's matrix cells, under two contexts: a push to `main`, and a pull request.

* Under both, it restores, and the cache it looks up is the one the push to
  `main` saves: every input the key and its restore keys are made of renders
  the same under both contexts, which a key reading the ref, the event or the
  pull request cannot.
* Under the push to `main` it saves, where its key missed; under the pull
  request it saves nothing, hit or miss.

Each action is read by the inputs it documents: the Rust toolchain's cache
saves unless `cache-save-if` renders `false`, uv's unless `save-cache` does,
and `actions/cache/save` unless its step's condition skips it. An action whose
restore and save cannot be told apart — `actions/cache` itself, and Bun's
cache of its own executable — must not cache at all, and an action this module
does not know is refused rather than assumed to keep nothing.

One cache is exempt, and it is named: the real-print suite's LFS store, keyed
on the cases' content and saved only when that key missed. Only a pull request
changing the cases saves one (about 150 MB), and saving it from `main` alone
would download them again on every push of that pull request, out of the
account's monthly LFS bandwidth.
"""

from __future__ import annotations

import re
from collections.abc import Iterator, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest
from repo_checks.expect import contains, equal, truth
from repo_checks.parsing import jobs_of, load_workflow, steps_of
from treecopy import REPO_ROOT

CI = REPO_ROOT / ".github/workflows/ci.yml"

#: A push to `main`, the one run whose caches every other run restores.
MAIN = {"github.ref": "refs/heads/main", "github.event_name": "push", "github.base_ref": ""}
#: A pull request's run, whose cache would be scoped to its own merge ref.
PULL_REQUEST = {
    "github.ref": "refs/pull/328/merge",
    "github.event_name": "pull_request",
    "github.base_ref": "main",
}

#: The actions that keep no cache whatever their inputs.
CACHELESS = frozenset(
    {
        "actions/checkout",
        "actions/upload-artifact",
        "extractions/setup-just",
        "nrwl/nx-set-shas",
        "taiki-e/install-action",
    }
)
#: Per action that keeps a cache: the input that turns it off and the value that
#: does (`None` where nothing does); the input deciding whether it saves, and the
#: value it takes absent (`None` where the action restores and saves as one, so
#: it may only be off); and the inputs its key and restore keys are made of
#: beside what it hashes itself — the job's id and the lockfiles, which a ref
#: does not change.
CACHING: Mapping[str, tuple[str | None, str, str | None, str, tuple[str, ...]]] = {
    "actions-rust-lang/setup-rust-toolchain": (
        "cache",
        "false",
        "cache-save-if",
        "true",
        ("cache-key", "cache-shared-key", "cache-workspaces", "cache-directories"),
    ),
    "astral-sh/setup-uv": (
        "enable-cache",
        "false",
        "save-cache",
        "true",
        ("cache-suffix", "cache-dependency-glob", "cache-local-path"),
    ),
    "oven-sh/setup-bun": ("no-cache", "true", None, "true", ()),
    "actions/cache": (None, "", None, "true", ()),
}
#: An action that keeps a cache only where it is handed this input, and then
#: restores and saves as one.
CACHING_WHEN_ASKED = {"actions/setup-node": "cache"}
RESTORE = "actions/cache/restore"
SAVE = "actions/cache/save"
#: The one cache a pull request may save, by the path it holds.
EXEMPT = ".git/lfs/objects"

EXPRESSION = re.compile(r"\$\{\{\s*(.*?)\s*\}\}")
TOKEN = re.compile(
    r"\s*(?:(?P<op>==|!=|&&|\|\||!|\(|\))"
    r"|'(?P<text>(?:[^']|'')*)'"
    r"|(?P<name>[A-Za-z_][\w.-]*))"
)


class Opaque(str):
    """A value no context here fixes, such as a step's output: equal only to itself."""

    __slots__ = ()


def evaluate(expression: str, context: Mapping[str, str]) -> str | bool:
    """A GitHub expression of literals, context values, `!`, `==`, `!=`, `&&` and `||`.

    A name the context does not carry is opaque — its own spelling, compared
    only with itself — so a key reading a step's output renders the same under
    every context, and one reading the ref does not.
    """
    tokens: list[tuple[str, str]] = []
    at = 0
    while at < len(expression.rstrip()):
        found = TOKEN.match(expression, at)
        if found is None:
            pytest.fail(f"`{expression}` is not an expression this test reads, at {at}")
        kind = next(name for name in ("op", "text", "name") if found[name] is not None)
        tokens.append((kind, found[kind]))
        at = found.end()
    position = 0

    def peek() -> tuple[str, str] | None:
        return tokens[position] if position < len(tokens) else None

    def take() -> tuple[str, str]:
        nonlocal position
        position += 1
        return tokens[position - 1]

    def primary() -> str | bool:
        kind, value = take()
        if (kind, value) == ("op", "!"):
            return not truthy(primary())
        if (kind, value) == ("op", "("):
            inner = disjunction()
            take()
            return inner
        if kind == "text":
            return value.replace("''", "'")
        if value in {"true", "false"}:
            return value == "true"
        return context.get(value, Opaque(f"<{value}>"))

    def comparison() -> str | bool:
        left = primary()
        while peek() in {("op", "=="), ("op", "!=")}:
            _, op = take()
            right = primary()
            left = (normal(left) == normal(right)) == (op == "==")
        return left

    def conjunction() -> str | bool:
        left = comparison()
        while peek() == ("op", "&&"):
            take()
            right = comparison()
            left = truthy(left) and truthy(right)
        return left

    def disjunction() -> str | bool:
        left = conjunction()
        while peek() == ("op", "||"):
            take()
            right = conjunction()
            left = truthy(left) or truthy(right)
        return left

    result = disjunction()
    truth(position == len(tokens), describing=f"`{expression}` read to its end")
    return result


def normal(value: str | bool) -> str:
    """A value as GitHub compares it: booleans as their words."""
    return str(value).lower() if isinstance(value, bool) else value


def truthy(value: str | bool) -> bool:
    """Whether a value passes as a condition: GitHub's empty string is false."""
    return value if isinstance(value, bool) else value not in {"", "false"}


def render(text: object, context: Mapping[str, str]) -> str:
    """An input's value with each `${{ }}` in it evaluated under `context`.

    YAML reads a bare `true` as a boolean, which an action reads as its word.
    """
    written = normal(text) if isinstance(text, bool) else str(text)
    return EXPRESSION.sub(lambda found: normal(evaluate(found[1], context)), written)


def condition(step: Mapping[str, Any], context: Mapping[str, str]) -> bool:
    """Whether a step runs under `context`: its `if`, with or without `${{ }}`."""
    declared = step.get("if")
    if declared is None:
        return True
    bare = EXPRESSION.fullmatch(str(declared).strip())
    return truthy(evaluate(bare[1] if bare else str(declared), context))


@dataclass(frozen=True)
class Cell:
    """One matrix cell of one job: what a step's expressions read beside the run's context."""

    job: str
    values: Mapping[str, str]

    def __str__(self) -> str:
        """The job, and the cell's platform and shard where it has them."""
        named = ", ".join(self.values.get(key, "") for key in ("id", "shard") if key in self.values)
        return f"job `{self.job}`" + (f" ({named})" if named else "")

    def under(self, run: Mapping[str, str], **outputs: str) -> dict[str, str]:
        """The context a step of this cell evaluates in, under one run's."""
        return {
            **run,
            **{f"matrix.platform.{key}": value for key, value in self.values.items()},
            **outputs,
        }


def cells(name: str, job: Mapping[str, Any]) -> list[Cell]:
    """Each cell a job runs as: one per `platform` matrix entry, or the job alone."""
    strategy = job.get("strategy")
    matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
    entries = matrix.get("platform") if isinstance(matrix, dict) else None
    if not isinstance(entries, list):
        return [Cell(name, {})]
    return [Cell(name, {str(k): str(v) for k, v in entry.items()}) for entry in entries]


def action(step: Mapping[str, Any]) -> str:
    """The action a step uses, without its version."""
    return str(step.get("uses", "")).split("@", 1)[0]


def hit(step_id: object, value: str) -> dict[str, str]:
    """A restore step's answer, as the save steps after it read it."""
    return {f"steps.{step_id}.outputs.cache-hit": value}


def findings(workflow: Mapping[str, Any]) -> list[str]:
    """Every way a cache step of `workflow` is not restored everywhere and saved on `main` alone."""
    found: list[str] = []
    for name, job in jobs_of(workflow).items():
        steps = steps_of(job)
        for cell in cells(name, job):
            found.extend(_cell_findings(cell, steps))
    return found


def _cell_findings(cell: Cell, steps: list[dict[str, Any]]) -> Iterator[str]:
    """What is wrong with one cell's cache steps."""
    restored: dict[str, tuple[str, str]] = {}
    for index, step in enumerate(steps):
        used = action(step)
        where = f"{cell} step {index + 1} (`{step.get('uses')}`)"
        inputs = step.get("with") if isinstance(step.get("with"), dict) else {}
        if not used or used in CACHELESS:
            continue
        if used == RESTORE:
            yield from _restore_findings(cell, step, inputs, where)
            restored[str(step.get("id"))] = (
                render(inputs.get("key"), cell.under(MAIN)),
                str(inputs.get("path")),
            )
            continue
        if used == SAVE:
            yield from _save_findings(cell, step, inputs, where, restored)
            continue
        if used in CACHING_WHEN_ASKED:
            if CACHING_WHEN_ASKED[used] in inputs:
                yield (
                    f"{where} caches, and restores and saves as one, so it cannot restore on "
                    f"a pull request without saving there: leave `{CACHING_WHEN_ASKED[used]}` out"
                )
            continue
        if used not in CACHING:
            yield f"{where} uses an action this test does not know whether it caches"
            continue
        off, off_value, saves, default, keyed = CACHING[used]
        if off is not None and render(inputs.get(off, ""), cell.under(MAIN)) == off_value:
            continue
        if saves is None:
            yield (
                f"{where} caches, and restores and saves as one, so it cannot restore on a "
                f"pull request without saving there"
                + (f": set `{off}: {off_value}`" if off is not None else "")
            )
            continue
        for run, label in ((MAIN, "push to `main`"), (PULL_REQUEST, "pull request")):
            if render(inputs.get("restore-cache", "true"), cell.under(run)) != "true":
                yield f"{where} restores nothing on a {label}"
        if render(inputs.get(saves, default), cell.under(PULL_REQUEST)) != "false":
            yield f"{where} saves on a pull request's ref: its `{saves}` does not render `false`"
        if render(inputs.get(saves, default), cell.under(MAIN)) != "true":
            yield f"{where} does not save on a push to `main`: its `{saves}` does not render `true`"
        for one in keyed:
            if one in inputs and render(inputs[one], cell.under(MAIN)) != render(
                inputs[one], cell.under(PULL_REQUEST)
            ):
                yield (
                    f"{where}'s `{one}` renders differently on a pull request, so its "
                    f"lookup cannot match the cache a push to `main` saves"
                )


def _restore_findings(
    cell: Cell, step: Mapping[str, Any], inputs: Mapping[str, Any], where: str
) -> Iterator[str]:
    """A restore runs on every run and looks up what a push to `main` saves."""
    for run, label in ((MAIN, "push to `main`"), (PULL_REQUEST, "pull request")):
        if not condition(step, cell.under(run)):
            yield f"{where} restores nothing on a {label}: its condition skips it"
    for one in ("key", "restore-keys"):
        if one in inputs and render(inputs[one], cell.under(MAIN)) != render(
            inputs[one], cell.under(PULL_REQUEST)
        ):
            yield (
                f"{where}'s `{one}` renders differently on a pull request, so its lookup "
                f"cannot match the cache a push to `main` saves"
            )


def _save_findings(
    cell: Cell,
    step: Mapping[str, Any],
    inputs: Mapping[str, Any],
    where: str,
    restored: Mapping[str, tuple[str, str]],
) -> Iterator[str]:
    """A save follows its restore, under its key, and runs on `main` alone."""
    key = render(inputs.get("key"), cell.under(MAIN))
    pair = [one for one, (restored_key, _) in restored.items() if restored_key == key]
    if not pair:
        yield f"{where} saves under a key no restore step before it looks up"
        return
    missed = cell.under(MAIN, **hit(pair[0], "false"))
    if not condition(step, missed):
        yield f"{where} does not save on a push to `main` whose lookup missed"
    if str(inputs.get("path")) == EXEMPT:
        return
    for answer in ("true", "false"):
        if condition(step, cell.under(PULL_REQUEST, **hit(pair[0], answer))):
            yield f"{where} saves on a pull request's ref (cache-hit {answer})"


def read(text: str, tmp_path: Path) -> dict[str, Any]:
    """A workflow as the checks read it, from its text."""
    path = tmp_path / "ci.yml"
    path.write_text(text, encoding="utf-8")
    return load_workflow(path)


def test_every_cache_in_the_committed_workflow_is_restored_everywhere_and_saved_on_main_alone(
    tmp_path: Path,
) -> None:
    """The committed `ci.yml`, every job, every cell, every cache step."""
    workflow = read(CI.read_text(encoding="utf-8"), tmp_path)

    equal(findings(workflow), [], describing="the committed workflow's cache findings")


def test_the_workflow_keeps_every_cache_it_claims_to(tmp_path: Path) -> None:
    """A test over no cache step would pass by reading nothing.

    The Rust toolchain's, uv's, the release program's and the real-print
    store's caches each appear, and Bun's is off on every step that sets it up.
    """
    workflow = read(CI.read_text(encoding="utf-8"), tmp_path)
    used = [
        (action(step), step.get("with") or {})
        for job in jobs_of(workflow).values()
        for step in steps_of(job)
    ]
    actions = [one for one, _ in used]
    for kept in (*CACHING.keys() - {"actions/cache"}, RESTORE, SAVE):
        contains(actions, kept, describing="the actions the committed workflow's steps use")
    for one, inputs in used:
        if one == "oven-sh/setup-bun":
            equal(inputs.get("no-cache"), True, describing="a Bun step's `no-cache`")
    contains(
        [str(inputs.get("path")) for one, inputs in used if one == SAVE],
        EXEMPT,
        describing="the paths the workflow's save steps hold",
    )


#: The integration job's release-program cache, as the committed workflow spells it.
RELEASE_KEY = (
    "          key: release-program-${{ matrix.platform.id }}-"
    "${{ steps.release-program.outputs.key }}\n"
)
RELEASE_SAVE_IF = (
    "      - if: github.ref == 'refs/heads/main' && "
    "steps.release-program-cache.outputs.cache-hit != 'true'\n"
)
SHARD_KEY = (
    "          cache-key: shard-${{ matrix.platform.shard }}-of-${{ matrix.platform.shards }}\n"
)
RUST_SAVE_IF = "          cache-save-if: ${{ github.ref == 'refs/heads/main' }}\n"
UV_SAVE = "          save-cache: ${{ github.ref == 'refs/heads/main' }}\n"


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        pytest.param(
            RUST_SAVE_IF,
            "",
            "saves on a pull request's ref: its `cache-save-if`",
            id="rust-default",
        ),
        pytest.param(
            RUST_SAVE_IF,
            "          cache-save-if: ${{ github.event_name != 'merge_group' }}\n",
            "saves on a pull request's ref: its `cache-save-if`",
            id="rust-any-push",
        ),
        pytest.param(UV_SAVE, "", "saves on a pull request's ref: its `save-cache`", id="uv"),
        pytest.param(
            UV_SAVE,
            "          save-cache: false\n",
            "does not save on a push to `main`",
            id="uv-never",
        ),
        pytest.param(
            RELEASE_SAVE_IF,
            "      - if: steps.release-program-cache.outputs.cache-hit != 'true'\n",
            "saves on a pull request's ref (cache-hit false)",
            id="save-on-miss-anywhere",
        ),
        pytest.param(
            RELEASE_SAVE_IF,
            "      - if: github.ref == 'refs/heads/main' || github.event_name == 'pull_request'\n",
            "saves on a pull request's ref (cache-hit true)",
            id="save-on-pull-requests",
        ),
        pytest.param(
            "          no-cache: true\n",
            "          no-cache: false\n",
            "restores and saves as one",
            id="bun-cached",
        ),
        pytest.param(
            "        uses: actions/cache/restore@v4\n",
            "        uses: actions/cache@v4\n",
            "restores and saves as one",
            id="cache-action",
        ),
        pytest.param(
            "      - uses: extractions/setup-just@v3\n",
            "      - uses: example/setup-just-cached@v1\n",
            "does not know whether it caches",
            id="unknown-action",
        ),
    ],
)
def test_a_cache_saved_on_a_pull_requests_ref_is_refused(
    tmp_path: Path, old: str, new: str, naming: str
) -> None:
    """Each way a step could save a cache no other run would restore."""
    text = CI.read_text(encoding="utf-8")
    contains(text, old, describing="the committed workflow")

    found = findings(read(text.replace(old, new, 1), tmp_path))

    contains("\n".join(found), naming, describing="the findings over the broken workflow")


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        pytest.param(
            SHARD_KEY,
            "          cache-key: shard-${{ matrix.platform.shard }}-${{ github.ref }}\n",
            "`cache-key` renders differently on a pull request",
            id="rust-key-by-ref",
        ),
        pytest.param(
            UV_SAVE,
            UV_SAVE + "          cache-suffix: ${{ github.event_name }}\n",
            "`cache-suffix` renders differently on a pull request",
            id="uv-suffix-by-event",
        ),
        pytest.param(
            RELEASE_KEY,
            "          key: release-program-${{ matrix.platform.id }}-${{ github.base_ref }}\n",
            "`key` renders differently on a pull request",
            id="restore-key-by-base",
        ),
        pytest.param(
            RELEASE_KEY,
            RELEASE_KEY + "          restore-keys: release-program-${{ github.ref }}-\n",
            "`restore-keys` renders differently on a pull request",
            id="restore-keys-by-ref",
        ),
        pytest.param(
            "      - id: release-program-cache\n        uses: actions/cache/restore@v4\n",
            "      - id: release-program-cache\n"
            "        if: github.event_name == 'push'\n"
            "        uses: actions/cache/restore@v4\n",
            "restores nothing on a pull request",
            id="restore-on-main-alone",
        ),
        pytest.param(
            RELEASE_SAVE_IF + "        uses: actions/cache/save@v4\n        with:\n"
            "          path: .release-program\n" + RELEASE_KEY,
            RELEASE_SAVE_IF + "        uses: actions/cache/save@v4\n        with:\n"
            "          path: .release-program\n"
            "          key: release-program-saved-${{ matrix.platform.id }}\n",
            "saves under a key no restore step before it looks up",
            id="save-under-another-key",
        ),
    ],
)
def test_a_lookup_that_cannot_match_mains_cache_is_refused(
    tmp_path: Path, old: str, new: str, naming: str
) -> None:
    """Each way a pull request's lookup could miss what a push to `main` saved."""
    text = CI.read_text(encoding="utf-8")
    contains(text, old, describing="the committed workflow")

    found = findings(read(text.replace(old, new, 1), tmp_path))

    contains("\n".join(found), naming, describing="the findings over the broken workflow")


@pytest.mark.parametrize(
    ("expression", "context", "expected"),
    [
        ("github.ref == 'refs/heads/main'", MAIN, True),
        ("github.ref == 'refs/heads/main'", PULL_REQUEST, False),
        ("!(github.event_name == 'push') || false", PULL_REQUEST, True),
        ("steps.a.outputs.key == steps.a.outputs.key", PULL_REQUEST, True),
        ("steps.a.outputs.key", MAIN, "<steps.a.outputs.key>"),
        ("'it''s'", MAIN, "it's"),
    ],
)
def test_the_expressions_read_as_github_reads_them(
    expression: str, context: Mapping[str, str], expected: str | bool
) -> None:
    """The reader this module judges conditions and keys by, on the forms they take."""
    equal(evaluate(expression, context), expected)
