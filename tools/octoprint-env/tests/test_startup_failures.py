"""Every declared startup failure, induced and read back by name.

Each journey below breaks the environment in exactly one way and runs the real
script over it. A declared failure is reported by its class and a concrete next
action; a failure outside the declared set is reported with the underlying
error's own text. One diagnosed failure path and a bare timeout everywhere else
is the shape that reads as diagnostics without being any, and the last two
journeys here are what refuse it. The two failures about the serial device —
a name this platform cannot have, and a device it cannot open — are induced in
`test_device_refusal.py`, which runs in the gate as well as here.

The closed-set journey reads the script itself: a raise the declared set does
not name, or one the outside-the-set report in `main` would not catch, is a
failure path a caller would meet as a bare error, and it fails here.
"""

from __future__ import annotations

import ast
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path

import pytest
from environment import SCRIPT, answer, said, script
from repo_checks.expect import contains, equal, failing, passing, refused_naming, truth

# What the server is replaced with to induce an instance that starts and never
# answers: a program that runs, and says nothing.
SILENT_SERVER = "#!/bin/sh\nsleep 300\n"

# How long that start is given, and how long its stop may take beyond that: well
# under the script's own thirty-second kill grace, which a stop only waits out
# when it mistakes an exited server for a running one.
START_TIMEOUT_S = 15
STOP_WITHIN_S = 20

# Raising one of these would leave `main`'s outside-the-set report — which
# catches `Exception` — with nothing to report.
UNCATCHABLE = frozenset({"BaseException", "SystemExit", "KeyboardInterrupt", "GeneratorExit"})


def _reported(state: str, *arguments: str) -> list[str]:
    """Run a start that is expected to fail, and hand back what it said, in lines."""
    result = script("up", "--state-dir", state, *arguments, timeout=600)
    failing(result, naming="octoprint-env: failed")
    return said(result).splitlines()


def test_a_configuration_it_cannot_read_is_reported_by_name(
    state_dir: Callable[[str], str],
) -> None:
    """A state directory whose configuration is not YAML at all."""
    state = Path(state_dir("unreadable-config"))
    (state / "instance").mkdir(parents=True)
    (state / "instance" / "config.yaml").write_text("server: [unclosed\n", encoding="utf-8")

    lines = _reported(str(state))

    refused_naming(lines, "config-unreadable")
    refused_naming(lines, "what happened:", "config.yaml")
    refused_naming(lines, "next action:", "run `install` again")


