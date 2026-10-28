"""The real Unix installer and migration against staged releases and manager subprocesses.

The subprocess doubles record only service-manager effects. Files and SHA-256
verification are real, and migrated records are written and read by real servers.
The host managers' lifecycle is exercised separately under the required switch.
"""

from __future__ import annotations

import functools
import gzip
import hashlib
import io
import json
import os
import plistlib
import shutil
import sys
import tarfile
import tomllib
from collections.abc import Callable
from pathlib import Path
from subprocess import CompletedProcess

import pytest
from journey import DEBUG_BUILD, REPO_ROOT, clean_environment, install_the_skill
from release_artifacts.world import Machine
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.shell import run, start
from test_per_user_windows_install_journey import (
    CREDENTIAL,
    IMAGE,
    _alert,
    _await_recorded,
    _client,
    _fill_in,
    _older,
    _serving,
    _version,
    machine,
    program,
)

pytestmark = [DEBUG_BUILD, pytest.mark.skipif(sys.platform == "win32", reason="Unix scripts")]
__all__ = ["machine", "program"]


def _tool(path: Path, body: str) -> None:
    path.write_text("#!/bin/sh\nset -eu\n" + body, encoding="utf-8")
    path.chmod(493)


@functools.cache
def _archive(program: Path) -> bytes:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        body = program.read_bytes()
        entry = tarfile.TarInfo("printobserver")
        entry.size, entry.mode = len(body), 0o755
        archive.addfile(entry, io.BytesIO(body))
    return gzip.compress(raw.getvalue(), compresslevel=1, mtime=0)


