# /// script
# requires-python = ">=3.12"
# dependencies = ["jsonschema>=4.23"]
# ///
# llmlint: ignore-file[new_code_lands_in_a_project] the export adds only this file, here
# llmlint: ignore-file[changed_behavior_has_e2e] no test file may be added beside it
# The export contract allows one added file, inside the exported directory, with
# nothing changed outside it: no project file or test module can join it. So
# `--self-test` proves every detector form in this file, and the whole check is
# proven end to end by its run over the real exported directory.
"""Check this directory against the recorded cases it was prepared from.

Run from the root of the repository the cases were recorded in:

    uv run real-print-cases/verify-export.py --base <commit the cases came from> \
        --printobserver <printobserver clone at 0f1c113e, outside this tree> \
        --model-repo <clone of the model repository> --model-commit <its commit>

This file is published with the cases, so it names nothing private itself:
every private name it searches for is read at run time, from the repository's
origin, the base commit's texts and tree, and the model repository.

It first proves its absolute-path search on every form of path it must find
(`PATH_FORMS`: rooted, doubled-slash, UNC, device, drive-letter, home and
file-URL paths, ASCII or not) and every text it must leave (`NOT_PATHS`), and
its search for LAN addresses, URL credentials and bearer credentials, plain,
inside base64 or URL-encoded, with the re-encoding that takes them out
(`SECRET_FORMS`, `NOT_SECRETS`);
`--self-test` runs only that, with no other argument. It then compares this
directory with the base commit's copy file by file and fails unless all of
these hold:

- every file of the base outside the dropped case is here under its own name,
  nothing of the dropped case is, and this file is the one added;
- every file but the case texts (each `case.json`, `provenance.json`,
  `printobserver-history.json`, `OUTCOME.md` and the README) is byte-identical:
  frames, agent images, G-code, models, `obico-predictions.json` and the rest;
- each case text is exactly its base copy with the substitutions of `rules`
  applied, each of which replaces or removes a private term (or a mention of
  the dropped case); the README may add lines besides; every substitution is
  listed with its line and kind;
- a JSON file changes only inside strings that held a private term, or drops a
  field that held one (never in a history);
- no file, this one, the G-code and the inside of every 3MF included, holds any
  term of the lexicon, and every occurrence the base held was replaced or
  removed. Names are searched in every byte; an absolute path, POSIX or
  Windows, in all of a file's text (`text_segments`), which leaves out only
  compressed pixel and zip data;
- every `case.json` satisfies printobserver's `case.schema.json`, every
  history its `HistoryAnswer.json`, every agent image's sha256 matches its
  event, every file a case names exists and every file of a case is named;
- the README lists the cases and states this directory's sizes.

It exits 0 when every check passes, 1 when one finds something (each finding
on standard error, the report on standard output) or an input it reads is not
what it must be, and 2 for a command line it refuses.

Each fixed pattern below is written so that its own source text does not match
it (`AppDat[a]` rather than the plain word), so this file can search itself.
"""

from __future__ import annotations

import argparse
import ast
import base64
import binascii
import builtins
import difflib
import hashlib
import json
import re
import subprocess
import sys
import tomllib
import urllib.parse
import zipfile
from collections import Counter
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from types import UnionType
from typing import (
    Any,
    NamedTuple,
    NewType,
    TypedDict,
    Union,
    cast,
    get_args,
    get_origin,
    get_type_hints,
    is_typeddict,
)

from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError

DIRECTORY = "real-print-cases"
DROPPED = "jar-bottom-petg-healthy"
PRINTOBSERVER_COMMIT = "0f1c113e"
CASE_SCHEMA = "tests/real-prints/case.schema.json"
HISTORY_SCHEMA = "schemas/printobserver-server/HistoryAnswer.json"
PRINTER_STATE = "schemas/printobserver-types/PrinterState.json"
OPERATIONS = "schemas/printobserver-server/operations.json"
HISTORY = "printobserver-history.json"
SCRUBBED = ("case.json", "provenance.json", HISTORY, "OUTCOME.md")
README = "README.md"
SELF = "verify-export.py"
MB = 1_000_000
# Exit statuses: every check passed; a check found something, or an input it
# reads (a clone, a schema, a source) is not what it must be; argparse's own
# status for a command line it refuses.
EXIT_PASSED = 0
EXIT_FINDINGS = 1
EXIT_USAGE = 2

CommitId = NewType("CommitId", str)
EventId = NewType("EventId", str)
ScenarioId = NewType("ScenarioId", str)


class Kind(StrEnum):
    """The kinds of private term, as the output lists them."""

    TEMPORARY = "temporary path"
    ABSOLUTE = "absolute path"
    RUN_FOLDER = "run-folder path"
    REPOSITORY = "repository name"
    COMMAND = "repository command line"
    DOCUMENT = "repository document path"
    USER = "user name"
    CONFIG = "configuration file path"
    DROPPED_MENTION = "mention of jar-bottom-petg-healthy"
    MODEL = "model repository name, project or commit"
    COMMIT = "repository commit"
    LAN = "LAN address"
    CREDENTIAL = "access credential in a URL"
    BEARER = "bearer credential"
    ENCODED = "LAN address and access token in an encoded payload"


def _git(*args: str, cwd: Path | None = None) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout


def _revision(value: str) -> str:
    """A revision given on the command line, refused when git would read it as an option."""
    if not value or value.startswith("-"):
        msg = f"{value!r} is not a revision: it must not be empty or start with '-'"
        raise argparse.ArgumentTypeError(msg)
    return value


def _resolve_commit(revision: str, cwd: Path | None = None) -> CommitId:
    """One commit's full identifier, read as a revision and never as an option."""
    resolved = _git(
        "rev-parse", "--verify", "--end-of-options", f"{revision}^{{commit}}", cwd=cwd
    ).strip()
    return CommitId(resolved)


def _show(base: str, path: str) -> str:
    """A file of the base commit, decoded with its line endings as they are."""
    shown = subprocess.run(["git", "show", f"{base}:{path}"], check=True, capture_output=True)
    return shown.stdout.decode("utf-8")


def _origin_name(cwd: Path | None = None) -> str:
    """A repository's name, as its origin's URL ends."""
    url = _git("remote", "get-url", "origin", cwd=cwd).strip()
    return url.rstrip("/").rsplit("/", 1)[-1].removesuffix(".git")


def _commits(tip: str, cwd: Path | None = None) -> frozenset[CommitId]:
    """Every commit reachable from a tip, by its full identifier."""
    return frozenset(CommitId(c) for c in _git("rev-list", tip, cwd=cwd).split())


def _one(found: set[str], what: str) -> str:
    if len(found) != 1:
        msg = f"expected one {what} in the base, found {sorted(found)}"
        raise SystemExit(msg)
    return found.pop()


@dataclass(frozen=True, slots=True)
class Private:
    """Every private name, read at run time rather than written here."""

    repository: str
    packages: frozenset[str]
    flags: frozenset[str]
    subcommands: frozenset[str]
    documents: frozenset[str]
    runs: str
    users: frozenset[str]
    temporary_ids: frozenset[str]
    config_files: frozenset[str]
    model: str
    projects: frozenset[str]
    model_commits: frozenset[CommitId]
    own_commits: frozenset[CommitId]

    @classmethod
    def read(
        cls,
        base: str,
        texts: dict[str, str],
        printobserver: Path,
        model_repo: Path,
        model_commit: str,
    ) -> Private:
        repository = _origin_name()
        tree = _git("ls-tree", "-r", "--name-only", base).splitlines()
        packages = {repository}
        for path in tree:
            if path.endswith("pyproject.toml"):
                match tomllib.loads(_show(base, path)):
                    case {"project": {"name": str(name)}}:
                        packages |= {name, name.replace("-", "_")}
                    case _:
                        # A workspace root names no project of its own.
                        continue
        cli = _show(base, f"packages/{repository}/{repository}/cli.py")
        # A flag of this repository's CLI that printobserver's source never
        # spells is this repository's own.
        public_flags = set(_git("grep", "-hoE", "--", "--[a-z][a-z-]+", cwd=printobserver).split())
        flags = set(re.findall(r'"(--[a-z][a-z-]+)"', cli)) - public_flags
        subcommands = set(re.findall(r'add_parser\(\s*"([a-z-]+)"', cli))
        documents = {p for p in tree if re.fullmatch(r"(?:docs/[\w-]+\.md|setup/[\w-]+\.ps1)", p)}
        joined = "\n".join(texts.values())
        sep = r"[\\/]+"
        runs = _one(
            set(re.findall(rf"{sep}{re.escape(repository)}{sep}([A-Za-z]+){sep}\d{{4}}-", joined)),
            "run folder",
        )
        users = set(re.findall(r"User[s]\\+([A-Za-z0-9._-]+)", joined))
        users |= set(re.findall(r"/(?:hom[e]|User[s])/([A-Za-z0-9._-]+)", joined))
        users |= set(re.findall(r"C--User[s]-([A-Za-z0-9]+)-", joined))
        users |= set(re.findall(r"github\.com/([A-Za-z0-9-]+)/", joined))
        users |= set(re.findall(r"\b([A-Za-z0-9-]+)/printobserver#", joined))
        temporary_ids = set(re.findall(r"C--User[s]-[^\\]+\\+([0-9a-f-]{36})", joined))
        config_files = set(re.findall(r"[\w-]+\.(?:in[i]|tom[l])\b", joined)) - {"pyproject.toml"}
        projects = {
            Path(p).name
            for p in _git(
                "ls-tree", "--name-only", model_commit, "packages/projects/", cwd=model_repo
            ).split()
        }
        return cls(
            repository=repository,
            packages=frozenset(packages),
            flags=frozenset(flags),
            subcommands=frozenset(subcommands),
            documents=frozenset(documents),
            runs=runs,
            users=frozenset(users),
            temporary_ids=frozenset(temporary_ids),
            config_files=frozenset(config_files),
            model=_origin_name(model_repo),
            projects=frozenset(projects),
            model_commits=_commits(model_commit, model_repo),
            own_commits=_commits(base),
        )


