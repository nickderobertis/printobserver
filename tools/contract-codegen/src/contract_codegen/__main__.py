"""`write` the three clients, or `check` that the committed ones are what it writes."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from contract_codegen.generate import drifted, write


def main(argv: list[str] | None = None) -> int:
    """Run one of the generator's two commands, answering a process exit status."""
    parser = argparse.ArgumentParser(prog="contract-codegen", description=__doc__)
    parser.add_argument("command", choices=["write", "check"])
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="the repository whose schemas are read and whose clients are written",
    )
    arguments = parser.parse_args(argv)

    if arguments.command == "write":
        changed = write(arguments.root)
        for path in changed:
            print(f"wrote {path}", file=sys.stderr)
        if not changed:
            print("every generated client was already what the schemas write", file=sys.stderr)
        return 0

    findings = drifted(arguments.root)
    for finding in findings:
        print(finding, file=sys.stderr)
    if findings:
        print(
            f"{len(findings)} difference(s): a generated client is not what the "
            f"committed schemas write. Run `just generate-clients` and commit what "
            f"it writes.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
