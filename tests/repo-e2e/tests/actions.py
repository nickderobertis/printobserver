"""Run a committed GitHub Actions workflow's jobs here, the way the forge schedules them.

A workflow's scheduling — which job waits on which, which job is skipped, what
one job reads off another's outputs — is decided by the forge, and a journey
cannot fire the forge. What it can do is take the committed workflow's own text
and drive it under the same rules the forge documents, substituting only the
boundaries that reach outside this host: every `uses:` step (a checkout, a
toolchain setup) is a boundary that runs nothing and is recorded with the
inputs it was given, and the programs a caller names as stand-ins — the release
program that would reach a registry and a forge — are found on the PATH first.

Two actions are stood in for rather than recorded, because what they move is
inside this host: `actions/upload-artifact` and `actions/download-artifact`
read and write an artifact store the caller hands the runner, keyed by run id,
so that a journey can drive the seam between two workflows — a record one run
uploads and a run of another workflow downloads — through the forge's own
store rather than through a boundary that runs nothing.

The rules kept are exactly the ones this repository's workflows lean on, and
nothing outside them is guessed at: a construct or an expression this runner
does not know is refused by name rather than approximated, so a workflow that
grows past what is modelled here fails the journey loudly. What the forge does
with the same text is what every push to the base branch shows, and that run is
where a rule modelled here wrongly would show up.

  * A job runs after every job it `needs`, and only if each of them succeeded —
    the implicit `success()` the forge applies to every job condition — and its
    `if:` expression, where it has one, is truthy. Otherwise it is skipped, and
    what it needs from a skipped job it never reads. A caller may name the set
    of jobs to run; every job outside it is skipped by name, so a journey can
    drive a workflow's resolving jobs without the ones whose steps reach a real
    registry. The refusal of a construct outside the modelled set still reaches
    every job that runs, and nothing that is skipped by name is approximated.
  * A step runs only if its `if:` expression, where it has one, is truthy; a
    skipped step answers no outputs under its `id`, exactly as the forge's.
  * `if:` and `${{ }}` expressions are the forge's grammar over the contexts
    this repository's workflows read: `needs.<job>.outputs.<name>`,
    `steps.<id>.outputs.<name>`, `secrets.<NAME>`, `matrix.<...>`,
    `github.event_name`, `github.event.workflow_run.<field>`, `github.ref`,
    `github.run_id`, `github.token`, `inputs.<name>`, `runner.temp`, string literals, `==`,
    `!=`, `&&`, `||`, `!` and parentheses. String comparison is
    case-insensitive, as the forge's is. The event a run is under is the
    caller's to say — `push` by default, `workflow_dispatch` with its inputs,
    or `workflow_run` with the triggering run's `event`, `id` and `head_sha`.
  * A `run:` step is `bash -e` over the step's text, in the checkout, with the
    job's `env` and then the step's on top of the caller's, `GITHUB_OUTPUT` a
    file of its own and `RUNNER_TEMP` the job's one directory; `name=value`
    lines it appends to `GITHUB_OUTPUT` become `steps.<id>.outputs.<name>`. An
    expression inside the step's text is refused: that is the injection the
    forge documents, and no workflow here writes one.
  * `actions/upload-artifact` copies what its `path` holds into the store
    under its `name`, and uploads nothing where the path holds nothing, as
    the action warns and does. `actions/download-artifact` by `name` copies
    that artifact — this run's, or the run `run-id` names — to its `path`
    and fails the step where no such artifact exists, as the action does; by
    `pattern` with `merge-multiple` it merges every match into `path`, and
    matches nothing without failing.
  * A job's `outputs:` are its expressions evaluated once its steps are done.
  * A job with a `strategy.matrix` runs once per cell, and succeeds when every
    cell does.
"""

from __future__ import annotations

import fnmatch
import os
import re
import shutil
import tempfile
from dataclasses import dataclass, field
from enum import StrEnum
from itertools import product
from pathlib import Path
from typing import Any