# A LAN address (a private IPv4 range, or a link-local one) and a URL query
# parameter that carries an access credential: neither needs a private name to
# be found, and an Obico event's image URL holds both.
LAN_ADDRESS = (
    rb"(?<![\d.])(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01])|169\.254)"
    rb"\.\d{1,3}\.\d{1,3}(?![\d.])"
)
ACCESS_TOKEN = rb"[?&](?:digest|token|access_token|api_key|key|signature|sig)="
# A bearer credential, as an HTTP Authorization header or a client's config
# spells one: the scheme, then a token of sixteen characters or more holding a
# digit, which the scheme's name in prose ("a bearer credential") never is.
_TOKEN = rb"[A-Za-z0-9._~+/-]"
BEARER = rb"(?i)\bbearer[\s:=\"']+(?=%s{16})%s*\d%s*=*" % (_TOKEN, _TOKEN, _TOKEN)
# A URL-encoded run: characters a URL carries, holding one percent escape or more.
URL_ENCODED = re.compile(
    rb"(?:[A-Za-z0-9._~!$&'()*+,;=:@/?-]|%[0-9A-Fa-f]{2})*%[0-9A-Fa-f]{2}"
    rb"(?:[A-Za-z0-9._~!$&'()*+,;=:@/?-]|%[0-9A-Fa-f]{2})*"
)
# A base64 string long enough to hold a payload, padded or not.
BASE64 = rb"[A-Za-z0-9+/]{24,}={0,2}"
BASE64_TOKEN = re.compile(rb"(?<![A-Za-z0-9+/=])" + BASE64 + rb"(?![A-Za-z0-9+/=])")


def _decoded(encoded: bytes) -> bytes | None:
    """A base64 string's bytes when they are printable UTF-8 text, else None."""
    if len(encoded) % 4:
        return None
    try:
        decoded = base64.b64decode(encoded, validate=True)
        text = decoded.decode("utf-8")
    except binascii.Error, UnicodeDecodeError:
        return None
    return decoded if "".join(text.split()).isprintable() else None


def _unquoted(encoded: bytes) -> bytes | None:
    """A URL-encoded run's bytes when its escapes decode to UTF-8 text, else None."""
    try:
        text = urllib.parse.unquote_to_bytes(encoded).decode("utf-8")
    except UnicodeDecodeError:
        return None
    return text.encode() if text.encode() != encoded else None


@dataclass(frozen=True, slots=True)
class Rule:
    """One substitution: what it matches, what replaces it, and which kind of term it removes."""

    kind: Kind
    pattern: str
    replacement: str | Callable[[re.Match[str]], str]


def rules(p: Private) -> tuple[Rule, ...]:
    """Every substitution, applied in one pass over each base text.

    The earliest match wins, and at one position the first rule listed, so the
    specific rules come first.
    """
    repo = re.escape(p.repository)
    runs = re.escape(p.runs)
    users = "|".join(re.escape(u) for u in sorted(p.users))
    drive = r"[A-Za-z]:\\\\User[s]\\\\"
    return (
        Rule(Kind.DROPPED_MENTION, rf"^\| `{DROPPED}` \|[^\n]*\n", ""),
        Rule(Kind.TEMPORARY, rf'^ *"path": "{drive}[^"]*",\r?\n', ""),
        Rule(
            Kind.CONFIG,
            rf'"{drive}[^"]*\\\\[\w-]+\.in[i]"',
            '"the operator\'s build-plate-only supports override"',
        ),
        Rule(
            Kind.ABSOLUTE,
            rf'"{drive}[^"]*?\\\\preflight\\\\([\w.-]+)"',
            '"\\1 in the run folder\'s preflight folder"',
        ),
        Rule(
            Kind.RUN_FOLDER,
            rf'"{runs}\\\\[^"]*?\\\\preflight\\\\([\w.-]+)"',
            '"\\1 in the run folder\'s preflight folder"',
        ),
        Rule(
            Kind.USER,
            r'"https://gist\.github\.com/[\w-]+/[0-9a-f]+"',
            '"the operator\'s own design, published as a gist"',
        ),
        Rule(Kind.USER, r"at https://gist\.github\.com/[\w-]+/[0-9a-f]+", "in the operator's gist"),
        Rule(
            Kind.RUN_FOLDER,
            rf"from `{runs}/[\w-]+` \(taken for (?:{users})/(printobserver#\d+)\)",
            r"from the evidence folder taken for \1",
        ),
        Rule(Kind.USER, rf"\b(?:{users})/(?=printobserver#)", ""),
        Rule(
            Kind.RUN_FOLDER,
            rf"{repo}' own camera frames \(`{runs}/[^`]+\/frames`\)",
            "the operator's own camera frames (the run folder's frames)",
        ),
        Rule(
            Kind.RUN_FOLDER,
            rf"{repo}' preflight bed frames, {runs}/<run>/preflight/(bed-<date>-<time>\.jpg)"
            rf" in the {repo} repository",
            "the operator's preflight bed frames, \\1 in each run folder's preflight folder",
        ),
        Rule(
            Kind.RUN_FOLDER,
            rf"{runs}/(\d{{4}}-\d\d-\d\d)-([^/]+)/finished\.jpg",
            r"the finished frame of the \1 \2 run",
        ),
        Rule(Kind.RUN_FOLDER, rf"{runs}/(\d{{4}}-\d\d-\d\d)-([\w-]+)", r"the \1 run of \2"),
        Rule(
            Kind.REPOSITORY,
            rf"copied unchanged from the {repo} run folder",
            "copied from the run folder",
        ),
        Rule(Kind.REPOSITORY, rf"from the {repo} run folder", "from the run folder"),
        Rule(Kind.REPOSITORY, rf'"{repo}_frame"', '"operator_frame"'),
        Rule(Kind.COMMAND, rf"\b{repo} start\b", "the operator's start command"),
        Rule(Kind.COMMAND, r"`p[o] obico (\w+)`", r"the operator's Obico \1 command"),
        Rule(
            Kind.COMMAND,
            r"`control resume --obico-ac[k] NOT_FAILED`",
            "the operator's resume command, acknowledging the Obico alert as NOT_FAILED",
        ),
        Rule(Kind.DOCUMENT, r"doc[s]/printer\.md", "the operator's printer notes"),
        Rule(Kind.REPOSITORY, rf"\b{repo} saved\b", "the operator's print tool saved"),
        Rule(Kind.REPOSITORY, rf"(?<=\. ){repo}' ", "The operator's "),
        Rule(Kind.REPOSITORY, rf"\b{repo}' ", "the operator's "),
        Rule(Kind.REPOSITORY, rf"\b{repo}\b", "the operator's print tool"),
        Rule(Kind.ENCODED, rf'(?<="raw": "){BASE64.decode()}(?=")', _reencoded),
    )


# The neutral host an Obico image URL names in place of the operator's server.
NEUTRAL_HOST = "obico-server.invalid"
_URL_HOST = re.compile(rf"(https?://){LAN_ADDRESS.decode()}(?::\d+)?")
_URL_TOKEN = re.compile(ACCESS_TOKEN.decode() + r"[^\s\"&]*")


def _reencoded(match: re.Match[str]) -> str:
    """A base64 payload with any LAN host and URL access token taken out of it.

    Every other byte of the decoded payload stays as it was, and the result is
    encoded as the original was; a payload that holds neither comes back as is.
    """
    encoded = match.group(0)
    decoded = _decoded(encoded.encode())
    if decoded is None:
        return encoded
    text = decoded.decode("utf-8")
    scrubbed = _URL_TOKEN.sub("", _URL_HOST.sub(rf"\g<1>{NEUTRAL_HOST}", text))
    if scrubbed == text:
        return encoded
    return base64.b64encode(scrubbed.encode("utf-8")).decode("ascii")


@dataclass(frozen=True, slots=True)
class Change:
    """One substitution made in one base text."""

    path: str
    line: int
    kind: Kind
    before: str
    after: str


