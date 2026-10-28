#!/bin/sh
# Move an isolated install to the invoking sudo user's own account.
# Usage: sudo sh scripts/migrate-service-to-per-user.sh [--root DIR]
# --root stages both the system layout and the account's home below DIR.
# Originals and the service account are retained for inspection and rollback.
set -eu
UNIT_NAME="printobserver.service"
LAUNCHD_LABEL="io.github.nickderobertis.printobserver"
ROOT=""
die() {
    echo "migrate-service-to-per-user.sh: $1" >&2
    echo "migrate-service-to-per-user.sh: ${2:-Correct the reported input, then rerun sudo sh scripts/migrate-service-to-per-user.sh from the operator account. Run it with --help for supported options.}" >&2
    exit 1
}
while [ "$#" -gt 0 ]; do
    case "$1" in
        --root) [ "$#" -ge 2 ] || die '--root takes a directory' 'Pass --root an existing staged service root, or omit it on the installed host.'; ROOT="$2"; shift 2 ;;
        --help | -h) sed -n '2,5p' "$0"; exit 0 ;;
        *) die "unknown option $1" "Run sh scripts/migrate-service-to-per-user.sh --help for supported options." ;;
    esac
done
[ "$(id -u)" -eq 0 ] || die 'Run this through sudo; nothing was changed.'
[ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ] || die 'SUDO_USER must name the user who will run the server; nothing was changed.' 'Invoke this script through sudo from that user account.'
id "$SUDO_USER" >/dev/null 2>&1 || die 'SUDO_USER is not an account; nothing was changed.' 'Invoke sudo from an existing operator account on this host.'
SYSTEM="$(uname -s)"
case "$SYSTEM" in
    Linux)
        USER_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
        DEFINITION="$ROOT/etc/systemd/system/$UNIT_NAME"
        ;;
    Darwin)
        USER_HOME="$(dscl . -read "/Users/$SUDO_USER" NFSHomeDirectory | sed 's/^NFSHomeDirectory: //')"
        DEFINITION="$ROOT/Library/LaunchDaemons/$LAUNCHD_LABEL.plist"
        ;;
    *) die "unsupported system $SYSTEM; nothing was changed." "Run this migration on Linux or macOS; on Windows use scripts/migrate-service-to-per-user.ps1." ;;