import yaml
from repo_checks.shell import run as shell_run

#: One job, one step, or one matrix cell as the YAML reader hands it back. The
#: keys are the workflow author's own and the values whatever the author wrote,
#: so there is no narrower shape to read one as; every reader below narrows the
#: one value it takes and refuses a key it does not model.
Declared = dict[str, Any]

#: The step keys this runner models. Anything else on a step is a semantic it
#: would otherwise silently drop, so it is refused instead.
STEP_KEYS = frozenset({"id", "name", "run", "uses", "with", "env", "if"})

#: The job keys this runner models, plus `permissions`: what the forge grants
#: its own token is a boundary here, since nothing a step runs reaches the forge.
JOB_KEYS = frozenset(
    {"name", "needs", "if", "runs-on", "outputs", "steps", "strategy", "env", "permissions"}
)

#: What the forge documents as the default shell for a `run:` step on Linux.
DEFAULT_SHELL = ("bash", "-e")

STEP_TIMEOUT_SECONDS = 600

#: How a workflow names a secret.
SECRET_REFERENCE = re.compile(r"secrets\.([A-Za-z0-9_]+)")

INTERPOLATION = re.compile(r"\$\{\{(.*?)\}\}")


class UnsupportedError(ValueError):
    """The workflow uses a construct this runner does not model, and will not guess at."""


class Result(StrEnum):
    """What the forge reports a job as, once the run is over."""

    SUCCESS = "success"
    FAILURE = "failure"
    SKIPPED = "skipped"


@dataclass(frozen=True, slots=True)
class StepRun:
    """One `run:` step that ran: its command and what it did.

    A stood-in artifact action that failed is recorded here too, as the
    action it was and what it said, so that a journey reads a failed download
    where it reads every other failure: the last step of the job.
    """

    command: str
    returncode: int
    output: str


@dataclass(frozen=True, slots=True)
class Boundary:
    """One `uses:` step that was reached: the action, and the inputs it was given."""

    uses: str
    #: The step's `with:`, each value interpolated as the forge would hand it
    #: to the action — so a journey can read which `ref` a checkout was given.
    given: dict[str, str] = field(default_factory=dict)


@dataclass(slots=True)
class JobRun:
    """One job as the run left it."""

    result: Result
    outputs: dict[str, str] = field(default_factory=dict)
    steps: list[StepRun] = field(default_factory=list)
    boundaries: list[Boundary] = field(default_factory=list)

    def boundary(self, action: str) -> Boundary:
        """The first boundary using `action`, whatever it is pinned at.

        Raises:
            LookupError: If the job reached no such boundary.
        """
        for boundary in self.boundaries:
            if boundary.uses.startswith(action):
                return boundary
        msg = f"no `{action}` boundary was reached; those reached were {self.boundaries}"
        raise LookupError(msg)


#: The two actions the store below stands in for, matched by name up to the pin.
UPLOAD = "actions/upload-artifact@"
DOWNLOAD = "actions/download-artifact@"

#: What an artifact's name may not carry, as the real action refuses it: a path
#: separator or anything the forge's own storage will not take. A name and a
#: run id are path components of the store below, so each is held to this
#: before it is joined to anything.
INVALID_IN_NAME = frozenset('"<>|*?\r\n\\/:')


class ArtifactError(ValueError):
    """An artifact action would have failed its step, in the action's own words."""


def component(value: str, what: str) -> str:
    """One artifact name or run id, once it is a single path component the action takes.

    Raises:
        ArtifactError: If it is empty, names a parent or the current directory,
            or carries a character the real action refuses.
    """
    if not value or value in {".", ".."} or any(char in INVALID_IN_NAME for char in value):
        msg = (
            f"{what} {value!r} is not valid: it must be a non-empty name carrying none of "
            f"{''.join(sorted(INVALID_IN_NAME))!r}"
        )
        raise ArtifactError(msg)
    return value


# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
class ArtifactStore:
    """What the forge keeps between jobs and between runs, keyed by run id.

    One directory per run, one directory per artifact name under it, holding
    what the uploading step's `path` held: a directory's contents at the
    artifact's root, as the real action stores them, and a file under its own
    name. A name and a run id are single path components, held to what the
    real action accepts before either is joined to the store's root.
    """

    def __init__(self, root: Path) -> None:
        """Keep artifacts under `root`."""
        self.root = root

    def _kept(self, run_id: str, name: str) -> Path:
        """Where one artifact of one run is kept, once both names are ones the action takes."""
        return self.root / component(run_id, "the run id") / component(name, "the artifact name")

    def names(self, run_id: str) -> list[str]:
        """Every artifact one run uploaded, by name."""
        kept = self.root / component(run_id, "the run id")
        return sorted(path.name for path in kept.iterdir()) if kept.is_dir() else []

    def read(self, run_id: str, name: str, relative: str) -> str:
        """One file of one artifact, as text."""
        return (self._kept(run_id, name) / relative).read_text(encoding="utf-8")

    def upload(self, run_id: str, name: str, path: Path) -> bool:
        """Keep what `path` holds under `name`, answering whether there was anything.

        Nothing is kept for a path that is not there or a directory holding
        nothing: the action warns that no files were found and creates no
        artifact, and a download by that name afterwards fails.

        Raises:
            ArtifactError: If the name is not one the action takes, or the run
                already holds an artifact under it — the action refuses a
                second upload under a taken name rather than replacing the
                first.
        """
        kept = self._kept(run_id, name)
        if not path.exists() or (path.is_dir() and not any(path.iterdir())):
            return False
        if kept.exists():
            msg = (
                f"Failed to CreateArtifact: an artifact with this name already exists on "
                f"the workflow run: {name}"
            )
            raise ArtifactError(msg)
        if path.is_dir():
            shutil.copytree(path, kept)
        else:
            kept.mkdir(parents=True)
            shutil.copy2(path, kept / path.name)
        return True

    def download(self, run_id: str, name: str, into: Path) -> None:
        """Copy one artifact's contents into `into`.

        Raises:
            ArtifactError: If that run uploaded no such artifact, in the words
                the action fails with.
        """
        kept = self._kept(run_id, name)
        if not kept.is_dir():
            msg = f"Unable to download artifact(s): Artifact not found for name: {name}"
            raise ArtifactError(msg)
        shutil.copytree(kept, into, dirs_exist_ok=True)

    def download_matching(self, run_id: str, pattern: str, into: Path, *, merge: bool) -> None:
        """Copy every artifact matching `pattern`: merged into `into`, or each under its name."""
        for name in fnmatch.filter(self.names(run_id), pattern):
            self.download(run_id, name, into if merge else into / name)


@dataclass(frozen=True, slots=True)
class WorkflowRun:
    """Every job of one run, by the job's key."""

    jobs: dict[str, JobRun]

    def result(self, job: str) -> Result:
        """What the forge would report `job` as."""
        return self.jobs[job].result

    def commands(self, job: str) -> list[str]:
        """The `run:` commands `job` executed, in order."""
        return [step.command for step in self.jobs[job].steps]


@dataclass(frozen=True, slots=True)
class Needed:
    """What one needed job hands the jobs after it."""

    result: str
    outputs: dict[str, str]


# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
@dataclass(frozen=True, slots=True)
class Event:
    """What fired the run, as the `github` and `inputs` contexts show it.

    `workflow_run` is the triggering run's `event`, `id` and `head_sha`, for a
    run under that event; `inputs` is what a dispatch was given.
    """

    name: str = "push"
    inputs: dict[str, str] = field(default_factory=dict)
    workflow_run: dict[str, str] = field(default_factory=dict)
    #: The ref the run was started on, as the forge names it.
    ref: str = "refs/heads/main"

    def github(self, run_id: str) -> Declared:
        """The `github` context: the event's name, payload and ref, this run's id and token."""
        payload: Declared = {}
        if self.workflow_run:
            payload["workflow_run"] = dict(self.workflow_run)
        if self.inputs:
            payload["inputs"] = dict(self.inputs)
        return {
            "event_name": self.name,
            "event": payload,
            "ref": self.ref,
            "run_id": run_id,
            # The forge's own token, which nothing here can reach a forge with;
            # a boundary given it records this placeholder.
            "token": "<github.token>",
        }


@dataclass(frozen=True, slots=True)
class Contexts:
    """The forge's contexts an expression may read, as one job sees them."""

    needs: dict[str, Needed] = field(default_factory=dict)
    steps: dict[str, dict[str, str]] = field(default_factory=dict)
    secrets: dict[str, str] = field(default_factory=dict)
    matrix: Declared = field(default_factory=dict)
    github: Declared = field(default_factory=dict)
    inputs: dict[str, str] = field(default_factory=dict)
    runner: dict[str, str] = field(default_factory=dict)

    def resolve(self, dotted: str) -> object:
        """The value at one dotted path, or `None` where the forge would give `null`.

        Raises:
            UnsupportedError: If the path starts in a context this runner does
                not carry.
        """
        head, *rest = dotted.split(".")
        match head:
            case "needs":
                value: object = {
                    name: {"result": needed.result, "outputs": needed.outputs}
                    for name, needed in self.needs.items()
                }
            case "steps":
                value = {name: {"outputs": outputs} for name, outputs in self.steps.items()}
            case "secrets":
                value = self.secrets
            case "matrix":
                value = self.matrix
            case "github":
                value = self.github
            case "inputs":
                value = self.inputs
            case "runner":
                value = self.runner
            case _:
                msg = f"the `{head}` context is not one this runner carries"
                raise UnsupportedError(msg)
        for part in rest:
            value = value.get(part) if isinstance(value, dict) else None
        return value


@dataclass(frozen=True, slots=True)
class Token:
    """One token of an expression: what kind it is, and its text."""

    kind: str
    text: str


TOKEN = re.compile(
    r"\s*(?:(?P<string>'(?:[^']|'')*')|(?P<op>==|!=|&&|\|\||[!()])"
    r"|(?P<name>[A-Za-z_][A-Za-z0-9_.-]*))"
)
LITERALS = {"true": True, "false": False, "null": None}


