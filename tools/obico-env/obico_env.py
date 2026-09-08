#!/usr/bin/env python3
"""The self-hosted Obico environment the scheduled reconciliation tier drives.

Nothing on the machine beside the printer runs Obico, so this environment is a
deliverable rather than an assumption: it brings up a self-hosted Obico from
that project's own development composition, in containers, unattended.

    obico_env.py install   clone the pinned sources and build the images
    obico_env.py up        install, start, wait until usable, and provision
    obico_env.py down      stop it, leaving no container of it running

Containers rather than a script, which is the opposite of the sibling OctoPrint
environment and deliberately so: OctoPrint has to reach a USB serial port on the
machine beside the printer, and Obico reaches nothing but the network.

`up` answers on stdout with one JSON document naming the state directory, the
URL the web application answers on, the account, the registered printer and its
token, and the webhook address the notification plugin was configured to post
to; progress goes to stderr. Before it starts anything it says on stderr what it
is about to start, naming every service, and roughly how long that takes.

`up` returns when the services are ready rather than after a duration it chose:
every service in `SERVICES` is polled for an answer of its own, in the order
below, and the first one that never answers is what the failure names. Every way
starting can fail is one of `FAILURE_CLASSES`, reported by name with the next
action to take; anything outside that closed set is reported with the underlying
error's own text.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Any

# The Obico release this environment brings up, pinned to a revision. An
# environment that tracked a moving branch would make this tier's findings
# depend on the day it ran, and this tier exists precisely to date a claim about
# an external producer — so the revision it reconciles against is written down.
OBICO_REPOSITORY = "https://github.com/TheSpaghettiDetective/obico-server.git"
OBICO_BRANCH = "release"
OBICO_REVISION = "49c0bc7001a3fd8d56297fc3032ba287bfe1d50b"

# The compose project every container of this environment belongs to. `down`
# names it, so nothing this environment did not create is ever stopped.
PROJECT = "printobserver-obico"

HOST = "127.0.0.1"
DEFAULT_STATE_DIR = ".obico-env"

# The address the containers reach the caller by. Compose maps it to the host
# gateway in the override this script writes, because the webhook is delivered
# by a container and the caller listens on the host.
CONTAINER_HOST_ALIAS = "host.docker.internal"

# The account, the printer and the print this environment provisions. The print
# is running rather than finished: Obico sends the empty string for an instant it
# has none of, and a *running* print is the state the committed sample records.
ACCOUNT_EMAIL = "printobserver@example.invalid"
PRINTER_NAME = "printobserver"
PRINT_FILENAME = "benchy.gcode"

# Obico's own name for the notification plugin that posts a webhook.
WEBHOOK_PLUGIN = "webhook"

# Roughly how long starting takes, which `announce` says before anything starts.
COLD_MINUTES = 25
WARM_MINUTES = 3


@dataclass(frozen=True, slots=True)
class Service:
    """One container this environment starts, and how it is asked if it is ready."""

    name: str
    role: str
    # The container port to publish, or None for a service reached through
    # `docker compose exec` because it listens on nothing.
    port: int | None
    probe: str


# Every service this environment brings up, in the order they are waited for.
# The order is the dependency order, so a stack whose queue never came up says
# `redis` rather than blaming what could not reach it.
SERVICES: tuple[Service, ...] = (
    Service("redis", "the queue and cache the web application and the worker share", 6379, "redis"),
    Service("ml_api", "the failure-detection service", 3333, "http"),
    Service("web", "the web application, and the SQLite database it holds", 3334, "http"),
    Service("tasks", "the worker that delivers notifications", None, "celery"),
)

# Every way starting this environment can fail, and the next action for each.
# The set is closed: `up` reaches no other diagnosed exit, and anything outside
# it is reported with the underlying error's own text.
FAILURE_CLASSES: dict[str, str] = {
    "docker-unavailable": (
        "install Docker and the Compose plugin, and check `docker info` answers for "
        "this user — this environment is containerized and has no scripted mode"
    ),
    "state-unusable": (
        "read the path the message names, move or delete whatever is in the way, and "
        "run `up` again — a state directory that is not there is created from scratch"
    ),
    "source-unavailable": (
        "check network access to github.com, then delete the `obico-server` directory "
        "the message names and run `install` again to clone it afresh"
    ),
    "images-unbuildable": (
        "read the build output the message names: the images are built from Obico's "
        "own sources, so its build is what says why"
    ),
    "containers-unstartable": (
        "read the message's own compose output, and check `docker ps` for a container "
        "of an earlier run still holding a port — `down` stops those"
    ),
    "service-not-ready": (
        "run the `docker compose ... logs` command the message names for the service "
        "the message names: that service's own startup output is what says why"
    ),
    "provisioning-failed": (
        "read the message's own output from Obico's `manage.py`: the stack came up but "
        "would not take the account, the printer or the notification setting"
    ),
    "webhook-address-unusable": (
        "pass `--webhook-url` as an absolute http URL carrying a host and a port that "
        "a container can reach, such as "
        f"`http://{CONTAINER_HOST_ALIAS}:41075/alert`"
    ),
}


class StartupError(Exception):
    """One of the declared failure classes, with what happened and what to do."""

    def __init__(self, failure_class: str, detail: str) -> None:
        """Name a declared class and say what happened.

        Args:
            failure_class: A key of `FAILURE_CLASSES`.
            detail: What happened, concretely enough to act on.

        Raises:
            KeyError: If the class is not one this script declares, so that an
                undeclared failure cannot be dressed up as a declared one.
        """
        self.failure_class = failure_class
        self.detail = detail
        self.next_action = FAILURE_CLASSES[failure_class]
        super().__init__(f"{failure_class}: {detail}")


class Stack:
    """One self-hosted Obico under one state directory."""

    def __init__(self, state_dir: Path, project: str = PROJECT) -> None:
        """Bind to the state directory this stack lives under."""
        self.state_dir = state_dir.resolve()
        self.project = project

    @property
    def source(self) -> Path:
        """Obico's own sources, cloned at the pinned revision."""
        return self.state_dir / "obico-server"

    @property
    def compose_file(self) -> Path:
        """Obico's own development composition, which this script does not edit."""
        return self.source / "docker-compose.yml"

    @property
    def override_file(self) -> Path:
        """The override this script writes: published ports and the host alias."""
        return self.state_dir / "compose-override.yml"

    @property
    def build_log(self) -> Path:
        """Everything building the images said."""
        return self.state_dir / "build.log"

    @property
    def record(self) -> Path:
        """What `up` wrote about the running stack, for `down` and for callers."""
        return self.state_dir / "instance.json"

    @property
    def logs_command(self) -> str:
        """The command a reader runs to see a service's own startup output."""
        return (
            f"docker compose -p {self.project} -f {self.compose_file} -f {self.override_file} logs"
        )

    def compose(self, *arguments: str) -> list[str]:
        """One `docker compose` command line against this stack's own files."""
        return [
            "docker",
            "compose",
            "-p",
            self.project,
            "-f",
            str(self.compose_file),
            "-f",
            str(self.override_file),
            *arguments,
        ]


