"""The release probe, driven the way `onevcs` drives it, against registries doubled at HTTP.

`release-targets.toml`'s `probe` is spawned by `onevcs` as a direct subprocess
from the repository root, with an environment of `PATH` and `HOME` alone and no
credential. That is exactly how each case here runs the committed script.

The four registries are one real HTTP server on the loopback interface, which
answers each the way that registry answers: crates.io's crate document, PyPI's
project JSON, npm's `latest` document and github.com's unfollowed redirects. The
probe reaches it through the real `curl`, by way of a `curl` earlier on `PATH`
that rewrites each registry's `https://` origin to that server and refuses any
other origin. `PATH` holds that and the three programs the probe runs, and
nothing else, so no case reaches the network and every status, body and
`Location` the probe reads crossed a real HTTP connection.

Not run on Windows: the probe is a bash script spawned directly, which is a
Unix host's form, and the hosts that wait on this repository's releases run it
there.
"""

from __future__ import annotations

import json
import re
import shutil
import socket
import sys
import sysconfig
import threading
import time
from collections.abc import Iterator
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import NamedTuple

import pytest
from journey import REPO_ROOT
from repo_checks.expect import absent, contains, equal, passing, truth
from repo_checks.shell import run

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the probe is a bash script a Unix host spawns directly"
)

#: Where the declaration says the probe is, relative to the repository root.
PROBE_PATH = "tools/repo-checks/src/repo_checks/release-probe.sh"

#: The committed probe.
PROBE = REPO_ROOT / PROBE_PATH

#: The registries the probe reads, by the origin it reads each at.
ORIGINS = ("crates.io", "pypi.org", "registry.npmjs.org", "github.com")

#: Where the GitHub Release of `release:printobserver` lives.
RELEASES = "/nickderobertis/printobserver/releases"

#: Longer than the probe's own bound on one request, so a stalled answer is
#: one the probe has to give up on.
STALL_SECONDS = 60

#: The programs the probe runs, besides the `curl` each case is handed.
PROGRAMS = ("bash", "env", "python3")


class Said(NamedTuple):
    """What one run of the probe said."""

    code: int
    stdout: str
    stderr: str


@dataclass
class Answer:
    """What the doubled registry answers one path with."""

    status: int
    body: str = ""
    location: str = ""
    #: Bytes sent instead of `body`, for a body that is not UTF-8 at all.
    raw: bytes = b""
    #: Hold the answer until the case ends, as a registry that never answers.
    stall: bool = False


@dataclass
class Registries:
    """The doubled registries: what each path answers, and every path asked."""

    answers: dict[str, Answer] = field(default_factory=dict)
    asked: list[str] = field(default_factory=list)
    #: Set when the case ends, which releases every stalled answer.
    ended: threading.Event = field(default_factory=threading.Event)


def _handler(registries: Registries) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def _answer(self, *, with_body: bool) -> None:
            registries.asked.append(f"{self.command} {self.path}")
            answer = registries.answers.get(self.path, Answer(404, '{"errors":"Not Found"}'))
            if answer.stall:
                registries.ended.wait(STALL_SECONDS)
                return
            body = answer.raw or answer.body.encode("utf-8")
            self.send_response(answer.status)
            if answer.location:
                self.send_header("Location", answer.location)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if with_body:
                self.wfile.write(body)

        def do_GET(self) -> None:
            self._answer(with_body=True)

        def do_HEAD(self) -> None:
            self._answer(with_body=False)

        def log_message(self, format: str, *args: object) -> None:
            del format, args

    return Handler


