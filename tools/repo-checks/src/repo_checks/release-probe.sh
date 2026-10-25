#!/usr/bin/env bash
# What a public registry serves right now for one artifact this repository
# publishes — the probe `release-targets.toml`'s `probe` names.
#
#   usage: release-probe.sh <registry>:<name>
#
# Exactly three answers, under the probe contract in onevcs's `docs/contract.md`
# ("What a probe prints"), and keeping the last two apart is the whole point:
#
#   * exit 0 with one line on stdout — the version that registry serves now;
#   * exit 0 with nothing on stdout — the registry has no release of it yet;
#   * a non-zero exit with the reason on stderr — not answered.
#
# A consumer holds indefinitely on "not answered" and never reads it as evidence
# that a release has not happened, so nothing here degrades a registry that did
# not answer into an empty answer: "no release" is only ever a registry saying
# so — a 404 from PyPI or npm, crates.io answering `"max_stable_version":null`,
# or GitHub sending `releases/latest` to the bare releases page — never a body
# this script could not read, and never an identifier it does not recognise.
#
# What each registry is asked, and why that one:
#
#   * crate:   crates.io's crate document, `crate.max_stable_version` — what `cargo
#              add` resolves, yanked versions and prereleases already excluded.
#   * pypi:    PyPI's project JSON, `info.version` — what `pip install` resolves.
#   * npm:     the `latest` dist-tag's own document — what `npm install`
#              resolves. A scoped name is asked for as `@scope%2fname`, the
#              registry's own spelling of one path segment.
#   * release: the redirect github.com answers for `releases/latest`, never the
#              REST API. A consumer's engine polls a held probe every sixty
#              seconds, and the unauthenticated REST limit is sixty an hour for
#              the whole host. The tag is answered without its leading `v`, and
#              only once that release carries the `SHA256SUMS` file
#              `scripts/install.sh` verifies every artifact against: a consumer
#              installs the program from that release, and one whose checksum
#              file is not up yet is one the install script refuses. Until it
#              is there the answer is "not answered".
#
# The targets it answers for are the ones the `[[target]]` tables of
# release-targets.toml declare; anything else is not answered. It resolves that
# file from its own location — four directories up, at the repository root —
# rather than from `$PWD`, because a probe answering
# about whatever repository it happened to be started in answers about the wrong
# artifact. It reads the declaration line by line rather than parsing TOML,
# because bash has no TOML reader and the shape it needs is one key in one kind
# of table; what the document *is* is held by onevcs's own reader, which `just
# check-repo`'s `release-declaration` runs over it, and the probe's journey
# asks this script about every id that reader lists, so the two readings of the
# committed file cannot drift apart. What is strict here is the identifier,
# which this script turns into a URL, and what it answers.
#
# A registry's body is read as untrusted: it is parsed whole, as strict JSON, by
# the standard library of `python3`, and answered from only at the registry's
# own path to its version — crates.io's `crate.max_stable_version`, PyPI's
# `info.version`, npm's top-level `version` — so a truncated or malformed body,
# or a version that sits anywhere else in it, is not answered. The value
# answered must be a semantic version, which is what the caller compares it as.
#
# What it may assume, and nothing beyond it: it is spawned as a direct
# subprocess from the repository root, with an environment carrying only PATH
# and HOME and no credential of any kind. Every target is on a public registry,
# so an unauthenticated read is all it needs. `curl -q` follows from the same
# rule — a ~/.curlrc is the caller's configuration and not this probe's.
#
# `curl` and `python3`, with `mktemp` and `rm` for the one file it writes, are
# the whole of what it runs beyond bash's own builtins.
# Each request is bounded by --max-time 25 and a GitHub answer takes two, well
# inside the sixty seconds the contract allows. There is no retry: a transient
# failure is "not answered", which the caller re-asks later.
#
# Every answer, for every registry, is driven end to end by
# tests/repo-e2e/tests/test_release_probe_journey.py.

set -euo pipefail