def note(message: str) -> None:
    """Say what is happening, on the stream the JSON answer is not on."""
    print(f"obico-env: {message}", file=sys.stderr, flush=True)


def announce(webhook_url: str) -> None:
    """Say what is about to be started, and roughly how long it takes.

    Said before anything is started, because this stack is heavy: a caller who
    learns what it is waiting for only once it has waited has learned it too
    late.
    """
    note(f"about to start a self-hosted Obico: {len(SERVICES)} containers, which are")
    for service in SERVICES:
        note(f"  {service.name} — {service.role}")
    note(
        f"roughly how long: about {COLD_MINUTES} minutes the first time, because the "
        f"images are built from Obico's own sources and one of them carries a "
        f"machine-learning model; about {WARM_MINUTES} minutes once they are built"
    )
    note(f"the notification plugin will be configured to post to {webhook_url}")


def _resolve(program: str) -> str:
    """The absolute path of a program this script starts.

    Raises:
        StartupError: If Docker or git — the only two it starts — is not there.
        FileNotFoundError: If any other program is not on PATH, which the
            outside-the-set report is what catches.
    """
    found = shutil.which(program)
    if found is not None:
        return found
    if program == "docker":
        raise StartupError("docker-unavailable", "docker is not on PATH")
    if program == "git":
        raise StartupError(
            "source-unavailable", "git is not on PATH; it is what fetches Obico's sources"
        )
    message = f"{program} is not on PATH"
    raise FileNotFoundError(message)