class Occurrence(NamedTuple):
    """One private term found in some bytes: its kind, its byte span and its text."""

    kind: Kind
    start: int
    end: int
    text: str


class Segment(NamedTuple):
    """A run of a file's bytes that can hold readable text, at its offset in the file."""

    offset: int
    data: bytes


class Searched(NamedTuple):
    """A file's bytes, or a 3MF member's, with the archive part names it may hold."""

    name: str
    data: bytes
    parts: frozenset[str]


class Span(NamedTuple):
    """Where one change sits in its base text, by character."""

    start: int
    end: int
    change: Change


class Scrubber:
    """The rules, compiled once into one pattern."""

    def __init__(self, private: Private) -> None:
        self.rules = rules(private)
        # Lines are anchored with `^`, so every pattern is multi-line.
        joined = "|".join(f"(?P<r{i}>{rule.pattern})" for i, rule in enumerate(self.rules))
        self.combined = re.compile(joined, re.MULTILINE)
        self.compiled = tuple(re.compile(rule.pattern, re.MULTILINE) for rule in self.rules)

    def scrub(self, path: str, text: str) -> tuple[str, list[Change]]:
        """A base text with every rule applied, and each change it made."""
        changes: list[Change] = []

        def replace(match: re.Match[str]) -> str:
            index = next(
                int(name[1:]) for name, value in match.groupdict().items() if value is not None
            )
            before = match.group(0)
            # The rule's own match at the same place, so its groups and
            # lookarounds read the whole text, as they did combined.
            own = self.compiled[index].match(text, match.start())
            if own is None or own.group(0) != before:
                msg = f"rule {index} does not match alone where it matched combined"
                raise AssertionError(msg)
            replacement = self.rules[index].replacement
            after = replacement(own) if callable(replacement) else own.expand(replacement)
            if after != before:
                line = text.count("\n", 0, match.start()) + 1
                changes.append(Change(path, line, self.rules[index].kind, before, after))
            return after

        return self.combined.sub(replace, text), changes


@dataclass(frozen=True, slots=True)
class Term:
    """One private term: the bytes pattern that finds it and its kind."""

    kind: Kind
    pattern: re.Pattern[bytes]


SECRET_TERMS = (
    Term(Kind.LAN, re.compile(LAN_ADDRESS)),
    Term(Kind.CREDENTIAL, re.compile(ACCESS_TOKEN)),
    Term(Kind.BEARER, re.compile(BEARER)),
)


def _word(word: str) -> re.Pattern[bytes]:
    """A word, matched case-insensitively wherever it is not part of a longer name."""
    escaped = re.escape(word.encode())
    return re.compile(rb"(?<![A-Za-z0-9])" + escaped + rb"(?![A-Za-z0-9])", re.IGNORECASE)


def _exact(word: str) -> re.Pattern[bytes]:
    return re.compile(re.escape(word.encode()))


class Lexicon:
    """Every private term searched for.

    Beside the terms it is given it always searches for an absolute path, a Kind.LAN
    address and an access token in a URL, which need no private name to find.
    """

    HEX = re.compile(rb"(?<![0-9A-Za-z])[0-9a-f]{7,40}(?![0-9A-Za-z])")

    def __init__(
        self, terms: list[Term], commits: tuple[tuple[Kind, frozenset[CommitId]], ...]
    ) -> None:
        self.terms = [*terms, *SECRET_TERMS]
        self.commits = commits

    @classmethod
    def of(cls, p: Private) -> Lexicon:
        """The terms of the repository, its user, its tools and the model repository."""
        model = re.escape(p.model).replace(r"\-", "[-_ ]?").encode()
        terms = [
            Term(Kind.REPOSITORY, re.compile(re.escape(p.repository.encode()), re.IGNORECASE)),
            *(Term(Kind.REPOSITORY, _word(name)) for name in sorted(p.packages)),
            Term(Kind.COMMAND, re.compile(rb"`p[o] [a-z]")),
            Term(
                Kind.COMMAND,
                re.compile(rb"\bp[o] (?:" + "|".join(sorted(p.subcommands)).encode() + rb")\b"),
            ),
            *(
                Term(Kind.COMMAND, re.compile(re.escape(f.encode()) + rb"(?![a-z-])"))
                for f in sorted(p.flags)
            ),
            *(Term(Kind.DOCUMENT, _exact(doc)) for doc in sorted(p.documents)),
            Term(Kind.RUN_FOLDER, re.compile(re.escape(p.runs.encode()) + rb"[/\\]")),
            Term(
                Kind.TEMPORARY,
                re.compile(rb"AppDat[a]|scratchpa[d]|C--User[s]|Temp\\+claude", re.IGNORECASE),
            ),
            *(Term(Kind.TEMPORARY, _exact(i)) for i in sorted(p.temporary_ids)),
            Term(
                Kind.CONFIG,
                re.compile(
                    rb"client[_-]confi[g]|--confi[g]\b|[/\\][\w.-]+\.(?:toml|ini|cfg|conf|ya?ml)(?![A-Za-z0-9])"
                ),
            ),
            *(Term(Kind.CONFIG, _exact(name)) for name in sorted(p.config_files)),
            *(Term(Kind.USER, _word(user)) for user in sorted(p.users)),
            Term(Kind.USER, re.compile(rb"gist\.github\.com")),
            Term(Kind.MODEL, re.compile(model, re.IGNORECASE)),
            *(Term(Kind.MODEL, _word(project)) for project in sorted(p.projects)),
        ]
        return cls(terms, ((Kind.MODEL, p.model_commits), (Kind.COMMIT, p.own_commits)))

    def find(self, data: bytes, segments: list[Segment] | None = None) -> Iterator[Occurrence]:
        """Every private term in some bytes: its kind, span and text.

        Names are searched in every byte. An absolute path, and every term
        again inside each base64 string that decodes to text and each
        URL-encoded run, is searched in the text `segments` of the bytes, which
        are all of them unless given; a term found encoded is reported at the
        encoded string's span.
        """
        for term in self.terms:
            for match in term.pattern.finditer(data):
                yield Occurrence(term.kind, *match.span(), match.group(0).decode(errors="replace"))
        for offset, segment in [Segment(0, data)] if segments is None else segments:
            for match in ABSOLUTE_PATH.finditer(segment):
                start, end = offset + match.start(), offset + match.end()
                yield Occurrence(Kind.ABSOLUTE, start, end, match.group(0).decode(errors="replace"))
            yield from self._encoded(Segment(offset, segment))
        for match in self.HEX.finditer(data):
            token = match.group(0).decode()
            for kind, commits in self.commits:
                if any(commit.startswith(token) for commit in commits):
                    yield Occurrence(kind, *match.span(), token)

    def _encoded(self, segment: Segment) -> Iterator[Occurrence]:
        """Every term inside each base64 string and URL-encoded run of a segment.

        Each is reported at the encoded string's span, and decoded text is
        searched again, so a term encoded twice over is found as well.
        """
        for pattern, decode, how in (
            (BASE64_TOKEN, _decoded, "base64-encoded"),
            (URL_ENCODED, _unquoted, "URL-encoded"),
        ):
            for match in pattern.finditer(segment.data):
                decoded = decode(match.group(0))
                if decoded is None:
                    continue
                start, end = segment.offset + match.start(), segment.offset + match.end()
                for found in self.find(decoded):
                    yield Occurrence(found.kind, start, end, f"{found.text} ({how})")

    def holds(self, text: str) -> bool:
        return next(self.find(text.encode()), None) is not None


# An absolute filesystem path, POSIX or Windows, where it starts a token. A
# path's segment may hold any non-ASCII character: `_HIGH` is the range of
# bytes from 0x80 to 0xFF that UTF-8 spells one with, written as those bytes so
# that this file's own text holds no backslash-led sequence the search finds.
# A slash after a word character, a dot, a colon or another slash belongs to a
# relative path or a URL (`../x`, `a/b`, `https://host/x`), and one after `}`,
# `>`, `)`, `]` or `%` follows a placeholder, an expression or a unit
# (`{dir}/x`, `<run>/x`, `%/s`); one after `<` closes a markup tag, and one
# after `+` is base64 (`+/w`).
_HIGH = bytes([0x80]) + b"-" + bytes([0xFF])
_SEGMENT = rb"[\w.~$%s-]" % _HIGH
ABSOLUTE_PATH = re.compile(
    (
        # A rooted POSIX path of one component or more, a trailing slash or
        # not, or a network path that two slashes start, server then share.
        rb"(?<![\w.~/\\:<}>)\]%%+%(high)s-])/{1,2}%(seg)s*[\w~$%(high)s-](?:/%(seg)s*)*"
        # A path in a home directory, the user's own or a named one, where `~`
        # starts a token (not `=~/regex/`).
        rb"|(?<![^\s\"'`(\[,;])~(?:[A-Za-z_][\w.-]*)?/(?:%(seg)s|/)*"
        # A drive letter, a colon and a separator, either slash, escaped or
        # not: the drive's root, or any path under it.
        rb"|(?<![A-Za-z0-9])[A-Za-z]:(?:\\+|/)(?:%(seg)s|[\\/])*"
        # A UNC or device path: two backslashes or more, a server (or `?` or
        # `.`), a separator and a share (or a drive), escaped or not.
        rb"|(?<![\w\\])\\{2,}[\w.$?%(high)s-]+\\+[\w.$:%(high)s-](?:%(seg)s|[:\\])*"
        # A path rooted at the current drive: a backslash and a folder of two
        # characters or more, then a separator and a name, not after a word or
        # another backslash, so that an escape such as a newline is not one.
        rb"|(?<![\w\\.:$%(high)s])\\[A-Za-z_$%(high)s][\w.$%(high)s-]+\\+[\w.$%(high)s-]"
        rb"(?:%(seg)s|\\)*"
        # A file URL names a local path after one slash or three, a drive
        # letter or not, so the whole URL is taken as the path.
        rb"|\bfile:/+(?:%(seg)s|[/:])*"
    )
    % {b"high": _HIGH, b"seg": _SEGMENT}
)


