"""What the scripted environment answers on each host it is run on.

One of the script's answers depends on the host: where a virtual environment
keeps its programs. The journey here runs the script's own entry point with the
host's answer to `sys.platform` fixed — Windows', and this suite's own for
contrast — over a state directory laid out the way that host lays one out, and
reads back what the script said. So the answer is proven on whichever host the
suite runs, rather than only on the one the tier happens to be on. How a serial
device is named and opened on each host is `test_device_refusal.py`'s.

The last journey is the one no fixture can reach: stopping a real process, which
on Windows goes through the process table and `taskkill` rather than signals. It
runs against the host it is on, so each platform's cell proves its own answer.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import octoprint_env
import pytest
import yaml
from environment import running
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run as shell_run
from repo_checks.shell import start


def _virtual_printer_module(state: Path) -> Path:
    """Where an earlier install on this host left the virtual printer's module.

    Windows keeps an environment's packages under `Lib`, every other host under
    `lib/python<version>`.
    """
    if sys.platform == "win32":
        site = state / "venv" / "Lib" / "site-packages"
    else:
        site = state / "venv" / "lib" / "python3.11" / "site-packages"
    return site.joinpath(*octoprint_env.VIRTUAL_PRINTER_MODULE)


def _host_program() -> Path:
    """Where this host's environments keep the OctoPrint program, relative to one."""
    return octoprint_env.environment_program(Path(), "octoprint")


def _provisioned_over(state: Path, program: Path) -> None:
    """A state directory an earlier install left, its OctoPrint at `program`.

    The account and its key are there too, so an install over it has nothing
    left to do but say so — which is what makes its answer readable.
    """
    (state / "venv" / program).parent.mkdir(parents=True)
    (state / "venv" / program).write_text("an installed program\n", encoding="utf-8")
    module = _virtual_printer_module(state)
    module.parent.mkdir(parents=True)
    module.write_bytes(b"# the release's own module\n" + octoprint_env.HEATER_DEFECT)
    (state / "instance").mkdir(parents=True)
    (state / "instance" / "users.yaml").write_text(
        yaml.safe_dump({octoprint_env.ACCOUNT: {"apikey": "a-provisioned-key"}}),
        encoding="utf-8",
    )


@pytest.mark.parametrize(
    ("platform", "program"),
    [
        pytest.param("win32", Path("Scripts") / "octoprint.exe", id="windows"),
        pytest.param("linux", Path("bin") / "octoprint", id="linux"),
    ],
)
def test_an_installed_instance_is_found_where_the_host_keeps_its_programs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    platform: str,
    program: Path,
) -> None:
    """Windows keeps an environment's programs under `Scripts`, with `.exe`.

    Looked for under `bin` there, an installed OctoPrint is never found: every
    run installs it again, and nothing is ever started from where it is.
    """
    state = tmp_path / "state"
    _provisioned_over(state, program)
    monkeypatch.setattr(sys, "platform", platform)

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    equal(code, 0, describing=f"installing over an instance already there: {said.err}")
    contains(
        said.err,
        f"is already installed in {state.resolve() / 'venv'}",
        describing=f"what the install said on a `{platform}` host",
    )
    equal(
        json.loads(said.out)["api_key_file"],
        str(state.resolve() / "api-key"),
        describing="the key file the install named",
    )


