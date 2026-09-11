"""Run a committed GitHub Actions workflow's jobs here, the way the forge schedules them.

A workflow's scheduling — which job waits on which, which job is skipped, what
one job reads off another's outputs — is decided by the forge, and a journey
cannot fire the forge. What it can do is take the committed workflow's own text
and drive it under the same rules the forge documents, substituting only the
boundaries that reach outside this host: every `uses:` step (a checkout, a
toolchain setup, an artifact upload) is a boundary and runs nothing, and the
programs a caller names as stand-ins — the release program that would reach a
registry and a forge — are found on the PATH first.

The rules kept are exactly the ones this repository's workflows lean on, and
nothing outside them is guessed at: a construct or an expression this runner
does not know is refused by name rather than approximated, so a workflow that
grows past what is modelled here fails the journey loudly.

  * A job runs after every job it `needs`, and only if each of them succeeded —
    the implicit `success()` the forge applies to every job condition — and its
    `if:` expression, where it has one, is truthy. Otherwise it is skipped, and
    what it needs from a skipped job it never reads.
  * `if:` and `${{ }}` expressions are the forge's grammar over the contexts
    this repository's workflows read: `needs.<job>.outputs.<name>`,
    `steps.<id>.outputs.<name>`, `secrets.<NAME>`, `matrix.<...>`, string
    literals, `==`, `!=`, `&&`, `||`, `!` and parentheses. String comparison is
    case-insensitive, as the forge's is.
  * A `run:` step is `bash -e` over the step's text, in the checkout, with the
    step's `env` on top of the caller's, `GITHUB_OUTPUT` a file of its own and
    `RUNNER_TEMP` the job's one directory; `name=value` lines it appends to
    `GITHUB_OUTPUT` become `steps.<id>.outputs.<name>`.
  * A job's `outputs:` are its expressions evaluated once its steps are done.
  * A job with a `strategy.matrix` runs once per cell, and succeeds when every
    cell does.
"""

from __future__ import annotations

import os
import re
import tempfile
from dataclasses import dataclass, field
from enum import StrEnum
from itertools import product
from pathlib import Path
from typing import Any

import yaml
from repo_checks.shell import run as shell_run

#: The step keys this runner knows what to do with. Anything else on a step is
#: a semantic it would otherwise silently drop, so it is refused instead.
STEP_KEYS = frozenset({"id", "name", "run", "uses", "with", "env", "if"})

#: The job keys this runner knows what to do with, on the same terms.
JOB_KEYS = frozenset(
    {"name", "needs", "if", "runs-on", "outputs", "steps", "strategy", "env", "permissions"}
)

#: What the forge documents as the default shell for a `run:` step on Linux.
DEFAULT_SHELL = ("bash", "-e")

STEP_TIMEOUT_SECONDS = 600

#: How a workflow names a secret.
SECRET_REFERENCE = re.compile(r"secrets\.([A-Za-z0-9_]+)")


class UnsupportedError(ValueError):
    """The workflow uses a construct this runner does not model, and will not guess at."""


class Result(StrEnum):
    """What the forge reports a job as, once the run is over."""

    SUCCESS = "success"
    FAILURE = "failure"
    SKIPPED = "skipped"


@dataclass(frozen=True, slots=True)
class StepRun:
    """One `run:` step that ran: its command and what it did."""

    command: str
    returncode: int
    output: str


@dataclass(slots=True)
class JobRun:
    """One job as the run left it."""

    result: Result
    outputs: dict[str, str] = field(default_factory=dict)
    steps: list[StepRun] = field(default_factory=list)
    boundaries: list[str] = field(default_factory=list)

    @property
    def ran(self) -> bool:
        """Whether the job executed at all, whatever it then did."""
        return self.result is not Result.SKIPPED


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


# --- expressions --------------------------------------------------------------

TOKEN = re.compile(
    r"\s*(?:(?P<string>'(?:[^']|'')*')|(?P<op>==|!=|&&|\|\||[!()])"
    r"|(?P<name>[A-Za-z_][A-Za-z0-9_.-]*))"
)