# Each form of absolute path the detector must find, and text it must leave
# alone, written with `{s}` for a slash and `{b}` for a backslash so that this
# file's own text holds none of them.
PATH_FORMS: tuple[tuple[str, str], ...] = (
    ("a rooted POSIX path", "{s}srv{s}cases{s}print.gcode"),
    ("a rooted POSIX path of one component", "{s}srv"),
    ("a rooted POSIX path, trailing slash", "{s}srv{s}"),
    ("a POSIX path of non-ASCII segments", "{s}数据{s}模型.stl"),
    ("a doubled-slash network path", "{s}{s}server{s}share{s}file.gcode"),
    ("a doubled-slash network path, non-ASCII", "{s}{s}服务器{s}共享{s}模型.stl"),
    ("a UNC path", "{b}{b}server{b}share{b}file.gcode"),
    ("a UNC path, escaped", "{b}{b}{b}{b}server{b}{b}share{b}{b}file.gcode"),
    ("a UNC path, non-ASCII", "{b}{b}服务器{b}共享{b}模型.stl"),
    ("a device path", "{b}{b}?{b}C:{b}cases{b}print.gcode"),
    ("a drive-letter path", "C:{b}cases{b}print.gcode"),
    ("a drive-letter path, escaped", "C:{b}{b}cases{b}{b}print.gcode"),
    ("a drive-letter path, forward slash", "D:{s}cases{s}print.gcode"),
    ("a drive-letter path, non-ASCII", "C:{b}数据{b}模型.stl"),
    ("a drive root", "C:{b}"),
    ("a rooted Windows path without a drive", "{b}cases{b}print.gcode"),
    ("a home-directory path", "~{s}cases{s}print.gcode"),
    ("a named home-directory path", "~operator{s}cases"),
    ("a file URL", "file:{s}{s}{s}srv{s}print.gcode"),
)
NOT_PATHS: tuple[tuple[str, str], ...] = (
    ("a web URL", "https:{s}{s}example.org{s}cases{s}print.gcode"),
    ("a relative path", "cases{s}print.gcode"),
    ("a relative path of non-ASCII segments", "数据{s}模型.stl"),
    ("a parent reference", "..{s}cases"),
    ("a current-directory reference", ".{s}cases"),
    ("a placeholder", "<run>{s}print.gcode"),
    ("a closing tag", "<{s}metadata>"),
    ("a speed unit", "50 mm{s}s and 50 %{s}s"),
    ("escaped newlines before words", "{b}n{b}nNext step{b}n"),
    ("a code comment", "{s}{s} a comment"),
    ("an escaped quote and newline", 'a {b}"label{b}" and {b}n line'),
    ("a ratio", "1{s}2 and 3{s}4"),
    ("base64 text", "u8vb6{s}wMHC r7{s}P3+{s}w=="),
)


def _form(template: str) -> bytes:
    return template.format(s="/", b="\\").encode()


def check_detector(report: Report) -> None:
    """Prove the absolute-path search on every form it must find and leave."""
    report.section("absolute-path detector")
    for name, template in PATH_FORMS:
        path = _form(template)
        for context in (b"%s", b'see "%s" here', b"at %s, then", b"(%s)"):
            found = [m.group(0) for m in ABSOLUTE_PATH.finditer(context % path)]
            if path not in found:
                report.fail(f"misses {name} in {_short((context % path).decode())!r}: {found}")
                break
        else:
            report.ok(f"finds {name}")
    for name, template in NOT_PATHS:
        text = b"in " + _form(template) + b" here"
        found = [m.group(0).decode() for m in ABSOLUTE_PATH.finditer(text)]
        if found:
            report.fail(f"takes {name} for a path: {found}")
        else:
            report.ok(f"leaves {name}")
    _check_secret_search(report)


# An Obico event's payload as it records an image, the server's address and
# the URL's credential spelled in pieces so that this file's own text holds
# neither. `{host}` is filled with a LAN address, `{query}` with a credential.
def _encode(text: str) -> str:
    return base64.b64encode(text.encode()).decode()


def _dotted(*octets: int) -> str:
    return ".".join(str(octet) for octet in octets)


_LAN_HOST = _dotted(192, 168, 0, 7) + ":3334"
_PAYLOAD = (
    '{{"event": {{"type": "PrintFailure"}}, "print": {{"filename": "plate.gcode"}}, '
    '"img_url": "http://{host}/media/snapshots/1/frame.jpg{query}"}}'
)
_CREDENTIAL = "?" + "digest=" + "c2lnbmVk"
_BEARER = "Authorization: " + "Bearer " + "eyJhbGciOiJIUzI1NiJ9.e30.c2lnbmF0dXJl"


def _quote(text: str) -> str:
    """URL-encoded as a query value is: every reserved character escaped."""
    return urllib.parse.quote(text, safe="")


def _quote_all(text: str) -> str:
    """URL-encoded with every byte escaped, digits and letters included."""
    return "".join(f"%{byte:02X}" for byte in text.encode())


# Each secret the export must never carry, in each form it could be carried:
# plain, base64-encoded and URL-encoded (escaping only what a URL reserves, and
# escaping every byte).
_SECRETS: tuple[tuple[str, str, Kind], ...] = (
    ("a private IPv4 address with a port", f"http://{_LAN_HOST}/x.jpg", Kind.LAN),
    ("a signed digest token", f"https://example.org/x.jpg{_CREDENTIAL}", Kind.CREDENTIAL),
    ("a bearer credential", _BEARER, Kind.BEARER),
)
_ENCODINGS: tuple[tuple[str, Callable[[str], str]], ...] = (
    ("plain", lambda text: text),
    ("base64-encoded", _encode),
    ("URL-encoded", _quote),
    ("URL-encoded, every byte", _quote_all),
    ("URL-encoded inside base64", lambda text: _encode(_quote_all(text))),
)
SECRET_FORMS: tuple[tuple[str, str, frozenset[str]], ...] = (
    *(
        (f"{name}, {how}", f"see {encode(text)} here", frozenset({kind}))
        for name, text, kind in _SECRETS
        for how, encode in _ENCODINGS
    ),
    ("a LAN address and port", f"on {_LAN_HOST}", frozenset({Kind.LAN})),
    ("a link-local address", "at " + _dotted(169, 254, 1, 2), frozenset({Kind.LAN})),
    (
        "a credential in a URL",
        f"https://example.org/x.jpg{_CREDENTIAL}",
        frozenset({Kind.CREDENTIAL}),
    ),
    (
        "a LAN address and a credential, base64-encoded",
        f'"raw": "{_encode(_PAYLOAD.format(host=_LAN_HOST, query=_CREDENTIAL))}"',
        frozenset({Kind.LAN, Kind.CREDENTIAL}),
    ),
    (
        "an absolute path, base64-encoded",
        base64.b64encode(_form("the file at {s}srv{s}cases{s}print.gcode")).decode(),
        frozenset({Kind.ABSOLUTE}),
    ),
)
NOT_SECRETS: tuple[tuple[str, str], ...] = (
    ("a version number", "PrusaSlicer 2.9.0 and firmware 6.2.4.12"),
    ("a public address", "the resolver at " + _dotted(8, 8, 8, 8)),
    ("a URL's ordinary query", "https://example.org/x.jpg?size=640&fit=crop"),
    ("a URL-encoded ordinary query", _quote("https://example.org/x.jpg?size=640&fit=crop")),
    ("the word bearer in prose", "the bearer of this note, a bearer bond"),
    ("a G-code percent", "M221 S95 ; flow 95%, then M220 S100%"),
    ("an encoded payload of neither", base64.b64encode(b'{"is_warning": true}' * 2).decode()),
    ("encoded binary data", base64.b64encode(bytes(range(256))).decode()),
)