def test_a_listen_port_already_in_use_is_reported_by_name(
    state_dir: Callable[[str], str],
) -> None:
    """A port something else is already listening on."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as taken:
        taken.bind(("127.0.0.1", 0))
        taken.listen(1)
        port = taken.getsockname()[1]

        lines = _reported(state_dir("port-in-use"), "--port", str(port))

    refused_naming(lines, "port-in-use")
    refused_naming(lines, "what happened:", str(port))
    refused_naming(lines, "next action:", "--port")


def test_an_instance_that_never_answers_is_reported_by_name(
    state_dir: Callable[[str], str],
) -> None:
    """A server that starts and then says nothing, which is not a bare timeout."""
    state = state_dir("never-answers")
    provisioned = script("install", "--state-dir", state)
    passing(provisioned, describing="provisioning the instance the stub server replaces")
    venv = Path(str(answer(provisioned)["state_dir"])) / "venv"
    if sys.platform == "win32":
        # A program Windows will run in the server's place, and one that answers
        # nothing: this interpreter, handed arguments it refuses.
        shutil.copyfile(sys.executable, venv / "Scripts" / "octoprint.exe")
    else:
        server = venv / "bin" / "octoprint"
        server.write_text(SILENT_SERVER, encoding="utf-8")
        server.chmod(0o755)

    started = time.monotonic()
    lines = _reported(state, "--start-timeout", str(START_TIMEOUT_S))
    took = time.monotonic() - started

    refused_naming(lines, "never-answered")
    # The stub it stopped is this script's own child. Reaped as it exits, the
    # stop is over in moments; counted as running while it is a zombie, the stop
    # waits out the whole kill grace and then signals a group holding nothing
    # but that zombie, which macOS refuses outright.
    truth(
        took < START_TIMEOUT_S + STOP_WITHIN_S,
        describing=f"the failed start to be over in {took:.0f}s",
    )
    refused_naming(lines, "what happened:", "server.log")
    refused_naming(lines, "next action:", "read the server log")


def test_a_failure_outside_the_declared_set_carries_the_underlying_errors_own_text(
    state_dir: Callable[[str], str],
) -> None:
    """Not one of the declared classes, and not a bare timeout either."""
    occupied = Path(state_dir("a-file-not-a-directory"))
    occupied.parent.mkdir(parents=True, exist_ok=True)
    occupied.write_text("a file, so no state directory can be made here\n", encoding="utf-8")

    result = script("up", "--state-dir", str(occupied), timeout=600)

    failing(result, naming="outside the declared failure classes")
    report = said(result)
    contains(report, "FileExistsError", describing="the underlying error's class")
    # FileExistsError renders its filename with repr(), which escapes Windows separators.
    rendered_path = repr(str(occupied))[1:-1]
    contains(report, rendered_path, describing="the path the underlying error names")


@pytest.mark.parametrize(("command", "document"), [("down", "[17]"), ("up", '"17"')])
def test_a_persisted_record_that_is_not_an_object_is_refused(
    tmp_path: Path, command: str, document: str
) -> None:
    """Both record readers narrow the decoded document before reading a PID off it."""
    state = tmp_path / command
    state.mkdir()
    (state / "instance.json").write_text(document + "\n", encoding="utf-8")
    if command == "up":
        (state / "api-key").write_text("fixture-key\n", encoding="utf-8")

    result = script(command, "--state-dir", str(state))

    failing(result, naming="outside the declared failure classes")
    refused_naming(
        said(result).splitlines(),
        "ValueError",
        "instance record must be a JSON object, not",
    )


@pytest.mark.parametrize(
    "url",
    [
        17,
        "ftp://127.0.0.1:5000",
        "http://127.0.0.1",
        "http://user@127.0.0.1:5000",
        "http://127.0.0.1:5000/api",
        "http://127.0.0.1:5000?key=1",
        "http://127.0.0.1:5000#top",
    ],
)
def test_a_persisted_url_that_is_not_an_http_address_is_refused(
    tmp_path: Path, url: object
) -> None:
    """The existing-instance path narrows the recorded address before asking at it.

    Exactly `http://host:port`: a value of another type, another scheme, no
    port, or anything carried beyond the address is refused naming it.
    """
    state = tmp_path / "up"
    state.mkdir()
    (state / "instance.json").write_text(
        json.dumps({"pid": os.getpid(), "url": url}) + "\n", encoding="utf-8"
    )
    (state / "api-key").write_text("fixture-key\n", encoding="utf-8")

    result = script("up", "--state-dir", str(state))

    failing(result, naming="outside the declared failure classes")
    refused_naming(
        said(result).splitlines(),
        "ValueError",
        f"instance record url must be an http://host:port address, not {url!r}",
    )


@pytest.mark.parametrize("mentions_the_instance", [False, True])
def test_a_persisted_pid_naming_another_program_is_not_signalled(
    tmp_path: Path, mentions_the_instance: bool
) -> None:
    """A record that outlived its server names whatever holds the id now.

    That is a sleeping interpreter in a session of its own here, so a
    bring-down that signalled it would take the decoy rather than this suite —
    and in one case its command line mentions the instance's own directory, as
    an editor or a shell open in it would, which is not what makes a process
    this instance's OctoPrint.
    """
    state = tmp_path / "down"
    state.mkdir()
    sleeping = "import time; time.sleep(300)"
    if mentions_the_instance:
        sleeping = f"{sleeping}  # {state / 'instance'}"
    # Started directly rather than through `repo_checks.shell`, which cannot
    # give it a session of its own; `suppressions.toml` carries the reason.
    decoy = subprocess.Popen(  # noqa: S603
        [sys.executable, "-c", sleeping], start_new_session=True
    )
    try:
        (state / "instance.json").write_text(
            json.dumps({"pid": decoy.pid, "url": "http://127.0.0.1:1"}) + "\n", encoding="utf-8"
        )

        result = script("down", "--state-dir", str(state))

        passing(result)
        equal(answer(result)["stopped"], False, describing="what the bring-down did")
        equal(answer(result)["pid"], decoy.pid, describing="the process it read")
        contains(said(result), f"process {decoy.pid} is not this instance's OctoPrint")
        contains(said(result), "nothing was signalled")
        truth(not (state / "instance.json").exists(), describing="the stale record to be removed")
        time.sleep(0.5)
        equal(decoy.poll(), None, describing="the unrelated process, which must still be running")
    finally:
        decoy.kill()
        decoy.wait()


@pytest.mark.parametrize(("command", "pid"), [("down", 0), ("up", -1)])
def test_a_persisted_pid_that_could_name_more_than_one_process_is_refused(
    tmp_path: Path, command: str, pid: int
) -> None:
    """Both record readers stop before handing unsafe PID semantics to the OS."""
    state = tmp_path / command
    state.mkdir()
    (state / "instance.json").write_text(json.dumps({"pid": pid}) + "\n", encoding="utf-8")
    if command == "up":
        (state / "api-key").write_text("fixture-key\n", encoding="utf-8")

    result = script(command, "--state-dir", str(state))

    failing(result, naming="outside the declared failure classes")
    refused_naming(
        said(result).splitlines(),
        "ValueError",
        f"instance record pid must be a positive integer, not {pid}",
    )


def test_the_script_can_reach_no_failure_path_the_declared_set_does_not_cover() -> None:
    """Every raise is a declared class, or one the outside-the-set report catches."""
    module = ast.parse(SCRIPT.read_text(encoding="utf-8"))
    declared = _declared_classes(module)

    truth(declared, describing="a declared set of failure classes in the script")
    raised = _classes_raised(module)
    for failure_class in raised:
        contains(declared, failure_class, describing="the declared set of failure classes")
    for failure_class in declared:
        contains(raised, failure_class, describing="the classes the script raises, so none is dead")
    _every_other_raise_is_catchable(module)
    _main_reports_both(module)


def _literal(node: ast.expr | None, *, describing: str) -> str:
    """One string literal, refusing anything a reader could not enumerate.

    Raises:
        AssertionError: If the node is not a string literal.
    """
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    found = "nothing" if node is None else ast.dump(node)
    message = f"expected {describing}; got {found}"
    raise AssertionError(message)


def _text_of(node: ast.expr) -> str:
    """Every string literal one declaration is composed of, joined."""
    return "".join(
        part.value
        for part in ast.walk(node)
        if isinstance(part, ast.Constant) and isinstance(part.value, str)
    )


def _declared_classes(module: ast.Module) -> frozenset[str]:
    """The failure classes the script declares, each with the next action it carries."""
    declared: set[str] = set()
    for node in module.body:
        if not isinstance(node, ast.AnnAssign) or not isinstance(node.target, ast.Name):
            continue
        if node.target.id != "FAILURE_CLASSES" or not isinstance(node.value, ast.Dict):
            continue
        for key, value in zip(node.value.keys, node.value.values, strict=True):
            named = _literal(key, describing="a failure class named by a string literal")
            truth(
                _text_of(value).strip(),
                describing=f"a next action for the failure class {named!r}",
            )
            declared.add(named)
    return frozenset(declared)


def _raises(module: ast.Module) -> list[tuple[ast.Raise, str, list[ast.expr]]]:
    """Every raise inside a function of the script, with what it raises.

    Module scope is left out on purpose: `raise SystemExit(main())` is how a
    program hands its exit status back, rather than a failure path a caller
    would meet.
    """
    found: list[tuple[ast.Raise, str, list[ast.expr]]] = []
    inside = [
        node
        for container in ast.walk(module)
        if isinstance(container, ast.FunctionDef | ast.AsyncFunctionDef)
        for node in ast.walk(container)
    ]
    for node in inside:
        if not isinstance(node, ast.Raise) or node.exc is None:
            continue
        raised = node.exc
        if isinstance(raised, ast.Call) and isinstance(raised.func, ast.Name):
            found.append((node, raised.func.id, list(raised.args)))
        elif isinstance(raised, ast.Name):
            found.append((node, raised.id, []))
    return found


def _classes_raised(module: ast.Module) -> frozenset[str]:
    """The class every `StartupError` in the script names, refusing a computed one."""
    return frozenset(
        _literal(
            arguments[0] if arguments else None,
            describing=f"a literal failure class at line {node.lineno}",
        )
        for node, name, arguments in _raises(module)
        if name == "StartupError"
    )


def _every_other_raise_is_catchable(module: ast.Module) -> None:
    """No raise escapes the outside-the-set report by not deriving from `Exception`."""
    for node, name, _ in _raises(module):
        truth(
            name not in UNCATCHABLE,
            describing=(
                f"no `{name}` at line {node.lineno}: the outside-the-set report catches "
                f"`Exception`, so raising that would be a failure path nothing reports"
            ),
        )


def _main_reports_both(module: ast.Module) -> None:
    """`main` reports a declared class by name, and anything else by its own text."""
    for node in module.body:
        if not isinstance(node, ast.FunctionDef) or node.name != "main":
            continue
        handled = {
            handler.type.id
            for inner in ast.walk(node)
            if isinstance(inner, ast.Try)
            for handler in inner.handlers
            if isinstance(handler.type, ast.Name)
        }
        contains(handled, "StartupError", describing="what `main` handles")
        contains(handled, "Exception", describing="what `main` handles")
        return
    message = "expected a `main` in the script, which is where every failure is reported"
    raise AssertionError(message)
