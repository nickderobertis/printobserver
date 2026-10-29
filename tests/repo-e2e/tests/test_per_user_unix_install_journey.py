"""The real Unix installer and migration against staged releases and manager subprocesses.

Only service-manager and privilege boundaries are substituted. Files, SHA-256,
account lookup and ownership commands are real; copy faults are injected after
the ownership command. Migrated records are written and read by real servers.
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
from dataclasses import dataclass
from pathlib import Path
from subprocess import CompletedProcess

import pytest
from journey import DEBUG_BUILD, REPO_ROOT, clean_environment, install_the_skill
from release_artifacts.world import Machine
from repo_checks import install_path
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import host as host_platform
from repo_checks.shell import run, start
from test_per_user_windows_install_journey import (
    CREDENTIAL,
    IMAGE,
    ImageId,
    PrintId,
    Recorded,
    _alert,
    _client,
    _fill_in,
    _older,
    _serving,
    _version,
    machine,
    program,
)

PLATFORM = host_platform(Repo(REPO_ROOT)).id
NATIVE_SYSTEM = "Darwin" if sys.platform == "darwin" else "Linux"

pytestmark = [DEBUG_BUILD, pytest.mark.skipif(sys.platform == "win32", reason="Unix scripts")]
__all__ = ["machine", "program"]


def _uid() -> int:
    """The logged-in host user's numeric UID, through the installer's own interface."""
    answer = run(["id", "-u"], timeout=30)
    passing(answer, describing="the host account query")
    return int((answer.stdout or "").strip())


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
    """A throwaway home and release, with recorded host-manager commands."""

    def __init__(self, root: Path, system: str, program: Path) -> None:
        native = NATIVE_SYSTEM
        if system != native:
            pytest.skip(f"{system} behavior runs in its native platform context")
        self.root, self.system = (root, system)
        self.home, self.bin = (root / "home", root / "tools")
        self.user_home = self.home
        self.system_definition = root / "unregistered"
        self.home.mkdir()
        self.bin.mkdir()
        self.log = root / "manager.log"
        self.release = root / "release"
        release = self.release / "latest/download"
        release.mkdir(parents=True)
        archive = _archive(program)
        checksums = []
        for platform in (PLATFORM,):
            name = f"printobserver-{platform}.tar.gz"
            (release / name).write_bytes(archive)
            checksums.append(f"{hashlib.sha256(archive).hexdigest()}  {name}\n")
        (release / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")
        for name in ("systemctl", "launchctl", "loginctl"):
            # llmlint: ignore[e2e_not_mocked] Manager boundary; native lifecycle has a journey.
            _tool(
                self.bin / name,
                f'printf "%s\\n" "{name} $*" >> "$RECORD"\n'
                f'if [ "${{REFUSES:-}}" = "{name} $*" ]; then\n'
                'echo "manager query refused" >&2; exit 1; fi\n'
                'case "$*" in "show --property=LoadState --value "*)\n'
                'if [ "${REGISTERED:-1}" = 0 ]; then\n'
                "echo loaded; else echo not-found; fi; exit 0 ;; esac\n"
                'case "$*" in "stop printobserver.service" | "bootout system/"*)\n'
                'if [ -n "${STOP_GATE:-}" ]; then IFS= read -r release < "$STOP_GATE"; fi ;; esac\n'
                f'if [ "{name}" = loginctl ]; then exit "${{LINGER_STATUS:-0}}"; fi\n'
                f'if [ "{name}" = launchctl ]; then\n'
                'case "$*" in\n'
                '"print system/"*) [ -f "$SYSTEM_AGENT_STATE" ] && exit 0;\n'
                'exit "${REGISTERED:-113}" ;;\n'
                '"bootout system/"*) [ "${STAYS_LOADED:-0}" = 1 ] ||\n'
                'rm -f "$SYSTEM_AGENT_STATE" ;;\n'
                '"print gui/"*/*) [ -f "$AGENT_STATE" ] && exit 0; exit 113 ;;\n'
                'bootstrap*) touch "$AGENT_STATE" ;;\n'
                'bootout*) [ -f "$AGENT_STATE" ] || exit 113;\n'
                '[ "${STAYS_LOADED:-0}" = 1 ] || rm "$AGENT_STATE" ;;\n'
                "esac; fi\n",
            )
        # llmlint: ignore[e2e_not_mocked] Reject any installer attempt to elevate this user.
        _tool(self.bin / "sudo", 'echo "unexpected sudo" >> "$RECORD"; exit 99\n')
        self.env = clean_environment(
            HOME=str(self.home),
            USER="operator",
            PATH=f"{self.bin}:{os.environ['PATH']}",
            PRINTOBSERVER_RELEASE_BASE=str(self.release),
            RECORD=str(self.log),
            AGENT_STATE=str(self.bin / "agent-registered"),
            SYSTEM_AGENT_STATE=str(self.bin / "system-agent-registered"),
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

    @property
    def migrated_config(self) -> Path:
        return self.user_home / (
            ".config/printobserver/config.toml"
            if self.system == "Linux"
            else "Library/Application Support/printobserver/config.toml"
        )

    @property
    def migrated_state(self) -> Path:
        return self.user_home / (
            ".local/state/printobserver"
            if self.system == "Linux"
            else "Library/Application Support/printobserver/state"
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
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_install_and_upgrade_preserve_user_files(
    tmp_path: Path, program: Path, system: str
) -> None:
    host = Host(tmp_path, system, program)
    installed = host.install("--observation-interval-seconds", "71")
    passing(installed)
    for instruction in (
        "credential issue",
        "api.credential_verifier",
        str(host.config),
        "gh skill install",
        str(host.state / "skills"),
        "sign-in",
    ):
        contains(installed.stderr or "", instruction)
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
        contains(installed.stderr or "", "only while you are logged in")
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


@pytest.mark.parametrize("value", ["", "0", "01", "-1", "1.5", "9223372036854775808", "1" * 20])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_invalid_interval_changes_nothing(tmp_path: Path, program: Path, value: str) -> None:
    host = Host(tmp_path, "Linux", program)
    failing(
        host.install("--observation-interval-seconds", value),
        naming="interval",
    )
    equal(_tree(host.home), {})


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
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


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_lingering_refusal_completes_and_explains(tmp_path: Path, program: Path) -> None:
    host = Host(tmp_path, "Linux", program)
    host.env["LINGER_STATUS"] = "1"
    result = host.install()
    passing(result)
    contains(result.stderr or "", "only while you are logged in")
    contains(result.stderr or "", 'loginctl enable-linger "operator"')


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
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


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_unverified_upgrade_keeps_the_running_registration_and_all_files(
    tmp_path: Path, program: Path
) -> None:
    """Verification precedes even stopping the manager during an upgrade."""
    host = Host(tmp_path, "Linux", program)
    passing(host.install())
    before, operations = _tree(host.home), host.said()
    (host.release / "latest/download/SHA256SUMS").write_text(
        "0" * 64 + f"  printobserver-{PLATFORM}.tar.gz\n", encoding="utf-8"
    )
    failing(host.install(), naming="does not match the digest")
    equal(_tree(host.home), before)
    equal(
        host.said(),
        operations + "systemctl show --property=LoadState --value printobserver.service\n",
    )


@dataclass(frozen=True, slots=True)
class MigrationFixture:
    """An isolated service layout and the user's host that will receive its record."""

    host: Host
    service_root: Path
    service_config: Path