def _search_path(bin_dir: Path, port: int | None, programs: tuple[str, ...] = PROGRAMS) -> str:
    """A PATH of `programs`, and a `curl` reaching `port` or none at all."""
    bin_dir.mkdir()
    for program in programs:
        found = shutil.which(program)
        truth(found, describing=f"`{program}` on this host, for the probe to run")
        (bin_dir / program).symlink_to(str(found))
    if port is not None:
        real = shutil.which("curl")
        truth(real, describing="a curl on this host for the probe to read registries with")
        doubled = f"http://127.0.0.1:{port}"
        arms = "\n".join(
            f'    https://{origin}/*) arg="{doubled}/{origin}/${{arg#https://{origin}/}}" ;;'
            for origin in ORIGINS
        )
        shim = bin_dir / "curl"
        shim.write_text(
            "#!/usr/bin/env bash\n"
            "# The real curl, with each registry's origin rewritten to the doubled one.\n"
            "args=()\n"
            'for arg in "$@"; do\n'
            '    case "$arg" in\n'
            f"{arms}\n"
            '    https://* | http://*) echo "curl: no registry is doubled at $arg" >&2; exit 7 ;;\n'
            "    esac\n"
            '    args+=("$arg")\n'
            "done\n"
            f'exec {real} "${{args[@]}}"\n',
            encoding="utf-8",
        )
        shim.chmod(0o755)
    return str(bin_dir)


@dataclass(frozen=True)
class Probe:
    """The committed probe, with the doubled registries in front of it."""

    registries: Registries
    path: str
    home: Path

    def __call__(self, *arguments: str, script: Path = PROBE, cwd: Path | None = None) -> Said:
        """Run the probe with `arguments`, from the root of `script`'s tree unless told."""
        result = run(
            [str(script), *arguments],
            cwd=cwd or script.parents[4],
            env={"PATH": self.path, "HOME": str(self.home)},
            timeout=90,
        )
        return Said(result.returncode, result.stdout, result.stderr)

    def answers(self, path: str, answer: Answer) -> None:
        """Make the doubled registry answer `path` (origin first) with `answer`."""
        self.registries.answers[path] = answer


def _home(tmp_path: Path) -> Path:
    home = tmp_path / "home"
    home.mkdir()
    return home


@pytest.fixture
def probe(tmp_path: Path) -> Iterator[Probe]:
    """The probe, on a PATH whose `curl` reaches the doubled registries and nothing else."""
    registries = Registries()
    server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(registries))
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    path = _search_path(tmp_path / "bin", server.server_address[1])
    try:
        yield Probe(registries, path, _home(tmp_path))
    finally:
        registries.ended.set()
        server.shutdown()
        server.server_close()


def _released(said: Said, version: str) -> None:
    equal(
        (said.code, said.stdout),
        (0, f"{version}\n"),
        describing=f"the probe's answer (stderr: {said.stderr!r})",
    )


def _no_release(said: Said) -> None:
    equal((said.code, said.stdout), (0, ""), describing=f"the probe's answer ({said.stderr!r})")


def _not_answered(said: Said, naming: str) -> None:
    truth(said.code != 0, describing=f"a non-zero exit, not {said.code} ({said.stdout!r})")
    equal(said.stdout, "", describing="stdout of a probe that did not answer")
    contains(said.stderr, naming, describing="why the probe did not answer")


def _tree(tmp_path: Path, declaration: str | None) -> Path:
    """The committed probe in a tree of its own, beside `declaration` or none at all."""
    root = tmp_path / "tree"
    script = root / PROBE_PATH
    script.parent.mkdir(parents=True)
    shutil.copy2(PROBE, script)
    if declaration is not None:
        (root / "release-targets.toml").write_text(declaration, encoding="utf-8")
    return script


def test_a_released_crate_answers_its_max_stable_version(probe: Probe) -> None:
    """`max_stable_version` is the version `cargo add` resolves."""
    probe.answers(
        "/crates.io/api/v1/crates/printobserver",
        Answer(200, '{"crate":{"id":"printobserver","max_stable_version":"0.4.3","x":1}}'),
    )

    _released(probe("crate:printobserver"), "0.4.3")


@pytest.mark.parametrize(
    "answer",
    [
        Answer(200, '{"crate":{"id":"printobserver-types","max_stable_version":null}}'),
        Answer(404, '{"errors":[{"detail":"crate `printobserver-types` does not exist"}]}'),
    ],
    ids=["null-max-stable-version", "unknown-crate"],
)
def test_a_crate_serving_nothing_is_no_release(probe: Probe, answer: Answer) -> None:
    """A null `max_stable_version` and a 404 both say crates.io serves nothing."""
    probe.answers("/crates.io/api/v1/crates/printobserver-types", answer)

    _no_release(probe("crate:printobserver-types"))