def _check_secret_search(report: Report) -> None:
    """Prove the LAN-address and credential search, plain and base64-encoded, and its scrub."""
    report.section("LAN-address, credential and encoded-payload detector")
    lexicon = Lexicon([], ())
    for name, text, kinds in SECRET_FORMS:
        found = {kind for kind, *_ in lexicon.find(text.encode())}
        if found == kinds:
            report.ok(f"finds {name}: {', '.join(sorted(found))}")
        else:
            report.fail(f"finds {sorted(found)} in {name}, not {sorted(kinds)}")
    for name, text in NOT_SECRETS:
        found = [(kind, term) for kind, _start, _end, term in lexicon.find(text.encode())]
        if found:
            report.fail(f"takes {name} for a private term: {found}")
        else:
            report.ok(f"leaves {name}")
    payload = _PAYLOAD.format(host=_LAN_HOST, query=_CREDENTIAL)
    encoded = base64.b64encode(payload.encode()).decode()
    match = re.search(BASE64.decode(), encoded)
    scrubbed = _decoded(_reencoded(match).encode()) if match else None
    expected = _PAYLOAD.format(host=NEUTRAL_HOST, query="").encode()
    if scrubbed == expected:
        report.ok("re-encodes a payload with its LAN host made neutral and its credential gone")
    else:
        report.fail(f"re-encodes a payload as {scrubbed!r}, not {expected!r}")


def text_segments(name: str, data: bytes) -> list[Segment]:
    """The parts of a file's bytes that can hold a readable path, each with its offset.

    A JPEG's pixel data and a 3MF's zipped bytes are compressed, so any path in
    them would be a chance run of bytes: a JPEG's text is in its metadata
    segments, a 3MF's in its members (searched as files of their own), and a
    binary STL's in its 80-byte header. Every other file is text throughout.
    """
    suffix = Path(name).suffix.lower()
    if suffix in {".jpg", ".jpeg"} and data.startswith(bytes([JPEG_MARKER, JPEG_START_OF_IMAGE])):
        return _jpeg_metadata(data)
    if suffix == ".3mf":
        return []
    if suffix == ".stl" and not data.startswith(b"solid"):
        return [Segment(0, data[:80])]
    return [Segment(0, data)]


# JPEG markers: every marker starts with 0xFF; start of scan, after which only
# compressed pixel data follows; the application segments and the comment.
JPEG_MARKER = 0xFF
JPEG_START_OF_IMAGE = 0xD8
JPEG_START_OF_SCAN = 0xDA
JPEG_LENGTH_BYTES = 2
JPEG_TEXT_MARKERS = frozenset({*range(0xE0, 0xF0), 0xFE})


def _jpeg_metadata(data: bytes) -> list[Segment]:
    """Each application (APPn) and comment segment of a JPEG, read up to its scan.

    A segment's length counts its own two length bytes, so it is at least 2,
    and it ends inside the file. A JPEG whose headers break either rule, or
    that ends before its scan, cannot be read this way: all of it is searched.
    """
    segments = []
    index = 2
    while index + 4 <= len(data) and data[index] == JPEG_MARKER:
        marker = data[index + 1]
        if marker == JPEG_MARKER:
            index += 1
            continue
        if marker == JPEG_START_OF_SCAN:
            return segments
        length = int.from_bytes(data[index + 2 : index + 4], "big")
        if length < JPEG_LENGTH_BYTES or index + 2 + length > len(data):
            break
        if marker in JPEG_TEXT_MARKERS:
            segments.append(Segment(index + 4, data[index + 4 : index + 2 + length]))
        index += 2 + length
    return [Segment(0, data)]


def searched(path: Path) -> Iterator[Searched]:
    """A file's bytes, and for a 3MF also every member's, to be searched.

    Each comes with the part names it may hold that are not filesystem paths:
    a 3MF is an OPC package, whose members refer to each other by absolute
    part names (`Target="/<member>"`) that name a member of the same archive.
    """
    data = path.read_bytes()
    yield Searched(path.name, data, frozenset())
    if path.suffix.lower() == ".3mf":
        with zipfile.ZipFile(path) as archive:
            members = archive.namelist()
            parts = frozenset(f"/{member}" for member in members)
            for member in members:
                yield Searched(f"{path.name}!{member}", archive.read(member), parts)


def blob_id(data: bytes) -> str:
    """Git's object id for a file's bytes, which is how git stores it once."""
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()  # noqa: S324 - git's object id, not a security hash


class Report:
    """What the run found, section by section; any failure makes the exit 1.

    The report itself goes to standard output and each finding to standard
    error, both flushed line by line so that one log of the two reads in order.
    """

    def __init__(self) -> None:
        self.failures = 0

    def section(self, title: str) -> None:
        print(f"\n== {title}", flush=True)

    def ok(self, line: str) -> None:
        print(f"  ok    {line}", flush=True)

    def note(self, line: str) -> None:
        print(f"        {line}", flush=True)

    def fail(self, line: str) -> None:
        self.failures += 1
        print(f"  FAIL  {line}", file=sys.stderr, flush=True)


def _short(text: str, width: int = 110) -> str:
    flat = text.replace("\r", "").replace("\n", "\\n")
    return flat if len(flat) <= width else flat[: width - 3] + "..."


def _is_scrubbed(relative: str) -> bool:
    return relative == README or Path(relative).name in SCRUBBED


@dataclass(frozen=True, slots=True)
class JsonDiff:
    """How a JSON value may change: only inside strings that held a private term.

    Where `drops` is true a field that held one may also be dropped.
    """

    private: Callable[[str], bool]
    drops: bool

    def problems(self, base: object, now: object, where: str = "$") -> Iterator[str]:
        """Each way `now` changed from `base` other than as allowed."""
        match base, now:
            case dict(), dict():
                yield from self._objects(base, now, where)
            case list(), list() if len(base) != len(now):
                yield f"{where} changed length"
            case list(), list():
                for index, (old, new) in enumerate(zip(base, now, strict=True)):
                    yield from self.problems(old, new, f"{where}[{index}]")
            case str(), str():
                if base != now and not self.private(base):
                    yield f"{where} changed, but held no private term"
            case _ if type(base) is not type(now) or base != now:
                yield f"{where} changed"
            case _:
                return

    def _objects(self, base: dict, now: dict, where: str) -> Iterator[str]:
        for key, value in base.items():
            if key in now:
                yield from self.problems(value, now[key], f"{where}.{key}")
            elif not self.drops or not self.private(json.dumps(value)):
                yield f"{where}.{key} was dropped"
        yield from (f"{where}.{key} was added" for key in now if key not in base)


def check_files(report: Report, root: Path, base_label: str, base: dict[str, str]) -> None:
    report.section(f"files: {DIRECTORY}/ against {base_label}:{DIRECTORY}/")
    here = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    kept = {path for path in base if not path.startswith(f"{DROPPED}/")}
    dropped = set(base) - kept
    missing = sorted(kept - here)
    if missing:
        report.fail(f"missing: {missing}")
    else:
        report.ok(
            f"all {len(kept)} files of the base outside {DROPPED}/ are here under their own names"
        )
    leftover = sorted(p for p in here if p.startswith(f"{DROPPED}/") or p in dropped)
    if leftover:
        report.fail(f"{DROPPED} left behind: {leftover}")
    else:
        report.ok(f"none of {DROPPED}'s {len(dropped)} files is here")
    added = sorted(here - set(base))
    if added == [SELF]:
        report.ok(f"the one added file is {SELF}")
    else:
        report.fail(f"added files other than {SELF} alone: {added}")
    kinds: Counter[str] = Counter()
    changed = False
    for path in sorted(kept & here):
        if _is_scrubbed(path):
            continue
        if blob_id((root / path).read_bytes()) == base[path]:
            kinds[Path(path).suffix.lower() or Path(path).name] += 1
        else:
            changed = True
            report.fail(f"{path} is not byte-identical to the base")
    if not changed:
        listed = ", ".join(f"{count} {kind}" for kind, count in sorted(kinds.items()))
        report.ok(f"{kinds.total()} files byte-identical to the base ({listed})")


def check_texts(
    report: Report, root: Path, texts: dict[str, str], scrubber: Scrubber, lexicon: Lexicon
) -> dict[str, list[Change]]:
    report.section("case texts: every change is a listed substitution of a private term")
    check = TextCheck(root, scrubber, lexicon)
    return {
        path: check.run(report, path, text)
        for path, text in sorted(texts.items())
        if not path.startswith(f"{DROPPED}/")
    }