class Host:
    """A throwaway home, release and account database, with recorded manager commands."""

    def __init__(self, root: Path, system: str, program: Path) -> None:
        self.root, self.system = (root, system)
        self.home, self.bin = (root / "home", root / "tools")
        self.home.mkdir()
        self.bin.mkdir()
        self.log = root / "manager.log"
        self.release = root / "release"
        release = self.release / "latest/download"
        release.mkdir(parents=True)
        archive = _archive(program)
        checksums = []
        for platform in ("linux-x86_64", "linux-aarch64", "macos-aarch64"):
            name = f"printobserver-{platform}.tar.gz"
            (release / name).write_bytes(archive)
            checksums.append(f"{hashlib.sha256(archive).hexdigest()}  {name}\n")
        (release / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")
        architecture = "arm64" if system == "Darwin" else "x86_64"
        _tool(
            self.bin / "uname",
            f"""case "$1" in -s) echo {system} ;; -m) echo {architecture} ;; esac\n""",
        )
        for name in ("systemctl", "launchctl", "loginctl"):
            _tool(
                self.bin / name,
                f'printf "%s\\n" "{name} $*" >> "$RECORD"\n'
                'case "$*" in cat* | "print system/"*) exit "${REGISTERED:-1}" ;; esac\n'
                f'if [ "{name}" = loginctl ]; then exit "${{LINGER_STATUS:-0}}"; fi\n',
            )
        _tool(self.bin / "sudo", 'echo "unexpected sudo" >> "$RECORD"; exit 99\n')
        self.env = clean_environment(
            HOME=str(self.home),
            USER="operator",
            PATH=f"{self.bin}:{os.environ['PATH']}",
            PRINTOBSERVER_RELEASE_BASE=str(self.release),
            RECORD=str(self.log),
        )
        self.env.pop("XDG_CONFIG_HOME", None)
        self.env.pop("XDG_STATE_HOME", None)

    @property
    def config(self) -> Path:
        base = self.home / (".config" if self.system == "Linux" else "Library/Application Support")
        return base / "printobserver/config.toml"

    @property
    def state(self) -> Path:
        return self.home / (
            ".local/state/printobserver"
            if self.system == "Linux"
            else "Library/Application Support/printobserver/state"
        )

    @property
    def definition(self) -> Path:
        return self.home / (
            ".config/systemd/user/printobserver.service"
            if self.system == "Linux"
            else "Library/LaunchAgents/io.github.nickderobertis.printobserver.plist"
        )

    def install(self, *args: str) -> CompletedProcess[str]:
        return run(
            ["sh", str(REPO_ROOT / "scripts/install.sh"), *args],
            cwd=self.home,
            env=self.env,
            timeout=120,
        )

    def said(self) -> str:
        return self.log.read_text(encoding="utf-8") if self.log.exists() else ""


def _tree(path: Path) -> dict[str, str]:
    return {
        str(file.relative_to(path)): hashlib.sha256(file.read_bytes()).hexdigest()
        for file in path.rglob("*")
        if file.is_file()
    }


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
def test_install_upgrade_and_client_only(tmp_path: Path, program: Path, system: str) -> None:
    host = Host(tmp_path, system, program)
    passing(host.install("--observation-interval-seconds", "71"))
    config = tomllib.loads(host.config.read_text(encoding="utf-8"))
    equal(config["state_dir"], str(host.state), describing="the user's state")
    equal(config["supervisor"]["skill_path"], str(host.state / "skills/printobserver/SKILL.md"))
    equal(config["supervisor"]["observation_interval_s"], 71)
    template = (REPO_ROOT / "scripts/install-service.sh").read_text(encoding="utf-8")
    template = template.split("<<CONFIG", 1)[1].split("\nCONFIG", 1)[0].split("\n", 1)[1]
    expected = template.replace("$RUNTIME_STATE", str(host.state)).replace(
        "$INTERVAL_LINE", "observation_interval_s = 71"
    )
    equal(config, tomllib.loads(expected), describing="the isolated installer's full template")
    equal(host.config.stat().st_mode & 511, 384)
    equal(host.state.stat().st_mode & 511, 448)
    if system == "Linux":
        unit = host.definition.read_text(encoding="utf-8")
        for setting in (
            "Restart=on-failure",
            "RestartSec=5",
            "WantedBy=default.target",
            f"WorkingDirectory={host.state}",
        ):
            contains(unit, setting)
        contains(host.said(), "systemctl --user enable --now printobserver.service")
        contains(host.said(), "loginctl enable-linger operator")
    else:
        definition = plistlib.loads(host.definition.read_bytes())
        equal(definition["RunAtLoad"], True)
        equal(definition["KeepAlive"], {"SuccessfulExit": False})
        equal(definition["ThrottleInterval"], 5)
        equal(
            definition["ProgramArguments"],
            [str(host.home / ".local/bin/printobserver"), "server", "--config", str(host.config)],
        )
        contains(host.said(), "launchctl bootstrap gui/")
    host.state.joinpath("kept").write_bytes(b"application state")
    before = _tree(host.home)
    passing(host.install("--observation-interval-seconds", "99"))
    equal(
        _tree(host.home),
        before,
        describing="upgrading leaves config and state untouched, one registration",
    )
    contains(host.said(), "systemctl --user stop" if system == "Linux" else "launchctl bootout")
    truth("unexpected sudo" not in host.said(), describing="no elevation")
    equal(
        sorted(path.name for path in tmp_path.iterdir()),
        ["home", "manager.log", "release", "tools"],
    )


@pytest.mark.parametrize("value", ["0", "01", "-1", "1.5", "9223372036854775808"])
def test_invalid_interval_changes_nothing(tmp_path: Path, program: Path, value: str) -> None:
    host = Host(tmp_path, "Linux", program)
    failing(
        host.install("--observation-interval-seconds", value),
        naming="interval",
    )
    equal(_tree(host.home), {})


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
def test_system_install_refusal_and_client_only(tmp_path: Path, program: Path, system: str) -> None:
    host = Host(tmp_path, system, program)
    host.env["REGISTERED"] = "0"
    result = host.install()
    failing(result, naming="migrate-service-to-per-user.sh")
    contains(result.stderr or "", "migrate-service-to-per-user.sh")
    equal(_tree(host.home), {})
    passing(host.install("--client-only"))
    truth(
        (host.home / ".local/bin/printobserver").is_file(),
        describing="the per-user install contract",
    )
    truth(not host.config.exists(), describing="the per-user install contract")


def test_lingering_refusal_completes_and_explains(tmp_path: Path, program: Path) -> None:
    host = Host(tmp_path, "Linux", program)
    host.env["LINGER_STATUS"] = "1"
    result = host.install()
    passing(result)
    contains(result.stderr or "", "only while you are logged in")
    contains(result.stderr or "", 'loginctl enable-linger "operator"')


def test_linux_honors_xdg_directories(tmp_path: Path, program: Path) -> None:
    """The user manager and server agree on nondefault XDG config and state."""
    host = Host(tmp_path, "Linux", program)
    config_home, state_home = host.home / "configuration", host.home / "state"
    host.env.update(XDG_CONFIG_HOME=str(config_home), XDG_STATE_HOME=str(state_home))
    passing(host.install())
    config = tomllib.loads((config_home / "printobserver/config.toml").read_text(encoding="utf-8"))
    equal(config["state_dir"], str(state_home / "printobserver"))
    equal(
        config["supervisor"]["skill_path"],
        str(state_home / "printobserver/skills/printobserver/SKILL.md"),
    )
    contains(
        (config_home / "systemd/user/printobserver.service").read_text(encoding="utf-8"),
        str(config_home / "printobserver/config.toml"),
    )


def test_unverified_upgrade_keeps_the_running_registration_and_all_files(
    tmp_path: Path, program: Path
) -> None:
    """Verification precedes even stopping the manager during an upgrade."""
    host = Host(tmp_path, "Linux", program)
    passing(host.install())
    before, operations = _tree(host.home), host.said()
    (host.release / "latest/download/SHA256SUMS").write_text(
        "0" * 64 + "  printobserver-linux-x86_64.tar.gz\n", encoding="utf-8"
    )
    failing(host.install(), naming="does not match the digest")
    equal(_tree(host.home), before)
    equal(host.said(), operations + "systemctl cat printobserver.service\n")


def _migration_host(
    tmp_path: Path, program: Path, system: str = "Linux"
) -> tuple[Host, Path, Path]:
    host = Host(tmp_path, system, program)
    root = tmp_path / "isolated"
    state = root / "var/lib/printobserver"
    state.mkdir(parents=True)
    config = root / "etc/printobserver/config.toml"
    config.parent.mkdir(parents=True)
    definition = root / (
        "etc/systemd/system/printobserver.service"
        if system == "Linux"
        else "Library/LaunchDaemons/io.github.nickderobertis.printobserver.plist"
    )
    definition.parent.mkdir(parents=True)
    definition.write_text("system registration", encoding="utf-8")
    _tool(host.bin / "id", 'case "$1" in -u) echo 0 ;; -gn) echo operator ;; *) exit 0 ;; esac\n')
    _tool(host.bin / "dscl", 'echo "NFSHomeDirectory: /home/operator"\n')
    _tool(host.bin / "getent", 'echo "operator:x:1000:1000::/home/operator:/bin/sh"\n')
    _tool(
        host.bin / "chown",
        'if [ -n "${ALTER_COPY:-}" ] && [ -f "$ALTER_COPY" ]; then\n'
        'echo corrupted >> "$ALTER_COPY"; fi\n',
    )
    host.env["SUDO_USER"] = "operator"
    return (host, root, config)


def _migrate(host: Host, root: Path) -> CompletedProcess[str]:
    return run(
        ["sh", str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"), "--root", str(root)],
        env=host.env,
        timeout=120,
    )


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
def test_migration_preserves_and_serves_the_real_record(
    tmp_path: Path, program: Path, machine: Machine, system: str
) -> None:
    host, root, config = _migration_host(tmp_path, program, system)
    state = root / "var/lib/printobserver"
    source = (
        (REPO_ROOT / "scripts/install-service.sh")
        .read_text(encoding="utf-8")
        .split("<<CONFIG", 1)[1]
        .split("\nCONFIG", 1)[0]
        .split("\n", 1)[1]
    )
    source = source.replace("$RUNTIME_STATE", str(state)).replace(
        "$INTERVAL_LINE", "observation_interval_s = 120"
    )
    config.write_text(source, encoding="utf-8")
    _fill_in(config, machine, CREDENTIAL, "127.0.0.1:0")
    install_the_skill(config)
    server = start([str(program), "server", "--config", str(config)], env=clean_environment())
    try:
        address = _serving(server)
        _alert(address, machine)
        recorded = _await_recorded(state)
    finally:
        server.terminate()
        server.wait(timeout=60)
    before, original = (_tree(state), config.read_text(encoding="utf-8"))
    passing(_migrate(host, root))
    copied_state = root / (
        "home/operator/.local/state/printobserver"
        if system == "Linux"
        else "home/operator/Library/Application Support/printobserver/state"
    )
    copied_config = root / (
        "home/operator/.config/printobserver/config.toml"
        if system == "Linux"
        else "home/operator/Library/Application Support/printobserver/config.toml"
    )
    equal(_tree(copied_state), before)
    equal(_tree(state), before)
    equal(config.read_text(encoding="utf-8"), original)
    equal(
        copied_config.read_text(encoding="utf-8"),
        "\n".join(
            line.replace(str(state), str(copied_state))
            if line.startswith(("state_dir =", "skill_path ="))
            else line
            for line in original.split("\n")
        ),
    )
    truth(
        not root.joinpath(
            "etc/systemd/system/printobserver.service"
            if system == "Linux"
            else "Library/LaunchDaemons/io.github.nickderobertis.printobserver.plist"
        ).exists(),
        describing="the per-user install contract",
    )
    server = start(
        [str(program), "server", "--config", str(copied_config)], env=clean_environment()
    )
    try:
        address = _serving(server)
        listed = _client(program, address, CREDENTIAL, "prints")
        passing((listed.code, listed.said))
        contains(listed.out, recorded.print_id)
        image = _client(program, address, CREDENTIAL, "image", "--image-id", recorded.image_id)
        passing((image.code, image.said))
        path = Path(json.loads(image.out)["path"])
        truth(copied_state in path.parents, describing="the per-user install contract")
        equal(path.read_bytes(), IMAGE)
    finally:
        server.terminate()
        server.wait(timeout=60)


@pytest.mark.parametrize(
    "refusal",
    [
        "config",
        "state",
        "registration",
        "source-config",
        "source-state",
        "layout",
        "elevation",
        "user",
        "verification",
    ],
)
def test_migration_refusals_keep_originals_and_registration(
    tmp_path: Path, program: Path, refusal: str
) -> None:
    host, root, config = _migration_host(tmp_path, program)
    state = root / "var/lib/printobserver"
    state.joinpath("database-wal").write_bytes(b"retained")
    config.write_text(
        f'state_dir = "{state}"\n[supervisor]\n'
        f'skill_path = "{state}/skills/printobserver/SKILL.md"\n',
        encoding="utf-8",
    )
    destination = root / "home/operator"
    if refusal in {"config", "state"}:
        target = destination / (
            ".config/printobserver/config.toml"
            if refusal == "config"
            else ".local/state/printobserver"
        )
        target.parent.mkdir(parents=True)
        target.write_bytes(b"already here")
    elif refusal == "registration":
        root.joinpath("etc/systemd/system/printobserver.service").unlink()
    elif refusal == "source-config":
        config.unlink()
    elif refusal == "source-state":
        shutil.rmtree(state)
    elif refusal == "layout":
        config.write_text("state_dir = 42\n", encoding="utf-8")
    elif refusal == "elevation":
        _tool(host.bin / "id", "echo 1000\n")
    elif refusal == "user":
        host.env.pop("SUDO_USER")
    elif refusal == "verification":
        host.env["ALTER_COPY"] = str(destination / ".local/state/printobserver/database-wal")
    before = _tree(root)
    result = _migrate(host, root)
    failing(result, naming="did not verify" if refusal == "verification" else "nothing was changed")
    equal(
        _tree(root),
        before,
        describing="the refused move leaves all original files and registration",
    )
    if refusal != "verification":
        truth("systemctl stop" not in host.said(), describing="refused before stopping")
    else:
        contains(host.said(), "systemctl stop printobserver.service")
        truth("systemctl disable" not in host.said(), describing="copy verified before removal")


def test_the_user_manager_serves_restarts_after_crash_and_upgrades(
    tmp_path: Path, program: Path, machine: Machine
) -> None:
    """The host's real systemd user manager or GUI launchd owns the server process."""
    import socket
    import time

    from test_service_manager_journey import Systemd, _unmet

    if sys.platform not in {"linux", "darwin"}:
        pytest.skip("systemd and launchd are Unix managers")
    older = tmp_path / "older-program"
    older.write_bytes(
        program.read_bytes().replace(_version().encode(), _older(_version()).encode())
    )
    staged = Host(tmp_path, "Darwin" if sys.platform == "darwin" else "Linux", older)
    container = Systemd("printobserver.service", program) if sys.platform == "linux" else None
    home = Path("/home/operator") if container else staged.home
    config = home / (
        ".config/printobserver/config.toml"
        if container
        else "Library/Application Support/printobserver/config.toml"
    )
    state = home / (
        ".local/state/printobserver"
        if container
        else "Library/Application Support/printobserver/state"
    )
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    address = f"http://127.0.0.1:{port}"
    env = clean_environment(
        HOME=str(home),
        USER=os.environ.get("USER", "operator"),
        PRINTOBSERVER_RELEASE_BASE=str(staged.release),
    )
    env.pop("XDG_CONFIG_HOME", None)
    env.pop("XDG_STATE_HOME", None)

    def user(*args: str) -> CompletedProcess[str]:
        if container:
            return container._as_root(
                "setpriv",
                "--reuid=1000",
                "--regid=1000",
                "--init-groups",
                "env",
                "HOME=/home/operator",
                "USER=operator",
                "XDG_RUNTIME_DIR=/run/user/1000",
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
                "PRINTOBSERVER_RELEASE_BASE=/release",
                *args,
            )
        return run(list(args), env=env, timeout=120)

    def checked(*args: str) -> str:
        result = user(*args)
        passing(result, describing="the real per-user manager: " + " ".join(args))
        return result.stdout or ""

    def eventually(ready: Callable[[], bool]) -> None:
        deadline = time.monotonic() + 120
        while not ready():
            if time.monotonic() >= deadline:
                raise AssertionError("the user manager did not serve within the hang guard")
            time.sleep(0.1)

    def serves() -> bool:
        return _client(program, address, CREDENTIAL, "prints").code == 0

    def pid() -> int:
        if container:
            return int(
                checked(
                    "systemctl",
                    "--user",
                    "show",
                    "-p",
                    "MainPID",
                    "--value",
                    "printobserver.service",
                ).strip()
                or "0"
            )
        answer = checked(
            "launchctl", "print", f"gui/{os.getuid()}/io.github.nickderobertis.printobserver"
        )
        for line in answer.splitlines():
            if line.strip().startswith("pid = "):
                return int(line.split("=", 1)[1].strip())
        return 0

    try:
        if container:
            container.prepare()
            passing(container._as_root("mkdir", "-p", "/etc/alternatives"))
            awk = str(Path(shutil.which("awk") or "/usr/bin/awk").resolve())
            passing(container._as_root("ln", "-s", awk, "/etc/alternatives/awk"))
            # This minimal container deliberately has no host PAM stack. The
            # runtime-dir unit supplies /run/user/1000; pass it explicitly to
            # the real user manager instead of importing the host's login setup.
            passing(container._as_root("mkdir", "-p", "/etc/systemd/system/user@.service.d"))
            passing(
                container._as_root(
                    "sh",
                    "-c",
                    "printf '%s\\n' '[Service]' 'PAMName=' "
                    "'Environment=XDG_RUNTIME_DIR=/run/user/%i' "
                    "> /etc/systemd/system/user@.service.d/journey.conf",
                )
            )
            passing(container._as_root("systemctl", "daemon-reload"))
            for command in (
                ("useradd", "--create-home", "--uid", "1000", "operator"),
                ("loginctl", "enable-linger", "operator"),
                ("systemctl", "start", "user@1000.service"),
            ):
                passing(
                    container._as_root(*command), describing="starting the journey's user manager"
                )
            passing(container._docker("cp", str(staged.release), f"{container.container}:/release"))
            passing(
                container._docker(
                    "cp",
                    str(REPO_ROOT / "scripts/install.sh"),
                    f"{container.container}:/install.sh",
                )
            )
            installer = "/install.sh"
        else:
            if run(["launchctl", "print", f"gui/{os.getuid()}"], timeout=30).returncode:
                _unmet("the per-user LaunchAgent needs this user's GUI login session")
            installer = str(REPO_ROOT / "scripts/install.sh")
        checked("sh", installer)
        contains(checked(str(home / ".local/bin/printobserver"), "--version"), _older(_version()))
        # Fill the configuration and install the real skill exactly as the
        # installer tells the operator to do, then restart through the manager.
        configured = tmp_path / "configured.toml"
        configured.write_text(checked("cat", str(config)), encoding="utf-8")
        _fill_in(configured, machine, CREDENTIAL, f"127.0.0.1:{port}")
        if container:
            passing(container._docker("cp", str(configured), f"{container.container}:{config}"))
            passing(container._as_root("mkdir", "-p", str(state / "skills")))
            passing(
                container._as_root(
                    "cp", "-R", "/journey/skill/printobserver", str(state / "skills/printobserver")
                )
            )
            passing(container._as_root("chown", "-R", "operator:operator", str(home)))
            checked("systemctl", "--user", "restart", "printobserver.service")
            equal(
                checked("systemctl", "--user", "is-enabled", "printobserver.service").strip(),
                "enabled",
            )
            equal(
                container._as_root(
                    "loginctl", "show-user", "operator", "-p", "Linger", "--value"
                ).stdout.strip(),
                "yes",
            )
        else:
            shutil.copyfile(configured, config)
            install_the_skill(config)
            checked(
                "launchctl",
                "kickstart",
                "-k",
                f"gui/{os.getuid()}/io.github.nickderobertis.printobserver",
            )
        eventually(serves)
        first_pid = pid()
        truth(first_pid > 0, describing="the manager's own server PID")
        # This process belongs to the manager this journey started, identified
        # from its own registration, never from a process-name pattern.
        if container:
            passing(container._as_root("kill", "-KILL", str(first_pid)))
        else:
            os.kill(first_pid, 9)
        eventually(lambda: pid() not in {0, first_pid} and serves())
        before = checked("cat", str(config))
        # A new release, carrying a different real program version, upgrades
        # the process that the manager owns rather than just the client file.
        release = staged.release / "latest/download"
        archive = _archive(program)
        checksums = []
        for artifact in release.glob("*.tar.gz"):
            artifact.write_bytes(archive)
            checksums.append(f"{hashlib.sha256(archive).hexdigest()}  {artifact.name}\n")
        (release / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")
        if container:
            passing(
                container._docker(
                    "cp", str(staged.release) + "/.", f"{container.container}:/release"
                )
            )
        checked("sh", installer)
        eventually(serves)
        equal(checked("cat", str(config)), before)
        if container:
            contains(container._as_root(f"/proc/{pid()}/exe", "--version").stdout, _version())
        equal(
            checked(str(home / ".local/bin/printobserver"), "--version").strip(),
            run([str(program), "--version"]).stdout.strip(),
        )
    finally:
        if container:
            container._discard()
        else:
            run(
                [
                    "launchctl",
                    "bootout",
                    f"gui/{os.getuid()}/io.github.nickderobertis.printobserver",
                ],
                env=env,
                timeout=30,
            )