class _Expression:
    """A recursive-descent reader of the forge's expression grammar, over one context set."""

    def __init__(self, text: str, contexts: dict[str, Any]) -> None:
        self.text = text
        self.contexts = contexts
        self.tokens = self._tokens(text)
        self.at = 0

    @staticmethod
    def _tokens(text: str) -> list[tuple[str, str]]:
        tokens: list[tuple[str, str]] = []
        position = 0
        while position < len(text):
            if text[position:].strip() == "":
                break
            match = TOKEN.match(text, position)
            if match is None:
                msg = (
                    f"expression `{text}` carries something this runner does not read at {position}"
                )
                raise UnsupportedError(msg)
            kind = str(match.lastgroup)
            tokens.append((kind, match.group(kind)))
            position = match.end()
        return tokens

    def evaluate(self) -> object:
        value = self._or()
        if self.at != len(self.tokens):
            msg = f"expression `{self.text}` has more after what this runner read"
            raise UnsupportedError(msg)
        return value

    def _peek(self) -> tuple[str, str] | None:
        return self.tokens[self.at] if self.at < len(self.tokens) else None

    def _take(self) -> tuple[str, str]:
        token = self.tokens[self.at]
        self.at += 1
        return token

    def _or(self) -> object:
        left = self._and()
        while self._peek() == ("op", "||"):
            self._take()
            right = self._and()
            left = left if _truthy(left) else right
        return left

    def _and(self) -> object:
        left = self._equality()
        while self._peek() == ("op", "&&"):
            self._take()
            right = self._equality()
            left = right if _truthy(left) else left
        return left

    def _equality(self) -> object:
        left = self._unary()
        while self._peek() in (("op", "=="), ("op", "!=")):
            _, operator = self._take()
            right = self._unary()
            same = _same(left, right)
            left = same if operator == "==" else not same
        return left

    def _unary(self) -> object:
        if self._peek() == ("op", "!"):
            self._take()
            return not _truthy(self._unary())
        return self._primary()

    def _primary(self) -> object:
        token = self._peek()
        if token is None:
            msg = f"expression `{self.text}` ends where a value was expected"
            raise UnsupportedError(msg)
        kind, text = self._take()
        if kind == "string":
            return text[1:-1].replace("''", "'")
        if kind == "op" and text == "(":
            value = self._or()
            if self._take() != ("op", ")"):
                msg = f"expression `{self.text}` opens a parenthesis it does not close"
                raise UnsupportedError(msg)
            return value
        if kind == "name":
            if self._peek() == ("op", "("):
                msg = f"expression `{self.text}` calls `{text}()`, which this runner does not model"
                raise UnsupportedError(msg)
            return self._lookup(text)
        msg = f"expression `{self.text}` has `{text}` where a value was expected"
        raise UnsupportedError(msg)

    def _lookup(self, dotted: str) -> object:
        if dotted in ("true", "false", "null"):
            return {"true": True, "false": False, "null": None}[dotted]
        head, *rest = dotted.split(".")
        if head not in self.contexts:
            msg = f"expression `{self.text}` reads the `{head}` context, which is not carried here"
            raise UnsupportedError(msg)
        value: object = self.contexts[head]
        for part in rest:
            value = value.get(part) if isinstance(value, dict) else None
        return value


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


def evaluate(expression: str, contexts: dict[str, Any]) -> object:
    """Evaluate one bare expression, as an `if:` carries it."""
    return _Expression(expression, contexts).evaluate()


INTERPOLATION = re.compile(r"\$\{\{(.*?)\}\}")


def interpolate(text: str, contexts: dict[str, Any]) -> str:
    """Replace every `${{ ... }}` in `text` with what it evaluates to."""

    def replace(match: re.Match[str]) -> str:
        value = evaluate(match.group(1).strip(), contexts)
        return "" if value is None else str(value)

    return INTERPOLATION.sub(replace, text)


# --- running ------------------------------------------------------------------


def _needs(job: dict[str, Any]) -> list[str]:
    match job.get("needs"):
        case str() as one:
            return [one]
        case list() as several:
            return [str(name) for name in several]
        case _:
            return []


def _ordered(jobs: dict[str, dict[str, Any]]) -> list[str]:
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


def _cells(job: dict[str, Any]) -> list[dict[str, Any]]:
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


