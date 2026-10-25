"""The release probe, driven the way `onevcs` drives it, against registries doubled at HTTP.

`release-targets.toml`'s `probe` is spawned by `onevcs` as a direct subprocess
from the repository root, with an environment of `PATH` and `HOME` alone and no
credential. That is exactly how each case here runs the committed script.

The four registries are one real HTTP server on the loopback interface, which
answers each the way that registry answers: crates.io's crate document, PyPI's
project JSON, npm's `latest` document and github.com's unfollowed redirects. The
probe reaches it through the real `curl`, by way of a `curl` earlier on `PATH`
that rewrites each registry's `https://` origin to that server and refuses any
other origin, so no case reaches the network and every status, body and
`Location` the probe reads crossed a real HTTP connection.

Not run on Windows: the probe is a bash script spawned directly, which is a
Unix host's form, and the hosts that wait on this repository's releases run it
there.
"""

from __future__ import annotations

import shutil
import sys
import threading
from collections.abc import Iterator
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from journey import REPO_ROOT
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the probe is a bash script a Unix host spawns directly"
)

#: The committed probe, as the declaration names it.
PROBE = REPO_ROOT / "scripts" / "release-probe.sh"

#: The registries the probe reads, by the origin it reads each at.
ORIGINS = ("crates.io", "pypi.org", "registry.npmjs.org", "github.com")

#: Where the GitHub Release of `release:printobserver` lives.
RELEASES = "/nickderobertis/printobserver/releases"


@dataclass
class Answer:
    """What the doubled registry answers one path with."""

    status: int
    body: str = ""
    location: str = ""


@dataclass
class Registries:
    """The doubled registries: what each path answers, and every path asked."""

    answers: dict[str, Answer] = field(default_factory=dict)
    asked: list[str] = field(default_factory=list)


def _handler(registries: Registries) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def _answer(self, *, with_body: bool) -> None:
            registries.asked.append(f"{self.command} {self.path}")
            answer = registries.answers.get(self.path, Answer(404, '{"errors":"Not Found"}'))
            body = answer.body.encode("utf-8")
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


@dataclass(frozen=True)
class Probe:
    """The committed probe, with the doubled registries in front of it."""

    registries: Registries
    path: str
    home: Path

    def __call__(self, identifier: str) -> tuple[int, str, str]:
        """Run the probe for one identifier: its exit status, stdout and stderr."""
        result = run(
            [str(PROBE), identifier],
            cwd=REPO_ROOT,
            env={"PATH": self.path, "HOME": str(self.home)},
            timeout=90,
        )
        return result.returncode, result.stdout, result.stderr

    def answers(self, path: str, answer: Answer) -> None:
        """Make the doubled registry answer `path` (origin first) with `answer`."""
        self.registries.answers[path] = answer


@pytest.fixture
def probe(tmp_path: Path) -> Iterator[Probe]:
    """The probe, on a PATH whose `curl` reaches the doubled registries and nothing else."""
    registries = Registries()
    server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(registries))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    real = shutil.which("curl")
    truth(real, describing="a curl on this host's PATH for the probe to read registries with")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    port = server.server_address[1]
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
    host_path = [
        entry
        for entry in (Path(shutil.which(tool) or "").parent for tool in ("bash", "grep", "env"))
        if str(entry) != "."
    ]
    path = ":".join(dict.fromkeys([str(bin_dir), *map(str, host_path)]))
    home = tmp_path / "home"
    home.mkdir()
    try:
        yield Probe(registries, path, home)
    finally:
        server.shutdown()
        server.server_close()


def _released(said: tuple[int, str, str], version: str) -> None:
    code, out, err = said
    equal((code, out), (0, f"{version}\n"), describing=f"the probe's answer (stderr: {err!r})")


def _no_release(said: tuple[int, str, str]) -> None:
    code, out, err = said
    equal((code, out), (0, ""), describing=f"the probe's answer (stderr: {err!r})")


def _not_answered(said: tuple[int, str, str], naming: str) -> None:
    code, out, err = said
    truth(code != 0, describing=f"a non-zero exit, not {code} (stdout: {out!r})")
    equal(out, "", describing="stdout of a probe that did not answer")
    contains(err, naming, describing="why the probe did not answer")


# crates.io


def test_a_released_crate_answers_its_max_stable_version(probe: Probe) -> None:
    """`max_stable_version` is the version `cargo add` resolves."""
    probe.answers(
        "/crates.io/api/v1/crates/printobserver",
        Answer(200, '{"crate":{"id":"printobserver","max_stable_version":"0.4.3","x":1}}'),
    )

    _released(probe("crate:printobserver"), "0.4.3")


def test_a_crate_with_no_stable_version_is_no_release(probe: Probe) -> None:
    """crates.io's null `max_stable_version` serves nothing."""
    probe.answers(
        "/crates.io/api/v1/crates/printobserver-types",
        Answer(200, '{"crate":{"id":"printobserver-types","max_stable_version":null}}'),
    )

    _no_release(probe("crate:printobserver-types"))


def test_a_crates_server_error_is_not_answered(probe: Probe) -> None:
    """A 500 says nothing about what is released."""
    probe.answers("/crates.io/api/v1/crates/printobserver", Answer(500, "oops"))

    _not_answered(probe("crate:printobserver"), "crates.io answered 500")


def test_an_unreadable_crate_document_is_not_answered(probe: Probe) -> None:
    """A 200 whose body carries no version is a document the probe cannot read."""
    probe.answers("/crates.io/api/v1/crates/printobserver", Answer(200, "<html>maintenance</html>"))

    _not_answered(probe("crate:printobserver"), "without one readable max_stable_version")


# PyPI


