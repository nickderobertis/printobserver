"""The real-print cases' Git LFS set, read from the pointers alone.

Two verbs, each reading what `git lfs ls-files --long` lists under
`tests/real-prints/` at `HEAD` and the pointer git holds for each file, from
the repository the working directory is in. None downloads anything: a pointer
names its object's id and size, which is all either needs.

* `miss-mb` is the `lfs-bytes-per-ci-run-cache-miss` budget's command: the
  bytes a fetch over an empty store downloads, every distinct object once, in
  megabytes (10^6 bytes).
* `monthly-gib` is the `lfs-bandwidth-gib-per-month` budget's command: that set
  times the full fetches a landing month makes, in GiB.

Both report through the onebudgetspec SDK, which writes the figure where a
check asks for it and nothing otherwise, and print it either way. Every line
git answers is validated before anything is summed: a listing line that is not
one, a pointer that is not a whole pointer, or one naming another object than
its listing refuses the measurement rather than being left out of it.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import NewType

from onebudgetspec_sdk import report
from repo_checks.shell import run

#: Where the cases are.
ROOT = "tests/real-prints/"

#: One line of `git lfs ls-files --long`: the object id, `*` or `-`, the path.
LISTED = re.compile(r"(?P<oid>[0-9a-f]{64}) [*-] (?P<path>.+)")
#: One object's header in `git cat-file --batch` output.
HEADER = re.compile(r"(?P<blob>[0-9a-f]{40,64}) blob (?P<size>[0-9]+)")
#: The whole of a Git LFS pointer, as the specification orders its lines.
POINTER = re.compile(
    r"version https://git-lfs\.github\.com/spec/v1\n"
    r"oid sha256:(?P<oid>[0-9a-f]{64})\n"
    r"size (?P<size>0|[1-9][0-9]*)\n"
)

#: A megabyte and a gibibyte, in bytes.
MB = 10**6
GIB = 2**30

#: The full fetches the landing month makes: CI cache misses, and fetches on
#: hosts whose clones share no store. A typical month after it, for the detail.
CI_MISSES = 4
HOST_FETCHES = 16
TYPICAL_CI_MISSES = 4
TYPICAL_HOST_FETCHES = 2

#: A Git LFS object id: the sha256 of the file it stands for.
Oid = NewType("Oid", str)


class SetError(RuntimeError):
    """Why the set could not be read, in words naming what to do."""


@dataclass(frozen=True, slots=True)
class Tracked:
    """One file of the set: where it is, and the object its pointer names."""

    path: str
    oid: Oid
    size: int

    @property
    def case(self) -> str:
        """The case directory the file belongs to."""
        return self.path.removeprefix(ROOT).split("/", 1)[0]


@dataclass(frozen=True, slots=True)
class MissDetail:
    """What a fetch over an empty store downloads."""

    bytes: int
    objects: int
    files: int
    bytes_per_case: dict[str, int]


@dataclass(frozen=True, slots=True)
class Fetches:
    """How many full fetches a month makes, by where they happen."""

    ci_cache_misses: int
    host_fetches: int


@dataclass(frozen=True, slots=True)
class MonthDetail:
    """The landing month's fetches, and a typical month's for comparison."""

    set_bytes: int
    fetches: Fetches
    typical_month: Fetches
    typical_month_gib: float


@dataclass(frozen=True, slots=True)
class Measurement:
    """One budget's figure, and the breakdown reported beside it."""

    value: float
    detail: MissDetail | MonthDetail

    def detail_json(self) -> str:
        """The breakdown as the JSON text the SDK reports."""
        return json.dumps(asdict(self.detail), sort_keys=True)


def _git(argv: list[str], *, stdin: str | None = None) -> str:
    """Run git in the working directory and answer what it printed, or refuse."""
    answer = run(["git", *argv], cwd=Path.cwd(), stdin=stdin)
    if answer.returncode != 0:
        msg = f"`git {' '.join(argv)}` failed: {answer.stderr.strip()}"
        raise SetError(msg)
    return answer.stdout


