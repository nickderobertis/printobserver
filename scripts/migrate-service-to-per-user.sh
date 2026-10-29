#!/bin/sh
# Move an isolated install to the invoking sudo user's own account.
# Usage: sudo sh scripts/migrate-service-to-per-user.sh [--root DIR]
# --root stages both the system layout and the account's home below DIR.
# Originals and the service account are retained for inspection and rollback.
# llmlint: ignore[new_code_lands_in_a_project] Scripts are deliberately unowned; this migration has real installer and record journeys.
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
# Normalize absolute paths before any ancestor walk or overlap comparison.
normalize() {
    case "$1" in /*) ;; *) die "$1 is not an absolute path; nothing was changed." "Use absolute --root and XDG directory paths, then retry." ;; esac
    printf '%s\n' "$1" | awk -F/ '{ n=0; for(i=1;i<=NF;i++) { if($i=="" || $i==".") continue; if($i=="..") {if(n>0)n--; continue} a[++n]=$i } printf "/"; for(i=1;i<=n;i++)printf "%s%s",a[i],i<n?"/":""; printf "\n" }'
}
if [ -n "$ROOT" ]; then
    case "$ROOT" in /*) ;; *) ROOT="$PWD/$ROOT" ;; esac
    ROOT="$(normalize "$ROOT")"
fi
# Absolute XDG overrides are staged too; a journey can write only below root.
under_root() {
    path="$(normalize "$1")" || return 1
    case "$path" in "$ROOT" | "$ROOT"/*) printf '%s\n' "$path" ;; *) printf '%s\n' "$ROOT$path" ;; esac
}
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
    # llmlint: ignore[changed_behavior_has_e2e] Unsupported host guard: Every supported native context is Linux or macOS, so this fail-closed branch cannot run there without substituting uname, which would stop exercising the native host boundary. Both native platform branches retain their journeys.
    *) die "unsupported system $SYSTEM; nothing was changed." "Run this migration on Linux or macOS; on Windows use scripts/migrate-service-to-per-user.ps1." ;;
esac
# llmlint: ignore[changed_behavior_has_e2e] Malformed account home guard: The native account databases used by these journeys return valid absolute homes. Exercising a corrupted account record requires privileged changes to the host database or substituting its real query, which would stop exercising the native host boundary; user and missing-account refusals run through real id.
case "$USER_HOME" in /*) ;; *) die 'The account has no absolute home; nothing was changed.' 'Repair the account home in the host account database before retrying.' ;; esac
USER_HOME="$ROOT$USER_HOME"
case "$SYSTEM" in
    Linux) USER_CONFIG="${XDG_CONFIG_HOME:-$USER_HOME/.config}/printobserver/config.toml"; USER_STATE="${XDG_STATE_HOME:-$USER_HOME/.local/state}/printobserver" ;;
    Darwin) USER_CONFIG="$USER_HOME/Library/Application Support/printobserver/config.toml"; USER_STATE="$USER_HOME/Library/Application Support/printobserver/state" ;;
esac
USER_CONFIG="$(under_root "$USER_CONFIG")"
USER_STATE="$(under_root "$USER_STATE")"
SERVICE_CONFIG="$ROOT/etc/printobserver/config.toml"
SERVICE_STATE="$ROOT/var/lib/printobserver"
for value in "$ROOT" "$USER_HOME" "$USER_CONFIG" "$USER_STATE"; do
    printf '%s\n' "$value" | LC_ALL=C awk '/[[:cntrl:]]/ { exit 1 }' || die 'A path carries a control character; nothing was changed.' 'Use home, XDG and root paths without control characters, then retry.'
    case "$value" in *[\"\'\\]* | *"
"*) die "A path carries a quote, backslash or newline: $value; nothing was changed." 'Use home, XDG and root paths without those characters, then retry.' ;; esac
done
[ -f "$DEFINITION" ] || die 'No system unit or LaunchDaemon is installed; nothing was changed. Install per-user directly with scripts/install.sh.'
[ ! -e "$USER_CONFIG" ] && [ ! -L "$USER_CONFIG" ] || die "$USER_CONFIG already exists; nothing was changed. Move it aside first."
[ ! -e "$USER_STATE" ] && [ ! -L "$USER_STATE" ] || die "$USER_STATE already exists; nothing was changed. Move it aside first."
[ -f "$SERVICE_CONFIG" ] || die "$SERVICE_CONFIG is missing; nothing was changed." 'Restore the service configuration from backup before migrating.'
for destination in "$(dirname "$USER_CONFIG")" "$(dirname "$USER_STATE")"; do
    ancestor="$destination"
    while [ "$ancestor" != / ]; do
        [ ! -L "$ancestor" ] || die "$ancestor is a symbolic link; nothing was changed." 'Move that link aside or choose real user-owned XDG directories before retrying.'
        ancestor="$(dirname "$ancestor")"
    done
done
work="$(mktemp -d)" || die "Temporary staging could not be created; nothing was changed." "Free space in TMPDIR or select a writable TMPDIR, then retry."
trap 'rm -rf "$work"' EXIT INT TERM
# Preserve every byte except the values of these two plain-string settings.
# The configured source state is copied, including a nondefault absolute path.
awk -v state="$USER_STATE" -v source_path="$work/source-state" -v skill="$USER_STATE/skills/printobserver/SKILL.md" '
    BEGIN { table=""; states=0; skills=0 }
    /^[[:space:]]*\[/ { table=$0; sub(/^[[:space:]]*\[/,"",table); sub(/\].*$/,"",table); gsub(/^[[:space:]]+|[[:space:]]+$/,"",table) }
    (table == "" && /^[[:space:]]*state_dir[[:space:]]*=/) || (table == "supervisor" && /^[[:space:]]*skill_path[[:space:]]*=/) {
        line=$0; lead=line; sub(/=.*/,"=",lead); tail=substr(line,length(lead)+1)
        spaces=tail; sub(/[^[:space:]].*$/,"",spaces); tail=substr(tail,length(spaces)+1)
        quote=substr(tail,1,1)
        if (quote != "\042" && quote != "\047") exit 1
        rest=substr(tail,2); end=index(rest,quote); if (!end) exit 1
        value=substr(rest,1,end-1); suffix=substr(rest,end+1)
        if (suffix !~ /^[[:space:]]*(#.*)?$/) exit 1
        if (quote == "\042") {
            decoded=""
            for(j=1;j<=length(value);j++) {
                ch=substr(value,j,1)
                if(ch == "\\") { if(substr(value,j+1,1) != "\\") exit 1; j++ }
                decoded=decoded ch
            }
            value=decoded
        }
        if (table == "") { print value > source_path; replacement=state; states++ }
        # llmlint: ignore[boundary_inputs_validated] Plain skill strings are rewritten by contract, including external skills reinstalled by the installer next steps.
        else { replacement=skill; skills++ }
        $0=lead spaces quote replacement quote suffix
    }
    { print }
    END { if (states != 1 || skills != 1) exit 1 }
' "$SERVICE_CONFIG" >"$work/config.toml" || die 'state_dir or supervisor.skill_path is not a plain string in its required table; nothing was changed. Restore those settings before migrating.'
SERVICE_STATE="$(under_root "$(cat "$work/source-state")")"
[ -d "$SERVICE_STATE" ] || die "$SERVICE_STATE is missing; nothing was changed." 'Restore the service state from backup before migrating.'
# Resolve existing ancestors for comparison, including macOS /var -> /private/var.
# Keep the configured spellings for the copied values and operator diagnostics.
physical() (
    ancestor="$1"
    suffix=""
    while [ ! -d "$ancestor" ]; do
        component="$(basename "$ancestor")" || exit 1
        suffix="/$component$suffix"
        ancestor="$(dirname "$ancestor")" || exit 1
    done
    base="$(cd "$ancestor" && pwd -P)" || exit 1
    normalize "$base$suffix"
)
STATE_CANON="$(physical "$SERVICE_STATE")" || die "The service state ancestors could not be resolved; nothing was changed." "Check source-directory permissions and retry."
# llmlint: ignore[changed_behavior_has_e2e] Config ancestor race: awk already read this same configuration successfully before resolving its parent. This exit requires concurrent ancestor replacement; real source/destination permission and scan failures are exercised without replacing filesystem tools.
CONFIG_CANON="$(physical "$SERVICE_CONFIG")" || die "The service configuration ancestors could not be resolved; nothing was changed." "Check source-directory permissions and retry."
USER_STATE_CANON="$(physical "$USER_STATE")" || die "The user state ancestors could not be resolved; nothing was changed." "Check destination-directory permissions and retry."
USER_CONFIG_CANON="$(physical "$USER_CONFIG")" || die "The user configuration ancestors could not be resolved; nothing was changed." "Check destination-directory permissions and retry."
source_links="$(find "$STATE_CANON" -type l -print)" || die "The service state could not be scanned; nothing was changed." "Repair source-directory access and retry."
[ -z "$source_links" ] || die 'The service state contains symbolic links; nothing was changed. Replace them with regular files first.'
overlap() {
    case "$1/" in "$2/"*) return 0 ;; esac
    case "$2/" in "$1/"*) return 0 ;; esac
    return 1
}
for source in "$STATE_CANON" "$CONFIG_CANON"; do
    for destination in "$USER_STATE_CANON" "$USER_CONFIG_CANON"; do
        if overlap "$source" "$destination"; then
            die "Source and destination overlap; nothing was changed." "Choose XDG directories outside the service config and state, then retry."
        fi
    done