esac
case "$USER_HOME" in /*) ;; *) die 'The account has no absolute home; nothing was changed.' 'Repair the account home in the host account database before retrying.' ;; esac
USER_HOME="$ROOT$USER_HOME"
case "$SYSTEM" in
    Linux) USER_CONFIG="${XDG_CONFIG_HOME:-$USER_HOME/.config}/printobserver/config.toml"; USER_STATE="${XDG_STATE_HOME:-$USER_HOME/.local/state}/printobserver" ;;
    Darwin) USER_CONFIG="$USER_HOME/Library/Application Support/printobserver/config.toml"; USER_STATE="$USER_HOME/Library/Application Support/printobserver/state" ;;
esac
SERVICE_CONFIG="$ROOT/etc/printobserver/config.toml"
SERVICE_STATE="$ROOT/var/lib/printobserver"
for value in "$ROOT" "$USER_HOME" "$USER_CONFIG" "$USER_STATE"; do
    case "$value" in *[\"\'\\]* | *"
"*) die "A path carries a quote, backslash or newline: $value; nothing was changed." 'Use home, XDG and root paths without those characters, then retry.' ;; esac
done
[ -f "$DEFINITION" ] || die 'No system unit or LaunchDaemon is installed; nothing was changed. Install per-user directly with scripts/install.sh.'
[ ! -e "$USER_CONFIG" ] && [ ! -L "$USER_CONFIG" ] || die "$USER_CONFIG already exists; nothing was changed. Move it aside first."
[ ! -e "$USER_STATE" ] && [ ! -L "$USER_STATE" ] || die "$USER_STATE already exists; nothing was changed. Move it aside first."
[ -f "$SERVICE_CONFIG" ] || die "$SERVICE_CONFIG is missing; nothing was changed." 'Restore the service configuration from backup before migrating.'
[ -d "$SERVICE_STATE" ] || die "$SERVICE_STATE is missing; nothing was changed." 'Restore the service state from backup before migrating.'
# No link may redirect the privileged copy into another user's files.
[ -z "$(find "$SERVICE_STATE" -type l -print)" ] || die 'The service state contains symbolic links; nothing was changed. Replace them with regular files first.'
for destination in "$(dirname "$USER_CONFIG")" "$(dirname "$USER_STATE")"; do
    ancestor="$destination"
    while [ "$ancestor" != / ]; do
        [ ! -L "$ancestor" ] || die "$ancestor is a symbolic link; nothing was changed." 'Move that link aside or choose real user-owned XDG directories before retrying.'
        ancestor="$(dirname "$ancestor")"
    done
done
case "$USER_STATE/" in "$SERVICE_STATE/"*) die 'Source and destination overlap; nothing was changed.' "Choose per-user XDG directories outside $SERVICE_STATE, then retry." ;; esac
case "$SERVICE_STATE/" in "$USER_STATE/"*) die 'Source and destination overlap; nothing was changed.' "Choose per-user XDG directories outside $SERVICE_STATE, then retry." ;; esac
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'Install sha256sum or shasum; nothing was changed.'
work="$(mktemp -d)" || die "Temporary staging could not be created; nothing was changed." "Free space in TMPDIR or select a writable TMPDIR, then retry."
trap 'rm -rf "$work"' EXIT INT TERM
# Preserve every byte except the values of these two plain-string settings.
# Requiring the installer layout avoids copying a different record by mistake.
awk -v state="$USER_STATE" -v old="$SERVICE_STATE" -v skill="$USER_STATE/skills/printobserver/SKILL.md" '
    BEGIN { table=""; states=0; skills=0 }
    /^[[:space:]]*\[/ { table=$0; sub(/^[[:space:]]*\[/,"",table); sub(/\].*$/,"",table) }
    (table == "" && /^[[:space:]]*state_dir[[:space:]]*=/) || (table == "supervisor" && /^[[:space:]]*skill_path[[:space:]]*=/) {
        line=$0; lead=line; sub(/=.*/,"=",lead); tail=substr(line,length(lead)+1)
        spaces=tail; sub(/[^[:space:]].*$/,"",spaces); tail=substr(tail,length(spaces)+1)
        quote=substr(tail,1,1)
        if (quote != "\042" && quote != "\047") exit 1
        rest=substr(tail,2); end=index(rest,quote); if (!end) exit 1
        value=substr(rest,1,end-1); suffix=substr(rest,end+1)
        if (suffix !~ /^[[:space:]]*(#.*)?$/ || value ~ /\\/) exit 1
        if (table == "") { if (value != old) exit 1; replacement=state; states++ }
        else { replacement=skill; skills++ }
        $0=lead spaces quote replacement quote suffix
    }
    { print }
    END { if (states != 1 || skills != 1) exit 1 }
' "$SERVICE_CONFIG" >"$work/config.toml" || die 'state_dir or supervisor.skill_path is not the service installer layout; nothing was changed. Restore those settings before migrating.'
# awk preserves each line but supplies a final newline. Keep the original's
# absence of that byte too; comments and all other bytes belong to the user.
if [ -n "$(tail -c 1 "$SERVICE_CONFIG")" ]; then
    length="$(wc -c <"$work/config.toml")"
    dd if="$work/config.toml" of="$work/without-newline" bs=1 count="$((length - 1))" 2>/dev/null || die "Configuration staging failed; nothing was changed." "Free space in TMPDIR and retry."
    mv "$work/without-newline" "$work/config.toml" || die "Configuration staging failed; nothing was changed." "Check TMPDIR permissions and retry."
fi
# Stop and wait before copying SQLite, including its WAL and SHM companions.
case "$SYSTEM" in
    Linux) systemctl stop "$UNIT_NAME" || die "The system unit could not be stopped; nothing was copied." "Run systemctl status $UNIT_NAME as administrator, resolve its reported error and retry." ;;
    Darwin) launchctl bootout "system/$LAUNCHD_LABEL" || die "The LaunchDaemon could not be stopped; nothing was copied." "Run launchctl print system/$LAUNCHD_LABEL as administrator, resolve its reported error and retry." ;;