def _migration_fixture(
    tmp_path: Path, program: Path, system: str = NATIVE_SYSTEM
) -> MigrationFixture:
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
    host.system_definition = definition
    if system == "Darwin":
        Path(host.env["SYSTEM_AGENT_STATE"]).touch()
    owner = run(["id", "-un"], timeout=30).stdout.strip()
    real_id = shutil.which("id")
    real_chown = shutil.which("chown")
    truth(real_id and real_chown, describing="the real host identity and ownership tools")
    host.env.update(REAL_ID=str(real_id), REAL_CHOWN=str(real_chown), SUDO_USER=owner)
    if system == "Linux":
        home = run(["getent", "passwd", owner], timeout=30).stdout.split(":")[5]
    else:
        home = (
            run(["dscl", ".", "-read", f"/Users/{owner}", "NFSHomeDirectory"], timeout=30)
            .stdout.split(": ", 1)[1]
            .strip()
        )
    host.user_home = root / Path(home).relative_to("/")
    # llmlint: ignore[e2e_not_mocked] Only the root token is supplied; account queries run real id.
    _tool(
        host.bin / "id",
        'if [ "$#" = 1 ] && [ "$1" = -u ]; then echo 0; else exec "$REAL_ID" "$@"; fi\n',
    )

    return MigrationFixture(host=host, service_root=root, service_config=config)


def _fault_copied_file(host: Host, path: Path, effect: str) -> None:
    """Inject a verification fault only after the real ownership operation succeeds."""
    host.env.update(ALTER_COPY=str(path), COPY_EFFECT=effect)
    # llmlint: ignore[e2e_not_mocked] Real chown runs; only a copied file is altered.
    _tool(
        host.bin / "chown",
        '"$REAL_CHOWN" "$@"\n'
        'if [ -n "${ALTER_COPY:-}" ] && [ -f "$ALTER_COPY" ]; then\n'
        'case "${COPY_EFFECT:-corrupt}" in\n'
        'corrupt) echo corrupted >> "$ALTER_COPY" ;;\n'
        'missing) rm "$ALTER_COPY" ;;\n'
        'unreadable) chmod 000 "$ALTER_COPY" ;;\n'
        "esac; fi\n",
    )


def _migrate(host: Host, root: Path) -> CompletedProcess[str]:
    return run(
        ["sh", str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"), "--root", str(root)],
        env=host.env,
        timeout=120,
    )


def _await_public_record(program: Path, address: str) -> Recorded:
    """Discover the alert's record through prints and context, without reading private tables."""
    import time

    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        listed = _client(program, address, CREDENTIAL, "prints")
        passing((listed.code, listed.said))
        for record in json.loads(listed.out)["prints"]:
            print_id = PrintId(str(record["id"]))
            context = _client(program, address, CREDENTIAL, "context", "--print-id", print_id)
            passing((context.code, context.said))
            image = json.loads(context.out)["context"].get("latest_image")
            if image:
                return Recorded(print_id=print_id, image_id=ImageId(str(image["id"])))
        time.sleep(0.05)
    raise AssertionError("the public commands did not report the alert image within the hang guard")


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
@pytest.mark.parametrize("source_nondefault", [False, True])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_preserves_and_serves_the_real_record(
    tmp_path: Path, program: Path, machine: Machine, system: str, source_nondefault: bool
) -> None:
    migration = _migration_fixture(tmp_path, program, system)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    if source_nondefault:
        state.joinpath("unrelated").write_bytes(b"not the configured record")
        state = root / "srv/printobserver-record"
        state.mkdir(parents=True)
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
    verifier_line, rest = config.read_text().split("\n", 1)
    config.write_text(rest + "\n[api]\n" + verifier_line.removeprefix("api.") + "\n")
    install_the_skill(config)
    no_harness = tmp_path / "no-harness"
    no_harness.mkdir()
    server = start(
        [str(program), "server", "--config", str(config)],
        env=clean_environment(PATH=str(no_harness)),
    )
    try:
        address = _serving(server)
        # llmlint: ignore[async_typed_clients_at_boundaries] One sequential real ingress post.
        _alert(address, machine)
        recorded = _await_public_record(program, address)
    finally:
        server.terminate()
        server.wait(timeout=60)
    before, original = (_tree(state), config.read_text(encoding="utf-8"))
    migrated = _migrate(host, root)
    passing(migrated)
    for instruction in (
        "scripts/install.sh",
        "printobserver sign-in",
        "sudo rm -rf",
        str(state),
        "service user",
    ):
        contains(migrated.stderr or "", instruction)
    copied_state = host.user_home / (
        ".local/state/printobserver"
        if system == "Linux"
        else "Library/Application Support/printobserver/state"
    )
    copied_config = host.user_home / (
        ".config/printobserver/config.toml"
        if system == "Linux"
        else "Library/Application Support/printobserver/config.toml"
    )
    equal(_tree(copied_state), before)
    equal(copied_state.stat().st_uid, _uid())
    equal(copied_config.stat().st_uid, _uid())
    equal(copied_state.stat().st_mode & 0o777, 0o700)
    equal(copied_config.stat().st_mode & 0o777, 0o600)
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
        [str(program), "server", "--config", str(copied_config)],
        env=clean_environment(PATH=str(no_harness)),
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
        "verification-missing",
        "verification-unreadable",
        "verification-config",
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_refusals_keep_originals_and_registration(
    tmp_path: Path, program: Path, refusal: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("database-wal").write_bytes(b"retained")
    config.write_text(
        f'state_dir = "{state}"\n[supervisor]\n'
        f'skill_path = "{state}/skills/printobserver/SKILL.md"\n',
        encoding="utf-8",
    )
    match refusal:
        case "config" | "state":
            target = host.migrated_config if refusal == "config" else host.migrated_state
            target.parent.mkdir(parents=True)
            target.write_bytes(b"already here")
        case "registration":
            host.system_definition.unlink()
        case "source-config":
            config.unlink()
        case "source-state":
            shutil.rmtree(state)
        case "layout":
            config.write_text("state_dir = 42\n", encoding="utf-8")
        case "elevation":
            (host.bin / "id").unlink()
        case "user":
            host.env.pop("SUDO_USER")
        case (
            "verification"
            | "verification-missing"
            | "verification-unreadable"
            | "verification-config"
        ):
            target = (
                host.migrated_config
                if refusal == "verification-config"
                else host.migrated_state / "database-wal"
            )
            effect = {
                "verification-missing": "missing",
                "verification-unreadable": "unreadable",
            }.get(refusal, "corrupt")
            _fault_copied_file(host, target, effect)
    before = _tree(root)
    result = _migrate(host, root)
    failing(
        result,
        naming="did not verify" if refusal.startswith("verification") else "nothing was changed",
    )
    if refusal == "verification-config":
        contains(result.stderr or "", "Configuration verification failed")
    elif refusal in {"verification", "verification-missing"}:
        contains(result.stderr or "", "State SHA-256 verification failed")
        contains(result.stderr or "", "database-wal")
    equal(
        _tree(root),
        before,
        describing="the refused move leaves all original files and registration",
    )
    if not refusal.startswith("verification"):
        truth("systemctl stop" not in host.said(), describing="refused before stopping")
    else:
        contains(
            host.said(),
            "systemctl stop printobserver.service"
            if host.system == "Linux"
            else "launchctl bootout system/io.github.nickderobertis.printobserver",
        )
        truth("systemctl disable" not in host.said(), describing="copy verified before removal")


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_the_user_manager_serves_restarts_after_crash_and_upgrades(
    tmp_path: Path, program: Path, machine: Machine
) -> None:
    """The host's real systemd user manager or GUI launchd owns the server process."""
    import socket
    import time

    from test_service_manager_journey import Launchd, Systemd, _disabled_services, _unmet

    if sys.platform not in {"linux", "darwin"}:
        pytest.skip("systemd and launchd are Unix managers")
    older = tmp_path / "older-program"
    # llmlint: ignore[tests_mirror_real_usage] Real server; only its version string differs.
    older.write_bytes(
        program.read_bytes().replace(_version().encode(), _older(_version()).encode())
    )
    if sys.platform == "darwin":
        passing(
            run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(older)], timeout=30),
            describing="ad-hoc signing the modified macOS release fixture",
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
            "launchctl", "print", f"gui/{_uid()}/io.github.nickderobertis.printobserver"
        )
        for line in answer.splitlines():
            if line.strip().startswith("pid = "):
                return int(line.split("=", 1)[1].strip())
        return 0

    owns_agent = False
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
            if run(["launchctl", "print", f"gui/{_uid()}"], timeout=30).returncode:
                _unmet("the per-user LaunchAgent needs this user's GUI login session")
            job = f"gui/{_uid()}/io.github.nickderobertis.printobserver"
            if run(["launchctl", "print", job], timeout=30).returncode == 0:
                _unmet(
                    "a pre-existing printobserver LaunchAgent belongs to this user; "
                    "leave it untouched"
                )
            installer = str(REPO_ROOT / "scripts/install.sh")
        # Real isolated definitions exercise the filesystem refusal before
        # either user manager has been installed or started.
        if container:
            container.install()
            for vendor in (False, True):
                if vendor:
                    passing(
                        container._as_root(
                            "mount", "-t", "tmpfs", "tmpfs", "/usr/lib/systemd/system"
                        )
                    )
                    passing(
                        container._as_root(
                            "mv",
                            "/etc/systemd/system/printobserver.service",
                            "/usr/lib/systemd/system/printobserver.service",
                        )
                    )
                try:
                    refused = user("sh", installer)
                    failing(refused, naming="the system printobserver.service is installed")
                    contains(refused.stderr or "", "migrate-service-to-per-user.sh")
                    for path in (home / ".local/bin/printobserver", config, state):
                        checked("test", "!", "-e", str(path))
                finally:
                    if vendor:
                        passing(container._as_root("umount", "/usr/lib/systemd/system"))
            passing(container._as_root("systemctl", "daemon-reload"))
        else:
            isolated = Launchd(program, None)
            isolated.prepare()
            try:
                isolated.install()
                refused = user("sh", installer)
                failing(refused, naming="the LaunchDaemon is installed")
                contains(refused.stderr or "", "migrate-service-to-per-user.sh")
                truth(
                    not any(
                        path.exists() for path in (home / ".local/bin/printobserver", config, state)
                    ),
                    describing="the filesystem refusal wrote no per-user files",
                )
            finally:
                isolated.remove()
        if container is None:
            domain = f"gui/{_uid()}"
            job = f"{domain}/io.github.nickderobertis.printobserver"
            disabled = _disabled_services(checked("launchctl", "print-disabled", domain))
            if disabled is None or "io.github.nickderobertis.printobserver" in disabled:
                _unmet(
                    "the existing LaunchAgent disabled preference is not this journey's to change"
                )
            checked("launchctl", "disable", job)
            try:
                refused = user("sh", installer)
                failing(refused, naming="the LaunchAgent could not be loaded")
                truth(config.is_file(), describing="the failed bootstrap retained the config")
                equal(
                    user("launchctl", "print", job).returncode,
                    113,
                    describing="the real manager's absent-job status",
                )
            finally:
                checked("launchctl", "enable", job)
        if container:
            checked("mkdir", "-p", str(state.parent))
            passing(container._as_root("mkdir", str(state)))
            refused = user("sh", "/install.sh")
            failing(refused, naming="could not be made private")
            contains(refused.stderr or "", "own this directory")
            passing(container._as_root("chown", "1000:1000", str(state)))
        checked("sh", installer)
        owns_agent = container is None
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
                f"gui/{_uid()}/io.github.nickderobertis.printobserver",
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
        elif owns_agent:
            run(
                [
                    "launchctl",
                    "bootout",
                    f"gui/{_uid()}/io.github.nickderobertis.printobserver",
                ],
                env=env,
                timeout=30,
            )


