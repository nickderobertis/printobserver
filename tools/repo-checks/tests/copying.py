"""The processes `test_isolation_treecopy` runs beside each other.

Each is started as its own interpreter, as an xdist worker or a second Nx suite
is, and says what it saw as one JSON line on standard output:

- `rewrite SOURCE SECONDS` rewrites one schema of `SOURCE` over and over, under
  the exclusive schema lock, truncating it and writing it back in two halves
  with a pause between, as the adapter's journey rewrites a checked-in schema.
- `copy SOURCE INTO SECONDS` copies the schema closure of `SOURCE` into fresh
  directories under `INTO` for as long as it is told, and reports every copied
  schema that was not one whole version of it.
- `break INTO NAME` copies the `ci` check's closure of the committed tree,
  adds a step to the gate job running a recipe only it names, runs the real check over its
  copy and over a second, unbroken one, and reports what each check said.
"""

from __future__ import annotations

import functools
import json
import math
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path

from closures import closure
from repo_checks.checks_ci import continuous_integration
from repo_checks.model import Repo
from treecopy import Closure, SchemaTreeLock, Tree, copy_tree

#: The schema the rewriting process rewrites, relative to its source tree.
SCHEMA = "schemas/printobserver-core/Rewritten.json"

#: Big enough that writing it takes several writes rather than one.
FIELDS = 4000

#: What a copy of the rewritten tree carries.
SCHEMAS = Closure(("schemas/",))

#: The workflow file the breaking processes break, and the step each adds to.
CI = ".github/workflows/ci.yml"
GATE_STEP = '      - run: just check "$(just gate-tier)"\n'


@dataclass(frozen=True, slots=True)
class Rewritten:
    """What the rewriting process did: how many times it rewrote the schema."""

    rewrites: int


@dataclass(frozen=True, slots=True)
class Copied:
    """What one copying process carried: its copies, how many tore, and the first few."""

    copies: int
    torn: int
    first: list[str]


@dataclass(frozen=True, slots=True)
class Broken:
    """What one breaking process's check said of its broken and its unbroken copy."""

    broken: list[str]
    unbroken: list[str]
    workflow: str


@functools.cache
def version(number: int) -> str:
    """One whole version of the rewritten schema, as JSON text."""
    fields = {f"field{n}": {"type": "string"} for n in range(FIELDS)}
    return json.dumps({"version": number, "properties": fields}, indent=2) + "\n"


def rewrite(source: Path, seconds: float) -> Rewritten:
    """Rewrite the schema under the exclusive lock until `seconds` have passed."""
    path = source / SCHEMA
    rewrites = 0
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        text = version(rewrites % 2 + 1)
        held = SchemaTreeLock(source)
        held.lock(exclusive=True)
        try:
            with path.open("w", encoding="utf-8") as handle:
                handle.write(text[: len(text) // 2])
                handle.flush()
                time.sleep(0.02)
                handle.write(text[len(text) // 2 :])
        finally:
            held.release()
        rewrites += 1
        time.sleep(0.005)
    return Rewritten(rewrites)


def copy(source: Path, into: Path, seconds: float) -> Copied:
    """Copy the schema closure until `seconds` have passed, and say what each copy carried."""
    copies = 0
    torn: list[str] = []
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        copies += 1
        copied = copy_tree(into / f"copy{copies}", closure=SCHEMAS, source=source)
        text = (copied / SCHEMA).read_text(encoding="utf-8")
        if text not in (version(1), version(2)):
            torn.append(f"copy{copies}: {len(text)} characters")
    # The first few say what tore; the count says how often.
    return Copied(copies, len(torn), torn[:3])


def breaking(into: Path, name: str) -> Broken:
    """Break this process's own copy, check it and an unbroken copy, and say what each said."""
    ci = closure("ci")
    broken = Tree(copy_tree(into / "broken", closure=ci))
    broken.edit(CI, GATE_STEP, f"{GATE_STEP}      - run: just verify-{name}\n")
    unbroken = Tree(copy_tree(into / "unbroken", closure=ci))
    return Broken(
        continuous_integration(broken.repo),
        continuous_integration(Repo(unbroken.root)),
        broken.read(CI),
    )


def _seconds(given: str) -> float:
    """A duration a role runs for, as a finite number of seconds above none.

    Raises:
        ValueError: If `given` is anything else, which would make a deadline
            that has passed already or never comes.
    """
    seconds = float(given)
    if not math.isfinite(seconds) or seconds <= 0:
        msg = f"a role runs for some seconds, not {given!r}"
        raise ValueError(msg)
    return seconds


def main(argv: list[str]) -> int:
    """Run the role `argv` names and print what it saw."""
    match argv:
        case ["rewrite", source, seconds]:
            said = rewrite(Path(source), _seconds(seconds))
        case ["copy", source, into, seconds]:
            said = copy(Path(source), Path(into), _seconds(seconds))
        case ["break", into, name]:
            said = breaking(Path(into), name)
        case _:
            print(f"copying: no such role: {argv!r}", file=sys.stderr)
            return 2
    print(json.dumps(asdict(said)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