def _spawn(
    argv: list[str],
    *,
    stdin: int | None = None,
    stdout: int | IO[str] | None = None,
    stderr: int | None = None,
) -> subprocess.Popen[str]:
    """Start a program by absolute path, never through a shell.

    Every subprocess this script starts goes through here, so `S603` has one
    reviewable site rather than one per caller, and `S607` is fixed rather than
    suppressed: `argv[0]` is resolved against PATH first.
    """
    return subprocess.Popen(  # noqa: S603
        [_resolve(argv[0]), *argv[1:]],
        text=True,
        stdin=stdin,
        stdout=stdout,
        stderr=stderr,
    )


def run_program(
    argv: list[str], *, timeout: float = 3600, stdin: str | None = None
) -> subprocess.CompletedProcess[str]:
    """Run a program to completion and hand back everything it said.

    Raises:
        subprocess.TimeoutExpired: If it does not finish in time — having killed
            it first, so a readiness probe that gave up leaves no process behind
            to be waited on by nobody.
    """
    process = _spawn(
        argv,
        stdin=None if stdin is None else subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        output, _ = process.communicate(input=stdin, timeout=timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate()
        raise
    return subprocess.CompletedProcess(argv, process.returncode, output, "")


def free_port() -> int:
    """A port nothing is listening on, so two runs on one host do not collide."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind((HOST, 0))
        return int(probe.getsockname()[1])


# ~~ provisioning the sources and the images


def install(stack: Stack) -> dict[str, int]:
    """Clone the pinned sources and build the images, doing nothing already done.

    Returns:
        The host port chosen for each service that publishes one.
    """
    _require_docker()
    _make_state_dir(stack)
    _clone(stack)
    ports = _chosen_ports(stack)
    _write_override(stack, ports)
    _build(stack)
    return ports


def _chosen_ports(stack: Stack) -> dict[str, int]:
    """The host port for each published service, keeping a running stack's own.

    Fresh ports on every call would be a different override on every call, and
    `docker compose up -d` would then recreate four containers each time `up` is
    run against a stack that is already up — which is what a caller running the
    bring-up recipe and then the tier does.
    """
    published = [service.name for service in SERVICES if service.port is not None]
    if stack.record.is_file() and running(stack):
        recorded = json.loads(stack.record.read_text(encoding="utf-8")).get("ports", {})
        if all(name in recorded for name in published):
            note(f"keeping the ports the running stack already published: {recorded}")
            return {name: int(recorded[name]) for name in published}
    return {name: free_port() for name in published}


def _require_docker() -> None:
    """Refuse before anything is cloned when there is no Docker to run it on.

    Raises:
        StartupError: If Docker or its Compose plugin is not usable here.
    """
    answered = run_program(["docker", "compose", "version"], timeout=120)
    if answered.returncode != 0:
        raise StartupError(
            "docker-unavailable", f"`docker compose version` failed:\n{answered.stdout}"
        )


def _make_state_dir(stack: Stack) -> None:
    """Make the state directory, saying so when something is in the way.

    Raises:
        StartupError: If the path cannot be made a directory.
    """
    try:
        stack.state_dir.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise StartupError(
            "state-unusable", f"{stack.state_dir} could not be made a directory: {error}"
        ) from error


def _clone(stack: Stack) -> None:
    """Put Obico's own sources at the pinned revision under the state directory.

    Raises:
        StartupError: If they cannot be fetched or checked out.
    """
    if (stack.source / ".git").is_dir():
        at = run_program(["git", "-C", str(stack.source), "rev-parse", "HEAD"], timeout=120)
        if at.returncode == 0 and at.stdout.strip() == OBICO_REVISION:
            note(f"Obico's sources are already at {OBICO_REVISION[:12]} in {stack.source}")
            return
    note(f"cloning {OBICO_REPOSITORY} at {OBICO_REVISION[:12]} into {stack.source}")
    if stack.source.exists():
        shutil.rmtree(stack.source, ignore_errors=True)
    cloned = run_program(
        ["git", "clone", "--branch", OBICO_BRANCH, OBICO_REPOSITORY, str(stack.source)],
        timeout=1800,
    )
    if cloned.returncode != 0:
        raise StartupError("source-unavailable", f"could not clone Obico:\n{cloned.stdout}")
    checked_out = run_program(
        ["git", "-C", str(stack.source), "checkout", "--detach", OBICO_REVISION], timeout=600
    )
    if checked_out.returncode != 0:
        raise StartupError(
            "source-unavailable",
            f"{stack.source} does not carry revision {OBICO_REVISION}:\n{checked_out.stdout}",
        )


def override(ports: dict[str, int], webhook_host: str = CONTAINER_HOST_ALIAS) -> str:
    """The compose override this script writes over Obico's own composition.

    Obico's own `docker-compose.yml` is left exactly as that project ships it —
    this is a self-hosted Obico rather than a rewrite of one. The override adds
    two things and nothing else: a published host port per service, so two runs
    on one host do not collide and a caller can poll each service itself, and
    the host alias the webhook is delivered to, because the plugin posts from a
    container and the caller listens on the host.
    """
    published = {
        service.name: [f"{HOST}:{ports[service.name]}:{service.port}"]
        for service in SERVICES
        if service.port is not None
    }
    lines = ["# Written by tools/obico-env/obico_env.py. Do not edit.", "services:"]
    for service in SERVICES:
        lines.append(f"  {service.name}:")
        lines.append("    extra_hosts:")
        lines.append(f'      - "{webhook_host}:host-gateway"')
        if service.name in published:
            # `!override` rather than a merge: Obico's own file publishes `web`
            # on a fixed 3334, and a merged list would keep that one too.
            lines.append("    ports: !override")
            lines.extend(f'      - "{mapping}"' for mapping in published[service.name])
    return "\n".join(lines) + "\n"


def _write_override(stack: Stack, ports: dict[str, int]) -> None:
    """Write the override beside the state directory, naming the chosen ports."""
    stack.override_file.write_text(override(ports), encoding="utf-8")
    note(
        "published " + ", ".join(f"{name} on {HOST}:{port}" for name, port in sorted(ports.items()))
    )


def _build(stack: Stack) -> None:
    """Build the images from Obico's own sources.

    Raises:
        StartupError: If the build fails, naming the log that says why.
    """
    note(f"building the images from {stack.source} (output goes to {stack.build_log})")
    with stack.build_log.open("w", encoding="utf-8") as log:
        process = _spawn(stack.compose("build"), stdout=log, stderr=subprocess.STDOUT)
        code = process.wait(timeout=7200)
    if code != 0:
        raise StartupError(
            "images-unbuildable", f"building the images failed; read {stack.build_log}"
        )


# ~~ starting and waiting


def start(stack: Stack) -> None:
    """Start every service, without waiting for any of them.

    Raises:
        StartupError: If compose refuses to start them.
    """
    note(f"starting the {len(SERVICES)} containers")
    started = run_program(stack.compose("up", "-d"), timeout=1800)
    if started.returncode != 0:
        raise StartupError(
            "containers-unstartable", f"`docker compose up -d` failed:\n{started.stdout}"
        )


def http_answers(port: int, path: str, *, timeout: float = 5.0) -> bool:
    """Whether a service answers 200 on one of its own paths."""
    connection = http.client.HTTPConnection(HOST, port, timeout=timeout)
    try:
        connection.request("GET", path)
        return connection.getresponse().status == 200
    except OSError, http.client.HTTPException:
        return False
    finally:
        connection.close()


def redis_answers(port: int, *, timeout: float = 5.0) -> bool:
    """Whether the queue answers its own PING."""
    try:
        with socket.create_connection((HOST, port), timeout=timeout) as probe:
            probe.sendall(b"PING\r\n")
            return probe.recv(64).startswith(b"+PONG")
    except OSError:
        return False


def celery_answers(stack: Stack, service: str, *, timeout: float = 30.0) -> bool:
    """Whether the worker answers Celery's own ping.

    The worker listens on nothing, so it is asked through the container rather
    than over a port — which is the same question Obico's own healthcheck asks.
    """
    try:
        answered = run_program(
            stack.compose("exec", "-T", service, "celery", "-A", "config", "inspect", "ping"),
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return False
    return answered.returncode == 0 and "pong" in answered.stdout


def ready(stack: Stack, service: Service, ports: dict[str, int]) -> bool:
    """Whether one service answers for itself."""
    if service.probe == "redis":
        return redis_answers(ports[service.name])
    if service.probe == "http":
        return http_answers(ports[service.name], "/hc/")
    return celery_answers(stack, service.name)


def wait_for_services(stack: Stack, ports: dict[str, int], timeout: float) -> None:
    """Answer only once every service answers for itself, in dependency order.

    This is what makes the wait a condition rather than a duration: each service
    is polled until it answers, and a run that answers sooner returns sooner.

    Raises:
        StartupError: If a service never answers, naming that service.
    """
    for service in SERVICES:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if ready(stack, service, ports):
                note(f"{service.name} is ready")
                break
            time.sleep(2.0)
        else:
            raise StartupError(
                "service-not-ready",
                f"the `{service.name}` service ({service.role}) did not answer within "
                f"{timeout:.0f}s; run `{stack.logs_command} {service.name}`",
            )


# ~~ provisioning the account, the printer and the notification plugin

PROVISION_SCRIPT = """
import json, os, secrets
from binascii import hexlify
from django.contrib.sites.models import Site
from django.utils import timezone
from app.models import User, Printer, Print, NotificationSetting
from app.models.syndicate_models import Syndicate

request = json.loads({request!r})
host = request['site']

# Obico builds every media URL from the domain of the first site of the default
# syndicate, so a stack whose site still said `example.com` would hand out image
# URLs nothing can fetch.
syndicate = Syndicate.objects.get(id=1)
site = syndicate.sites.first()
if site.domain != host:
    Site.objects.filter(domain=host).exclude(pk=site.pk).delete()
    site.domain = host
    site.name = host
    site.save()

user = User.objects.filter(email=request['email']).first()
if user is None:
    user = User.objects.create_user(email=request['email'], password=secrets.token_hex(16))
    user.is_active = True
    user.save()

printer = Printer.objects.filter(user=user, name=request['printer']).first()
if printer is None:
    printer = Printer.objects.create(
        user=user, name=request['printer'], auth_token=hexlify(os.urandom(10)).decode()
    )

# A print that is running, not one that finished: the producer sends the empty
# string for an instant it has none of, which is what `ended_at` carries here.
current = printer.current_print
if current is None or current.finished_at or current.cancelled_at:
    current = Print.objects.create(
        user=user,
        printer=printer,
        filename=request['filename'],
        started_at=timezone.now(),
        ext_id=int(timezone.now().timestamp()),
    )
    printer.current_print = current
    printer.save()

setting, _ = NotificationSetting.objects.get_or_create(user=user, name=request['plugin'])
setting.enabled = True
setting.notify_on_failure_alert = True
setting.config_json = json.dumps({{'custom_webhook_URL': request['webhook_url']}})
setting.save()

print('OBICO_ENV_PROVISIONED ' + json.dumps({{
    'site': site.domain,
    'account_id': user.id,
    'account_email': user.email,
    'printer_id': printer.id,
    'printer_name': printer.name,
    'printer_token': printer.auth_token,
    'print_id': current.id,
    'print_filename': current.filename,
    'notification_setting_id': setting.id,
    'webhook_url': json.loads(setting.config_json)['custom_webhook_URL'],
}}))
"""

PROVISIONED = "OBICO_ENV_PROVISIONED "


def provision(stack: Stack, *, site: str, webhook_url: str) -> dict[str, Any]:
    """Create the account, register the printer and configure the webhook plugin.

    Every one of these goes through Obico's own models inside the running web
    container, so what is provisioned is what Obico itself would hold.

    Raises:
        StartupError: If Obico refuses any of it, or says nothing this can read.
    """
    note(
        f"provisioning the `{ACCOUNT_EMAIL}` account, the `{PRINTER_NAME}` printer "
        f"and the `{WEBHOOK_PLUGIN}` notification plugin"
    )
    request = json.dumps(
        {
            "site": site,
            "email": ACCOUNT_EMAIL,
            "printer": PRINTER_NAME,
            "filename": PRINT_FILENAME,
            "plugin": WEBHOOK_PLUGIN,
            "webhook_url": webhook_url,
        }
    )
    answered = run_program(
        stack.compose("exec", "-T", "web", "python", "manage.py", "shell"),
        timeout=600,
        stdin=PROVISION_SCRIPT.format(request=request),
    )
    for line in answered.stdout.splitlines():
        if line.startswith(PROVISIONED):
            return dict(json.loads(line[len(PROVISIONED) :]))
    raise StartupError(
        "provisioning-failed",
        f"Obico's `manage.py shell` did not report a provisioned stack:\n{answered.stdout}",
    )


# ~~ the commands


def webhook_address(named: str) -> str:
    """The address the notification plugin is configured to post to.

    `auto` takes a free port on the host and names the alias a container reaches
    the host by; anything else is the caller's own address, refused unless a
    container could actually post to it.

    Raises:
        StartupError: If the named address is not one a container can post to.
    """
    if named == "auto":
        return f"http://{CONTAINER_HOST_ALIAS}:{free_port()}/alert"
    parts = urllib.parse.urlsplit(named)
    if parts.scheme not in {"http", "https"} or not parts.hostname or not parts.port:
        raise StartupError(
            "webhook-address-unusable",
            f"`{named}` is not an absolute http URL carrying a host and a port",
        )
    return named


def up(
    stack: Stack, *, webhook_url: str, timeout: float, announcing: bool = True
) -> dict[str, Any]:
    """Install, start, wait until ready, provision, and answer with the record.

    Raises:
        StartupError: If it does not come up, having stopped what it started.
    """
    if announcing:
        announce(webhook_url)
    ports = install(stack)
    start(stack)
    try:
        wait_for_services(stack, ports, timeout)
        site = f"localhost:{ports['web']}"
        provisioned = provision(stack, site=site, webhook_url=webhook_url)
    except StartupError:
        # A start that failed leaves nothing running: the diagnosis is in the
        # logs, which are files, rather than in containers nobody will stop.
        down(stack)
        raise

    record: dict[str, Any] = {
        "state_dir": str(stack.state_dir),
        "project": stack.project,
        "source": str(stack.source),
        "revision": OBICO_REVISION,
        "compose_file": str(stack.compose_file),
        "override_file": str(stack.override_file),
        "url": f"http://{site}",
        "ports": ports,
        "services": [service.name for service in SERVICES],
        "webhook_url": webhook_url,
        **provisioned,
    }
    stack.record.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    note(f"the stack is up on {record['url']}")
    return record


def hand_back_ownership(stack: Stack) -> bool:
    """Give the state directory back to the user who ran this, before stopping.

    Obico's own composition bind-mounts its `backend` and `frontend` directories
    into containers that run as root, so a run leaves root-owned caches, static
    files and a database file under the state directory that the caller cannot
    delete or even re-clone over. Handing them back is done from inside the
    running container, because that is the only place with the privilege to.

    Done before the containers are stopped, and reported rather than raised on:
    a bring-down that refused to stop a stack because it could not chown a cache
    would be worse than the leftovers.
    """
    handed = run_program(
        stack.compose(
            "exec", "-T", "web", "chown", "-R", f"{os.getuid()}:{os.getgid()}", "/app", "/frontend"
        ),
        timeout=600,
    )
    if handed.returncode != 0:
        note(
            f"could not hand {stack.source} back to uid {os.getuid()}; files the "
            f"containers wrote as root may need `sudo` to remove"
        )
    return handed.returncode == 0


def down(stack: Stack) -> dict[str, Any]:
    """Stop and remove every container this project created, leaving none running."""
    if not stack.compose_file.is_file() or not stack.override_file.is_file():
        note(f"no stack is installed under {stack.state_dir}")
        return {"state_dir": str(stack.state_dir), "project": stack.project, "stopped": False}
    handed_back = hand_back_ownership(stack)
    stopped = run_program(
        stack.compose("down", "--remove-orphans", "--volumes"),
        timeout=1800,
    )
    stack.record.unlink(missing_ok=True)
    note(f"stopped every container of project `{stack.project}`")
    return {
        "state_dir": str(stack.state_dir),
        "project": stack.project,
        "stopped": stopped.returncode == 0,
        "handed_back": handed_back,
        "said": stopped.stdout,
    }


def running(stack: Stack) -> list[str]:
    """The containers of this project that are still running, by name."""
    if not stack.compose_file.is_file() or not stack.override_file.is_file():
        return []
    listed = run_program(
        stack.compose("ps", "--status", "running", "--format", "{{.Name}}"), timeout=300
    )
    return [line.strip() for line in listed.stdout.splitlines() if line.strip()]


def argument_parser() -> argparse.ArgumentParser:
    """The command surface, whose defaults every recipe and caller shares."""
    parser = argparse.ArgumentParser(prog="obico-env", description=__doc__)
    parser.add_argument("command", choices=("install", "up", "down"))
    parser.add_argument(
        "--state-dir",
        default=os.environ.get("OBICO_ENV_STATE_DIR", DEFAULT_STATE_DIR),
        help="the directory this environment lives under (default: %(default)s)",
    )
    parser.add_argument(
        "--project",
        default=os.environ.get("OBICO_ENV_PROJECT", PROJECT),
        help="the compose project its containers belong to (default: %(default)s)",
    )
    parser.add_argument(
        "--webhook-url",
        default=os.environ.get("OBICO_ENV_WEBHOOK_URL", "auto"),
        help=(
            "the address the notification plugin is configured to post to, or `auto` "
            "for a free port on the host (default: %(default)s)"
        ),
    )
    parser.add_argument(
        "--start-timeout",
        type=float,
        default=float(os.environ.get("OBICO_ENV_START_TIMEOUT", "600")),
        help="seconds to wait for each service to answer (default: %(default)s)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    """Run one command, reporting every failure by name and next action."""
    arguments = argument_parser().parse_args(argv)
    try:
        stack = Stack(Path(arguments.state_dir), arguments.project)
        if arguments.command == "down":
            answer = down(stack)
        elif arguments.command == "install":
            answer = {"state_dir": str(stack.state_dir), "ports": install(stack)}
        else:
            answer = up(
                stack,
                webhook_url=webhook_address(arguments.webhook_url),
                timeout=arguments.start_timeout,
            )
    except StartupError as failure:
        print(f"obico-env: failed: {failure.failure_class}", file=sys.stderr)
        print(f"  what happened: {failure.detail}", file=sys.stderr)
        print(f"  next action: {failure.next_action}", file=sys.stderr)
        return 1
    except Exception as error:
        print(
            "obico-env: failed outside the declared failure classes "
            f"({', '.join(FAILURE_CLASSES)}), with the underlying error's own text:",
            file=sys.stderr,
        )
        print(f"  {type(error).__name__}: {error}", file=sys.stderr)
        return 1
    print(json.dumps(answer, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