class _Expression:
    """A recursive-descent reader of the forge's expression grammar, over one context set."""

    def __init__(self, text: str, contexts: Contexts) -> None:
        self.text = text
        self.contexts = contexts
        self.tokens = self._tokens(text)
        self.at = 0

    @staticmethod
    def _tokens(text: str) -> list[Token]:
        tokens: list[Token] = []
        position = 0
        while text[position:].strip():
            match = TOKEN.match(text, position)
            if match is None:
                msg = (
                    f"expression `{text}` carries something this runner does not read at {position}"
                )
                raise UnsupportedError(msg)
            kind = str(match.lastgroup)
            tokens.append(Token(kind, match.group(kind)))
            position = match.end()
        return tokens

    def evaluate(self) -> object:
        value = self._or()
        if self.at != len(self.tokens):
            msg = f"expression `{self.text}` has more after what this runner read"
            raise UnsupportedError(msg)
        return value

    def _peek(self) -> Token | None:
        return self.tokens[self.at] if self.at < len(self.tokens) else None

    def _take(self) -> Token:
        token = self.tokens[self.at]
        self.at += 1
        return token

    def _or(self) -> object:
        left = self._and()
        while self._peek() == Token("op", "||"):
            self._take()
            right = self._and()
            left = left if _truthy(left) else right
        return left

    def _and(self) -> object:
        left = self._equality()
        while self._peek() == Token("op", "&&"):
            self._take()
            right = self._equality()
            left = right if _truthy(left) else left
        return left

    def _equality(self) -> object:
        left = self._unary()
        while self._peek() in (Token("op", "=="), Token("op", "!=")):
            operator = self._take().text
            right = self._unary()
            same = _same(left, right)
            left = same if operator == "==" else not same
        return left

    def _unary(self) -> object:
        if self._peek() == Token("op", "!"):
            self._take()
            return not _truthy(self._unary())
        return self._primary()

    def _primary(self) -> object:
        if self._peek() is None:
            msg = f"expression `{self.text}` ends where a value was expected"
            raise UnsupportedError(msg)
        match self._take():
            case Token("string", text):
                return text[1:-1].replace("''", "'")
            case Token("op", "("):
                value = self._or()
                if self._take() != Token("op", ")"):
                    msg = f"expression `{self.text}` opens a parenthesis it does not close"
                    raise UnsupportedError(msg)
                return value
            case Token("name", name) if self._peek() == Token("op", "("):
                msg = f"expression `{self.text}` calls `{name}()`, which this runner does not model"
                raise UnsupportedError(msg)
            case Token("name", name) if name in LITERALS:
                return LITERALS[name]
            case Token("name", name):
                try:
                    return self.contexts.resolve(name)
                except UnsupportedError as outside:
                    msg = f"expression `{self.text}` reads {outside}"
                    raise UnsupportedError(msg) from outside
            case Token(_, text):
                msg = f"expression `{self.text}` has `{text}` where a value was expected"
                raise UnsupportedError(msg)
        return None


def _truthy(value: object) -> bool:
    """The forge's truthiness: `null`, `false`, `0` and `''` are false."""
    return value not in (None, False, 0, "")


def _same(left: object, right: object) -> bool:
    """The forge's equality: strings compare case-insensitively, `null` equals `''`."""
    if isinstance(left, str) and isinstance(right, str):
        return left.casefold() == right.casefold()
    if (left is None and right == "") or (right is None and left == ""):
        return True
    return left == right


def evaluate(expression: str, contexts: Contexts) -> object:
    """Evaluate one bare expression, as an `if:` carries it."""
    return _Expression(expression, contexts).evaluate()


def interpolate(text: str, contexts: Contexts) -> str:
    """Replace every `${{ ... }}` in `text` with what it evaluates to."""

    def replace(match: re.Match[str]) -> str:
        return rendered(evaluate(match.group(1).strip(), contexts))

    return INTERPOLATION.sub(replace, text)


def rendered(value: object) -> str:
    """A value as the forge writes it into a string: `null` empty, booleans lower-case."""
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def _needs(job: Declared) -> list[str]:
    match job.get("needs"):
        case str() as one:
            return [one]
        case list() as several:
            return [str(name) for name in several]
        case _:
            return []


def _ordered(jobs: dict[str, Declared]) -> list[str]:
    """Every job after everything it needs, refusing a cycle."""
    ordered: list[str] = []
    remaining = dict(jobs)
    while remaining:
        ready = [name for name, job in remaining.items() if all(n in ordered for n in _needs(job))]
        if not ready:
            msg = f"jobs {sorted(remaining)} need one another in a cycle"
            raise UnsupportedError(msg)
        for name in ready:
            ordered.append(name)
            del remaining[name]
    return ordered


def _cells(job: Declared) -> list[Declared]:
    """The matrix cells a job runs over: one empty cell where it declares none."""
    strategy = job.get("strategy") or {}
    unknown = set(strategy) - {"matrix", "fail-fast"}
    if unknown:
        msg = f"strategy keys {sorted(unknown)} are not modelled"
        raise UnsupportedError(msg)
    matrix = strategy.get("matrix")
    if not matrix:
        return [{}]
    if not isinstance(matrix, dict) or any(not isinstance(v, list) for v in matrix.values()):
        msg = "a matrix that is not a mapping of lists is not modelled"
        raise UnsupportedError(msg)
    keys = list(matrix)
    return [dict(zip(keys, values, strict=True)) for values in product(*matrix.values())]