done
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'Install sha256sum or shasum; nothing was changed.'
# awk preserves each line but supplies a final newline. Keep the original's
# absence of that byte too; comments and all other bytes belong to the user.
if [ -n "$(tail -c 1 "$SERVICE_CONFIG")" ]; then
    length="$(wc -c <"$work/config.toml")"
    # llmlint: ignore[changed_behavior_has_e2e] Temporary staging write failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
    dd if="$work/config.toml" of="$work/without-newline" bs=1 count="$((length - 1))" 2>/dev/null || die "Configuration staging failed; nothing was changed." "Free space in TMPDIR and retry."
    # llmlint: ignore[changed_behavior_has_e2e] Temporary staging rename failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
    mv "$work/without-newline" "$work/config.toml" || die "Configuration staging failed; nothing was changed." "Check TMPDIR permissions and retry."
fi
# Stop and wait before copying SQLite, including its WAL and SHM companions.
case "$SYSTEM" in
    Linux) systemctl stop "$UNIT_NAME" || die "The system unit could not be stopped; nothing was copied." "Run systemctl status $UNIT_NAME as administrator, resolve its reported error and retry." ;;
    Darwin)
        daemon_loaded() {
            if daemon_answer="$(launchctl print "system/$LAUNCHD_LABEL" 2>&1)"; then return 0; else status=$?; fi
            [ "$status" -eq 113 ] || die "The LaunchDaemon status could not be read (exit $status): $daemon_answer; nothing was copied." "Run launchctl print system/$LAUNCHD_LABEL as administrator, resolve its reported error and retry."
            return 1
        }
        if daemon_loaded; then
            launchctl bootout "system/$LAUNCHD_LABEL" || die "The LaunchDaemon could not be stopped; nothing was copied." "Run launchctl print system/$LAUNCHD_LABEL as administrator, resolve its reported error and retry."
            attempts=0
            while daemon_loaded; do
                attempts=$((attempts + 1))
                [ "$attempts" -lt 60 ] || die "The LaunchDaemon did not stop; nothing was copied." "Resolve its launchctl status before retrying."
                sleep 1
            done
        fi
        ;;