def _listed() -> list[tuple[str, Oid]]:
    """Every file git-lfs lists under the cases at `HEAD`, as `(path, oid)`.

    Raises:
        SetError: If a line is not a listing line, or none is under the cases.
    """
    listed: list[tuple[str, Oid]] = []
    for line in _git(["lfs", "ls-files", "--long"]).splitlines():
        found = LISTED.fullmatch(line.strip())
        if found is None:
            msg = f"`git lfs ls-files --long` answered {line!r}, which is no listing line"
            raise SetError(msg)
        if found["path"].startswith(ROOT):
            listed.append((found["path"], Oid(found["oid"])))
    if not listed:
        msg = f"git-lfs lists no file under {ROOT} at HEAD"
        raise SetError(msg)
    return listed


def _pointers(paths: list[str]) -> list[str]:
    """The pointer text git holds at `HEAD` for each path, in order.

    Raises:
        SetError: If git answers anything but one blob a path.
    """
    batch = _git(["cat-file", "--batch"], stdin="".join(f"HEAD:{path}\n" for path in paths))
    texts: list[str] = []
    position = 0
    for path in paths:
        end = batch.find("\n", position)
        header = HEADER.fullmatch(batch[position:end]) if end >= 0 else None
        if header is None:
            msg = f"git holds no blob for {path} at HEAD"
            raise SetError(msg)
        start = end + 1
        position = start + int(header["size"]) + 1
        texts.append(batch[start : position - 1])
    return texts


def tracked() -> list[Tracked]:
    """Every file of the set, with the size its committed pointer declares.

    Raises:
        SetError: If a listing line or a pointer is malformed, a pointer names
            another object than git-lfs listed for its file — a change staged
            and not committed — or two pointers give one object two sizes.
    """
    listed = _listed()
    files: list[Tracked] = []
    for (path, oid), text in zip(listed, _pointers([path for path, _ in listed]), strict=True):
        pointer = POINTER.fullmatch(text)
        if pointer is None:
            msg = f"{path} is listed by git-lfs and what HEAD holds for it is no whole pointer"
            raise SetError(msg)
        if pointer["oid"] != oid:
            msg = f"{path}'s pointer names {pointer['oid']}, and git-lfs lists {oid}"
            raise SetError(msg)
        files.append(Tracked(path, oid, int(pointer["size"])))
    sizes: dict[Oid, Tracked] = {}
    for file in files:
        first = sizes.setdefault(file.oid, file)
        if first.size != file.size:
            msg = (
                f"{first.path} and {file.path} name one object, {file.oid}, at two sizes: "
                f"{first.size} and {file.size} bytes"
            )
            raise SetError(msg)
    return files


def distinct(files: list[Tracked]) -> dict[Oid, int]:
    """Each object the set holds, once, with its size: identical files are one object."""
    return {file.oid: file.size for file in files}


def _per_case(files: list[Tracked]) -> dict[str, int]:
    """The bytes each case's own distinct objects hold."""
    seen: dict[str, set[Oid]] = defaultdict(set)
    sized: dict[str, int] = defaultdict(int)
    for file in files:
        if file.oid not in seen[file.case]:
            seen[file.case].add(file.oid)
            sized[file.case] += file.size
    return dict(sorted(sized.items()))


def miss_mb(files: list[Tracked]) -> Measurement:
    """What a fetch over an empty store downloads, in MB, and its breakdown."""
    objects = distinct(files)
    total = sum(objects.values())
    detail = MissDetail(total, len(objects), len(files), _per_case(files))
    return Measurement(total / MB, detail)


def monthly_gib(files: list[Tracked]) -> Measurement:
    """The landing month's LFS download, in GiB, and its breakdown."""
    total = sum(distinct(files).values())
    landing = Fetches(CI_MISSES, HOST_FETCHES)
    typical = Fetches(TYPICAL_CI_MISSES, TYPICAL_HOST_FETCHES)
    typical_gib = total * (typical.ci_cache_misses + typical.host_fetches) / GIB
    detail = MonthDetail(total, landing, typical, typical_gib)
    return Measurement(total * (landing.ci_cache_misses + landing.host_fetches) / GIB, detail)


VERBS = {"miss-mb": miss_mb, "monthly-gib": monthly_gib}


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
    measured = VERBS[argv[0]](files)
    report(measured.value, measured.detail_json())
    print(f"{argv[0]}: {measured.value:.4f} ({measured.detail_json()})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