readonly USER_AGENT="printobserver-release-probe (+https://github.com/nickderobertis/printobserver)"
readonly CONNECT_TIMEOUT=10
readonly MAX_TIME=25
# The forge account a `release:<name>` target is published under, and the file
# every release artifact is verified against: `scripts/install.sh`'s own OWNER
# and CHECKSUMS, which the probe's journey holds these two to.
readonly GITHUB_OWNER="nickderobertis"
readonly CHECKSUMS="SHA256SUMS"

# Not answered: the reason, and what the caller does about it. Both on stderr,
# because stdout is the answer and an empty one means something specific.
refuse() {
    printf 'release-probe.sh: %s\n' "$1" >&2
    printf 'ACTION: %s\n' "$2" >&2
    exit "${3:-1}"
}

readonly RE_ASK="re-ask once the registry answers; nothing about a release has been established"

probe_dir="${BASH_SOURCE[0]%/*}"
[ "$probe_dir" = "${BASH_SOURCE[0]}" ] && probe_dir="."
cd -- "$probe_dir/../../../.." 2>/dev/null || refuse \
    "cannot reach the repository root from ${BASH_SOURCE[0]}" \
    "run this script from a checkout of the repository, at tools/repo-checks/src/repo_checks/release-probe.sh"
readonly DECLARATION="release-targets.toml"

if [ $# -ne 1 ]; then
    refuse "takes exactly one registry-qualified identifier, and was given $#" \
        "run 'release-probe.sh <registry>:<name>', e.g. 'release-probe.sh release:printobserver'" 2
fi
identifier="$1"

for tool in curl python3 mktemp rm; do
    command -v "$tool" >/dev/null 2>&1 || refuse \
        "$tool is not on PATH, so no registry can be read" \
        "install $tool, or put it on the PATH this probe is spawned with"
done

# A declared identifier, checked before it is used as one. Its name becomes a
# path segment of a registry URL, so a name outside letters, digits, `.`, `-`
# and `_` is refused — except npm's scoped `@scope/name`, whose two halves are
# each held to that alphabet.
declared_shape() {
    local name="${1#*:}"
    case "$1" in
    crate:* | pypi:* | release:*) ;;
    npm:@*/*)
        local scope="${name%%/*}"
        scope="${scope#@}"
        local package="${name#*/}"
        case "$scope" in "" | [!A-Za-z0-9]* | *[!A-Za-z0-9._-]*) ;; *)
            case "$package" in "" | [!A-Za-z0-9]* | *[!A-Za-z0-9._-]*) ;; *) return 0 ;; esac ;;
        esac
        refuse "$DECLARATION declares '$1', whose scoped name is not one npm serves" \
            "spell a scoped name as npm:@<scope>/<name>"
        ;;
    npm:*) ;;
    *) refuse "$DECLARATION declares '$1', which names no registry this probe reads" \
        "spell every target's id as crate:, pypi:, npm: or release:<name>" ;;
    esac
    case "$name" in
    "" | [!A-Za-z0-9]* | *[!A-Za-z0-9._-]*)
        refuse "$DECLARATION declares '$1', whose name is not one a registry serves" \
            "spell the name exactly as its registry does" ;;
    esac
}

trim() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    printf '%s' "${value%"${value##*[![:space:]]}"}"
}

# The declaration, read in full even once a match is found: a file this script
# cannot read is a declaration nobody has checked, and answering from half of it
# would answer about a target whose spelling was never held to anything.
[ -r "$DECLARATION" ] || refuse \
    "$PWD/$DECLARATION is missing or unreadable, so nothing is declared" \
    "restore release-targets.toml from git; it is this repository's one declaration of what it releases"
matched=1
targets=""
table=""
while IFS= read -r line || [ -n "$line" ]; do
    line="$(trim "$line")"
    case "$line" in
    "" | "#"*) continue ;;
    "[["*)
        table="${line%%]]*}]]"
        continue
        ;;
    "["*)
        table="${line%%]*}]"
        continue
        ;;
    esac
    [ "$table" = "[[target]]" ] || continue
    case "$line" in
    id | id[!A-Za-z0-9_-]*) ;;
    *) continue ;;
    esac
    value="$(trim "${line#id}")"
    case "$value" in
    "="*) ;;
    *) continue ;;
    esac
    value="$(trim "${value#=}")"
    case "$value" in
    '"'*'"'*) ;;
    *) refuse "$DECLARATION gives a [[target]] the id $value, which is not a quoted string" \
        "spell every id as a quoted <registry>:<name>, e.g. id = \"crate:printobserver\"" ;;
    esac
    value="${value#\"}"
    value="${value%%\"*}"
    declared_shape "$value"
    targets="$targets $value"
    [ "$value" = "$identifier" ] && matched=0