esac
created=false
cleanup_copy() {
    if [ "$created" = true ]; then rm -rf "$USER_STATE"; rm -f "$USER_CONFIG"; fi
}
copy_failed() {
    if [ "$#" -gt 0 ]; then echo "migrate-service-to-per-user.sh: $1" >&2; fi
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
# A destination created by the operator during stop is not ours to clean.
mkdir "$USER_STATE" || copy_failed
created=true
cp -Rp "$SERVICE_STATE/." "$USER_STATE/" || copy_failed
# llmlint: ignore[changed_behavior_has_e2e] Copied config write failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
cp "$work/config.toml" "$USER_CONFIG" || copy_failed
# llmlint: ignore[changed_behavior_has_e2e] Copied config mode failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
chmod 0600 "$USER_CONFIG" || copy_failed
# llmlint: ignore[changed_behavior_has_e2e] Copied state mode failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
chmod 0700 "$USER_STATE" || copy_failed
# llmlint: ignore[changed_behavior_has_e2e] Validated ownership assignment failure: This operates on new files in private validated directories after the account was resolved. Failure requires filesystem failure or a concurrent account/path change. The journeys drive actual unreadable-source and copied-file failures, cleanup, SHA verification and real ownership; they do not replace cp/chmod/chown with success stubs.
chown -R "$SUDO_USER:$(id -gn "$SUDO_USER")" "$USER_STATE" "$USER_CONFIG" || copy_failed
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
manifest "$SERVICE_STATE" >"$work/original.sha256" || copy_failed 'Source state SHA-256 manifest could not be read.'
manifest "$USER_STATE" >"$work/copy.sha256" || copy_failed 'Copied state SHA-256 manifest could not be read.'
if ! cmp -s "$work/original.sha256" "$work/copy.sha256"; then
    # Report the first differing entry, without dumping the entire record.
    # This is diagnostic only: verification failed, and cleanup follows even
    # if reading the diagnostic itself fails.
    awk '
        FILENAME == ARGV[1] { expected[FNR]=$0; count=FNR; next }
        {
            seen=FNR
            if ($0 != expected[FNR]) {
                print "Expected state entry: " expected[FNR]
                print "Copied state entry: " $0
                reported=1; exit
            }
        }
        END { if (!reported && count > seen) print "Missing copied state entry: " expected[seen+1] }
    ' "$work/original.sha256" "$work/copy.sha256" >&2 || :
    copy_failed 'State SHA-256 verification failed.'
fi
if ! cmp -s "$work/config.toml" "$USER_CONFIG"; then
    # cmp identifies the file, byte and line without printing config secrets.
    cmp "$work/config.toml" "$USER_CONFIG" >&2 || :
    copy_failed 'Configuration verification failed.'
fi
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