@pytest.mark.parametrize(
    ("answer", "naming"),
    [
        (Answer(500, "oops"), "crates.io answered 500"),
        (Answer(200, "<html>maintenance</html>"), "without one readable crate.max_stable_version"),
        (Answer(200, '{"crate":{"max_stable_version":4}}'), "without one readable crate."),
        (Answer(200, '{"crate":{"max_stable_version":"1..2"}}'), "not a semantic version"),
    ],
    ids=["server-error", "not-json", "not-a-string", "not-a-version"],
)
def test_a_crate_answer_the_probe_cannot_read_is_not_answered(
    probe: Probe, answer: Answer, naming: str
) -> None:
    """A server error, a body that is not one crate document, or a non-version."""
    probe.answers("/crates.io/api/v1/crates/printobserver", answer)

    _not_answered(probe("crate:printobserver"), naming)


def test_a_released_distribution_answers_its_info_version(probe: Probe) -> None:
    """`info.version` is what `pip install` resolves, and `python_version` is not it."""
    probe.answers(
        "/pypi.org/pypi/printobserver-cli/json",
        Answer(
            200,
            '{"info":{"name":"printobserver-cli","version":"0.4.3"},'
            '"urls":[{"python_version":"py3"}]}\n',
        ),
    )

    _released(probe("pypi:printobserver-cli"), "0.4.3")


def test_a_distribution_pypi_does_not_know_is_no_release(probe: Probe) -> None:
    """PyPI's 404 is the one answer meaning nothing is published."""
    probe.answers("/pypi.org/pypi/printobserver-sdk/json", Answer(404, '{"message": "Not Found"}'))

    _no_release(probe("pypi:printobserver-sdk"))


@pytest.mark.parametrize(
    ("answer", "naming"),
    [
        (Answer(503, "busy"), "PyPI answered 503"),
        (Answer(200, '{"info":{"version":"0.4.3"'), "without one readable info.version"),
        (Answer(200, '{"info":{"version":""}}'), "not a semantic version"),
    ],
    ids=["server-error", "truncated", "empty-version"],
)
def test_a_pypi_answer_the_probe_cannot_read_is_not_answered(
    probe: Probe, answer: Answer, naming: str
) -> None:
    """A truncated body is refused even though the field it carries is whole."""
    probe.answers("/pypi.org/pypi/printobserver-cli/json", answer)

    _not_answered(probe("pypi:printobserver-cli"), naming)


@pytest.mark.parametrize(
    ("identifier", "path", "body"),
    [
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            '{\n  "info": {\n    "name": "printobserver-cli",\n    "version": "0.4.3"\n  }\n}\n',
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            '{ "name" : "@printobserver/sdk" , "version" : "0.4.3" }',
        ),
        (
            "crate:printobserver",
            "/crates.io/api/v1/crates/printobserver",
            '{"crate": {"id": "printobserver", "max_stable_version": "0.4.3"}}',
        ),
    ],
    ids=["pypi", "npm", "crate"],
)
def test_a_whitespace_formatted_document_answers_its_version(
    probe: Probe, identifier: str, path: str, body: str
) -> None:
    """JSON may put whitespace between its tokens, and the version is read through it."""
    probe.answers(path, Answer(200, body))

    _released(probe(identifier), "0.4.3")


@pytest.mark.parametrize(
    ("identifier", "path", "body"),
    [
        (
            "crate:printobserver",
            "/crates.io/api/v1/crates/printobserver",
            '{"crate":{"max_stable_version":"0.4.3"},"x":{"max_stable_version":"9.9.9"}}',
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            '{"version":"0.4.3","deps":{"version":"9.9.9"}}',
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            '{"vulnerabilities":[{"version":"9.9.9"}],"info":{"version":"0.4.3"}}',
        ),
    ],
    ids=["crate", "npm", "pypi"],
)
def test_the_version_is_read_at_the_registrys_own_path_alone(
    probe: Probe, identifier: str, path: str, body: str
) -> None:
    """A `version` elsewhere in the document neither answers for it nor hides it."""
    probe.answers(path, Answer(200, body))

    _released(probe(identifier), "0.4.3")


