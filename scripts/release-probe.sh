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
#   * crate:   crates.io's crate document, `max_stable_version` — what `cargo
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
# file from its own location rather than from `$PWD`, because a probe answering
# about whatever repository it happened to be started in answers about the wrong
# artifact. It reads the declaration line by line rather than parsing TOML,
# because bash has no TOML reader and the shape it needs is one key in one kind
# of table; what the document *is* is held by onevcs's own reader, which `just
# check-repo`'s `release-declaration` runs over it. What is strict here is the
# identifier, because that is the one value this script turns into a URL.
#
# What it may assume, and nothing beyond it: it is spawned as a direct
# subprocess from the repository root, with an environment carrying only PATH
# and HOME and no credential of any kind. Every target is on a public registry,
# so an unauthenticated read is all it needs. `curl -q` follows from the same
# rule — a ~/.curlrc is the caller's configuration and not this probe's.
#
# `curl` and `grep` are the whole of what it runs beyond bash's own builtins.
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
# The forge account a `release:<name>` target is published under — the owner
# `scripts/install.sh` downloads from — and the file that script verifies.
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
cd -- "$probe_dir/.." 2>/dev/null || refuse \
    "cannot reach the repository root from ${BASH_SOURCE[0]}" \
    "run this script from a checkout of the repository, at scripts/release-probe.sh"
readonly DECLARATION="release-targets.toml"

if [ $# -ne 1 ]; then
    refuse "takes exactly one registry-qualified identifier, and was given $#" \
        "run 'release-probe.sh <registry>:<name>', e.g. 'release-probe.sh release:printobserver'" 2
fi
identifier="$1"

for tool in curl grep; do
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

# One registry read: the body, then its HTTP status, in BODY and STATUS. Extra
# curl options go before the URL; with `-o /dev/null`, BODY is instead what the
# write-out format in REPORT says, which is how a redirect is read unfollowed.
BODY=""
STATUS=""
REPORT=""
fetch() {
    local url="${*: -1}" out rc=0
    out="$(curl -q -sS --connect-timeout "$CONNECT_TIMEOUT" --max-time "$MAX_TIME" \
        -A "$USER_AGENT" -w "$REPORT"'\n%{http_code}' "${@:1:$#-1}" -- "$url")" || rc=$?
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

# The one value the fetched body gives for a string key, in FIELD. Matched as
# `,"key":"` or `{"key":"`, so a longer key ending in the one asked for — PyPI's
# `python_version`, npm's `_npmVersion` — cannot answer for it. Returns 1 when
# the body names it nowhere and 2 when it names it more than once with
# different values: a document that is not the shape this script reads.
FIELD=""
json_string() {
    local matches line value found="" seen=1
    matches="$(printf '%s' "$BODY" | grep -o "[,{]\"$1\":\"[^\"]*\"")" || matches=""
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        value="${line#*\":\"}"
        value="${value%\"}"
        if [ "$seen" -eq 0 ] && [ "$value" != "$found" ]; then
            return 2
        fi
        found="$value"
        seen=0
    done <<<"$matches"
    FIELD="$found"
    return "$seen"
}

# The answer, held closed on its way out: what a caller reads on stdout is read
# as a released version, so anything that is not one is not answered at all.
answer() {
    case "$1" in
    "" | [!0-9]* | *[!0-9A-Za-z.+-]*)
        refuse "the registry answered '$1' for $identifier, which is not a version" "$RE_ASK" ;;
    esac
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
crate:*)
    fetch "https://crates.io/api/v1/crates/$name"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "crates.io" ;;
    esac
    json_string max_stable_version || case "$?" in
    1)
        # Every version yanked or a prerelease: the registry serves nothing.
        if printf '%s' "$BODY" | grep -q '"max_stable_version":null'; then
            exit 0
        fi
        unread_body "crates.io" "max_stable_version"
        ;;
    *) unread_body "crates.io" "max_stable_version" ;;
    esac
    answer "$FIELD"
    ;;
pypi:*)
    fetch "https://pypi.org/pypi/$name/json"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "PyPI" ;;
    esac
    json_string version || unread_body "PyPI" "info.version"
    answer "$FIELD"
    ;;
npm:*)
    fetch "https://registry.npmjs.org/${name/\//%2f}/latest"
    case "$STATUS" in
    200) ;;
    404) exit 0 ;;
    *) unreadable "npm" ;;
    esac
    json_string version || unread_body "npm" "version"
    answer "$FIELD"
    ;;
release:*)
    releases="https://github.com/$GITHUB_OWNER/$name/releases"
    # Not followed: where github.com sends the request is the answer.
    REPORT='%{redirect_url}'
    fetch -o /dev/null "$releases/latest"
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
    case "$version" in
    "" | [!0-9]* | *[!0-9A-Za-z.+-]*)
        refuse "GitHub's latest release of $identifier is tagged '$tag', which is not v<version>" "$RE_ASK" ;;
    esac
    # The checksum file is a release asset; github.com answers its download
    # with a redirect to the asset store when it is there, and 404 when not.
    fetch -I -o /dev/null "$releases/download/$tag/$CHECKSUMS"
    case "$STATUS" in
    200 | 302) ;;
    404) refuse "GitHub's release $tag of $identifier carries no $CHECKSUMS yet, so scripts/install.sh would refuse every artifact in it" \
        "re-ask once that release's artifacts and $CHECKSUMS are uploaded; nothing a consumer can install has been established" ;;
    *) unreadable "GitHub's $CHECKSUMS download" ;;
    esac
    answer "$version"
    ;;
esac