@dataclass(frozen=True, slots=True)
class TextCheck:
    """One case text against its scrubbed base, with each of its changes listed."""

    root: Path
    scrubber: Scrubber
    lexicon: Lexicon

    def run(self, report: Report, path: str, text: str) -> list[Change]:
        scrubber, lexicon = self.scrubber, self.lexicon
        expected, changes = scrubber.scrub(path, text)
        actual = (self.root / path).read_bytes().decode("utf-8")
        for change in changes:
            if change.kind != Kind.DROPPED_MENTION and not lexicon.holds(change.before):
                report.fail(
                    f"{path}:{change.line} a rule matched no private term: {_short(change.before)}"
                )
            if lexicon.holds(change.after):
                report.fail(
                    f"{path}:{change.line} a replacement holds a private term: "
                    f"{_short(change.after)}"
                )
        if path == README:
            _check_readme_diff(report, path, expected, actual)
        elif actual != expected:
            report.fail(f"{path} differs from its base other than by the listed substitutions")
            diff = difflib.unified_diff(expected.splitlines(), actual.splitlines(), lineterm="")
            for line in list(diff)[:12]:
                report.note(_short(line))
        if path.endswith(".json"):
            allowed = JsonDiff(lexicon.holds, drops=Path(path).name != HISTORY)
            for problem in allowed.problems(json.loads(text), json.loads(actual)):
                report.fail(f"{path}: {problem}")
        report.ok(f"{path}: {len(changes)} change(s)" if changes else f"{path}: unchanged")
        for change in changes:
            report.note(f"line {change.line} [{change.kind}]")
            report.note(f"  - {_short(change.before)}")
            report.note(f"  + {_short(change.after) if change.after else '(line removed)'}")
        return changes


def _check_readme_diff(report: Report, path: str, expected: str, actual: str) -> None:
    """The README is its scrubbed base with lines only inserted."""
    old, new = expected.splitlines(), actual.splitlines()
    matcher = difflib.SequenceMatcher(a=old, b=new, autojunk=False)
    for tag, a0, a1, b0, b1 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if tag != "insert":
            report.fail(
                f"{path}: scrubbed base lines {a0 + 1}-{a1} were {tag}d rather than only added to"
            )
            continue
        for line in new[b0:b1]:
            report.note(f"added line [export note]: {_short(line)}")


def check_lexicon(
    report: Report,
    root: Path,
    texts: dict[str, str],
    changes: dict[str, list[Change]],
    lexicon: Lexicon,
) -> None:
    report.section("private terms: none in any file, this one, the G-code and 3MF members included")
    report.note(
        f"{len(lexicon.terms)} patterns; "
        + "; ".join(
            f"{len(c)} commits as {k}, in full or by a 7+ character prefix"
            for k, c in lexicon.commits
        )
    )
    files = sorted(p for p in root.rglob("*") if p.is_file())
    if not _search_files(report, root, files, lexicon):
        members = sum(1 for p in files for _ in searched(p)) - len(files)
        report.ok(f"no private term in {len(files)} files or the {members} members of their 3MFs")

    report.section("private terms the base held, and what became of each")
    held: Counter[str] = Counter()
    for path, text in sorted(texts.items()):
        data = text.encode()
        spans = _spans(text, changes.get(path, []))
        for kind, start, end, term in lexicon.find(data):
            held[kind] += 1
            line = data.count(b"\n", 0, start) + 1
            where = f"{path}:{line} [{kind}] {term!r}"
            if path.startswith(f"{DROPPED}/"):
                report.ok(f"{where}: removed with {DROPPED}")
                continue
            char_start, char_end = len(data[:start].decode()), len(data[:end].decode())
            covering = next(
                (sp.change for sp in spans if sp.start <= char_start and char_end <= sp.end),
                None,
            )
            if covering is None:
                report.fail(f"{where}: not replaced")
            elif covering.after:
                report.ok(f"{where}: replaced by {_short(covering.after, 70)!r}")
            else:
                report.ok(f"{where}: removed with its line")
    report.note(
        f"{held.total()} occurrences in the base's texts: "
        + ", ".join(f"{n} {k}" for k, n in sorted(held.items()))
    )
    report.note(
        "every other base file is byte-identical here and was searched above, so it held none"
    )


def _search_files(report: Report, root: Path, files: list[Path], lexicon: Lexicon) -> int:
    """Report every private term in the files, and answer how many were found."""
    found = 0
    for path in files:
        for part in searched(path):
            for hit in lexicon.find(part.data, text_segments(part.name, part.data)):
                if hit.kind == Kind.ABSOLUTE and hit.text in part.parts:
                    continue
                found += 1
                line = part.data.count(b"\n", 0, hit.start) + 1
                report.fail(
                    f"{path.relative_to(root)} ({part.name}) line {line} [{hit.kind}]: {hit.text!r}"
                )
    return found


def _spans(text: str, changes: list[Change]) -> list[Span]:
    """Each change's span in its base text, found from its line and its matched text."""
    starts = [0, *(i + 1 for i, ch in enumerate(text) if ch == "\n")]
    spans = []
    for change in changes:
        offset = text.index(change.before, starts[change.line - 1])
        spans.append(Span(offset, offset + len(change.before), change))
    return spans


def _read_json(path: Path) -> Any:  # noqa: ANN401 - a JSON document, read as the schemas check it
    return json.loads(path.read_bytes().decode("utf-8"))


# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] Drift-checked on every run.
# These views are read only by this script, and each run holds them to
# printobserver's schemas with `typed_view_drift` before it reads a case,
# failing on any drift: the export may add no gate recipe outside this
# directory, so this script's own run is their drift gate.
# Each typed view of a JSON document lists its required keys in one class and
# its optional ones in a `total=False` subclass: `NotRequired` is not seen
# when annotations are strings, as they are in this file.
class _StepKeys(TypedDict):
    operation: str


class Step(_StepKeys, total=False):
    """One operation an outcome makes or a scenario forbids."""

    args: dict[str, str | float]


class _LookKeys(TypedDict):
    image: str


class Look(_LookKeys, total=False):
    """One look the agent takes in a scenario."""

    arrived_event_ids: list[EventId]


class _ScenarioKeys(TypedDict):
    id: ScenarioId
    printer_state: str
    event_image: str
    looks: list[Look]
    accept_any_of: list[list[Step]]
    never: list[Step]


class Scenario(_ScenarioKeys, total=False):
    """One scenario of a case's assertions."""

    event_id: EventId


class Assertions(TypedDict):
    scenarios: list[Scenario]


class AgentImage(TypedDict):
    file: str
    event_id: EventId


class _CaseKeys(TypedDict):
    case: str
    assertions: Assertions


class Case(_CaseKeys, total=False):
    """A `case.json`, once printobserver's case schema has accepted it."""

    sources: dict[str, str]
    obico_scores: str
    agent_images: list[AgentImage]


class ImageRef(TypedDict):
    """The image an event arrived with, as its history records it."""

    sha256: str


class _HistoryEventKeys(TypedDict):
    id: EventId
    kind: str


class HistoryEvent(_HistoryEventKeys, total=False):
    """One event of a history, once its HistoryAnswer schema has accepted it."""

    image: ImageRef | None


class History(TypedDict):
    events: list[HistoryEvent]


# The typed view of each document, held to the schema that owns its shape by
# `typed_view_drift`, which follows every nested view, list and map from it.
CASE_VIEW: type = Case
HISTORY_VIEW: type = History
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate] the typed views end here


def typed_view_drift(schema: dict[str, Any], view: type) -> list[str]:
    """Each way a typed view, and every view nested in it, has drifted from its schema.

    This runs on every run of this check, which is the gate the export passes
    through, so a schema that changes under a view fails it.
    """
    return _view_drift(schema, view, schema, view.__name__)


def _view_drift(schema: dict[str, Any], view: type, node: object, where: str) -> list[str]:
    """A typed view against the schema object it reads.

    Every key it declares must be a property there, of the same JSON types
    all the way down; a key it requires must be one the schema requires, and
    a key it leaves optional one the schema leaves optional.
    """
    resolved = _resolve(schema, node)
    if resolved is None:
        return [f"{where}: the schema has no object here"]
    properties = resolved.get("properties", {})
    required = set(resolved.get("required", []))
    problems = []
    for key, annotation in sorted(get_type_hints(view).items()):
        if key not in properties:
            problems.append(f"{where}.{key} is no property")
            continue
        if (key in view.__required_keys__) != (key in required):
            state = "required" if key in view.__required_keys__ else "optional"
            problems.append(f"{where}.{key} is {state} here but not in the schema")
        problems += _type_drift(schema, annotation, properties[key], f"{where}.{key}")
    return problems


def _type_drift(schema: dict[str, Any], annotation: object, node: object, where: str) -> list[str]:
    """An annotation against the schema object for the same value, recursively.

    `object` admits any value, so it is held to nothing; a list's items, a
    map's values and a nested view are each held to the schema's own.
    """
    if annotation is object:
        return []
    declared, spelled = _json_types(annotation), _schema_types(schema, node)
    if declared != spelled:
        return [f"{where} is {sorted(declared)} here, {sorted(spelled)} there"]
    problems = []
    for arg in _members(annotation):
        base = get_origin(arg) or arg
        if isinstance(base, type) and is_typeddict(base):
            branch = _branch(schema, node, "object")
            problems += _view_drift(schema, base, branch, f"{where}<{base.__name__}>")
            continue
        match base, get_args(arg), _branch(schema, node, "array" if base is list else "object"):
            case _, _, None:
                continue
            case builtins.list, (item,), branch:
                problems += _type_drift(schema, item, branch.get("items"), f"{where}[]")
            case builtins.dict, (_, builtins.object), _:
                continue
            case builtins.dict, (_, value), {"additionalProperties": dict(values)}:
                problems += _type_drift(schema, value, values, f"{where}{{}}")
            case builtins.dict, (_, value), _:
                problems.append(f"{where} maps to {value} here, unconstrained there")
            case _:
                continue
    return problems