@pytest.mark.parametrize(
    ("identifier", "path", "body"),
    [
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            '{"info":broken,"version":"9.9.9"}',
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            '{"info":{"name":"printobserver-cli"},"vulnerabilities":[{"version":"9.9.9"}]}',
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            '{"info":{"version":"9.9.9"},"info":{"version":"0.4.3"}}',
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            '{"name":"@printobserver/sdk","dist":{"version":"9.9.9"}}',
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            '{"version":"0.4.3",}',
        ),
        (
            "crate:printobserver",
            "/crates.io/api/v1/crates/printobserver",
            '{"crate":{"id":"printobserver"},"versions":[{"max_stable_version":"9.9.9"}]}',
        ),
        (
            "crate:printobserver",
            "/crates.io/api/v1/crates/printobserver",
            '{"crate":{"max_stable_version":"0.4.3"}} trailing',
        ),
    ],
    ids=[
        "pypi-malformed",
        "pypi-wrong-path",
        "pypi-duplicate-key",
        "npm-wrong-path",
        "npm-trailing-comma",
        "crate-wrong-path",
        "crate-trailing-garbage",
    ],
)
def test_a_malformed_or_wrong_path_document_is_not_answered(
    probe: Probe, identifier: str, path: str, body: str
) -> None:
    """Only a whole, valid document's field at the registry's own path is a version."""
    probe.answers(path, Answer(200, body))

    _not_answered(probe(identifier), "without one readable")


def test_a_released_scoped_package_answers_its_latest_dist_tag(probe: Probe) -> None:
    """A scoped name is asked for as one path segment, `@scope%2fname`."""
    probe.answers(
        "/registry.npmjs.org/@printobserver%2fsdk/latest",
        Answer(200, '{"name":"@printobserver/sdk","version":"0.4.3","_npmVersion":"10.0.0"}'),
    )

    _released(probe("npm:@printobserver/sdk"), "0.4.3")
    equal(
        probe.registries.asked,
        ["GET /registry.npmjs.org/@printobserver%2fsdk/latest"],
        describing="what the probe asked npm",
    )


def test_a_scoped_package_npm_does_not_know_is_no_release(probe: Probe) -> None:
    """A 404 from npm is no release."""
    probe.answers("/registry.npmjs.org/@printobserver%2fsdk/latest", Answer(404, '"Not Found"'))

    _no_release(probe("npm:@printobserver/sdk"))


@pytest.mark.parametrize(
    ("answer", "naming"),
    [
        (Answer(502, "bad gateway"), "npm answered 502"),
        (Answer(200, '{"name":"@printobserver/sdk"}'), "without one readable version"),
        (Answer(200, '{"version":["0.4.3"]}'), "without one readable version"),
    ],
    ids=["server-error", "no-version", "not-a-string"],
)
def test_an_npm_answer_the_probe_cannot_read_is_not_answered(
    probe: Probe, answer: Answer, naming: str
) -> None:
    """A document naming no version, or two, is not the shape the probe reads."""
    probe.answers("/registry.npmjs.org/@printobserver%2fsdk/latest", answer)

    _not_answered(probe("npm:@printobserver/sdk"), naming)


def _latest(probe: Probe, location: str, status: int = 302) -> None:
    probe.answers(f"/github.com{RELEASES}/latest", Answer(status, "", location))


def _checksums(probe: Probe, tag: str, answer: Answer) -> None:
    probe.answers(f"/github.com{RELEASES}/download/{tag}/SHA256SUMS", answer)