done <"$DECLARATION"

if [ -z "$targets" ]; then
    refuse "$PWD/$DECLARATION declares no release target at all" \
        "declare what this repository publishes as a [[target]] table with an id of <registry>:<name>"
fi
if [ "$matched" -ne 0 ]; then
    # Not answered rather than empty: an identifier this repository does not
    # release says nothing about whether anything was released.
    refuse "'$identifier' is not a release target this repository declares" \
        "ask for one of:$targets — or, if this repository has started publishing it, declare it as a [[target]] in $DECLARATION"
fi
name="${identifier#*:}"

# Where a registry's body is written, byte for byte. A file rather than a
# variable: bash drops every NUL byte a command substitution carries, which
# could turn a malformed body into a valid one before the parser read it.
BODY_FILE="$(mktemp)" || refuse "mktemp could not create a file to read a registry's body into" \
    "free space in, or permission on, the temporary directory, and re-ask"
trap 'rm -f -- "$BODY_FILE"' EXIT

# One registry read: the body into BODY_FILE, its HTTP status into STATUS, and
# what the write-out format in REPORT says into BODY — which is how a redirect
# is read unfollowed. Extra curl options go before the URL.
BODY=""
STATUS=""
REPORT=""
fetch() {
    local url="${*: -1}" out rc=0
    out="$(curl -q -sS --connect-timeout "$CONNECT_TIMEOUT" --max-time "$MAX_TIME" \
        -A "$USER_AGENT" -o "$BODY_FILE" -w "$REPORT"'\n%{http_code}' \
        "${@:1:$#-1}" -- "$url")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        refuse "curl exited $rc reading $url, so no registry answered (its diagnostic is above)" \
            "re-ask once the registry is reachable; nothing about a release has been established"
    fi
    STATUS="${out##*$'\n'}"
    BODY="${out%$'\n'*}"
    case "$STATUS" in
    [0-9][0-9][0-9]) ;;
    *) refuse "curl reported '$STATUS' rather than an HTTP status for $url" "$RE_ASK" ;;
    esac
}

# The parse of a registry's body, as a program for `python3 -I`: its
# arguments are the file holding the body, then the path of keys to one field.
# The whole document is parsed as strict JSON — UTF-8, no trailing text, no
# NaN or Infinity, and no object naming one key twice, since a duplicate is a
# document two readers can read two ways — and the field is selected at that
# path alone. It prints `string <value>` or `null`, and exits 1 for a document
# that does not parse or holds no string or null at that path — or a string
# carrying a control character, which a shell would mangle on its way out — or
# one nested deeper than `MAX_DEPTH`: how deep the parser itself reaches before
# refusing depends on the interpreter's stack, so the bound is stated here.
readonly READ_FIELD='
import json, sys

def unique(pairs):
    keys = [key for key, _ in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError("an object names one key twice")
    return dict(pairs)

def refuse_constant(name):
    raise ValueError(name)

try:
    node = json.loads(
        open(sys.argv[1], "rb").read().decode("utf-8"),
        object_pairs_hook=unique,
        parse_constant=refuse_constant,
    )
except (ValueError, RecursionError):
    sys.exit(1)
MAX_DEPTH = 64
level = [(node, 1)]
while level:
    value, depth = level.pop()
    if depth > MAX_DEPTH:
        sys.exit(1)
    if isinstance(value, dict):
        level.extend((child, depth + 1) for child in value.values())
    elif isinstance(value, list):
        level.extend((child, depth + 1) for child in value)
for key in sys.argv[2:]:
    if not isinstance(node, dict) or key not in node:
        sys.exit(1)
    node = node[key]
if node is None:
    print("null")
elif isinstance(node, str) and not any(ord(c) < 32 or ord(c) == 127 for c in node):
    print("string " + node)
else:
    sys.exit(1)
'

# The one value the fetched body holds at the path of keys given: its JSON type
# in KIND, `string` or `null`, and a string's contents in FIELD — two variables,
# so the string "null" is never read as JSON's null. Returns 1 for a body that
# is not one valid JSON document holding a string or null at exactly that path.
KIND=""
FIELD=""
json_field() {
    local read
    read="$(python3 -I -c "$READ_FIELD" "$BODY_FILE" "$@")" || return 1
    case "$read" in
    null) KIND=null && FIELD="" ;;
    "string "*) KIND=string && FIELD="${read#string }" ;;
    *) return 1 ;;
    esac
}

