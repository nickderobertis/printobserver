"""The real-print cases' LFS journeys, over a fixture repository and a real git-lfs.

A small fixture repository carries the tree's own `.gitattributes`,
`.lfsconfig` and git-lfs hooks, and a few kilobytes of LFS objects pushed to a
local LFS remote at a file URL. Every journey drives the real git and git-lfs,
and printobserver's own `real-prints-fetch`, `real-prints-checkout` and
`real-prints-cache-key` recipes out of the committed justfile, and counts the
bytes that remote serves. The committed CI steps (`ci-steps.yml`) are run step
by step: each `run:` for real, and the two cache actions over a directory
standing for the Actions cache.

What the journeys measure is written to `telemetry/lfs.json`, which the
`lfs-bytes-per-ci-run-cache-hit` and `lfs-bytes-per-new-checkout` budgets read.
No figure is held to a bound here: that is the budgets' check.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
from collections.abc import Iterator
from dataclasses import asdict, dataclass
from pathlib import Path

import pytest
import yaml
from counting_remote import MessageError, parse
from lfs_world import (
    COUNTING_REMOTE,
    FIXTURE_OBJECT_BYTES,
    FIXTURE_OBJECTS,
    HERE,
    JUSTFILE,
    LFS_FILES,
    Host,
    differing_from_originals,
    git_lfs_or_fail,
    is_pointer,
    make_host,
    path_without,
    publish_fixture,
)
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run

CI_STEPS = HERE / "ci-steps.yml"
TELEMETRY = HERE / "telemetry" / "lfs.json"


@dataclass(frozen=True, slots=True)
class NewCheckout:
    """What a fresh clone and a new worktree of it were served."""

    clone_bytes: int
    worktree_bytes: int


@dataclass(frozen=True, slots=True)
class Transfer:
    """What one path was served: its bytes and its objects."""

    bytes: int
    objects_served: int


@dataclass(frozen=True, slots=True)
class CiRuns:
    """The CI steps over an empty cache, and over the cache that run saved."""

    cache_key: str
    miss: Transfer
    hit: Transfer


@dataclass
class Recorded:
    """What the journeys measured, filled in as each runs; a path not run is none."""

    new_checkout: NewCheckout | None = None
    fetch: Transfer | None = None
    second_worktree_bytes: int | None = None
    shared_store_second_clone_bytes: int | None = None
    ci: CiRuns | None = None

    def figures(self) -> dict[str, int]:
        """The figures the budgets read, for each path that was measured."""
        figures: dict[str, int] = {}
        if self.new_checkout is not None:
            checkout = self.new_checkout
            figures["new_checkout_bytes"] = checkout.clone_bytes + checkout.worktree_bytes
        if self.ci is not None:
            figures["ci_cache_miss_bytes"] = self.ci.miss.bytes
            figures["ci_cache_hit_bytes"] = self.ci.hit.bytes
        return figures

    def document(self) -> str:
        """The telemetry file: the figures, and every measurement beside the fixture's size."""
        detail = {
            **asdict(self),
            "fixture_object_bytes": FIXTURE_OBJECT_BYTES,
            "fixture_objects": FIXTURE_OBJECTS,
        }
        return json.dumps({"figures": self.figures(), "detail": detail}, indent=2, sort_keys=True)


RECORDED = Recorded()


@pytest.fixture(scope="module", autouse=True)
def telemetry() -> Iterator[None]:
    """Write what the journeys measured once they have all run."""
    yield
    TELEMETRY.parent.mkdir(parents=True, exist_ok=True)
    TELEMETRY.write_text(RECORDED.document() + "\n", encoding="utf-8")