@pytest.mark.parametrize("system", ["Linux", "Darwin"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_an_upgrade_that_cannot_stop_reports_recovery_and_replaces_nothing(
    tmp_path: Path, program: Path, system: str
) -> None:
    """A manager refusal is reported before the running program is replaced."""
    host = Host(tmp_path, system, program)
    passing(host.install())
    before = _tree(host.home)
    command = (
        "systemctl --user stop printobserver.service"
        if system == "Linux"
        else f"launchctl bootout gui/{_uid()}/io.github.nickderobertis.printobserver"
    )
    host.env["REFUSES"] = command
    result = host.install()
    failing(result, naming="nothing was replaced")
    contains(result.stderr or "", "correct its reported error and retry")
    equal(_tree(host.home), before)


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_stop_failure_reports_recovery_before_any_copy(
    tmp_path: Path, program: Path
) -> None:
    """A stopped-service refusal retains every original and names the diagnostic command."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    config.write_text(
        f'state_dir = "{state}"\n[supervisor]\n'
        f'skill_path = "{state}/skills/printobserver/SKILL.md"\n',
        encoding="utf-8",
    )
    host.env["REFUSES"] = (
        "systemctl stop printobserver.service"
        if host.system == "Linux"
        else "launchctl bootout system/io.github.nickderobertis.printobserver"
    )
    before = _tree(root)
    result = _migrate(host, root)
    failing(result, naming="nothing was copied")
    contains(
        result.stderr or "",
        "systemctl status printobserver.service"
        if host.system == "Linux"
        else "launchctl print system/io.github.nickderobertis.printobserver",
    )
    equal(_tree(root), before)


@pytest.mark.parametrize("custom", [False, True])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_documented_migration_forwards_custom_xdg_directories(
    tmp_path: Path, program: Path, custom: bool
) -> None:
    """The documented privileged command preserves custom directories, including spaces."""
    migration = _migration_fixture(tmp_path, program, "Linux")
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("retained").write_bytes(b"existing state")
    config.write_text(
        f'state_dir = "{state}"\n[supervisor]\n'
        f'skill_path = "{state}/skills/printobserver/SKILL.md"\n',
        encoding="utf-8",
    )
    custom_config = host.user_home / ("custom configuration" if custom else ".config")
    custom_state = host.user_home / ("custom state" if custom else ".local/state")
    host.env.update(
        MIGRATION_ROOT=str(root),
        MIGRATION_SCRIPT=str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"),
    )
    if custom:
        host.env.update(XDG_CONFIG_HOME=str(custom_config), XDG_STATE_HOME=str(custom_state))
    # llmlint: ignore[e2e_not_mocked] Only elevation is substituted; env and migration run real.
    _tool(host.bin / "sudo", 'printf "%s\n" "sudo $*" >> "$RECORD"; exec "$@"\n')
    declared = install_path.parse((REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"))
    command = next(line for line in declared.migration if "migrate-service-to-per-user.sh" in line)
    command = command.replace(
        command.split()[2], (REPO_ROOT / "scripts/migrate-service-to-per-user.sh").as_uri()
    )
    result = run(
        ["sh", "-c", command + ' -s -- --root "$MIGRATION_ROOT"'], env=host.env, timeout=120
    )
    passing(result)
    copied_state = custom_state / "printobserver"
    copied_config = custom_config / "printobserver/config.toml"
    equal(_tree(copied_state), _tree(state))
    parsed = tomllib.loads(copied_config.read_text(encoding="utf-8"))
    equal(parsed["state_dir"], str(copied_state))
    equal(parsed["supervisor"]["skill_path"], str(copied_state / "skills/printobserver/SKILL.md"))
    if custom:
        contains(
            host.said(),
            f"sudo env XDG_CONFIG_HOME={custom_config} XDG_STATE_HOME={custom_state} sh",
        )
        truth(
            not host.user_home.joinpath(".config/printobserver").exists(),
            describing="no fallback configuration when XDG is set",
        )
    else:
        contains(host.said(), "sudo env sh -s -- --root")
        truth("XDG_CONFIG_HOME=" not in host.said(), describing="unset XDG variables stay unset")


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_interval_boundaries_and_missing_value(tmp_path: Path, program: Path) -> None:
    """Missing interval is refused; the largest TOML integer is accepted."""
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    failing(host.install("--observation-interval-seconds"), naming="takes a number")
    equal(_tree(host.home), {})
    passing(host.install("--observation-interval-seconds", "9223372036854775807"))
    equal(
        tomllib.loads(host.config.read_text())["supervisor"]["observation_interval_s"],
        9223372036854775807,
    )


@pytest.mark.parametrize(
    "value",
    [
        "relative",
        'bad"quote',
        "bad\\slash",
        "bad%specifier",
        "bad$VARIABLE",
        "bad&xml",
        "bad\nline",
        "bad\rline",
        "bad\tline",
        "bad\x01line",
        "bad\x7fline",
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_installer_refuses_unrepresentable_paths_without_writes(
    tmp_path: Path, program: Path, value: str
) -> None:
    """Unsafe or relative manager paths cannot become a different ExecStart."""
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    host.env["HOME"] = value if value == "relative" else str(host.home / value)
    result = host.install()
    failing(result, naming="path")
    contains(
        result.stderr or "", "absolute paths" if value == "relative" else "without those characters"
    )
    equal(_tree(host.home), {})


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_installer_temporary_directory_failure_has_recovery(tmp_path: Path, program: Path) -> None:
    """A real ENOTDIR from mktemp leaves installation untouched."""
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    temporary = tmp_path / "not-a-directory"
    temporary.write_bytes(b"not a temporary directory")
    host.env["TMPDIR"] = str(temporary)
    failing(host.install(), naming="select a writable TMPDIR")
    equal(_tree(host.home), {})


@pytest.mark.parametrize("failure", ["parent", "config-write", "definition-write", "state-link"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_installer_provisioning_failure_explains_the_real_filesystem_error(
    tmp_path: Path, program: Path, failure: str
) -> None:
    """Real files and permissions drive provisioning refusals without replacing filesystem tools."""
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    restricted: Path | None = None
    match failure:
        case "parent":
            host.config.parent.parent.mkdir(parents=True)
            host.config.parent.write_bytes(b"not a directory")
            expected = "could not be created"
        case "config-write":
            host.config.parent.mkdir(parents=True)
            restricted = host.config.parent
            restricted.chmod(0o500)
            expected = "could not be written"
        case "definition-write":
            host.definition.mkdir(parents=True)
            expected = "could not be written"
        case "state-link":
            if _uid() == 0:
                pytest.skip("permission refusal requires an unprivileged process")
            host.state.parent.mkdir(parents=True)
            host.state.symlink_to("/usr", target_is_directory=True)
            expected = "symbolic link"
        case _:
            raise AssertionError(failure)
    try:
        result = host.install()
        failing(result, naming=expected)
        contains(
            result.stderr or "", "rerun" if failure != "state-link" else "real user-owned path"
        )
    finally:
        if restricted:
            restricted.chmod(0o700)


@pytest.mark.parametrize("operation", ["reload", "enable", "bootstrap"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_manager_start_failure_has_a_diagnostic_and_can_be_retried(
    tmp_path: Path, program: Path, operation: str
) -> None:
    """An actual installer can recover after its host manager refuses setup."""
    system = "Darwin" if operation == "bootstrap" else "Linux"
    host = Host(tmp_path, system, program)
    commands = {
        "reload": "systemctl --user daemon-reload",
        "enable": "systemctl --user enable --now printobserver.service",
        "bootstrap": f"launchctl bootstrap gui/{_uid()} {host.definition}",
    }
    host.env["REFUSES"] = commands[operation]
    failing(host.install(), naming="retry" if operation != "enable" else "rerun this installer")
    config = host.config.read_bytes()
    host.env.pop("REFUSES")
    passing(host.install())
    equal(host.config.read_bytes(), config)


def _minimal_service_config(config: Path, state: Path) -> None:
    config.write_text(
        f'state_dir = "{state}"\n[supervisor]\n'
        f'skill_path = "{state}/skills/printobserver/SKILL.md"\n'
    )


@pytest.mark.parametrize(
    "refusal",
    [
        "root-relative",
        "xdg-relative",
        "relative-source",
        "config-overlap",
        "state-overlap",
        "source-ancestor-overlap",
        "bad-path",
        "control-path",
        "source-link",
        "destination-link",
        "malformed-skill",
        "missing-skill",
        "missing-state",
        "root-user",
        "unknown-user",
        "temporary",
        "missing-root-argument",
        "unknown-option",
        "no-hash",
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_boundary_refusals_preserve_every_original(
    tmp_path: Path, program: Path, refusal: str
) -> None:
    """Normalization and real account/filesystem queries precede any stop or copy."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("kept").write_bytes(b"original state")
    _minimal_service_config(config, state)
    args = ["--root", str(root)]
    match refusal:
        case "root-relative":
            args = ["--root", "relative"]
        case "xdg-relative":
            if host.system != "Linux":
                pytest.skip("XDG is Linux configuration")
            host.env["XDG_STATE_HOME"] = "relative"
        case "relative-source":
            config.write_text('state_dir = "relative"\n[supervisor]\nskill_path = "external"\n')
        case "config-overlap":
            if host.system == "Linux":
                host.env["XDG_CONFIG_HOME"] = str(state / "destination")
            else:
                source = host.migrated_config.parent
                source.mkdir(parents=True)
                _minimal_service_config(config, source)
        case "state-overlap":
            if host.system == "Linux":
                host.env["XDG_STATE_HOME"] = str(state / "child/../destination")
            else:
                source = host.migrated_state.parent
                source.mkdir(parents=True)
                _minimal_service_config(config, source)
        case "source-ancestor-overlap":
            alias = root / "record-alias"
            if host.system == "Linux":
                alias.symlink_to(state.parent, target_is_directory=True)
                config.write_text(config.read_text().replace(str(state), str(alias / state.name)))
                host.env["XDG_CONFIG_HOME"] = str(state / "destination")
            else:
                host.user_home.joinpath("Library").mkdir(parents=True)
                alias.symlink_to(host.user_home, target_is_directory=True)
                _minimal_service_config(config, alias / "Library")
        case "bad-path":
            args = ["--root", str(root) + 'bad"quote']
        case "control-path":
            args = ["--root", str(root) + "bad\rpath"]
        case "source-link":
            state.joinpath("link").symlink_to(state / "kept")
        case "destination-link":
            host.user_home.mkdir(parents=True)
            host.user_home.joinpath(".config" if host.system == "Linux" else "Library").symlink_to(
                root, target_is_directory=True
            )
        case "malformed-skill":
            config.write_text(f'state_dir = "{state}"\n[supervisor]\nskill_path = 42\n')
        case "missing-skill":
            config.write_text(f'state_dir = "{state}"\n[supervisor]\n')
        case "missing-state":
            config.write_text(
                f'[supervisor]\nskill_path = "{state}/skills/printobserver/SKILL.md"\n'
            )
        case "root-user":
            host.env["SUDO_USER"] = "root"
        case "unknown-user":
            host.env["SUDO_USER"] = "printobserver-account-that-does-not-exist"
        case "temporary":
            temporary = tmp_path / "not-temporary"
            temporary.write_bytes(b"a file")
            host.env["TMPDIR"] = str(temporary)
        case "missing-root-argument":
            args = ["--root"]
        case "unknown-option":
            args = ["--unknown"]
        case "no-hash":
            for name in (
                "uname",
                "getent",
                "dscl",
                "cut",
                "sed",
                "dirname",
                "mktemp",
                "basename",
                "cat",
                "rm",
                "find",
                "awk",
                "sh",
            ):
                real = shutil.which(name)
                if real:
                    (host.bin / name).symlink_to(real)
            host.env["PATH"] = str(host.bin)
        case _:
            raise AssertionError(refusal)
    before = _tree(root)
    result = run(
        ["sh", str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"), *args],
        env=host.env,
        timeout=30,
    )
    expected = {
        "root-relative": "No system unit or LaunchDaemon",
        "xdg-relative": "not an absolute path",
        "relative-source": "not an absolute path",
        "config-overlap": "overlap",
        "state-overlap": "overlap",
        "source-ancestor-overlap": "overlap",
        "bad-path": "carries a quote",
        "control-path": "control character",
        "source-link": "contains symbolic links",
        "destination-link": "is a symbolic link",
        "malformed-skill": "not a plain string",
        "missing-skill": "not a plain string",
        "missing-state": "not a plain string",
        "root-user": "SUDO_USER must name",
        "unknown-user": "SUDO_USER is not an account",
        "temporary": "Temporary staging could not be created",
        "missing-root-argument": "--root takes a directory",
        "unknown-option": "unknown option",
        "no-hash": "Install sha256sum or shasum",
    }
    failing(result, naming=expected[refusal])
    equal(_tree(root), before)
    truth(" stop " not in host.said(), describing="refused before stopping any system service")


@pytest.mark.parametrize("ending", [b"", b"\n", b"\r\n"])
@pytest.mark.parametrize("external", [False, True])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_preserves_quotes_comments_and_line_endings(
    tmp_path: Path, program: Path, ending: bytes, external: bool
) -> None:
    """Only the two path values change, including when the last newline is absent."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    skill_source = (
        root / "external/SKILL.md" if external else state / "skills/printobserver/SKILL.md"
    )
    if external:
        skill_source.parent.mkdir()
        skill_source.write_bytes(b"# externally installed skill")
    newline = b"\r\n" if ending == b"\r\n" else b"\n"
    original = (
        newline.join(
            [
                f"state_dir = '{state}' # state comment".encode(),
                b"[supervisor] # table comment",
                f'  skill_path = "{skill_source}" # skill comment'.encode(),
            ]
        )
        + ending
    )
    config.write_bytes(original)
    if host.system == "Darwin" and not ending:
        # An installed but unloaded daemon has no process left to stop.
        Path(host.env["SYSTEM_AGENT_STATE"]).unlink()
    passing(_migrate(host, root))
    expected = original.replace(
        f"state_dir = '{state}'".encode(), f"state_dir = '{host.migrated_state}'".encode()
    ).replace(
        f'skill_path = "{skill_source}"'.encode(),
        f'skill_path = "{host.migrated_state}/skills/printobserver/SKILL.md"'.encode(),
    )
    equal(host.migrated_config.read_bytes(), expected)
    equal(config.read_bytes(), original)
    if external:
        equal(skill_source.read_bytes(), b"# externally installed skill")


@pytest.mark.parametrize("failure", ["parent", "copy", "disable", "remove", "reload"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_failures_report_the_retained_registration_and_copies(
    tmp_path: Path, program: Path, failure: str
) -> None:
    """Actual filesystem failures and manager refusals retain the documented recoverable state."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    if failure in {"disable", "reload"} and host.system != "Linux":
        pytest.skip("systemd has these separate deregistration operations")
    state = root / "var/lib/printobserver"
    content = state / "kept"
    content.write_bytes(b"original state")
    _minimal_service_config(config, state)
    before_config, before_state = config.read_bytes(), _tree(state)
    restricted: Path | None = None
    match failure:
        case "parent":
            host.user_home.mkdir(parents=True)
            restricted = host.user_home
            restricted.chmod(0o500)
        case "copy":
            restricted = content
            restricted.chmod(0o000)
        case "disable":
            host.env["REFUSES"] = "systemctl disable printobserver.service"
        case "remove":
            restricted = host.system_definition.parent
            restricted.chmod(0o500)
        case "reload":
            host.env["REFUSES"] = "systemctl daemon-reload"
        case _:
            raise AssertionError(failure)
    try:
        result = _migrate(host, root)
        failing(
            result,
            naming="copy failed"
            if failure in {"parent", "copy"}
            else "copy verified"
            if failure in {"disable", "remove"}
            else "registration was removed",
        )
    finally:
        if restricted:
            restricted.chmod(0o700 if restricted.is_dir() else 0o600)
    equal(config.read_bytes(), before_config)
    equal(_tree(state), before_state)
    equal(host.system_definition.exists(), failure != "reload")
    equal(host.migrated_state.exists(), failure in {"disable", "remove", "reload"})


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_local_help_is_read_only(tmp_path: Path, program: Path) -> None:
    """Both local scripts explain their options without provisioning anything."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    _config = migration.service_config
    before = _tree(root)
    passing(host.install("--help"))
    passing(
        run(
            ["sh", str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"), "--help"],
            env=host.env,
            timeout=30,
        )
    )
    equal(_tree(root), before)
    equal(_tree(host.home), {})


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_relative_migration_root_is_normalized(tmp_path: Path, program: Path) -> None:
    """A relative --root resolves against the invocation directory, as on Windows."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("kept").write_bytes(b"record")
    _minimal_service_config(config, state)
    passing(
        run(
            [
                "sh",
                str(REPO_ROOT / "scripts/migrate-service-to-per-user.sh"),
                "--root",
                str(root.relative_to(tmp_path)),
            ],
            cwd=tmp_path,
            env=host.env,
            timeout=30,
        )
    )
    equal(_tree(host.migrated_state), _tree(state))


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_leaves_unrelated_client_file_ownership_untouched(
    tmp_path: Path, program: Path
) -> None:
    """An existing client file's distinct group is not transferred with the new config."""
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    primary = int(run(["id", "-g"], timeout=30).stdout.strip())
    groups = [int(group) for group in run(["id", "-G"], timeout=30).stdout.split()]
    other = next((group for group in groups if group != primary), None)
    if other is None:
        pytest.skip("the host has no secondary group to distinguish unrelated ownership")
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    host.migrated_config.parent.mkdir(parents=True)
    client = host.migrated_config.parent / "client.toml"
    client.write_bytes(b"existing operator configuration")
    passing(run(["chgrp", str(other), str(client)], timeout=30))
    before = client.stat()
    passing(_migrate(host, root))
    equal(client.stat().st_gid, before.st_gid, describing="unrelated client's group")
    equal(client.stat().st_uid, before.st_uid, describing="unrelated client's owner")
    equal(client.read_bytes(), b"existing operator configuration")


@pytest.mark.parametrize("destination", ["state", "config", "ancestor"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_installer_rejects_links_before_writing_user_files(
    tmp_path: Path, program: Path, destination: str
) -> None:
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    outside = tmp_path / "outside"
    outside.mkdir()
    outside.joinpath("retained").write_bytes(b"unrelated")
    target = host.state if destination == "state" else host.config
    if destination == "ancestor":
        target = host.config.parent
    target.parent.mkdir(parents=True, exist_ok=True)
    target.symlink_to(outside / "absent" if destination == "config" else outside)
    before = _tree(host.home), _tree(outside)
    failing(host.install(), naming="symbolic link")
    equal((_tree(host.home), _tree(outside)), before)
    truth("--user" not in host.said(), describing="no user-manager changes")


@pytest.mark.parametrize("location", ["source", "scan", "config", "state"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_unreadable_ancestors_and_scan_leave_originals(
    tmp_path: Path, program: Path, location: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    state.joinpath("retained").write_bytes(b"record")
    target = {
        "source": state,
        "scan": state / "unreadable",
        "config": host.migrated_config.parent,
        "state": host.migrated_state.parent,
    }[location]
    target.mkdir(parents=True, exist_ok=True)
    before = _tree(root)
    mode = target.stat().st_mode & 511
    target.chmod(0)
    try:
        result = _migrate(host, root)
        failing(result, naming="could not be scanned" if location == "scan" else "ancestors")
    finally:
        target.chmod(mode)
    equal(_tree(root), before)
    truth(" stop " not in host.said(), describing="refused before stopping the system service")


@pytest.mark.skipif(sys.platform != "darwin", reason="native launchctl boundary")
@pytest.mark.parametrize("scope", ["agent", "daemon"])
@pytest.mark.parametrize("failure", ["query", "pending-stop"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_launchctl_query_and_pending_stop_preserve_the_installation(
    tmp_path: Path, program: Path, scope: str, failure: str
) -> None:
    if scope == "agent":
        host = Host(tmp_path, "Darwin", program)
        passing(host.install())
        root = host.home
        query = f"launchctl print gui/{_uid()}/io.github.nickderobertis.printobserver"
    else:
        migration = _migration_fixture(tmp_path, program)
        host = migration.host
        root = migration.service_root
        config = migration.service_config
        _minimal_service_config(config, root / "var/lib/printobserver")
        query = "launchctl print system/io.github.nickderobertis.printobserver"
    before = _tree(root)
    if failure == "query":
        host.env["REFUSES"] = query
    else:
        host.env["STAYS_LOADED"] = "1"
    result = host.install() if scope == "agent" else _migrate(host, root)
    failing(result, naming="nothing was replaced" if scope == "agent" else "nothing was copied")
    contains(result.stderr or "", "launchctl print" if failure == "query" else "stop")
    if failure == "query":
        contains(result.stderr or "", "manager query refused")
        contains(result.stderr or "", "exit 1")
    equal(_tree(root), before)
    truth("bootstrap" not in host.said().split(query, 1)[-1], describing="no replacement job")


@pytest.mark.parametrize("upgrading", [False, True])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_initial_registration_query_failure_changes_no_user_files(
    tmp_path: Path, program: Path, upgrading: bool
) -> None:
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    if upgrading:
        passing(host.install())
    before = _tree(host.home)
    command = (
        "systemctl show --property=LoadState --value printobserver.service"
        if host.system == "Linux"
        else "launchctl print system/io.github.nickderobertis.printobserver"
    )
    host.env["REFUSES"] = command
    result = host.install()
    failing(result, naming="registration could not be queried")
    contains(result.stderr or "", command)
    contains(result.stderr or "", "manager query refused")
    equal(_tree(host.home), before)
    truth(
        "--user stop" not in host.said(), describing="refused before interrupting the user server"
    )


@pytest.mark.parametrize("directory", ["relative/bin", 'relative"unsafe'])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_per_user_relative_program_directory_is_resolved_from_the_callers_directory(
    tmp_path: Path, program: Path, directory: str
) -> None:
    host = Host(tmp_path, NATIVE_SYSTEM, program)
    result = host.install("--to", directory)
    if '"' in directory:
        failing(result, naming="path carries")
        equal(_tree(host.home), {})
        return
    passing(result)
    executable = host.home / directory / "printobserver"
    passing(run([str(executable), "--version"], timeout=30))
    if host.system == "Linux":
        contains(
            host.definition.read_text(),
            f'ExecStart="{executable}" server --config "{host.config}"',
        )
    else:
        equal(
            plistlib.loads(host.definition.read_bytes())["ProgramArguments"],
            [str(executable), "server", "--config", str(host.config)],
        )


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_preserves_a_destination_created_while_the_service_stops(
    tmp_path: Path, program: Path
) -> None:
    """An operator can create state while stop waits; cleanup owns only its own copy."""
    import threading
    import time

    migration = _migration_fixture(tmp_path, program)

    host = migration.host

    root = migration.service_root

    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("record").write_bytes(b"service record")
    _minimal_service_config(config, state)
    before = _tree(state), config.read_bytes()
    gate = tmp_path / "stop-gate"
    passing(run(["mkfifo", str(gate)], timeout=30))
    host.env["STOP_GATE"] = str(gate)
    stop = (
        "systemctl stop printobserver.service"
        if host.system == "Linux"
        else "launchctl bootout system/io.github.nickderobertis.printobserver"
    )
    kept = host.migrated_state / "operator-record"
    errors: list[BaseException] = []

    def operator() -> None:
        stopped = False
        try:
            deadline = time.monotonic() + 20
            while stop not in host.said():
                if time.monotonic() >= deadline:
                    raise AssertionError("stop did not reach the rendezvous within the hang guard")
                time.sleep(0.01)
            stopped = True
            kept.parent.mkdir(parents=True)
            kept.parent.chmod(0o750)
            kept.write_bytes(b"operator record")
            kept.chmod(0o640)
        except BaseException as error:
            errors.append(error)
        finally:
            if stopped:
                with gate.open("w", encoding="utf-8") as release:
                    release.write("continue\n")

    actor = threading.Thread(target=operator, daemon=True)
    actor.start()
    try:
        result = _migrate(host, root)
    finally:
        actor.join(timeout=40)
    truth(not actor.is_alive(), describing="the actual operator actor finished")
    equal(errors, [])
    failing(result, naming="did not verify")
    truth(kept.is_file(), describing="cleanup preserves the operator's concurrent destination")
    equal(kept.read_bytes(), b"operator record")
    equal(kept.parent.stat().st_mode & 0o777, 0o750)
    equal(kept.stat().st_mode & 0o777, 0o640)
    equal((_tree(state), config.read_bytes()), before)
    truth(host.system_definition.is_file(), describing="the system registration remains installed")
    truth("systemctl disable" not in host.said(), describing="no unverified removal")


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_uses_the_filesystem_meaning_of_a_symlink_followed_by_parent(
    tmp_path: Path, program: Path
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    physical = root / "physical/state"
    lexical = root / "spelled/state"
    physical.mkdir(parents=True)
    lexical.mkdir(parents=True)
    physical.joinpath("record").write_bytes(b"the printer's actual record")
    lexical.joinpath("record").write_bytes(b"an unrelated lexical destination")
    physical.parent.joinpath("deep").mkdir()
    link = root / "spelled/link"
    link.symlink_to(physical.parent / "deep", target_is_directory=True)
    configured = link / ".." / "state"
    _minimal_service_config(config, configured)
    original = _tree(physical), _tree(lexical), config.read_bytes()
    passing(_migrate(host, root))
    equal((host.migrated_state / "record").read_bytes(), b"the printer's actual record")
    equal((_tree(physical), _tree(lexical), config.read_bytes()), original)


@pytest.mark.parametrize("quote", ['"', "'"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_decodes_basic_backslashes_and_preserves_literal_backslashes(
    tmp_path: Path, program: Path, quote: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "record\\archive"
    state.mkdir()
    state.joinpath("record").write_bytes(b"backslash path record")
    value = json.dumps(str(state)) if quote == '"' else f"'{state}'"
    original = f"state_dir = {value}\n[supervisor]\nskill_path = '/external/SKILL.md'\n"
    config.write_text(original)
    passing(_migrate(host, root))
    equal((host.migrated_state / "record").read_bytes(), b"backslash path record")
    equal(config.read_text(), original)
    equal(tomllib.loads(host.migrated_config.read_text())["state_dir"], str(host.migrated_state))


@pytest.mark.parametrize("field", ["state_dir", "skill_path"])
@pytest.mark.parametrize("malformed", ["escape", "unterminated", "trailing", "duplicate"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_rejects_malformed_and_duplicate_path_settings_before_stop(
    tmp_path: Path, program: Path, field: str, malformed: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("record").write_bytes(b"existing record")
    values = {"state_dir": f'"{state}"', "skill_path": '"/external/SKILL.md"'}
    match malformed:
        case "escape":
            values[field] = r'"/invalid\qpath"'
        case "unterminated":
            values[field] = values[field][:-1]
        case "trailing":
            values[field] += " unexpected-content"
        case "duplicate":
            pass
        case _:
            raise AssertionError(malformed)
    state_line = f"state_dir = {values['state_dir']}\n"
    skill_line = f"skill_path = {values['skill_path']}\n"
    if malformed == "duplicate":
        if field == "state_dir":
            state_line *= 2
        else:
            skill_line *= 2
    config.write_text(state_line + "[supervisor]\n" + skill_line)
    before = _tree(root)
    failing(_migrate(host, root), naming="not a plain string")
    equal(_tree(root), before)
    truth(" stop " not in host.said(), describing="malformed settings refuse before stopping")


def _opaque_note(kind: str, payload: str) -> str:
    match kind:
        case "multiline-basic" | "escaped-multiline":
            delimiter = '"' * 3
            if kind == "escaped-multiline":
                payload = r'\""" is string data' + "\n" + payload
            return f"notes = {delimiter}\n{payload}\n{delimiter}\n"
        case "multiline-literal":
            delimiter = "'" * 3
            return f"notes = {delimiter}\n{payload}\n{delimiter}\n"
        case "quoted-basic":
            return "notes = " + json.dumps('""" and ' + "'" * 3 + " and # [] {} are text") + "\n"
        case "quoted-literal":
            return 'notes = \'""" and # [] {} are text\'\n'
        case "comment":
            return 'notes = "ordinary" # ' + '"' * 3 + " and " + "'" * 3 + " are comments\n"
        case "array":
            return "notes = [\n []\n]\n"
        case "inline":
            return 'notes = { value = "[] {} # are text", state_dir = "/decoy" }\n'
        case _:
            raise AssertionError(kind)


@pytest.mark.parametrize("scope", ["root", "supervisor"])
@pytest.mark.parametrize(
    "kind",
    [
        "multiline-basic",
        "multiline-literal",
        "escaped-multiline",
        "quoted-basic",
        "quoted-literal",
        "comment",
        "array",
        "inline",
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_preserves_opaque_toml_context_around_real_settings(
    tmp_path: Path, program: Path, scope: str, kind: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("record").write_bytes(b"actual record")
    state_line = f'state_dir = "{state}"\n'
    skill_line = 'skill_path = "/external/SKILL.md"\n'
    note = _opaque_note(kind, '# string data\nstate_dir = "/decoy"\n[decoy]\nskill_path = "/decoy"')
    original = (
        note + state_line + "[supervisor]\n" + skill_line
        if scope == "root"
        else state_line + "[supervisor]\n" + note + skill_line
    )
    tomllib.loads(original)
    config.write_text(original)
    passing(_migrate(host, root))
    expected = original.replace(state_line, f'state_dir = "{host.migrated_state}"\n').replace(
        skill_line, f'skill_path = "{host.migrated_state}/skills/printobserver/SKILL.md"\n'
    )
    equal(host.migrated_config.read_text(), expected)
    tomllib.loads(expected)
    equal((host.migrated_state / "record").read_bytes(), b"actual record")
    equal(config.read_text(), original)


@pytest.mark.parametrize(
    ("kind", "missing"),
    [
        ("multiline-basic", "state_dir"),
        ("multiline-literal", "state_dir"),
        ("multiline-basic", "skill_path"),
        ("multiline-literal", "skill_path"),
        ("array", "state_dir"),
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_toml_string_or_container_text_cannot_supply_a_missing_setting(
    tmp_path: Path, program: Path, kind: str, missing: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("record").write_bytes(b"actual record")
    state_line = f'state_dir = "{state}"\n'
    skill_line = 'skill_path = "/external/SKILL.md"\n'
    if kind == "array":
        original = "[supervisor]\n" + skill_line + _opaque_note(kind, "") + state_line
    elif missing == "state_dir":
        original = _opaque_note(kind, state_line.rstrip()) + "[supervisor]\n" + skill_line
    else:
        original = state_line + "[supervisor]\n" + _opaque_note(kind, skill_line.rstrip())
    parsed = tomllib.loads(original)
    truth(
        missing not in (parsed if missing == "state_dir" else parsed["supervisor"]),
        describing="the required setting is genuinely absent",
    )
    config.write_text(original)
    before = _tree(root)
    failing(_migrate(host, root), naming="not a plain string")
    equal(_tree(root), before)
    equal(host.said(), "", describing="no service-manager operation before refusal")


@pytest.mark.parametrize("opening", ['"""', "'''", '"', "'", "[", "{"])
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_refuses_unclosed_string_and_container_context(
    tmp_path: Path, program: Path, opening: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    with config.open("a") as target:
        target.write(f"notes = {opening}unterminated\n")
    with pytest.raises(tomllib.TOMLDecodeError):
        tomllib.loads(config.read_text())
    before = _tree(root)
    failing(_migrate(host, root), naming="context")
    equal(_tree(root), before)
    equal(host.said(), "", describing="no service-manager operation before refusal")


@pytest.mark.parametrize(
    "malformed", ["trailing", "duplicate", "spaced-duplicate", "dotted-duplicate"]
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_refuses_malformed_or_duplicate_tables_before_stop(
    tmp_path: Path, program: Path, malformed: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    original = config.read_text()
    match malformed:
        case "trailing":
            original = original.replace("[supervisor]", "[supervisor] trailing-garbage")
        case "duplicate":
            original += "[supervisor]\n"
        case "spaced-duplicate":
            original += "[ supervisor ]\n"
        case "dotted-duplicate":
            original += "[extra.nested]\nvalue = 1\n[ extra . nested ]\n"
        case _:
            raise AssertionError(malformed)
    with pytest.raises(tomllib.TOMLDecodeError):
        tomllib.loads(original)
    config.write_text(original)
    before = _tree(root)
    failing(_migrate(host, root), naming="table")
    equal(_tree(root), before)
    equal(host.said(), "", describing="table errors refuse before stopping the service")


@pytest.mark.parametrize(
    "header", ['["extra"]', "['extra']", r'["\u0065xtra"]', '[extra."nested"]', "[[extra]]"]
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_unsupported_table_key_forms_refuse_without_any_mutation(
    tmp_path: Path, program: Path, header: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    _minimal_service_config(config, root / "var/lib/printobserver")
    with config.open("a") as target:
        target.write(header + '\nvalue = "retained"\n')
    tomllib.loads(config.read_text())
    before = _tree(root)
    result = _migrate(host, root)
    failing(result, naming="table header")
    contains(result.stderr or "", header.rstrip("]"))
    contains(result.stderr or "", "generated bare/dotted header form")
    contains(result.stderr or "", "migrate by hand")
    equal(_tree(root), before)
    equal(host.said(), "", describing="unsupported forms refuse before stopping")


@pytest.mark.parametrize(
    "headers",
    [
        "[extra]\nvalue = 1\n",
        "[extra.nested]\nvalue = 1\n",
        "[ extra . nested-key ] # retained\nvalue = 1\n",
        "[extra__2.sub-3]\nvalue = 1\n",
        "[extra.nested]\nvalue = 1\n[extra]\nother = 2\n",
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_generated_bare_and_dotted_tables_survive_migration(
    tmp_path: Path, program: Path, headers: str
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    with config.open("a") as target:
        target.write(headers)
    original = config.read_text()
    tomllib.loads(original)
    passing(_migrate(host, root))
    expected = original.replace(str(state), str(host.migrated_state))
    equal(host.migrated_config.read_text(), expected)
    tomllib.loads(expected)
    equal(config.read_text(), original)


@pytest.mark.parametrize(
    ("extra", "valid_toml"),
    [
        ('"state_dir" = "/decoy"', False),
        ("unrelated = INVALID", False),
        ("unrelated = 01", False),
        ('unrelated = "bad\\q"', False),
        ("unrelated = [1,,2]", False),
        ("unrelated = { value = INVALID }", False),
        ("unrelated = { value = 1, value = 2 }", False),
        ("unrelated = { value = 1,\n other = 2 }", False),
        ('unrelated = "\\ud800"', False),
        ('unrelated = "\\U00110000"', False),
        ("unrelated = 9223372036854775808", True),
        ("unrelated = -9223372036854775809", True),
        ("unrelated = 0x8000000000000000", True),
        ("unrelated = 0o1000000000000000000000", True),
        ("unrelated = 0b1000000000000000000000000000000000000000000000000000000000000000", True),
        ('"unrelated" = 1', True),
        ("unrelated.nested = 1", True),
        ("unrelated = 2026-10-28", True),
        (
            "unrelated = " + "[" * 65 + "0" + "]" * 65,
            True,
        ),
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_refuses_unsupported_keys_and_invalid_values_before_stop(
    tmp_path: Path, program: Path, extra: str, valid_toml: bool
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    root = migration.service_root
    config = migration.service_config
    state = root / "var/lib/printobserver"
    state.joinpath("record").write_bytes(b"existing record")
    _minimal_service_config(config, state)
    original = config.read_text()
    config.write_text(extra + "\n" + original)
    if valid_toml:
        tomllib.loads(config.read_text())
    else:
        with pytest.raises(tomllib.TOMLDecodeError):
            tomllib.loads(config.read_text())
    before = _tree(root)
    result = _migrate(host, root)
    failing(result, naming="line 1")
    equal(_tree(root), before, describing="source config, state and service registration unchanged")
    truth(not host.migrated_config.exists(), describing="no destination config created")
    truth(not host.migrated_state.exists(), describing="no destination state created")
    equal(host.said(), "", describing="no service-manager operation before refusal")


@pytest.mark.parametrize(
    "value",
    [
        "true",
        "false",
        "+inf",
        "-nan",
        "0",
        "+1",
        "-9223372036854775808",
        "9223372036854775807",
        "0x7fff_ffff_ffff_ffff",
        "0o777777777777777777777",
        "0b1111",
        "1_234",
        "1.25",
        "-0.5e+2",
        "1e-3",
        '[true, 1, "text", { nested = [2, 3] },]',
        '"escaped\\ntext\\u0061\\U0001F600"',
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_preserves_supported_values(tmp_path: Path, program: Path, value: str) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    config = migration.service_config
    state = migration.service_root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    original = "unrelated = " + value + "\n" + config.read_text()
    tomllib.loads(original)
    config.write_text(original)
    passing(_migrate(host, migration.service_root))
    equal(host.migrated_config.read_text(), original.replace(str(state), str(host.migrated_state)))
    equal(config.read_text(), original)


@pytest.mark.parametrize(
    ("scope", "key", "supported"),
    [
        ("safety.allowed", '"tool_target:0"', True),
        ("safety.allowed", '"tool_target:12"', True),
        ("safety.allowed", '"tool_target:-1"', False),
        ("safety.allowed", "'tool_target:0'", False),
        ("safety.allowed", '"feedrate"', False),
        ("supervisor", '"tool_target:0"', False),
        ("other", '"tool_target:0"', False),
        ("", '"tool_target:0"', False),
    ],
)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] Required journey location.
def test_migration_accepts_only_factory_quoted_tool_keys(
    tmp_path: Path, program: Path, scope: str, key: str, supported: bool
) -> None:
    migration = _migration_fixture(tmp_path, program)
    host = migration.host
    config = migration.service_config
    root = migration.service_root
    state = root / "var/lib/printobserver"
    _minimal_service_config(config, state)
    extra = f"{key} = {{ min = 0.0, max = 260.0 }}\n"
    original = config.read_text()
    if scope == "supervisor":
        original += extra
    elif scope:
        original += f"[{scope}]\n" + extra
    else:
        original = extra + original
    tomllib.loads(original)
    config.write_text(original)
    before = _tree(root)
    result = _migrate(host, root)
    if supported:
        passing(result)
        equal(
            host.migrated_config.read_text(), original.replace(str(state), str(host.migrated_state))
        )
    else:
        failing(result, naming="key/value at line")
        equal(_tree(root), before)
        equal(host.said(), "", describing="unsupported quoted key refuses before service effects")
        truth(not host.migrated_config.exists(), describing="no config destination created")
        truth(not host.migrated_state.exists(), describing="no state destination created")
