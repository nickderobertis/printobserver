"""Writing the three clients, and refusing a tree in which one has drifted.

The generator writes **in place**: regenerating over the committed tree leaves
it unchanged, so `check` is the same walk as `write` with the writing left out
and the differences reported. That is what makes the drift gate a gate over
correspondence rather than over a checksum — a hand-written type that agrees
today fails the moment the schema it was copied from moves.

Every file is handed to its own language's formatter on the way out, which is
the same formatter `just format-check` runs. Without that the gate would have
two opinions about the generated tree: the generator's and the formatter's, and
whichever ran second would refuse what the other wrote.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from repo_checks.shell import run

from contract_codegen import emit_python, emit_rust, emit_typescript
from contract_codegen.model import Contract
from contract_codegen.schemas import load

#: How long any one formatter is given. Generous: the first `rustfmt` of a
#: session pays for a cold toolchain, and a generator that gave up on that
#: would report a drift that is nothing but a slow start.
FORMAT_TIMEOUT_SECONDS = 300


@dataclass(frozen=True, slots=True)
class Output:
    """One file the generator owns, and how it is written."""

    #: Where it lives, relative to the repository root.
    path: str
    #: Which client it belongs to, which is also which formatter lays it out.
    client: str
    #: What writes it, from the contract.
    emit: Callable[[Contract], str]


#: Every file the generator owns, in the order it writes them.
OUTPUTS: tuple[Output, ...] = (
    Output(
        path="crates/printobserver-sdk/src/contract.rs",
        client="rust",
        emit=emit_rust.emit,
    ),
    Output(
        path="crates/printobserver-sdk/tests/operations.rs",
        client="rust",
        emit=emit_rust.emit_walk,
    ),
    Output(
        path="crates/printobserver-sdk/tests/live.rs",
        client="rust",
        emit=emit_rust.emit_live,
    ),
    Output(
        path="python/printobserver-sdk/src/printobserver_sdk/contract.py",
        client="python",
        emit=emit_python.emit,
    ),
    Output(
        path="python/printobserver-sdk/tests/test_operations.py",
        client="python",
        emit=emit_python.emit_walk,
    ),
    Output(
        path="python/printobserver-sdk/integration/test_live.py",
        client="python",
        emit=emit_python.emit_live,
    ),
    Output(
        path="npm/printobserver-sdk/src/contract.ts",
        client="typescript",
        emit=emit_typescript.emit,
    ),
    Output(
        path="npm/printobserver-sdk/integration/live.test.ts",
        client="typescript",
        emit=emit_typescript.emit_live,
    ),
    Output(
        path="npm/printobserver-sdk/test/operations.test.ts",
        client="typescript",
        emit=emit_typescript.emit_walk,
    ),
)


class FormatterError(RuntimeError):
    """A generated file's own formatter would not read what was generated."""


def _formatter(client: str, path: str, root: Path) -> list[str]:
    """The command that lays out one generated file, reading it on standard input.

    Each is the formatter `just format-check` runs over that client, given the
    file's own path so that the configuration which decides how to lay it out
    is the one the gate will hold it to. None of them touches the tree, which
    is what lets the drift check run all three over files it must not write.

    Raises:
        FormatterError: If the client is not one of the three.
    """
    if client == "rust":
        return ["rustfmt", "--edition", "2024", "--emit", "stdout"]
    if client == "python":
        return ["ruff", "format", "--stdin-filename", path, "-"]
    if client == "typescript":
        # Not `bunx`: biome reads `biome.json`'s own include list, and the
        # binary the locked install put in the tree is the version that list
        # was written for.
        return [
            str(root / "node_modules" / ".bin" / "biome"),
            "format",
            f"--stdin-file-path={path}",
        ]
    msg = f"there is no `{client}` client for a generated file to be laid out for"
    raise FormatterError(msg)


def formatted(text: str, client: str, path: str, root: Path) -> str:
    """One generated file, laid out by the formatter the gate holds it to.

    Raises:
        FormatterError: If the formatter is absent or refused the text. Writing
            what a formatter would not accept would leave a tree the gate
            refuses and the generator calls finished.
    """
    program = _formatter(client, path, root)
    result = run(program, cwd=root, timeout=FORMAT_TIMEOUT_SECONDS, stdin=text)
    if result.returncode != 0:
        msg = (
            f"`{program[0]}` refused what was generated for {path} "
            f"({result.returncode}): {result.stderr.strip()}"
        )
        raise FormatterError(msg)
    return result.stdout


def rendered(contract: Contract, root: Path) -> dict[str, str]:
    """Every generated file's whole text, by the path it belongs at."""
    return {
        output.path: formatted(output.emit(contract), output.client, output.path, root)
        for output in OUTPUTS
    }


def write(root: Path) -> list[str]:
    """Write every generated file, answering the ones that changed."""
    changed: list[str] = []
    for path, text in rendered(load(root), root).items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.is_file() or target.read_text(encoding="utf-8") != text:
            target.write_text(text, encoding="utf-8")
            changed.append(path)
    return changed


def drifted(root: Path) -> list[str]:
    """Every way the committed clients differ from what the generator writes."""
    findings: list[str] = []
    for path, text in rendered(load(root), root).items():
        target = root / path
        if not target.is_file():
            findings.append(
                f"{path} is absent: the generator writes it, and nothing in the tree "
                f"carries what it wrote"
            )
            continue
        found = target.read_text(encoding="utf-8")
        if found != text:
            findings.extend(
                f"{path} is not what the generator writes: {difference}"
                for difference in _differences(found, text)
            )
    return findings


def _differences(found: str, expected: str) -> list[str]:
    """Every line one generated file differs from what it should be by.

    Named rather than counted: a drift check that said only *that* a file had
    moved would leave a reader diffing by hand to find which field it was.
    """
    import difflib

    named = [
        line.rstrip()
        for line in difflib.unified_diff(
            found.splitlines(),
            expected.splitlines(),
            fromfile="committed",
            tofile="generated",
            lineterm="",
            n=0,
        )
        if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))
    ]
    return named or ["the two differ in whitespace alone"]