@pytest.fixture(scope="module")
def remote(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The fixture's LFS remote, published once for every journey."""
    git_lfs_or_fail()
    root = tmp_path_factory.mktemp("lfs-remote")
    publisher, _ = publish_fixture(root)
    return publisher.remote


@pytest.fixture
def host(tmp_path: Path, remote: Path) -> Host:
    """A fresh machine with nothing of the fixture on it."""
    return make_host(tmp_path / "host", remote)


def pointers_in(checkout: Path) -> list[str]:
    """The fixture's LFS files a checkout carries as pointers rather than content."""
    return [relative for relative in LFS_FILES if is_pointer(checkout / relative)]


def test_the_published_remote_holds_every_object_the_pre_push_hook_uploaded(remote: Path) -> None:
    """The committed `pre-push` hands the push to git-lfs, which uploads each object once."""
    stored = sorted(
        path.stat().st_size for path in (remote / "lfs" / "objects").rglob("*") if path.is_file()
    )
    equal(len(stored), FIXTURE_OBJECTS, describing="objects the remote's store holds")
    equal(sum(stored), FIXTURE_OBJECT_BYTES, describing="bytes the remote's store holds")


def test_a_fresh_clone_and_a_new_worktree_leave_pointers(host: Host) -> None:
    """`.lfsconfig` excludes every case file, so neither downloads one."""
    clone = host.clone("clone")
    clone_bytes = host.served_bytes()
    equal(pointers_in(clone), list(LFS_FILES), describing="files the clone left as pointers")

    host.forget_served()
    worktree = host.root / "worktree"
    host.git("worktree", "add", "-q", str(worktree), cwd=clone)
    worktree_bytes = host.served_bytes()
    equal(pointers_in(worktree), list(LFS_FILES), describing="files the worktree left as pointers")
    equal(host.git("status", "--porcelain", cwd=worktree), "", describing="the worktree's status")

    RECORDED.new_checkout = NewCheckout(clone_bytes, worktree_bytes)


def test_without_the_exclusion_a_clone_downloads_every_object(host: Host) -> None:
    """The control: the counting remote does see a download, and `.lfsconfig` is what stops it."""
    clone = host.clone("clone", "-c", "lfs.fetchexclude=")
    equal(host.served_bytes(), FIXTURE_OBJECT_BYTES, describing="bytes served with no exclusion")
    equal(differing_from_originals(clone), [], describing="files not their originals")


def test_the_fetch_recipe_serves_the_objects_once_for_every_worktree(host: Host) -> None:
    """`just real-prints-fetch` downloads each object once; a second worktree reuses the store."""
    clone = host.clone("clone")
    host.just("real-prints-fetch", cwd=clone)
    served = host.served()
    equal(
        sum(one.size for one in served), FIXTURE_OBJECT_BYTES, describing="bytes the fetch served"
    )
    equal(len(served), FIXTURE_OBJECTS, describing="objects the fetch served")
    equal(
        differing_from_originals(clone), [], describing="files not their originals after the fetch"
    )
    equal(host.git("status", "--porcelain", cwd=clone), "", describing="the fetched clone's status")

    host.forget_served()
    worktree = host.root / "second-worktree"
    host.git("worktree", "add", "-q", str(worktree), cwd=clone)
    host.just("real-prints-fetch", cwd=worktree)
    equal(host.served_bytes(), 0, describing="bytes a second worktree's fetch served")
    equal(
        differing_from_originals(worktree),
        [],
        describing="files not their originals in the worktree",
    )
    RECORDED.fetch = Transfer(sum(one.size for one in served), len(served))
    RECORDED.second_worktree_bytes = 0


def test_a_second_clone_sharing_the_hosts_lfs_storage_serves_nothing(
    tmp_path: Path, remote: Path
) -> None:
    """A host's `lfs.storage` is one store for every clone, and the fetch recipe reuses it."""
    shared = tmp_path / "shared-lfs-store"
    host = make_host(tmp_path / "host", remote, **{"lfs.storage": str(shared)})
    first = host.clone("first")
    host.just("real-prints-fetch", cwd=first)
    equal(
        host.served_bytes(), FIXTURE_OBJECT_BYTES, describing="bytes the first clone's fetch served"
    )
    truth(any(shared.rglob("objects/*")), describing="objects kept in the host's store")

    host.forget_served()
    second = host.clone("second")
    host.just("real-prints-fetch", cwd=second)
    equal(host.served_bytes(), 0, describing="bytes the second clone's fetch served")
    equal(
        differing_from_originals(second),
        [],
        describing="files not their originals in the second clone",
    )
    RECORDED.shared_store_second_clone_bytes = 0


#: The one condition and the one interpolation the committed steps use.
CONDITION = re.compile(r"steps\.(?P<id>[\w-]+)\.outputs\.(?P<name>[\w-]+) (?P<op>==|!=) 'true'")
INTERPOLATION = re.compile(r"\$\{\{ steps\.(?P<id>[\w-]+)\.outputs\.(?P<name>[\w-]+) \}\}")


def run_ci_steps(host: Host, checkout: Path, cache: Path) -> list[str]:
    """Run the committed CI steps in `checkout`, with `cache` standing for the Actions cache.

    Every `run:` runs for real under bash, `just` reaching the committed
    justfile; `actions/cache/restore` and `actions/cache/save` copy the step's
    path to and from `cache/<key>`, and answer `cache-hit` as the action does.
    A step or an expression this does not know fails the journey rather than
    being skipped. Answers what ran, in order.
    """
    steps = yaml.safe_load(CI_STEPS.read_text(encoding="utf-8"))
    outputs: dict[str, dict[str, str]] = {}
    ran: list[str] = []

    def interpolated(text: str) -> str:
        return INTERPOLATION.sub(lambda found: outputs[found["id"]][found["name"]], text)

    for index, step in enumerate(steps):
        condition = step.get("if")
        if condition is not None:
            parsed = CONDITION.fullmatch(condition)
            if parsed is None:
                pytest.fail(f"step {index}'s condition `{condition}` is in no form this runs")
            holds = outputs.get(parsed["id"], {}).get(parsed["name"], "") == "true"
            if holds != (parsed["op"] == "=="):
                continue
        if "run" in step:
            written = host.root / f"github-output-{index}"
            written.write_text("", encoding="utf-8")
            environment = {
                **host.env,
                "GITHUB_OUTPUT": str(written),
                "JUST_JUSTFILE": str(JUSTFILE),
                "JUST_WORKING_DIRECTORY": str(checkout),
            }
            run(
                ["bash", "-euo", "pipefail", "-c", step["run"]],
                cwd=checkout,
                env=environment,
                check=True,
            )
            lines = written.read_text(encoding="utf-8").splitlines()
            outputs[step.get("id", str(index))] = dict(line.split("=", 1) for line in lines if line)
            ran.append(step["run"])
            continue
        uses = step.get("uses")
        key = interpolated(step["with"]["key"])
        path = checkout / step["with"]["path"]
        saved = cache / key
        match uses:
            case "actions/cache/restore@v4":
                hit = saved.is_dir()
                if hit:
                    shutil.copytree(saved, path, dirs_exist_ok=True)
                outputs[step["id"]] = {"cache-hit": "true" if hit else "false"}
            case "actions/cache/save@v4":
                shutil.copytree(path, saved)
            case _:
                pytest.fail(f"step {index} uses `{uses}`, which this does not run")
        ran.append(f"{uses} {key}")
    return ran


def test_the_ci_steps_fetch_on_a_miss_save_the_store_and_restore_it_on_a_hit(
    host: Host, tmp_path: Path
) -> None:
    """Over an empty cache the steps download and save; over that cache they download nothing."""
    cache = tmp_path / "actions-cache"
    cache.mkdir()
    missed = host.clone("ci-miss")
    ran = run_ci_steps(host, missed, cache)
    miss_bytes = host.served_bytes()
    miss_objects = len(host.served())
    equal(miss_bytes, FIXTURE_OBJECT_BYTES, describing="bytes a cache miss served")
    equal(differing_from_originals(missed), [], describing="files not their originals after a miss")
    contains("\n".join(ran), "just real-prints-fetch")
    truth("just real-prints-checkout" not in ran, describing="no checkout step on a miss")
    saved = [path for path in cache.iterdir() if path.is_dir()]
    equal(len(saved), 1, describing="caches saved by the miss")
    equal(
        sum(path.stat().st_size for path in saved[0].rglob("*") if path.is_file()),
        FIXTURE_OBJECT_BYTES,
        describing="bytes the saved cache holds",
    )

    host.forget_served()
    hit = host.clone("ci-hit")
    ran = run_ci_steps(host, hit, cache)
    hit_bytes = host.served_bytes()
    equal(differing_from_originals(hit), [], describing="files not their originals after a hit")
    truth("just real-prints-fetch" not in ran, describing="no fetch step on a hit")
    contains("\n".join(ran), "just real-prints-checkout")
    equal(len(list(cache.iterdir())), 1, describing="caches after a hit, which saves none")

    RECORDED.ci = CiRuns(
        saved[0].name, Transfer(miss_bytes, miss_objects), Transfer(hit_bytes, len(host.served()))
    )


def key_of(host: Host, checkout: Path) -> str:
    """What the cache-key recipe prints, as the step appends it to `GITHUB_OUTPUT`."""
    printed = host.just("real-prints-cache-key", cwd=checkout).strip()
    truth(
        re.fullmatch(r"key=real-prints-lfs-[0-9a-f]{40}", printed),
        describing=f"a key line, not {printed!r}",
    )
    return printed


def test_the_cache_key_moves_with_an_object_and_not_with_a_plain_file(host: Host) -> None:
    """The key is the objects' ids: an object's change moves it, a `case.json` edit does not."""
    clone = host.clone("keyed")
    first = key_of(host, clone)
    equal(host.served_bytes(), 0, describing="bytes the cache-key recipe served")

    (clone / "tests/real-prints/case-a/case.json").write_text(
        '{"name": "edited"}\n', encoding="utf-8"
    )
    host.git("commit", "-q", "-am", "edit a plain file", cwd=clone)
    equal(key_of(host, clone), first, describing="the key after a plain file changed")

    (clone / "tests/real-prints/case-b/01-frame.jpeg").write_bytes(b"another frame entirely")
    host.git("commit", "-q", "-am", "change one object", cwd=clone)
    truth(key_of(host, clone) != first, describing="a new key after one object changed")


def test_a_push_without_git_lfs_is_refused_naming_bootstrap(host: Host, tmp_path: Path) -> None:
    """The committed `pre-push` refuses a push that would publish pointers without objects."""
    clone = host.clone("pusher")
    host.git("commit", "-q", "--allow-empty", "-m", "anything", cwd=clone)
    host.git("config", "core.hooksPath", (JUSTFILE.parent / ".githooks").as_posix(), cwd=clone)
    bare = path_without("git-lfs", ("git", "sh"), tmp_path / "shims")
    refused = run(
        ["git", "push", "origin", "HEAD:refs/heads/refused"],
        cwd=clone,
        env={**host.env, "PATH": bare},
    )
    truth(refused.returncode != 0, describing=f"a refused push, not:\n{refused.stderr}")
    contains(refused.stderr, "git-lfs is not on PATH")
    contains(refused.stderr, "just bootstrap")


@pytest.mark.parametrize(
    ("line", "why"),
    [
        ("not json", "is not JSON"),
        ("[1, 2]", "is not a message"),
        ('{"event": "delete", "oid": "' + "a" * 64 + '"}', "is no event of the transfer protocol"),
        ('{"event": "download", "oid": "../../escape"}', "is not an object id"),
        ('{"event": "upload", "oid": "' + "a" * 64 + '", "path": 7}', "names no file"),
    ],
)
def test_the_remote_refuses_a_message_it_cannot_act_on(line: str, why: str) -> None:
    """A message is validated before its object id reaches a path in the store."""
    with pytest.raises(MessageError) as refused:
        parse(line)
    contains(str(refused.value), why)


def test_a_session_answers_a_refused_message_and_goes_on(tmp_path: Path) -> None:
    """Over the agent's own stdin and stdout: a bad line is answered with an error, not obeyed."""
    remote = tmp_path / "remote.git"
    (remote / "lfs" / "objects").mkdir(parents=True)
    served = tmp_path / "served"
    environment = {
        **os.environ,
        "LFS_FIXTURE_REMOTE": str(remote),
        "LFS_FIXTURE_SERVED": str(served),
        "LFS_FIXTURE_SCRATCH": str(tmp_path),
    }
    missing = "f" * 64
    session = "\n".join(
        [
            '{"event": "download", "oid": "../../etc/passwd"}',
            '{"event": "init", "operation": "download"}',
            json.dumps({"event": "download", "oid": missing}),
            '{"event": "terminate"}',
        ]
    )
    answer = run(
        [sys.executable, str(COUNTING_REMOTE)], env=environment, stdin=session + "\n", check=True
    )
    answers = [json.loads(line) for line in answer.stdout.splitlines()]
    equal(len(answers), 3, describing=f"one answer a message before terminate: {answers}")
    equal(answers[0]["error"]["code"], 400, describing="the refused message's answer")
    contains(answers[0]["error"]["message"], "is not an object id")
    equal(answers[1], {}, describing="the answer to init, after the refusal")
    equal(answers[2]["error"]["code"], 404, describing="an object the store does not hold")
    equal(served.exists(), False, describing="anything recorded as served")


def test_the_cache_key_recipe_refuses_rather_than_printing_a_key_it_could_not_read(
    host: Host, tmp_path: Path
) -> None:
    """Without git-lfs, or with no case pointer at `HEAD`, it names why and prints no key."""
    clone = host.clone("unkeyable")
    environment = {**host.env, "JUST_JUSTFILE": str(JUSTFILE), "JUST_WORKING_DIRECTORY": str(clone)}
    bare = path_without("git-lfs", ("just", "bash", "git", "cut", "sort"), tmp_path / "shims")
    without = run(["just", "real-prints-cache-key"], cwd=clone, env={**environment, "PATH": bare})
    truth(without.returncode != 0, describing=f"a refusal without git-lfs, not {without.stdout!r}")
    equal(without.stdout, "", describing="a key printed without git-lfs")
    contains(without.stderr, "git-lfs is not installed")
    contains(without.stderr, "run just bootstrap")

    host.git("rm", "-q", "-r", "tests/real-prints", cwd=clone)
    host.git("commit", "-q", "-m", "no cases", cwd=clone)
    empty = run(["just", "real-prints-cache-key"], cwd=clone, env=environment)
    truth(empty.returncode != 0, describing=f"a refusal over no pointers, not {empty.stdout!r}")
    equal(empty.stdout, "", describing="a key printed over no pointers")
    contains(empty.stderr, "git-lfs lists no object under tests/real-prints/ at HEAD")