@pytest.mark.parametrize("status", [301, 302, 307])
@pytest.mark.parametrize(
    "checksums",
    [Answer(302, "", "https://release-assets.githubusercontent.com/SHA256SUMS"), Answer(200)],
    ids=["asset-redirect", "asset-served"],
)
def test_a_release_carrying_its_checksums_answers_its_tag_without_the_v(
    probe: Probe, status: int, checksums: Answer
) -> None:
    """The redirect names the tag; the checksum file being there makes it installable."""
    _latest(probe, f"https://github.com{RELEASES}/tag/v0.4.3", status)
    _checksums(probe, "v0.4.3", checksums)

    _released(probe("release:printobserver"), "0.4.3")
    equal(
        probe.registries.asked,
        [
            f"GET /github.com{RELEASES}/latest",
            f"HEAD /github.com{RELEASES}/download/v0.4.3/SHA256SUMS",
        ],
        describing="what the probe asked github.com: two web reads and no REST call",
    )


@pytest.mark.parametrize("page", [RELEASES, f"{RELEASES}/"], ids=["bare", "trailing-slash"])
def test_a_repository_with_no_release_is_no_release(probe: Probe, page: str) -> None:
    """github.com sends `releases/latest` to the bare releases page when there is none."""
    _latest(probe, f"https://github.com{page}")

    _no_release(probe("release:printobserver"))


def test_a_release_whose_checksums_are_not_up_yet_is_not_answered(probe: Probe) -> None:
    """A release without `SHA256SUMS` is one `scripts/install.sh` refuses, so not yet."""
    _latest(probe, f"https://github.com{RELEASES}/tag/v0.4.4")
    _checksums(probe, "v0.4.4", Answer(404, "Not Found"))

    _not_answered(probe("release:printobserver"), "carries no SHA256SUMS yet")


@pytest.mark.parametrize(
    ("latest", "checksums", "naming"),
    [
        (Answer(500, "unicorn"), None, "GitHub answered 500"),
        (Answer(404, "Not Found"), None, "GitHub answered 404"),
        (
            Answer(302, "", "https://github.com/login?return_to=releases"),
            None,
            "neither a release tag nor the releases page",
        ),
        (
            Answer(302, "", f"https://github.com{RELEASES}/tag/vnext"),
            None,
            "which is not v<version>",
        ),
        (
            Answer(302, "", f"https://github.com{RELEASES}/tag/v0.4.3"),
            Answer(500, "unicorn"),
            "GitHub's SHA256SUMS download answered 500",
        ),
    ],
    ids=["server-error", "unknown-repository", "elsewhere", "not-a-version-tag", "asset-error"],
)
def test_a_github_answer_the_probe_cannot_read_is_not_answered(
    probe: Probe, latest: Answer, checksums: Answer | None, naming: str
) -> None:
    """A 404 here is an unknown repository rather than no release: that is the redirect."""
    probe.answers(f"/github.com{RELEASES}/latest", latest)
    if checksums is not None:
        _checksums(probe, "v0.4.3", checksums)

    _not_answered(probe("release:printobserver"), naming)


def test_an_identifier_the_declaration_does_not_declare_is_not_answered(probe: Probe) -> None:
    """Something this repository does not release says nothing about a release."""
    _not_answered(
        probe("crate:printobserver-store-api"), "is not a release target this repository declares"
    )
    equal(probe.registries.asked, [], describing="what the probe asked any registry")


def test_a_probe_asked_about_nothing_or_two_things_is_a_usage_error(probe: Probe) -> None:
    """One identifier per run, and the usage error is its own exit status."""
    for arguments in ((), ("crate:printobserver", "pypi:printobserver-cli")):
        said = probe(*arguments)
        equal(said.code, 2, describing=f"the exit status for {len(arguments)} identifiers")
        contains(said.stderr, "takes exactly one", describing="what the probe said")


def test_a_registry_nobody_can_reach_is_not_answered(tmp_path: Path) -> None:
    """A connection refused is curl's failure, which says nothing about a release."""
    with socket.socket() as closed:
        closed.bind(("127.0.0.1", 0))
        port = closed.getsockname()[1]
    unreachable = Probe(Registries(), _search_path(tmp_path / "bin", port), _home(tmp_path))

    _not_answered(unreachable("pypi:printobserver-cli"), "so no registry answered")


