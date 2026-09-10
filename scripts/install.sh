#!/bin/sh
# Put the printobserver program on your path, already built for your platform.
#
# The third of the three ALTERNATIVE routes AGENTS.md's "The end-user install
# path" section states; `just check-repo` refuses a tree in which this path
# differs from what that section's fetch URL names. It is the route for a
# machine that has neither package manager — nothing here needs Python, Node or
# a Rust toolchain, and nothing here compiles anything: the host this runs on is
# the small machine beside the printer, which is the worst place to build a Rust
# workspace.
#
# It FAILS CLOSED. Where it cannot work out the platform, cannot download, or
# cannot verify what it downloaded, it stops without installing and says both
# why and what to do next. Verification happens before anything reaches a path:
# the archive is unpacked into a directory of this script's own, checked against
# the release's own checksum file, and only then moved to where you asked for it.
#
# Usage:
#   install.sh [--version vX.Y.Z] [--to DIR]
#
#   --version  install that release rather than the newest one.
#   --to       install into DIR rather than into ~/.local/bin.
#
# PRINTOBSERVER_RELEASE_BASE points this at somewhere other than the project's
# own releases — a mirror, or a directory holding a release. It takes the same
# two shapes the forge serves: `<base>/latest/download/<asset>` for the newest
# release and `<base>/download/<tag>/<asset>` for a pinned one.
set -eu

PROGRAM="printobserver"
OWNER="nickderobertis"
REPOSITORY="printobserver"
CHECKSUMS="SHA256SUMS"
DEFAULT_DIRECTORY="$HOME/.local/bin"
RELEASE_BASE="${PRINTOBSERVER_RELEASE_BASE:-https://github.com/$OWNER/$REPOSITORY/releases}"

VERSION=""
DIRECTORY=""

# Every failure says why it stopped and what to do next. A route that fails
# silently is worse for the user in front of the printer than one that is
# absent, and a route that says only that it failed is barely better.
die() {
    echo "install.sh: $1" >&2
    echo "install.sh: $2" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || die "--version takes a release tag and was given none" \
                "Run it as \`--version v0.1.0\`, or leave it out for the newest release."
            VERSION="$2"
            shift 2
            ;;
        --to)
            [ $# -ge 2 ] || die "--to takes a directory and was given none" \
                "Run it as \`--to ~/.local/bin\`, or leave it out for $DEFAULT_DIRECTORY."
            DIRECTORY="$2"
            shift 2
            ;;
        -h | --help)
            sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            die "\`$1\` is not an option this script takes" \
                "It takes \`--version vX.Y.Z\` and \`--to DIR\`. Run it with --help."
            ;;
    esac
done

[ -n "$DIRECTORY" ] || DIRECTORY="$DEFAULT_DIRECTORY"

# The platform, as the release artifacts name it. A platform this release
# publishes nothing for is a stop rather than a guess: installing a program
# built for another machine is a failure the user meets at the printer.
system="$(uname -s)"
machine="$(uname -m)"
case "$system/$machine" in
    Linux/x86_64) platform="linux-x86_64" ;;
    Linux/aarch64 | Linux/arm64) platform="linux-aarch64" ;;
    *)
        die "this is $system/$machine, which printobserver publishes no program for" \
            "The platforms it publishes for are linux-x86_64 and linux-aarch64. On \
anything else, build it from source with \`cargo install printobserver\`."
        ;;
esac

asset="$PROGRAM-$platform.tar.gz"
if [ -n "$VERSION" ]; then
    from="$RELEASE_BASE/download/$VERSION"
    which="release $VERSION"
else
    from="$RELEASE_BASE/latest/download"
    which="the newest release"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

# One way of obtaining a file, whichever kind of place the release is in. A
# directory is what a test, a mirror on a shared filesystem and an air-gapped
# install all look like; everything else is fetched.
obtain() {
    case "$from" in
        /* | file://*)
            source_path="${from#file://}"
            [ -f "$source_path/$1" ] || return 1
            cp "$source_path/$1" "$2"
            ;;
        *)
            curl -fsSL --retry 2 "$from/$1" -o "$2"
            ;;
    esac
}

obtain "$asset" "$work/$asset" || die \
    "$which has no $asset to download from $from" \
    "Check that $which publishes a program for $platform, or pass \`--version\` a \
release that does."

obtain "$CHECKSUMS" "$work/$CHECKSUMS" || die \
    "$which publishes no $CHECKSUMS, so what was downloaded cannot be verified" \
    "Nothing was installed. Check that $from serves $CHECKSUMS beside its artifacts."

# What the release says the artifact's digest is, and what it actually is. A
# mismatch stops here, with the download still in this script's own directory
# and nothing on any path.
expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1 }' \
    "$work/$CHECKSUMS" | head -n 1)"
[ -n "$expected" ] || die \
    "$CHECKSUMS names no digest for $asset, so what was downloaded cannot be verified" \
    "Nothing was installed. Check that $which published $asset and its digest together."

if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$work/$asset" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$work/$asset" | cut -d' ' -f1)"
else
    die "this machine has neither sha256sum nor shasum, so nothing here can verify \
what it downloaded" \
        "Nothing was installed. Install coreutils, or download $asset and verify it \
against $from/$CHECKSUMS by hand."
fi

[ "$actual" = "$expected" ] || die \
    "$asset does not match the digest $which published for it (expected $expected, \
got $actual)" \
    "Nothing was installed. The download was altered or is incomplete; try again, \
and if it happens twice report it rather than installing it."

tar -xzf "$work/$asset" -C "$work" || die \
    "$asset could not be unpacked" \
    "Nothing was installed. The download may be incomplete; try again."

[ -f "$work/$PROGRAM" ] || die \
    "$asset carries no $PROGRAM program" \
    "Nothing was installed. Report this against $which: the artifact is not the one \
this script installs."

mkdir -p "$DIRECTORY" || die \
    "$DIRECTORY could not be created" \
    "Nothing was installed. Pass \`--to\` a directory you can write to."

chmod 0755 "$work/$PROGRAM"
mv -f "$work/$PROGRAM" "$DIRECTORY/$PROGRAM" || die \
    "$PROGRAM could not be put in $DIRECTORY" \
    "Nothing was installed. Pass \`--to\` a directory you can write to."

echo "install.sh: installed $DIRECTORY/$PROGRAM from $which" >&2
case ":$PATH:" in
    *":$DIRECTORY:"*) ;;
    *)
        echo "install.sh: $DIRECTORY is not on your PATH. Add it, or run \
$DIRECTORY/$PROGRAM by its whole name." >&2
        ;;
esac
echo "install.sh: next, put the service in place and then start it:" >&2
echo "  curl -fsSL https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/install-service.sh | sudo sh" >&2
echo "  sudo systemctl enable --now $PROGRAM.service" >&2
