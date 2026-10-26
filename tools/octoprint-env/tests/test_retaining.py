"""What a failed tier keeps of the OctoPrint instance it ran against.

The first journey makes the tier's own recipe fail on purpose once a real
instance has written all three of its logs, and finds them kept before the
bring-down, with the instance named, still readable after it. The two after it
leave an instance short of a log in the way a real start does — refused before
anything started, or started and dead before OctoPrint set its logging up — and
find exactly the logs it never wrote reported absent, by name, and every one it
did write kept as it wrote it, its secrets redacted.
"""

from __future__ import annotations

import json
import os
import shutil
from collections.abc import Callable
from pathlib import Path

from environment import SCRIPT, answer, script
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import run

#: Every log a failed tier keeps, in the order `retain` reports them, and where
#: each sits under the instance's state directory.
WRITTEN = {
    "server.log": "server.log",
    "logs/octoprint.log": "instance/logs/octoprint.log",
    "logs/serial.log": "instance/logs/serial.log",
}
REQUIRED = list(WRITTEN)

#: The checkout whose recipes the tier runs from.
REPO_ROOT = SCRIPT.parents[2]


def test_a_tier_that_fails_keeps_every_log_before_the_bring_down(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:
    """The tier's own recipe, failed on purpose, keeps the instance's logs before teardown.

    The instance is a real one this journey brought up, and has written all
    three logs by the time the tier runs. The graph runner the recipe hands the
    tier to is one that fails at once, on a path of this journey's own:
    everything else — the recipe, the `retain` it runs and the bring-down after
    it — is the committed one.
    """
    state = Path(state_dir("tier"))
    kept = tmp_path / "kept"
    failing_runner = tmp_path / "failing-runner"
    failing_runner.mkdir()
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    runner = failing_runner / "bunx"
    runner.write_text(
        "#!/bin/sh\necho 'a deliberately failing tier' >&2\nexit 1\n", encoding="utf-8"
    )
    runner.chmod(0o755)

    started = answer(script("up", "--state-dir", str(state)))
    try:
        for written in WRITTEN.values():
            truth((state / written).is_file(), describing=f"the instance to have {written}")
        tier = run(
            ["just", "test-integration"],
            cwd=REPO_ROOT,
            timeout=600,
            env={
                **os.environ,
                "PATH": f"{failing_runner}{os.pathsep}{os.environ['PATH']}",
                "OCTOPRINT_ENV_STATE_DIR": str(state),
                "OCTOPRINT_ENV_RETAIN_DIR": str(kept),
            },
        )
    finally:
        passing(script("down", "--state-dir", str(state)), describing="the bring-down")

    truth(tier.returncode != 0, describing="the deliberately failed tier to fail")
    contains(
        tier.stdout + tier.stderr,
        "a deliberately failing tier",
        describing="where the tier failed",
    )
    retained = json.loads((kept / "retained.json").read_text(encoding="utf-8"))
    equal(
        retained["instance"]["state_dir"],
        str(state.resolve()),
        describing="the instance the kept logs name",
    )
    equal(
        retained["instance"]["record"]["pid"],
        started["pid"],
        describing="the process the kept logs name",
    )
    equal(retained["retained"], REQUIRED, describing="the logs kept")
    equal(retained["absent"], [], describing="the logs reported absent")
    for name, written in WRITTEN.items():
        text = (kept / name).read_text(encoding="utf-8")
        truth(bool(text), describing=f"the kept {name} to carry what the instance wrote")
        truth(
            (state / written).read_text(encoding="utf-8", errors="replace").startswith(text),
            describing=f"the kept {name} to be what the instance wrote to {written}",
        )


def _retained(state: str, into: Path) -> dict[str, object]:
    kept = script("retain", "--state-dir", state, "--into", str(into))
    passing(kept, describing="keeping the instance's logs")
    record = answer(kept)
    equal(
        json.loads((into / "retained.json").read_text(encoding="utf-8")),
        record,
        describing="the record kept beside the logs, against what `retain` answered",
    )
    equal(
        record["instance"],
        {"state_dir": str(Path(state).resolve()), "record": None},
        describing="the instance the kept logs name",
    )
    return record


def test_an_instance_refused_before_it_started_has_every_log_reported_absent(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:
    """Refused before anything was provisioned, it wrote no log, and each is named."""
    state = state_dir("refused")
    into = tmp_path / "kept"
    refused = script(
        "up", "--state-dir", state, "--mode", "serial", "--device", str(tmp_path / "no-device")
    )
    failing(refused, naming="device")

    record = _retained(state, into)

    equal(record["retained"], [], describing="the logs kept")
    equal(record["absent"], REQUIRED, describing="the logs reported absent")
    for name in REQUIRED:
        truth(not (into / name).exists(), describing=f"no kept {name}")


def test_an_instance_that_died_before_logging_keeps_what_it_said_and_names_the_rest(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:
    """Started and dead before its logging was set up: its own output is kept, the rest named.

    OctoPrint's log directory is made a file before the start, so the server
    process says why it cannot go on and exits before it writes either of
    OctoPrint's own logs — and a copy an earlier run left of one of those is
    removed rather than passed off as this run's.
    """
    state = state_dir("died")
    into = tmp_path / "kept"
    passing(script("install", "--state-dir", state), describing="provisioning the instance")
    logs = Path(state) / "instance" / "logs"
    shutil.rmtree(logs, ignore_errors=True)
    logs.write_text("not a directory\n", encoding="utf-8")
    (into / "logs").mkdir(parents=True)
    (into / "logs" / "serial.log").write_text("an earlier run's serial log\n", encoding="utf-8")
    failing(
        script("up", "--state-dir", state, "--start-timeout", "60"),
        naming="the server exited before it answered",
    )

    # What the server said is uploaded once kept, so a secret of the instance's
    # own that reached its log — here, its key and its account's password —
    # is kept redacted.
    secrets = [
        (Path(state) / name).read_text(encoding="utf-8").strip()
        for name in ("api-key", "account-password")
    ]
    with (Path(state) / "server.log").open("a", encoding="utf-8") as log:
        log.write(f"X-Api-Key: {secrets[0]}\nlogging in with {secrets[1]}\n")

    record = _retained(state, into)

    equal(record["retained"], ["server.log"], describing="the logs kept")
    equal(record["absent"], REQUIRED[1:], describing="the logs reported absent")
    said = (Path(state) / "server.log").read_text(encoding="utf-8")
    truth(bool(said), describing="the server to have said something before it died")
    redacted = said
    for secret in secrets:
        redacted = redacted.replace(secret, "[redacted]")
    kept = (into / "server.log").read_text(encoding="utf-8")
    equal(kept, redacted, describing="the kept server.log, against what the server wrote")
    for secret in secrets:
        truth(secret not in kept, describing="no secret of the instance's in what was kept")
    for name in REQUIRED[1:]:
        truth(not (into / name).exists(), describing=f"no kept {name}")