def test_a_host_without_curl_is_not_answered(tmp_path: Path) -> None:
    """The probe names the program it is missing rather than failing inside a pipeline."""
    curl_less = Probe(Registries(), _search_path(tmp_path / "bin", None), _home(tmp_path))

    _not_answered(curl_less("pypi:printobserver-cli"), "curl is not on PATH")


def test_a_host_without_python3_is_not_answered(probe: Probe, tmp_path: Path) -> None:
    """The JSON parser is a program of its own, named before any registry is read."""
    python_less = Probe(
        probe.registries,
        _search_path(tmp_path / "python-less", 1, ("bash", "env")),
        probe.home,
    )

    _not_answered(python_less("pypi:printobserver-cli"), "python3 is not on PATH")
    equal(probe.registries.asked, [], describing="what the probe asked any registry")


@pytest.mark.parametrize(
    ("declaration", "naming"),
    [
        (None, "is missing or unreadable"),
        ("schema_version = 3\n", "declares no release target at all"),
        ('[[target]]\nid = "conda:printobserver"\n', "names no registry this probe reads"),
        ('[[target]]\nid = "crate:-printobserver"\n', "is not one a registry serves"),
        ('[[target]]\nid = "npm:@/sdk"\n', "scoped name is not one npm serves"),
        ("[[target]]\nid = 'crate:printobserver'\n", "which is not a quoted string"),
    ],
    ids=["no-declaration", "no-targets", "unknown-registry", "bad-name", "bad-scope", "unquoted"],
)
def test_a_declaration_the_probe_cannot_hold_to_its_identifiers_is_not_answered(
    probe: Probe, tmp_path: Path, declaration: str | None, naming: str
) -> None:
    """The probe reads the declaration beside it in full before it asks any registry."""
    script = _tree(tmp_path, declaration)

    _not_answered(probe("crate:printobserver", script=script), naming)
    equal(probe.registries.asked, [], describing="what the probe asked any registry")


def test_the_probe_recognises_every_target_onevcs_reads_in_the_committed_declaration(
    probe: Probe,
) -> None:
    """The probe's line reading of the file and onevcs's TOML reading list one set of ids.

    Every registry answers 404 here, so a recognised id is no release (or, for
    the forge, a repository nothing answers for) — and never the refusal an id
    the probe does not find in the declaration gets.
    """
    reader = Path(sysconfig.get_path("scripts")) / "onevcs"
    read = run([str(reader), "release", "declaration", str(REPO_ROOT), "--json"], cwd=REPO_ROOT)
    passing(read, describing="onevcs reading the committed declaration")
    listed = [str(target["id"]) for target in json.loads(read.stdout)["target"]]
    truth(listed, describing="the targets onevcs reads in the committed declaration")
    for identifier in listed:
        said = probe(identifier)
        absent(said.stderr, "is not a release target", describing=f"the probe on {identifier}")
        if not identifier.startswith("release:"):
            _no_release(said)


def _assigned(script: str, name: str) -> str:
    """The value `script` assigns `name`, as `name="value"` or `readonly name="value"`."""
    text = (REPO_ROOT / script).read_text(encoding="utf-8")
    found = re.findall(rf'^(?:readonly )?{name}="([^"]*)"$', text, flags=re.MULTILINE)
    equal(len(found), 1, describing=f"how many times {script} assigns {name}")
    return found[0]


def test_the_probe_reads_the_release_the_install_script_installs_from() -> None:
    """The owner and checksum file are the install script's, so the two cannot drift."""
    equal(
        _assigned(PROBE_PATH, "GITHUB_OWNER"),
        _assigned("scripts/install.sh", "OWNER"),
        describing="the forge account the probe and the install script read",
    )
    equal(
        _assigned(PROBE_PATH, "CHECKSUMS"),
        _assigned("scripts/install.sh", "CHECKSUMS"),
        describing="the checksum file the probe waits for and the install script verifies",
    )
    contains(
        (REPO_ROOT / "release-targets.toml").read_text(encoding="utf-8"),
        f'id = "release:{_assigned("scripts/install.sh", "REPOSITORY")}"\n',
        describing="the release target, named for the repository the install script reads",
    )