# A semantic version (semver.org 2.0.0) — what the caller compares an answer
# as, so anything else would be a release it cannot order. A prerelease
# identifier is numeric with no leading zero, or alphanumeric.
readonly SEMVER_IDENTIFIER='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
readonly SEMVER="^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-${SEMVER_IDENTIFIER}(\\.${SEMVER_IDENTIFIER})*)?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?\$"

# The answer, held closed on its way out: what a caller reads on stdout is read
# as a released version, so anything that is not one is not answered at all.
answer() {
    [[ "$1" =~ $SEMVER ]] || refuse \
        "the registry answered '$1' for $identifier, which is not a semantic version" "$RE_ASK"
    printf '%s\n' "$1"
    exit 0
}

unreadable() {
    refuse "$1 answered $STATUS for $identifier, so it cannot say what is released" "$RE_ASK"
}

unread_body() {
    refuse "$1 answered 200 for $identifier without one readable $2" "$RE_ASK"
}

case "$identifier" in
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
crate:*)
    fetch "https://crates.io/api/v1/crates/$name"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "crates.io" ;;
    esac
    json_field crate max_stable_version || unread_body "crates.io" "crate.max_stable_version"
    # Every version yanked or a prerelease: the registry serves nothing.
    [ "$KIND" = null ] && exit 0
    answer "$FIELD"
    ;;
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
pypi:*)
    fetch "https://pypi.org/pypi/$name/json"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "PyPI" ;;
    esac
    json_field info version && [ "$KIND" = string ] || unread_body "PyPI" "info.version"
    answer "$FIELD"
    ;;
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
npm:*)
    fetch "https://registry.npmjs.org/${name/\//%2f}/latest"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "npm" ;;
    esac
    json_field version && [ "$KIND" = string ] || unread_body "npm" "version"
    answer "$FIELD"
    ;;
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
release:*)
    releases="https://github.com/$GITHUB_OWNER/$name/releases"
    # Not followed: where github.com sends the request is the answer.
    REPORT='%{redirect_url}'
    fetch "$releases/latest"
    REPORT=""
    case "$STATUS" in
    301 | 302 | 303 | 307 | 308) ;;
    *) unreadable "GitHub" ;;
    esac
    location="$BODY"
    case "$location" in
    "$releases" | "$releases/") exit 0 ;;
    "$releases/tag/v"*) tag="${location#"$releases/tag/"}" ;;
    *) refuse "GitHub sent $releases/latest to '$location', which is neither a release tag nor the releases page" "$RE_ASK" ;;
    esac
    version="${tag#v}"
    [[ "$version" =~ $SEMVER ]] || refuse \
        "GitHub's latest release of $identifier is tagged '$tag', which is not v<version>" "$RE_ASK"
    # The checksum file is a release asset; github.com answers its download
    # with a redirect to the asset store when it is there, and 404 when not.
    fetch -I "$releases/download/$tag/$CHECKSUMS"
    case "$STATUS" in
    200 | 302) ;;
    404) refuse "GitHub's release $tag of $identifier carries no $CHECKSUMS yet, so scripts/install.sh would refuse every artifact in it" \
        "re-ask once that release's artifacts and $CHECKSUMS are uploaded; nothing a consumer can install has been established" ;;
    *) unreadable "GitHub's $CHECKSUMS download" ;;
    esac
    answer "$version"
    ;;
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]
esac