def _members(annotation: object) -> tuple[object, ...]:
    """The arms of a union annotation, or the annotation itself."""
    if get_origin(annotation) in (Union, UnionType):
        return get_args(annotation)
    return (annotation,)


def _branch(schema: dict[str, Any], node: object, json_type: str) -> dict[str, Any] | None:
    """The schema object, or the `anyOf`/`oneOf` arm of it, that admits one JSON type."""
    resolved = _resolve(schema, node)
    if resolved is None:
        return None
    for arm in [resolved, *resolved.get("anyOf", []), *resolved.get("oneOf", [])]:
        found = _resolve(schema, arm)
        if found is not None and json_type in _own_types(found):
            return found
    return None


def _own_types(node: dict[str, Any]) -> set[str]:
    match node.get("type"):
        case str(single):
            return {single}
        case [*several]:
            return {str(t) for t in several}
        case _:
            return set()


def _at(schema: dict[str, Any], path: tuple[str, ...]) -> object:
    node: object = schema
    for step in path:
        node = node.get(step) if isinstance(node, dict) else None
    return node


def _resolve(schema: dict[str, Any], node: object) -> dict[str, Any] | None:
    """A schema object, through any local `$ref` it is."""
    while isinstance(node, dict) and isinstance(node.get("$ref"), str):
        node = _at(schema, tuple(node["$ref"].removeprefix("#/").split("/")))
    return node if isinstance(node, dict) else None


def _schema_types(schema: dict[str, Any], node: object) -> set[str]:
    """The JSON types a schema object admits, through `$ref`, `anyOf` and `oneOf`."""
    resolved = _resolve(schema, node)
    if resolved is None:
        return set()
    types = _own_types(resolved)
    for branch in [*resolved.get("anyOf", []), *resolved.get("oneOf", [])]:
        types |= _schema_types(schema, branch)
    if not types and "const" in resolved:
        types = {_JSON_TYPES.get(type(resolved["const"]), "unknown")}
    return types


_JSON_TYPES: dict[type, str] = {
    str: "string",
    bool: "boolean",
    int: "integer",
    float: "number",
    list: "array",
    dict: "object",
    type(None): "null",
}


def _json_types(annotation: object) -> set[str]:
    """The JSON types a Python annotation of a typed view admits."""
    # A NewType (an EventId) admits what the type it wraps admits.
    while (supertype := getattr(annotation, "__supertype__", None)) is not None:
        annotation = supertype
    if get_origin(annotation) in (Union, UnionType):
        return set().union(*(_json_types(arg) for arg in get_args(annotation)))
    base = get_origin(annotation) or annotation
    if isinstance(base, type) and is_typeddict(base):
        return {"object"}
    return {_JSON_TYPES[base]} if isinstance(base, type) and base in _JSON_TYPES else {"unknown"}


def _printer_states(document: object) -> set[str]:
    """The printer states printobserver names: each `const` of its PrinterState `oneOf`."""
    match document:
        case {"oneOf": [*arms]} if all(isinstance(arm, dict) for arm in arms):
            states = {arm["const"] for arm in arms if isinstance(arm.get("const"), str)}
            if states:
                return states
    msg = f"{PRINTER_STATE} holds no `oneOf` of string `const` states"
    raise SystemExit(msg)


def _operations(document: object) -> dict[str, set[str]]:
    """Each operation printobserver's operations list names, with its parameters' names."""
    operations: dict[str, set[str]] = {}
    match document:
        case {"operations": [*listed]}:
            for operation in listed:
                match operation:
                    case {"name": str(name), "parameters": [*parameters]} if all(
                        isinstance(p, dict) and isinstance(p.get("name"), str) for p in parameters
                    ):
                        operations[name.replace("_", "-")] = {p["name"] for p in parameters}
                    case _:
                        msg = f"{OPERATIONS} lists an operation without a name and named parameters"
                        raise SystemExit(msg)
            return operations
    msg = f"{OPERATIONS} holds no `operations` list"
    raise SystemExit(msg)


def _file_token(printobserver: Path) -> re.Pattern[str]:
    """printobserver's own reading of a file a case's prose names, read from its source.

    One Python file of printobserver spells the pattern, as
    `FILE_TOKEN = re.compile("...")`; this finds that file and reads the
    literal rather than copying it, so the two cannot drift apart.
    """
    listed = _git("grep", "-lE", r"^FILE_TOKEN = re\.compile\(", "--", "*.py", cwd=printobserver)
    sources = listed.split()
    if len(sources) != 1:
        msg = f"printobserver spells FILE_TOKEN in {len(sources)} Python files, not one"
        raise SystemExit(msg)
    source = (printobserver / sources[0]).read_text(encoding="utf-8")
    for node in ast.walk(ast.parse(source)):
        match node:
            case ast.Assign(
                targets=[ast.Name(id="FILE_TOKEN")],
                value=ast.Call(args=[ast.Constant(value=str(pattern))]),
            ):
                return re.compile(pattern)
    msg = "printobserver's FILE_TOKEN is not `re.compile(<string literal>)`"
    raise SystemExit(msg)


class Contracts:
    """printobserver's contracts for a case, read from its clone by path."""

    def __init__(self, printobserver: Path) -> None:
        case_schema = _read_json(printobserver / CASE_SCHEMA)
        history_schema = _read_json(printobserver / HISTORY_SCHEMA)
        for name, schema in ((CASE_SCHEMA, case_schema), (HISTORY_SCHEMA, history_schema)):
            try:
                Draft202012Validator.check_schema(schema)
            except SchemaError as error:
                msg = f"printobserver's {name} is not a valid JSON schema: {error.message}"
                raise SystemExit(msg) from error
        self.case_schema = Draft202012Validator(case_schema)
        self.history_schema = Draft202012Validator(history_schema)
        self.drift = typed_view_drift(case_schema, CASE_VIEW) + typed_view_drift(
            history_schema, HISTORY_VIEW
        )
        self.states = _printer_states(_read_json(printobserver / PRINTER_STATE))
        self.operations = _operations(_read_json(printobserver / OPERATIONS))
        self.file_token = _file_token(printobserver)

    def check(self, report: Report, directory: Path) -> None:
        """One case: its schemas, the files it names, its agent images and its scenarios.

        A document either schema refuses is reported and read no further, since
        every later check reads fields the schema promises.
        """
        raw = _read_json(directory / "case.json")
        problems = [
            f"case.json at {e.json_path}: {e.message}" for e in self.case_schema.iter_errors(raw)
        ]
        has_history = (directory / HISTORY).is_file()
        raw_history = _read_json(directory / HISTORY) if has_history else None
        if has_history:
            problems += [
                f"{HISTORY} at {e.json_path}: {e.message}"
                for e in self.history_schema.iter_errors(raw_history)
            ]
        if problems:
            for problem in problems:
                report.fail(f"{directory.name}: {problem}")
            return
        document = cast("Case", raw)
        history = cast("History", raw_history) if has_history else None
        events = {event["id"]: event for event in history["events"]} if history else {}
        if document["case"] != directory.name:
            problems.append(f"case.json names its case {document['case']!r}")
        scenarios = document["assertions"]["scenarios"]
        for scenario in scenarios:
            problems += _scenario_problems(scenario, events, self.states, self.operations)
        named = self._named(document)
        problems += _missing(directory, named)
        problems += self._unnamed(report, directory, document)
        pictures = sorted(p for p in directory.glob("agent-*") if p.suffix != ".json")
        problems += _image_problems(document, events, pictures)
        for problem in problems:
            report.fail(f"{directory.name}: {problem}")
        if not problems:
            history_note = (
                f"{HISTORY} valid ({len(events)} events); " if history else "no history; "
            )
            report.ok(
                f"{directory.name}: case.json valid; {history_note}{len(named)} named files exist; "
                f"{len(pictures)} agent images match their events; "
                f"{len(scenarios)} scenario(s) resolve"
            )

    def _named(self, document: Case) -> set[str]:
        """Every file a case.json refers to, as printobserver reads one."""
        named = set(_file_fields({k: v for k, v in document.items() if k != "assertions"}))
        for scenario in document["assertions"]["scenarios"]:
            named |= {scenario["event_image"], *(look["image"] for look in scenario["looks"])}
        for text in [*document.get("sources", {}).values(), document.get("obico_scores", "")]:
            named |= set(self.file_token.findall(text))
        return named

    def _unnamed(self, report: Report, directory: Path, document: Case) -> list[str]:
        """Each file of a case its case.json names nowhere."""
        files = [p.name for p in sorted(directory.iterdir()) if p.name != "case.json"]
        prose = json.dumps(document, ensure_ascii=False)
        # printobserver's own check reads a name only as its file token spells
        # one, which no STL, 3MF, Markdown file or name with a space is: noted,
        # not failed.
        tokens = set(self.file_token.findall(json.dumps(document)))
        unread = [name for name in files if name not in tokens]
        if unread:
            report.note(
                f"{directory.name}: named, but not as printobserver's file token reads: {unread}"
            )
        return [
            f"{name} is in the case but case.json names it nowhere"
            for name in files
            if name not in prose
        ]