def _refuse_unknown(mapping: Declared, allowed: frozenset[str], what: str) -> None:
    unknown = set(mapping) - allowed
    if unknown:
        msg = f"{what} carries {sorted(unknown)}, which this runner does not model"
        raise UnsupportedError(msg)


#: The shape each job key this runner reads must have, checked once at the
#: boundary so that nothing below meets a value it did not expect.
JOB_SHAPES: dict[str, type | tuple[type, ...]] = {
    "needs": (str, list),
    "if": str,
    "outputs": dict,
    "steps": list,
    "strategy": dict,
    "env": dict,
}
STEP_SHAPES: dict[str, type | tuple[type, ...]] = {
    "id": str,
    "run": str,
    "uses": str,
    "with": dict,
    "env": dict,
    "if": str,
}


def _shaped(mapping: Declared, shapes: dict[str, type | tuple[type, ...]], what: str) -> None:
    """Every key `shapes` names carries a value of that shape, or the workflow is refused."""
    for key, shape in shapes.items():
        if key in mapping and not isinstance(mapping[key], shape):
            msg = (
                f"{what} carries `{key}: {mapping[key]!r}`, which is not the shape the forge reads"
            )
            raise UnsupportedError(msg)


def _jobs(workflow: Path) -> dict[str, Declared]:
    """The workflow's jobs, once the file is a workflow at all and each job is shaped as one.

    Each job's steps are held to the modelled set by `_steps` below, once it
    is known which jobs run: a job skipped by name runs no step, so a step of
    it outside the set is nothing this runner approximates.
    """
    data = yaml.safe_load(workflow.read_text(encoding="utf-8"))
    jobs = data.get("jobs") if isinstance(data, dict) else None
    if not isinstance(jobs, dict) or not all(isinstance(job, dict) for job in jobs.values()):
        msg = f"{workflow} is not a workflow: it carries no mapping of jobs"
        raise UnsupportedError(msg)
    for name, job in jobs.items():
        _refuse_unknown(job, JOB_KEYS, f"job `{name}`")
        _shaped(job, JOB_SHAPES, f"job `{name}`")
    return {str(name): job for name, job in jobs.items()}


def _steps(name: str, job: Declared) -> list[Declared]:
    """One job's steps, each shaped as a step this runner models."""
    steps = job.get("steps") or []
    for step in steps:
        if not isinstance(step, dict):
            msg = f"job `{name}` carries a step that is not a mapping: {step!r}"
            raise UnsupportedError(msg)
        _refuse_unknown(step, STEP_KEYS, f"a step of job `{name}`")
        _shaped(step, STEP_SHAPES, f"a step of job `{name}`")
        if ("run" in step) == ("uses" in step):
            msg = f"a step of job `{name}` must carry exactly one of `run` and `uses`"
            raise UnsupportedError(msg)
    return steps


def _environment(*layers: Declared | None, contexts: Contexts) -> dict[str, str]:
    """`env:` mappings evaluated and layered, the later winning."""
    environment: dict[str, str] = {}
    for layer in layers:
        for key, value in (layer or {}).items():
            environment[str(key)] = interpolate(str(value), contexts)
    return environment


# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
class Runner:
    """Run one committed workflow's jobs in a checkout, with stand-ins first on the PATH."""

    def __init__(
        self,
        workflow: Path,
        checkout: Path,
        *,
        path_first: Path,
        env: dict[str, str],
        secrets: dict[str, str] | None = None,
        event: Event | None = None,
        artifacts: ArtifactStore | None = None,
        run_id: str = "1",
    ) -> None:
        """Read the workflow and remember where and how its steps run.

        Args:
            workflow: The committed workflow file.
            checkout: The tree a `checkout` boundary would have produced.
            path_first: A directory of stand-in programs, ahead of everything.
            env: The environment every step runs under.
            secrets: What `secrets.<NAME>` answers; a placeholder per name the
                workflow references if omitted, since nothing a stand-in does
                should need the value.
            event: What fired the run; a push if omitted.
            artifacts: The store the two artifact actions read and write; a
                store of this run's own if omitted.
            run_id: This run's number, which is what its artifacts are kept
                under and what another run's download names.
        """
        self.jobs = _jobs(workflow)
        self.checkout = checkout
        self.path_first = path_first
        self.env = env
        named = set(SECRET_REFERENCE.findall(workflow.read_text(encoding="utf-8")))
        self.secrets: dict[str, str] = secrets or {name: f"<secret {name}>" for name in named}
        self.event = event or Event()
        self.artifacts = artifacts or ArtifactStore(checkout / ".runner-artifacts")
        self.run_id = run_id

    def run(self, only: set[str] | None = None) -> WorkflowRun:
        """Run every job in dependency order, as the forge would schedule them.

        `only` names the jobs to run; every other job is skipped by name, and
        a job needing one of those is skipped as the forge skips it. The steps
        of every job that may run are held to the modelled set before any of
        them does.
        """
        running = set(self.jobs) if only is None else only
        unknown = running - set(self.jobs)
        if unknown:
            msg = f"jobs {sorted(unknown)} are not jobs of this workflow"
            raise UnsupportedError(msg)
        for name in _ordered(self.jobs):
            if name in running:
                _steps(name, self.jobs[name])
        done: dict[str, JobRun] = {}
        for name in _ordered(self.jobs):
            done[name] = (
                self._job(name, self.jobs[name], done)
                if name in running
                else JobRun(Result.SKIPPED)
            )
        return WorkflowRun(done)

    def _job(self, name: str, job: Declared, done: dict[str, JobRun]) -> JobRun:
        needs = _needs(job)
        if any(done[needed].result is not Result.SUCCESS for needed in needs):
            return JobRun(Result.SKIPPED)
        contexts = Contexts(
            needs={n: Needed(str(done[n].result), dict(done[n].outputs)) for n in needs},
            secrets=self.secrets,
            github=self.event.github(self.run_id),
            inputs=dict(self.event.inputs),
        )
        condition = job.get("if")
        if condition is not None and not _truthy(evaluate(str(condition), contexts)):
            return JobRun(Result.SKIPPED)

        run = JobRun(Result.SUCCESS)
        for cell in _cells(job):
            steps: dict[str, dict[str, str]] = {}
            # One `RUNNER_TEMP` per job, as the forge gives it: a file one step
            # writes there is what the step after it reads.
            with tempfile.TemporaryDirectory(prefix="runner-temp-") as runner_temp:

                def seen_by(
                    step_outputs: dict[str, dict[str, str]],
                    cell: Declared = cell,
                    runner_temp: str = runner_temp,
                ) -> Contexts:
                    return Contexts(
                        needs=contexts.needs,
                        steps=step_outputs,
                        secrets=self.secrets,
                        matrix=cell,
                        github=contexts.github,
                        inputs=contexts.inputs,
                        runner={"temp": runner_temp},
                    )

                for step in _steps(name, job):
                    seen = seen_by(steps)
                    if "if" in step and not _truthy(evaluate(str(step["if"]), seen)):
                        continue
                    if "uses" in step:
                        step_run = self._boundary(run, step, seen, Path(runner_temp))
                    else:
                        step_run, outputs = self._step(job, step, seen, Path(runner_temp))
                        if step.get("id"):
                            steps[str(step["id"])] = outputs
                    if step_run is not None:
                        run.steps.append(step_run)
                        if step_run.returncode != 0:
                            run.result = Result.FAILURE
                            return run
            seen = seen_by(steps)
            for key, expression in (job.get("outputs") or {}).items():
                run.outputs[str(key)] = interpolate(str(expression), seen)
        return run

    def _boundary(
        self, run: JobRun, step: Declared, contexts: Contexts, runner_temp: Path
    ) -> StepRun | None:
        """Record a `uses:` step, standing in for the two artifact actions.

        Answers the step's run where the action was stood in for and failed,
        so that the job fails where the forge's would, and nothing otherwise.
        """
        uses = interpolate(str(step["uses"]), contexts)
        given = {
            str(key): interpolate(rendered(value), contexts)
            for key, value in (step.get("with") or {}).items()
        }
        run.boundaries.append(Boundary(uses, given))
        if not uses.startswith((UPLOAD, DOWNLOAD)):
            return None
        try:
            if uses.startswith(UPLOAD):
                where = self._resolved(given.get("path", ""), runner_temp)
                self.artifacts.upload(self.run_id, given.get("name", ""), where)
                return None
            run_id = given.get("run-id") or self.run_id
            into = self._resolved(given.get("path", ""), runner_temp)
            if given.get("name"):
                self.artifacts.download(run_id, given["name"], into)
            else:
                self.artifacts.download_matching(
                    run_id,
                    given.get("pattern", "*"),
                    into,
                    merge=given.get("merge-multiple", "false") == "true",
                )
        except ArtifactError as failed:
            return StepRun(f"uses: {uses}", 1, str(failed))
        return None

    def _resolved(self, path: str, runner_temp: Path) -> Path:
        """A path an action was given, as the forge resolves it: against the checkout.

        Confined to the checkout and the job's own temporary directory, which
        is everything a step of this repository's workflows writes: a path
        an action was given anywhere else is refused rather than read or
        written there.

        Raises:
            UnsupportedError: If the path resolves outside both.
        """
        resolved = (self.checkout / path).resolve() if path else self.checkout.resolve()
        inside = (self.checkout.resolve(), runner_temp.resolve())
        if not any(resolved == root or root in resolved.parents for root in inside):
            msg = (
                f"an artifact path `{path}` outside the checkout and the job's temp is not modelled"
            )
            raise UnsupportedError(msg)
        return resolved

    def _step(
        self, job: Declared, step: Declared, contexts: Contexts, runner_temp: Path
    ) -> tuple[StepRun, dict[str, str]]:
        command = str(step["run"])
        if INTERPOLATION.search(command):
            msg = f"step `{command}` carries an expression in its text, which is not modelled"
            raise UnsupportedError(msg)
        with tempfile.TemporaryDirectory(prefix="runner-output-") as scratch:
            output_file = Path(scratch) / "output"
            output_file.touch()
            env = dict(self.env)
            env["PATH"] = (
                f"{self.path_first}{os.pathsep}{env.get('PATH', os.environ.get('PATH', ''))}"
            )
            env["GITHUB_OUTPUT"] = str(output_file)
            env["RUNNER_TEMP"] = str(runner_temp)
            env.update(_environment(job.get("env"), step.get("env"), contexts=contexts))
            completed = shell_run(
                [*DEFAULT_SHELL, "-c", command],
                cwd=self.checkout,
                env=env,
                timeout=STEP_TIMEOUT_SECONDS,
            )
            outputs = _outputs(output_file.read_text(encoding="utf-8"))
        said = (completed.stdout or "") + (completed.stderr or "")
        return StepRun(command, completed.returncode, said), outputs


def _outputs(text: str) -> dict[str, str]:
    """What a step appended to `GITHUB_OUTPUT`, as `name=value` lines."""
    outputs: dict[str, str] = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        if "<<" in line and "=" not in line.split("<<", 1)[0]:
            msg = "a multi-line `GITHUB_OUTPUT` value is not modelled"
            raise UnsupportedError(msg)
        name, separator, value = line.partition("=")
        if not separator or not name.strip():
            msg = f"`GITHUB_OUTPUT` carries `{line}`, which is not `name=value`"
            raise UnsupportedError(msg)
        outputs[name.strip()] = value
    return outputs