def _refuse_unknown(mapping: dict[str, Any], allowed: frozenset[str], what: str) -> None:
    unknown = set(mapping) - allowed
    if unknown:
        msg = f"{what} carries {sorted(unknown)}, which this runner does not model"
        raise UnsupportedError(msg)


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
    ) -> None:
        """Read the workflow and remember where and how its steps run.

        Args:
            workflow: The committed workflow file.
            checkout: The tree a `checkout` boundary would have produced.
            path_first: A directory of stand-in programs, ahead of everything.
            env: The environment every step runs under.
            secrets: What `secrets.<NAME>` answers; a placeholder per name if
                omitted, since nothing a stand-in does should need the value.
        """
        text = workflow.read_text(encoding="utf-8")
        self.jobs: dict[str, dict[str, Any]] = dict(yaml.safe_load(text)["jobs"])
        self.checkout = checkout
        self.path_first = path_first
        self.env = env
        # Every secret the workflow names, each a value that is visibly not it.
        self.secrets: dict[str, str] = secrets or {
            name: f"<secret {name}>" for name in set(SECRET_REFERENCE.findall(text))
        }

    def run(self) -> WorkflowRun:
        """Run every job in dependency order, as the forge would schedule them."""
        done: dict[str, JobRun] = {}
        for name in _ordered(self.jobs):
            done[name] = self._job(name, self.jobs[name], done)
        return WorkflowRun(done)

    def _job(self, name: str, job: dict[str, Any], done: dict[str, JobRun]) -> JobRun:
        _refuse_unknown(job, JOB_KEYS, f"job `{name}`")
        needs = _needs(job)
        if any(done[needed].result is not Result.SUCCESS for needed in needs):
            return JobRun(Result.SKIPPED)
        contexts = self._contexts(needs, done)
        condition = job.get("if")
        if condition is not None and not _truthy(evaluate(str(condition), contexts)):
            return JobRun(Result.SKIPPED)

        run = JobRun(Result.SUCCESS)
        for cell in _cells(job):
            cell_contexts = {**contexts, "matrix": cell}
            steps: dict[str, dict[str, dict[str, str]]] = {}
            # One `RUNNER_TEMP` per job, as the forge gives it: a file one step
            # writes there is what the step after it reads.
            with tempfile.TemporaryDirectory(prefix="runner-temp-") as runner_temp:
                for step in job.get("steps") or []:
                    _refuse_unknown(step, STEP_KEYS, f"a step of job `{name}`")
                    if "uses" in step:
                        run.boundaries.append(interpolate(str(step["uses"]), cell_contexts))
                        continue
                    if "if" in step:
                        msg = f"a step condition on job `{name}` is not modelled"
                        raise UnsupportedError(msg)
                    step_run, outputs = self._step(
                        step, {**cell_contexts, "steps": steps}, Path(runner_temp)
                    )
                    run.steps.append(step_run)
                    if step.get("id"):
                        steps[str(step["id"])] = {"outputs": outputs}
                    if step_run.returncode != 0:
                        run.result = Result.FAILURE
                        return run
            for key, expression in (job.get("outputs") or {}).items():
                run.outputs[str(key)] = interpolate(
                    str(expression), {**cell_contexts, "steps": steps}
                )
        return run

    def _contexts(self, needs: list[str], done: dict[str, JobRun]) -> dict[str, Any]:
        return {
            "needs": {
                n: {"outputs": dict(done[n].outputs), "result": str(done[n].result)} for n in needs
            },
            "secrets": self.secrets,
        }

    def _step(
        self, step: dict[str, Any], contexts: dict[str, Any], runner_temp: Path
    ) -> tuple[StepRun, dict[str, str]]:
        command = str(step["run"])
        with tempfile.TemporaryDirectory(prefix="runner-output-") as scratch:
            output_file = Path(scratch) / "output"
            output_file.touch()
            env = dict(self.env)
            env["PATH"] = (
                f"{self.path_first}{os.pathsep}{env.get('PATH', os.environ.get('PATH', ''))}"
            )
            env["GITHUB_OUTPUT"] = str(output_file)
            env["RUNNER_TEMP"] = str(runner_temp)
            for key, value in (step.get("env") or {}).items():
                env[str(key)] = interpolate(str(value), contexts)
            completed = shell_run(
                [*DEFAULT_SHELL, "-c", interpolate(command, contexts)],
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
        if not separator:
            msg = f"`GITHUB_OUTPUT` carries `{line}`, which is not `name=value`"
            raise UnsupportedError(msg)
        outputs[name.strip()] = value
    return outputs