def test_an_install_over_an_unrepaired_release_repairs_it_without_touching_its_links(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """An environment installed before the repair is repaired by the next install.

    The module is a hard link, as `uv` installs it from its own cache, and the
    repair leaves the other name of that file exactly as it found it.
    """
    state = tmp_path / "state"
    _provisioned_over(state, _host_program())
    module = _virtual_printer_module(state)
    cached = tmp_path / "cache" / "virtual.py"
    cached.parent.mkdir()
    os.link(module, cached)
    published = cached.read_bytes()

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    equal(code, 0, describing=f"installing over an unrepaired release: {said.err}")
    contains(said.err, "repaired the virtual printer", describing="what the install said")
    contains(module.read_bytes(), octoprint_env.HEATER_REPAIR, describing="the module")
    equal(cached.read_bytes(), published, describing="the cache's copy of the module")
    equal(module.stat().st_nlink, 1, describing="the links to the repaired module")


def test_a_second_install_over_a_repaired_release_leaves_it_as_it_is(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Installing again over the environment the first install repaired succeeds.

    The module then carries the repair rather than the defect, and the second
    install neither refuses it nor writes it again.
    """
    state = tmp_path / "state"
    _provisioned_over(state, _host_program())
    module = _virtual_printer_module(state)
    equal(octoprint_env.main(["install", "--state-dir", str(state)]), 0, describing="the first")
    capsys.readouterr()
    repaired = module.read_bytes()
    written = module.stat().st_mtime_ns

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    equal(code, 0, describing=f"installing over a repaired release: {said.err}")
    truth("repaired the virtual printer" not in said.err, describing=f"a repeat: {said.err}")
    equal(module.read_bytes(), repaired, describing="the module a second install read")
    equal(module.stat().st_mtime_ns, written, describing="when the module was last written")


@pytest.mark.parametrize("modules", [0, 2], ids=["none", "two"])
def test_an_environment_without_exactly_one_virtual_printer_is_refused(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], modules: int
) -> None:
    """An install that cannot tell which virtual printer to repair says so."""
    state = tmp_path / "state"
    _provisioned_over(state, _host_program())
    module = _virtual_printer_module(state)
    if modules == 0:
        module.unlink()
    else:
        second = state / "venv" / "lib" / "python3.12" / "site-packages"
        second = second.joinpath(*octoprint_env.VIRTUAL_PRINTER_MODULE)
        second.parent.mkdir(parents=True)
        second.write_bytes(module.read_bytes())

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    truth(code != 0, describing=f"an install over {modules} virtual printers failing")
    contains(said.err, f"found {modules}", describing="what the install said")


def test_a_virtual_printer_carrying_neither_the_defect_nor_its_repair_is_refused(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A release whose heater simulation changed is reported, not patched blind."""
    state = tmp_path / "state"
    _provisioned_over(state, _host_program())
    module = _virtual_printer_module(state)
    rewritten = b"# a release that rewrote the heater simulation\n"
    module.write_bytes(rewritten)

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    truth(code != 0, describing="an install over an unrecognised heater simulation failing")
    contains(said.err, "carries neither the heater defect", describing="what the install said")
    equal(module.read_bytes(), rewritten, describing="the module the install refused")


# A process that outlives the one that started it, as the server `up` starts
# does: started in a group of its own by a launcher that exits at once, so what
# is left is nobody's child and a stopped one is gone rather than unreaped. Its
# argv is handed in as JSON, because `down` reads the command line of whatever
# holds a recorded id and signals only this instance's own server.
LAUNCHER = """
import json, subprocess, sys
detached = (
    {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS}
    if sys.platform == "win32"
    else {"start_new_session": True}
)
child = subprocess.Popen(
    json.loads(sys.argv[1]),
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **detached
)
print(child.pid)
"""


def _server_stand_in(state: Path) -> list[str]:
    """The argv of a process `down` will take for this instance's own server.

    The instance's `octoprint` program, where this host's virtual environment
    keeps it, run under the interpreter with the `--basedir` and `serve` words
    `up` starts a server with — the shape `is_this_instances_server` reads a
    running process's command line for. The program itself only sleeps.
    """
    connection = octoprint_env.Connection("virtual", octoprint_env.VIRTUAL_DEVICE, 115200)
    instance = octoprint_env.Instance(state, connection)
    instance.executable.parent.mkdir(parents=True)
    instance.executable.write_text("import time; time.sleep(300)\n", encoding="utf-8")
    return [
        sys.executable,
        *octoprint_env.server_argv_prefix(instance),
        "--host",
        octoprint_env.HOST,
        "--port",
        "1",
    ]


def test_stopping_a_recorded_instance_leaves_no_process_behind(tmp_path: Path) -> None:
    """`down` over the running server ends it, by this host's own means."""
    state = tmp_path / "state"
    state.mkdir()
    launched = shell_run(
        [sys.executable, "-c", LAUNCHER, json.dumps(_server_stand_in(state))], timeout=60
    )
    pid = int((launched.stdout or "").strip())
    (state / "instance.json").write_text(json.dumps({"pid": pid}), encoding="utf-8")
    truth(running(pid), describing=f"process {pid} before it is stopped")

    code = octoprint_env.main(["down", "--state-dir", str(state)])

    equal(code, 0, describing="stopping the recorded instance")
    truth(not running(pid), describing=f"process {pid} to be gone once the instance is stopped")
    truth(
        not (state / "instance.json").exists(),
        describing="the record of a stopped instance to be gone",
    )


def test_command_lookup_reads_a_real_childs_entire_command_line(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Read arguments, including spaces and single quotes, through this host's API."""
    argv = [sys.executable, "-c", "import sys; sys.stdin.read()", "lookup marker 'quoted' tail"]
    # A macOS framework interpreter re-executes as Python.app, so the image the
    # host reports is its own; every argument after it must survive exactly.
    arguments = " " + " ".join(argv[1:])
    child = start(argv)
    try:
        reported = octoprint_env.command_of(child.pid)
        equal(
            reported[-len(arguments) :],
            arguments,
            describing="the child's entire argument list rather than just its image",
        )
        truth(len(reported) > len(arguments), describing="an image before those arguments")
        equal(child.poll(), None, describing="the child to remain alive after observation")
    finally:
        child.communicate(timeout=10)

    monkeypatch.setenv("PATH", "")
    with pytest.raises(FileNotFoundError, match="is not on PATH"):
        octoprint_env.command_of(child.pid)