@pytest.mark.parametrize(
    ("identifier", "path", "answer", "naming"),
    [
        (
            "crate:printobserver",
            "/crates.io/api/v1/crates/printobserver",
            Answer(200, '{"crate":{"max_stable_version":"null"}}'),
            "not a semantic version",
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            Answer(200, '{"info":{"version":null}}'),
            "without one readable info.version",
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            Answer(200, '{"info":{"version":"0.4.3","size":NaN}}'),
            "without one readable info.version",
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            Answer(200, raw=b'{"info":{"version":"0.4.3","name":"\xff"}}'),
            "without one readable info.version",
        ),
        (
            "pypi:printobserver-cli",
            "/pypi.org/pypi/printobserver-cli/json",
            Answer(200, '{"info":{"version":"0.4.3"},"x":' + "[" * 100_000 + "]" * 100_000 + "}"),
            "without one readable info.version",
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            Answer(200, '{"version":"0.4.3\\n9.9.9"}'),
            "without one readable version",
        ),
        (
            "npm:@printobserver/sdk",
            "/registry.npmjs.org/@printobserver%2fsdk/latest",
            Answer(200, '{"version":"1.2.3-01"}'),
            "not a semantic version",
        ),
    ],
    ids=[
        "crate-string-null",
        "pypi-null-version",
        "non-finite-constant",
        "not-utf-8",
        "nested-past-the-parser",
        "multi-line-string",
        "leading-zero-prerelease",
    ],
)
def test_a_body_strict_json_or_semver_refuses_is_not_answered(
    probe: Probe, identifier: str, path: str, answer: Answer, naming: str
) -> None:
    """The string "null" is not JSON's null, and what strict JSON refuses answers nothing."""
    probe.answers(path, answer)

    _not_answered(probe(identifier), naming)


def test_a_prerelease_with_build_metadata_is_a_version(probe: Probe) -> None:
    """A full semantic version, prerelease and build metadata included, is answered whole."""
    probe.answers(
        "/registry.npmjs.org/printobserver-cli/latest",
        Answer(200, '{"version":"0.5.0-rc.1+build.7"}'),
    )

    _released(probe("npm:printobserver-cli"), "0.5.0-rc.1+build.7")


def test_the_probe_answers_about_its_own_repository_from_any_directory(
    probe: Probe, tmp_path: Path
) -> None:
    """It finds the declaration from its own path, not from where it was started."""
    probe.answers(
        "/pypi.org/pypi/printobserver-cli/json", Answer(200, '{"info":{"version":"0.4.3"}}')
    )
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()

    _released(probe("pypi:printobserver-cli", cwd=elsewhere), "0.4.3")


def test_the_callers_curl_configuration_is_not_read(probe: Probe) -> None:
    """A `~/.curlrc` sending every request to a dead proxy changes nothing the probe asks."""
    (probe.home / ".curlrc").write_text('proxy = "http://127.0.0.1:1"\n', encoding="utf-8")
    probe.answers(
        "/pypi.org/pypi/printobserver-cli/json", Answer(200, '{"info":{"version":"0.4.3"}}')
    )

    _released(probe("pypi:printobserver-cli"), "0.4.3")


def test_a_registry_that_never_answers_is_given_up_on_within_the_bound(probe: Probe) -> None:
    """One request, bounded by curl's own `--max-time`, well inside the contract's sixty."""
    probe.answers("/pypi.org/pypi/printobserver-cli/json", Answer(200, stall=True))

    started = time.monotonic()
    said = probe("pypi:printobserver-cli")
    took = time.monotonic() - started

    _not_answered(said, "curl exited 28")
    truth(took < STALL_SECONDS - 5, describing=f"a probe that gave up in {took:.0f}s")
    equal(
        probe.registries.asked,
        ["GET /pypi.org/pypi/printobserver-cli/json"],
        describing="what the probe asked: once, and no retry",
    )