def _missing(directory: Path, named: set[str]) -> list[str]:
    """Each name that is no file of this directory's cases.

    A case may name a file of its own or, through `../`, of another case, so a
    name must be relative and resolve inside the directory that holds the
    cases: an absolute name, or one that climbs out of it, names nothing here.
    """
    root = directory.parent.resolve()
    problems = []
    for name in sorted(named):
        target = (directory / name).resolve()
        if Path(name).is_absolute() or not target.is_relative_to(root):
            problems.append(f"names {name}, which is outside {DIRECTORY}/")
        elif not target.is_file():
            problems.append(f"names {name}, which is not a file")
    return problems


def _image_problems(
    document: Case, events: dict[EventId, HistoryEvent], pictures: list[Path]
) -> list[str]:
    """Each agent image whose sha256 is not the one its event records."""
    owners = {entry["file"]: entry["event_id"] for entry in document.get("agent_images", [])}
    problems = []
    for picture in pictures:
        owner = owners.get(picture.name)
        event = events.get(owner) if owner is not None else None
        image = event.get("image") if event else None
        recorded = image["sha256"] if image else None
        if recorded != hashlib.sha256(picture.read_bytes()).hexdigest():
            problems.append(f"{picture.name}'s sha256 is not the one its event records")
    return problems


def check_contracts(report: Report, root: Path, printobserver: Path) -> None:
    report.section(f"contracts: printobserver at {PRINTOBSERVER_COMMIT}, read from its clone")
    contracts = Contracts(printobserver)
    for problem in contracts.drift:
        report.fail(f"typed view drifted from its schema: {problem}")
    if not contracts.drift:
        report.ok("the typed views of a case and a history, nested ones too, match their schemas")
    for directory in sorted(p for p in root.iterdir() if p.is_dir()):
        contracts.check(report, directory)


def _file_fields(value: object) -> Iterator[str]:
    """Every string a `file` key holds, at any depth of a JSON value."""
    match value:
        case dict():
            for key, item in value.items():
                if key == "file" and isinstance(item, str):
                    yield item
                else:
                    yield from _file_fields(item)
        case list():
            for item in value:
                yield from _file_fields(item)
        case _:
            return


def _scenario_problems(
    scenario: Scenario,
    events: dict[EventId, HistoryEvent],
    states: set[str],
    operations: dict[str, set[str]],
) -> list[str]:
    """Each event, state or operation a scenario names that its case or printobserver lacks."""
    where = f"scenario {scenario['id']}"
    problems = []
    if scenario["printer_state"] not in states:
        problems.append(f"{where} has unknown printer_state {scenario['printer_state']!r}")
    event_id = scenario.get("event_id")
    if event_id is not None and (
        event_id not in events or events[event_id]["kind"] != "obico_failure_alert"
    ):
        problems.append(f"{where} names {event_id}, which is no obico_failure_alert in {HISTORY}")
    for look in scenario["looks"]:
        problems += [
            f"{where} delivers unknown event {e}"
            for e in look.get("arrived_event_ids", [])
            if e not in events
        ]
    steps = [step for outcome in scenario["accept_any_of"] for step in outcome]
    for step in [*steps, *scenario["never"]]:
        parameters = operations.get(step["operation"])
        if parameters is None:
            problems.append(f"{where} names unknown operation {step['operation']!r}")
        else:
            problems += [
                f"{where}: {step['operation']} has no {a!r}"
                for a in step.get("args", {})
                if a not in parameters
            ]
    return problems


def check_readme(report: Report, root: Path) -> None:
    report.section("README: the cases and the directory's sizes")
    text = (root / README).read_bytes().decode("utf-8")
    cases = sorted(p.name for p in root.iterdir() if p.is_dir())
    rows = sorted(re.findall(r"(?m)^\| `([\w-]+)` \|", text))
    if rows == cases:
        report.ok(f"its table lists the {len(cases)} cases")
    else:
        report.fail(f"its table lists {rows}, the directory holds {cases}")
    blobs: dict[str, int] = {}
    raw = 0
    for path in root.rglob("*"):
        if path.is_file():
            data = path.read_bytes()
            raw += len(data)
            blobs[blob_id(data)] = len(data)
    for label, size in (("raw", raw), ("of unique blobs", sum(blobs.values()))):
        stated = f"{size / MB:.1f} MB {label}"
        if stated in text:
            report.ok(f"states {stated!r} ({size} bytes)")
        else:
            report.fail(f"does not state {stated!r} ({size} bytes)")


def _clone_refusal(printobserver: Path, root: Path) -> str | None:
    """Why a path is not the printobserver clone this check reads, or None when it is."""
    if printobserver.is_relative_to(Path.cwd().resolve()):
        return f"the printobserver clone {printobserver} is inside this tree"
    shown = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=printobserver, capture_output=True, text=True, check=False
    )
    head = shown.stdout.strip()
    if shown.returncode != 0 or not head.startswith(PRINTOBSERVER_COMMIT):
        return f"{printobserver} is no git clone at printobserver {PRINTOBSERVER_COMMIT}"
    incoming = printobserver / "tests/real-prints"
    if not incoming.is_dir():
        return f"{printobserver} has no tests/real-prints, so it is no printobserver clone"
    copied = sorted(
        p.name for p in incoming.iterdir() if (root / p.name).is_dir() or p.name == "incoming"
    )
    if copied:
        return f"cases of this directory were copied into the clone: {copied}"
    return None


def _verdict(report: Report) -> int:
    if report.failures:
        print(f"\nFAILED: {report.failures} finding(s)", file=sys.stderr, flush=True)
        return EXIT_FINDINGS
    print("\nPASSED", flush=True)
    return EXIT_PASSED


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        epilog=(
            f"Exit status: {EXIT_PASSED} when every check passes; {EXIT_FINDINGS} when one finds "
            f"something (each finding on standard error) or an input is unreadable; "
            f"{EXIT_USAGE} for a command line it refuses."
        ),
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="only prove the absolute-path detector on every path form, then stop",
    )
    parser.add_argument(
        "--base",
        type=_revision,
        help="the commit whose copy of this directory the cases came from",
    )
    parser.add_argument(
        "--printobserver", type=Path, help="a printobserver clone, outside this tree"
    )
    parser.add_argument("--model-repo", type=Path, help="a clone of the model repository")
    parser.add_argument(
        "--model-commit", type=_revision, help="the model repository's commit to read"
    )
    args = parser.parse_args()
    needed = ("base", "printobserver", "model_repo", "model_commit")
    missing = [f"--{n.replace('_', '-')}" for n in needed if getattr(args, n) is None]
    if missing and not args.self_test:
        parser.error(f"the export check needs {', '.join(missing)} (or --self-test alone)")
    root = Path(DIRECTORY)
    report = Report()
    check_detector(report)
    if args.self_test:
        return _verdict(report)

    report.section("inputs")
    printobserver = args.printobserver.resolve()
    refusal = _clone_refusal(printobserver, root)
    if refusal:
        report.fail(refusal)
        # Every later check reads this clone's schemas and source: one it
        # refuses is read no further.
        return _verdict(report)
    report.ok(
        f"printobserver clone at {PRINTOBSERVER_COMMIT}, outside this tree, holding no case of "
        "this directory"
    )
    model_commit = _resolve_commit(args.model_commit, cwd=args.model_repo)
    report.ok("model repository holds the commit given")
    base_commit = _resolve_commit(args.base)
    report.ok(f"base resolves to one commit, {base_commit[:7]}")

    listing = _git("ls-tree", "-r", base_commit, f"{DIRECTORY}/").splitlines()
    base = {
        line.split("\t", 1)[1].removeprefix(f"{DIRECTORY}/"): line.split()[2] for line in listing
    }
    texts = {path: _show(base_commit, f"{DIRECTORY}/{path}") for path in base if _is_scrubbed(path)}
    private = Private.read(base_commit, texts, printobserver, args.model_repo, model_commit)
    report.ok(
        f"read {len(private.packages)} package names, {len(private.users)} user names, "
        f"{len(private.projects)} model projects and {len(private.flags)} CLI flags of the "
        "repository"
    )
    scrubber = Scrubber(private)
    lexicon = Lexicon.of(private)

    check_files(report, root, "base", base)
    changes = check_texts(report, root, texts, scrubber, lexicon)
    check_lexicon(report, root, texts, changes, lexicon)
    check_contracts(report, root, printobserver)
    check_readme(report, root)

    report.section("summary of changes by kind")
    for kind, count in sorted(Counter(c.kind for cs in changes.values() for c in cs).items()):
        report.note(f"{count:4d}  {kind}")
    return _verdict(report)


if __name__ == "__main__":
    sys.exit(main())
