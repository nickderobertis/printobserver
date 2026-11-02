"""The real-print cases' Git LFS set, read from the pointers alone.

Two verbs, each reading what `git lfs ls-files --long` lists under
`tests/real-prints/` at `HEAD` and the pointer git holds for each file, from
the repository the working directory is in. None downloads anything: a pointer
names its object's id and size, which is all any of them needs.

* `miss-mb` is the `lfs-bytes-per-ci-run-cache-miss` budget's command: the
  bytes a fetch over an empty store downloads, every distinct object once, in
  megabytes (10^6 bytes).
* `monthly-gib` is the `lfs-bandwidth-gib-per-month` budget's command: that set
  times the full fetches a landing month makes, in GiB.

Both budget verbs report through the onebudgetspec SDK, which writes the
figure where a check asks for it and nothing otherwise, and print it either way.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

from onebudgetspec_sdk import report
from repo_checks.shell import run

#: Where the cases are.
ROOT = "tests/real-prints/"

#: One line of `git lfs ls-files --long`: the object id, `*` or `-`, the path.
LISTED = re.compile(r"(?P<oid>[0-9a-f]{64}) [*-] (?P<path>.+)")
SIZE = re.compile(r"^size (?P<size>[0-9]+)$", re.MULTILINE)

#: A megabyte and a gibibyte, in bytes.
MB = 10**6
GIB = 2**30

#: The full fetches the landing month makes: CI cache misses, and fetches on
#: hosts whose clones share no store. A typical month after it, for the detail.
CI_MISSES = 4
HOST_FETCHES = 16
TYPICAL_CI_MISSES = 4
TYPICAL_HOST_FETCHES = 2


class SetError(RuntimeError):
    """Why the set could not be read, in words naming what to do."""


@dataclass(frozen=True, slots=True)
class Tracked:
    """One file of the set: where it is, and the object its pointer names."""

    path: str
    oid: str
    size: int

    @property
    def case(self) -> str:
        """The case directory the file belongs to."""
        return self.path.removeprefix(ROOT).split("/", 1)[0]


def _git(argv: list[str], *, stdin: str | None = None) -> str:
    """Run git in the working directory and answer what it printed, or refuse."""
    answer = run(["git", *argv], cwd=Path.cwd(), stdin=stdin)
    if answer.returncode != 0:
        msg = f"`git {' '.join(argv)}` failed: {answer.stderr.strip()}"
        raise SetError(msg)
    return answer.stdout


def tracked() -> list[Tracked]:
    """Every file of the set, with the size its committed pointer declares."""
    listed = [
        found
        for line in _git(["lfs", "ls-files", "--long"]).splitlines()
        if (found := LISTED.fullmatch(line.strip())) and found["path"].startswith(ROOT)
    ]
    if not listed:
        msg = f"git-lfs lists no file under {ROOT} at HEAD"
        raise SetError(msg)
    pointers = _git(
        ["cat-file", "--batch"], stdin="".join(f"HEAD:{found['path']}\n" for found in listed)
    )
    sizes = [int(size) for size in SIZE.findall(pointers)]
    if len(sizes) != len(listed):
        msg = f"read {len(sizes)} pointer sizes for the {len(listed)} files git-lfs lists"
        raise SetError(msg)
    return [
        Tracked(found["path"], found["oid"], size)
        for found, size in zip(listed, sizes, strict=True)
    ]


def distinct(files: list[Tracked]) -> dict[str, int]:
    """Each object the set holds, once, with its size: identical files are one object."""
    return {file.oid: file.size for file in files}


def _per_case(files: list[Tracked]) -> dict[str, int]:
    """The bytes each case's own distinct objects hold."""
    seen: dict[str, set[str]] = defaultdict(set)
    sized: dict[str, int] = defaultdict(int)
    for file in files:
        if file.oid not in seen[file.case]:
            seen[file.case].add(file.oid)
            sized[file.case] += file.size
    return dict(sorted(sized.items()))


def miss_mb(files: list[Tracked]) -> tuple[float, dict[str, object]]:
    """What a fetch over an empty store downloads, in MB, and its breakdown."""
    objects = distinct(files)
    total = sum(objects.values())
    return total / MB, {
        "bytes": total,
        "objects": len(objects),
        "files": len(files),
        "bytes_per_case": _per_case(files),
    }


def monthly_gib(files: list[Tracked]) -> tuple[float, dict[str, object]]:
    """The landing month's LFS download, in GiB, and its breakdown."""
    total = sum(distinct(files).values())
    fetches = CI_MISSES + HOST_FETCHES
    typical = TYPICAL_CI_MISSES + TYPICAL_HOST_FETCHES
    return total * fetches / GIB, {
        "set_bytes": total,
        "fetches": {"ci_cache_misses": CI_MISSES, "host_fetches": HOST_FETCHES},
        "typical_month": {
            "ci_cache_misses": TYPICAL_CI_MISSES,
            "host_fetches": TYPICAL_HOST_FETCHES,
            "gib": total * typical / GIB,
        },
    }


VERBS = ("miss-mb", "monthly-gib")


def main(argv: list[str]) -> int:
    """Run one verb over the set the working directory's repository holds."""
    if len(argv) != 1 or argv[0] not in VERBS:
        print(f"usage: lfs_set.py {{{','.join(VERBS)}}}", file=sys.stderr)
        return 2
    try:
        files = tracked()
    except SetError as error:
        print(f"lfs_set: {error}", file=sys.stderr)
        return 1
    value, detail = miss_mb(files) if argv[0] == "miss-mb" else monthly_gib(files)
    report(value, json.dumps(detail, sort_keys=True))
    print(f"{argv[0]}: {value:.4f} ({json.dumps(detail, sort_keys=True)})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