def test_a_released_distribution_answers_its_info_version(probe: Probe) -> None:
    """`info.version` is what `pip install` resolves, and `python_version` is not it."""
    probe.answers(
        "/pypi.org/pypi/printobserver-cli/json",
        Answer(
            200,
            '{"info":{"name":"printobserver-cli","version":"0.4.3"},'
            '"urls":[{"python_version":"py3"}]}',
        ),
    )

    _released(probe("pypi:printobserver-cli"), "0.4.3")


def test_a_distribution_pypi_does_not_know_is_no_release(probe: Probe) -> None:
    """PyPI's 404 is the one answer meaning nothing is published."""
    probe.answers("/pypi.org/pypi/printobserver-sdk/json", Answer(404, '{"message": "Not Found"}'))

    _no_release(probe("pypi:printobserver-sdk"))


def test_a_pypi_server_error_is_not_answered(probe: Probe) -> None:
    """A 503 is a registry that did not say."""
    probe.answers("/pypi.org/pypi/printobserver-cli/json", Answer(503, "busy"))

    _not_answered(probe("pypi:printobserver-cli"), "PyPI answered 503")


def test_an_unreadable_pypi_document_is_not_answered(probe: Probe) -> None:
    """A truncated body names no version, so nothing is established."""
    probe.answers("/pypi.org/pypi/printobserver-cli/json", Answer(200, '{"info":{"name":'))

    _not_answered(probe("pypi:printobserver-cli"), "without one readable info.version")


# npm, scoped


def test_a_released_scoped_package_answers_its_latest_dist_tag(probe: Probe) -> None:
    """A scoped name is asked for as one path segment, `@scope%2fname`."""
    probe.answers(
        "/registry.npmjs.org/@printobserver%2fsdk/latest",
        Answer(200, '{"name":"@printobserver/sdk","version":"0.4.3","_npmVersion":"10.0.0"}'),
    )

    _released(probe("npm:@printobserver/sdk"), "0.4.3")
    contains(
        probe.registries.asked,
        "GET /registry.npmjs.org/@printobserver%2fsdk/latest",
        describing="what the probe asked npm",
    )


def test_a_scoped_package_npm_does_not_know_is_no_release(probe: Probe) -> None:
    """A 404 from npm is no release."""
    probe.answers("/registry.npmjs.org/@printobserver%2fsdk/latest", Answer(404, '"Not Found"'))

    _no_release(probe("npm:@printobserver/sdk"))


def test_an_npm_server_error_is_not_answered(probe: Probe) -> None:
    """A 502 is a registry that did not say."""
    probe.answers("/registry.npmjs.org/@printobserver%2fsdk/latest", Answer(502, "bad gateway"))

    _not_answered(probe("npm:@printobserver/sdk"), "npm answered 502")


def test_an_unreadable_npm_document_is_not_answered(probe: Probe) -> None:
    """Two different versions in one document is not a shape the probe reads."""
    probe.answers(
        "/registry.npmjs.org/printobserver-cli/latest",
        Answer(200, '{"version":"0.4.3","deps":{"version":"9.9.9"}}'),
    )

    _not_answered(probe("npm:printobserver-cli"), "without one readable version")


# The GitHub Release


def _latest(probe: Probe, location: str, status: int = 302) -> None:
    probe.answers(f"/github.com{RELEASES}/latest", Answer(status, "", location))


def test_a_release_carrying_its_checksums_answers_its_tag_without_the_v(probe: Probe) -> None:
    """The redirect names the tag; the checksum file being there makes it installable."""
    _latest(probe, f"https://github.com{RELEASES}/tag/v0.4.3")
    probe.answers(
        f"/github.com{RELEASES}/download/v0.4.3/SHA256SUMS",
        Answer(302, "", "https://release-assets.githubusercontent.com/SHA256SUMS"),
    )

    _released(probe("release:printobserver"), "0.4.3")
    equal(
        probe.registries.asked,
        [
            f"GET /github.com{RELEASES}/latest",
            f"HEAD /github.com{RELEASES}/download/v0.4.3/SHA256SUMS",
        ],
        describing="what the probe asked github.com: two web reads and no REST call",
    )


def test_a_repository_with_no_release_is_no_release(probe: Probe) -> None:
    """github.com sends `releases/latest` to the bare releases page when there is none."""
    _latest(probe, f"https://github.com{RELEASES}")

    _no_release(probe("release:printobserver"))


def test_a_release_whose_checksums_are_not_up_yet_is_not_answered(probe: Probe) -> None:
    """A release without `SHA256SUMS` is one `scripts/install.sh` refuses, so not yet."""
    _latest(probe, f"https://github.com{RELEASES}/tag/v0.4.4")
    probe.answers(f"/github.com{RELEASES}/download/v0.4.4/SHA256SUMS", Answer(404, "Not Found"))

    _not_answered(probe("release:printobserver"), "carries no SHA256SUMS yet")


def test_a_github_server_error_is_not_answered(probe: Probe) -> None:
    """A 500 on `releases/latest` says nothing about a release."""
    probe.answers(f"/github.com{RELEASES}/latest", Answer(500, "unicorn"))

    _not_answered(probe("release:printobserver"), "GitHub answered 500")


def test_a_redirect_to_somewhere_else_is_not_answered(probe: Probe) -> None:
    """A redirect the probe cannot read as a tag or the releases page is not an answer."""
    _latest(probe, "https://github.com/login?return_to=releases")

    _not_answered(probe("release:printobserver"), "neither a release tag nor the releases page")


# The identifier


def test_an_identifier_the_declaration_does_not_declare_is_not_answered(probe: Probe) -> None:
    """Something this repository does not release says nothing about a release."""
    _not_answered(
        probe("crate:printobserver-store-api"), "is not a release target this repository declares"
    )
    equal(probe.registries.asked, [], describing="what the probe asked any registry")