esac
created=false
cleanup_copy() {
    if [ "$created" = true ]; then rm -rf "$USER_STATE"; rm -f "$USER_CONFIG"; fi
}
copy_failed() {
    cleanup_copy
    die 'The copy failed or did not verify. The service is stopped and still installed; originals are untouched. Restart the system service before retrying.'
}
umask 077
# New parent directories must belong to the operator too: sudo's umask would
# otherwise leave an inaccessible root-owned .local/state above the copy.
make_parent() {
    if [ -d "$1" ]; then return; fi
    make_parent "$(dirname "$1")" || return 1
    mkdir "$1" || return 1
    chown "$SUDO_USER:$(id -gn "$SUDO_USER")" "$1"
}
make_parent "$(dirname "$USER_CONFIG")" || copy_failed
make_parent "$(dirname "$USER_STATE")" || copy_failed
created=true
mkdir "$USER_STATE" || copy_failed
cp -Rp "$SERVICE_STATE/." "$USER_STATE/" || copy_failed
cp "$work/config.toml" "$USER_CONFIG" || copy_failed
chmod 0600 "$USER_CONFIG" || copy_failed
chmod 0700 "$USER_STATE" || copy_failed
chown -R "$SUDO_USER:$(id -gn "$SUDO_USER")" "$USER_STATE" "$(dirname "$USER_CONFIG")" || copy_failed
# Compare a sorted manifest of every directory and every file's SHA-256.
manifest() (
    cd "$1" || exit 1
    [ -z "$(find . -type l -print)" ] || exit 1
    find . -type d -print >"$work/directories" || exit 1
    LC_ALL=C sort "$work/directories" || exit 1
    find . -type f -exec sh -c '
        for file do
            if command -v sha256sum >/dev/null 2>&1; then sha256sum "$file" || exit 1
            else shasum -a 256 "$file" || exit 1; fi
        done
    ' sh {} + >"$work/files" || exit 1
    LC_ALL=C sort "$work/files"
)
manifest "$SERVICE_STATE" >"$work/original.sha256" || copy_failed
manifest "$USER_STATE" >"$work/copy.sha256" || copy_failed
cmp -s "$work/original.sha256" "$work/copy.sha256" || copy_failed
cmp -s "$work/config.toml" "$USER_CONFIG" || copy_failed
# Removing the system registration is the last mutation, after verification.
case "$SYSTEM" in
    Linux) systemctl disable "$UNIT_NAME" || die "The copy verified but the system unit could not be disabled." "Disable $UNIT_NAME as administrator before installing the per-user server; both copies remain." ;;
esac
rm "$DEFINITION" || die "The copy verified but $DEFINITION could not be removed." "Remove this system registration as administrator before installing per-user; both copies remain."
if [ "$SYSTEM" = Linux ]; then systemctl daemon-reload || die "The system registration was removed, but daemon-reload failed." "Run systemctl daemon-reload as administrator before installing per-user; both copies remain."; fi
# llmlint: ignore[tool_output_is_signal] The migration contract requires unprivileged install/sign-in steps and instructions for removing the deliberately retained originals; each following line is an actionable part of that report.
printf '%s\n' "Migration verified. As $SUDO_USER, without sudo:" \
    '  curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh' \
    '  printobserver sign-in' \
    "Retained $ROOT/etc/printobserver, $SERVICE_STATE, $ROOT/usr/local/lib/printobserver and the printobserver service user." \
    "Once satisfied, remove the retained directories with sudo rm -rf, and remove the service account with userdel printobserver (Linux) or dscl . -delete /Users/printobserver (macOS)." >&2
