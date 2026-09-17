#!/bin/sh
# Put the printobserver service in place, and start nothing.
#
# The second of the three commands of the end-user install path AGENTS.md's "The
# end-user install path" section states; `just check-repo` refuses a tree in
# which this path, or the unit name below, differs from what that section says.
#
# It places FOUR things and no more: the program, the configuration, the state
# directory, and the service definition — a systemd unit on Linux, a launchd
# property list on macOS. It does NOT enable, load or start the service, and it
# must never be changed to — nor given an option that does. This service
# commands a 3D printer, so installing a package must not, as a side effect,
# start a process that can move a machine. Starting is the operator's own
# second command, which this script prints when it is done.
#
# Usage:
#   install-service.sh [--root DIR] [--binary PATH] [--user NAME]
#
#   --root    install beneath DIR rather than beneath /. This is how a test owns
#             a throwaway root; the paths under it are the real ones.
#   --binary  the printobserver program to install. Defaults to the one on PATH,
#             which is what any of the install path's three routes puts there.
#   --user    the user the service runs as. Defaults to a system user named
#             `printobserver` when this runs as root, and to the invoking user
#             otherwise — because a caller who cannot create a user cannot hand
#             the state directory to one either.
#
# Which service manager is written for is read off `uname -s`: Linux is systemd
# and Darwin is launchd, as AGENTS.md's supported-platform list states.
set -eu

UNIT_NAME="printobserver.service"
LAUNCHD_LABEL="io.github.nickderobertis.printobserver"
PROGRAM="printobserver"
SERVICE_USER=""
BINARY=""
ROOT=""

die() {
    echo "install-service.sh: $1" >&2
    exit 1
}

# Every value this script is given is written into the TOML configuration, and
# on Linux into a systemd unit. Those formats do not give this installer a safe
# spelling for a quote, a backslash or a newline, so refuse one before writing a
# document that the program or service manager would read as something else.
plain() {
    [ -n "$2" ] || die "$1 is empty"
    case "$2" in
        *[\"\\]*)
            die "$1 carries a quote or a backslash, which a unit file and a TOML \
document have no escape for. Pass a path without quotes or backslashes."
            ;;
    esac
    case "$2" in
        *"
"*)
            die "$1 carries a newline, which a unit file reads as the end of a setting. \
Pass a path without newlines."
            ;;
    esac
}

# A system user name, as `useradd` and a unit's `User=` take one: a letter or an
# underscore, then letters, digits, underscores and hyphens. The letters are
# spelled out rather than written as a range: macOS's own `sh` matches `[a-z]` by
# the locale's collation, in which capitals fall between the lower-case letters.
LOWER="abcdefghijklmnopqrstuvwxyz"
user_name() {
    case "$1" in
        ["$LOWER"_]*) ;;
        *) die "$1 is not a system user name: it does not begin with a letter or an \
underscore" ;;
    esac
    case "$1" in
        *[!"$LOWER"0123456789_-]*)
            die "$1 is not a system user name: it carries something other than \
letters, digits, underscores and hyphens"
            ;;
    esac
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --root)
            [ "$#" -ge 2 ] || die "--root takes a directory"
            plain "--root" "$2"
            ROOT="$2"
            shift 2
            ;;
        --binary)
            [ "$#" -ge 2 ] || die "--binary takes the path of the $PROGRAM program"
            plain "--binary" "$2"
            BINARY="$2"
            shift 2
            ;;
        --user)
            [ "$#" -ge 2 ] || die "--user takes a user name"
            user_name "$2"
            SERVICE_USER="$2"
            shift 2
            ;;
        --help | -h)
            sed -n '2,29p' "$0"
            exit 0
            ;;
        *)
            die "$1 is not an option of this script"
            ;;
    esac
done

SYSTEM="$(uname -s)" || die "\`uname -s\` failed, so this host's service manager could not be selected. Fix uname, then run the installer again."
case "$SYSTEM" in
    Linux) MANAGER="systemd" ;;
    Darwin) MANAGER="launchd" ;;
    *)
        die "this is $SYSTEM, for which this script writes no service definition. Run it \
on Linux or macOS, or run \`printobserver server --config <file>\` under this system's own \
service manager yourself."
        ;;
esac

if [ -z "$BINARY" ]; then
    BINARY="$(command -v "$PROGRAM" || true)"
fi
[ -n "$BINARY" ] || die "no $PROGRAM program on PATH. Take one of the three routes \
AGENTS.md's install path states, or pass --binary."
plain "the $PROGRAM program on PATH" "$BINARY"
[ -x "$BINARY" ] || die "$BINARY is not executable. Run \`chmod +x $BINARY\`, or pass \
--binary the program one of the install path's three routes put on your path."

# A property list is XML, which reads an ampersand or an angle bracket as markup
# rather than as part of a value. Validate after resolving the default binary,
# so a program found on PATH crosses the same boundary as an explicit one.
if [ "$MANAGER" = "launchd" ]; then
    for given in "$ROOT" "$BINARY"; do
        case "$given" in
            *[\&\<\>]*)
                die "$given carries an ampersand or an angle bracket, which a property \
list reads as markup. Pass --root and --binary paths without them."
                ;;
        esac
    done
fi

if [ -z "$SERVICE_USER" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        SERVICE_USER="$PROGRAM"
    else
        SERVICE_USER="$(id -un)"
    fi
    user_name "$SERVICE_USER"
fi

# The four places, spelled once. Everything below writes into one of them. The
# first three are the same on every platform, so the sign-in command AGENTS.md
# states is one command; the service definition's is its manager's own.
BIN_DIR="$ROOT/usr/local/lib/$PROGRAM"
CONF_DIR="$ROOT/etc/$PROGRAM"
STATE_DIR="$ROOT/var/lib/$PROGRAM"
if [ "$MANAGER" = "launchd" ]; then
    # The directory launchd loads system-wide daemons from at boot.
    DEFINITION_DIR="$ROOT/Library/LaunchDaemons"
    INSTALLED_DEFINITION="$DEFINITION_DIR/$LAUNCHD_LABEL.plist"
    START_COMMAND="sudo launchctl bootstrap system /Library/LaunchDaemons/$LAUNCHD_LABEL.plist"
else
    DEFINITION_DIR="$ROOT/etc/systemd/system"
    INSTALLED_DEFINITION="$DEFINITION_DIR/$UNIT_NAME"
    START_COMMAND="sudo systemctl enable --now $UNIT_NAME"
fi

INSTALLED_BINARY="$BIN_DIR/$PROGRAM"
INSTALLED_CONFIG="$CONF_DIR/config.toml"

# The paths the unit and the configuration name are the ones the SERVICE will
# see, which are the ones without the throwaway root in front of them. A unit
# written under `--root` that named the root would stop working the moment it
# was copied to the machine it describes.
RUNTIME_BINARY="/usr/local/lib/$PROGRAM/$PROGRAM"
RUNTIME_CONFIG="/etc/$PROGRAM/config.toml"
RUNTIME_STATE="/var/lib/$PROGRAM"
if [ -n "$ROOT" ]; then
    RUNTIME_BINARY="$INSTALLED_BINARY"
    RUNTIME_CONFIG="$INSTALLED_CONFIG"
    RUNTIME_STATE="$STATE_DIR"
fi

# The service user's home is inside the state directory, and so is every file
# the supervising agent's harness keeps. The unit hides every home under /home,
# /root and /run/user and lets the service write to the state directory alone,
# so a harness pointed at a home anywhere else could neither sign in nor run.
HOME_DIR="$STATE_DIR/home"
RUNTIME_HOME="$RUNTIME_STATE/home"

# A macOS system user, as `dscl` records one: an id below 500, where macOS keeps
# the users services run as, shared by a group of the same name, hidden from the
# login window and given no shell.
create_launchd_user() {
    users="$(dscl . -list /Users UniqueID)" ||
        die "\`dscl . -list /Users UniqueID\` failed while finding a free system-user id. Fix the reported directory-service error, or pass --user a user that already exists."
    groups="$(dscl . -list /Groups PrimaryGroupID)" ||
        die "\`dscl . -list /Groups PrimaryGroupID\` failed while finding a free system-user id. Fix the reported directory-service error, or pass --user a user that already exists."
    taken="$(printf '%s\n%s\n' "$users" "$groups" | awk '
        NF == 0 { next }
        NF != 2 || $2 !~ /^[0-9]+$/ { exit 1 }
        { print $2 }
    ')" || die "directory service returned a malformed user or group id while finding a free system-user id. Run the two reported \`dscl -list\` commands and repair the record they print, or pass --user a user that already exists."
    number=400
    while printf '%s\n' "$taken" | grep -qx "$number"; do
        number=$((number + 1))
        [ "$number" -lt 500 ] ||
            die "every macOS system-user id from 400 to 499 is already taken, so there is no free one for $1. Remove a user or group that no longer needs its id, or pass --user a user that already exists."
    done
    dscl_create "/Groups/$1" PrimaryGroupID "$number"
    dscl_create "/Groups/$1" Password '*'
    dscl_create "/Users/$1" UniqueID "$number"
    dscl_create "/Users/$1" PrimaryGroupID "$number"
    dscl_create "/Users/$1" UserShell /usr/bin/false
    dscl_create "/Users/$1" NFSHomeDirectory "$RUNTIME_HOME"
    dscl_create "/Users/$1" RealName "$PROGRAM service"
    dscl_create "/Users/$1" IsHidden 1
    dscl_create "/Users/$1" Password '*'
}

dscl_create() {
    record="$1"
    attribute="$2"
    value="$3"
    dscl . -create "$record" "$attribute" "$value" ||
        die "\`dscl . -create $record $attribute\` failed while creating the system user. Fix the reported directory-service error, or pass --user a user that already exists."
}

if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    [ "$(id -u)" -eq 0 ] || die "there is no user $SERVICE_USER and this is not root"
    if [ "$MANAGER" = "launchd" ]; then
        create_launchd_user "$SERVICE_USER" ||
            die "the system user $SERVICE_USER could not be created. Create it yourself \
with \`dscl\`, or pass --user a user that already exists."
    else
        useradd --system --no-create-home --home-dir "$RUNTIME_HOME" \
            --shell /usr/sbin/nologin "$SERVICE_USER" ||
            die "the system user $SERVICE_USER could not be created. Create it yourself \
(\`useradd --system $SERVICE_USER\`), or pass --user a user that already exists."
    fi
fi

# What every failed write here says, so that the two heredocs below can name it
# without a line continuation inside a quoted string, which reads as a syntax
# error to a shell linter.
WRITE_REFUSED="a file could not be written under $ROOT/. Run this as root, or pass \
--root a directory you can write to."

mkdir -p "$BIN_DIR" "$CONF_DIR" "$DEFINITION_DIR" ||
    die "$ROOT/ could not be made writable for the install. Run this as root, or pass \
--root a directory you own."

install -m 0755 "$BINARY" "$INSTALLED_BINARY" ||
    die "$BINARY could not be copied to $INSTALLED_BINARY. Run this as root, or pass \
--root a directory you can write to."

# 0700 and owned by the service's own user: this directory holds the whole
# record of what a printer did and what an agent decided, including the
# snapshots. Nothing else on the machine has any business reading it.
mkdir -p "$STATE_DIR" ||
    die "$STATE_DIR could not be created. Run this as root, or pass --root a directory \
you can write to."
chown "$SERVICE_USER" "$STATE_DIR" ||
    die "$STATE_DIR could not be handed to $SERVICE_USER. Run this as root, or pass \
--user the user running this script."
chmod 0700 "$STATE_DIR" ||
    die "$STATE_DIR could not be made private. Run this as root, or pass --root a \
directory you own."
mkdir -p "$HOME_DIR" ||
    die "$HOME_DIR could not be created. Run this as root, or pass --root a directory \
you own."
chown "$SERVICE_USER" "$HOME_DIR" ||
    die "$HOME_DIR could not be handed to $SERVICE_USER. Run this as root."
chmod 0700 "$HOME_DIR" ||
    die "$HOME_DIR could not be made private. Run this as root."

# An existing configuration is left exactly as it is: a reinstall must not
# overwrite the operator's own values with a template's.
if [ -e "$INSTALLED_CONFIG" ]; then
    # llmlint: ignore[tool_output_is_signal] suppressions.toml has the reason.
    echo "install-service.sh: $INSTALLED_CONFIG is already there and was left alone" >&2
else
    cat >"$INSTALLED_CONFIG" <<CONFIG || die "$WRITE_REFUSED"
# printobserver's one configuration file.
#
# Every value below is validated when the service starts, and a value that
# cannot work is refused naming this file's own field for it. Fill in the three
# marked FILL IN before enabling the service.

state_dir = "$RUNTIME_STATE"
listen = "127.0.0.1:8420"

[octoprint]
# FILL IN: where your OctoPrint answers, and the API key it authenticates by.
url = "http://127.0.0.1:5000"
api_key = ""
# "commandable" if the machine has a part-cooling fan this service may command,
# "absent" if it has none.
fan = "commandable"

[supervisor]
# The harness identity supervision turns run on.
harness = "claude-code"

[ingress]
# FILL IN: the shared secret Obico's webhook notification plugin must carry.
# Anything that can post to the ingress can pause a printer.
shared_secret = ""
# The answer bound is left out on purpose. The default this program ships is
# already below the timeout Obico posts under, and writing that number here
# would be a second copy of it to keep right; see AGENTS.md, "The Obico ingress
# answer bound".

# The API credential every client presents is not written in this file. Left
# out, the service generates one the first time it starts, into the file
# api-credential in its state directory, readable by the service's user alone,
# and writes it with the address into client.toml beside it for the clients on
# this host. To choose the credential yourself, add an [api] table whose
# credential key holds a long random value; the generated file is then neither
# read nor written.

# What any actor may ask for at all. A manifest may narrow these; nothing may
# widen them.
[safety]
agent_min_interval_s = 30

[safety.allowed]
feedrate = { min = 0.5, max = 1.5 }
flowrate = { min = 0.9, max = 1.1 }
fan = { min = 0.0, max = 100.0 }
bed_target = { min = 0.0, max = 110.0 }
"tool_target:0" = { min = 0.0, max = 260.0 }

[safety.actions]
operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
            "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
            "set_fan_percent", "acknowledge_failure"]
agent = ["pause", "set_feedrate_factor", "set_flowrate_factor",
         "set_fan_percent", "acknowledge_failure"]
system = ["set_feedrate_factor", "set_flowrate_factor", "set_tool_target_c",
          "set_bed_target_c", "set_fan_percent"]
CONFIG
    chown "$SERVICE_USER" "$INSTALLED_CONFIG" ||
        die "$INSTALLED_CONFIG could not be handed to $SERVICE_USER. Run this as root."
    chmod 0600 "$INSTALLED_CONFIG" ||
        die "$INSTALLED_CONFIG could not be made private. Run this as root."
fi

if [ "$MANAGER" = "launchd" ]; then
    # RunAtLoad starts it when launchd loads it, which for a property list in
    # /Library/LaunchDaemons is at every boot; KeepAlive with SuccessfulExit false
    # starts it again whenever it ends other than successfully, which a process
    # killed by a signal has not. The PATH is where a harness program installed
    # as root is found on either processor.
    cat >"$INSTALLED_DEFINITION" <<PLIST || die "$WRITE_REFUSED"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$LAUNCHD_LABEL</string>
    <key>ProgramArguments</key>
    <array>
        <string>$RUNTIME_BINARY</string>
        <string>server</string>
        <string>--config</string>
        <string>$RUNTIME_CONFIG</string>
    </array>
    <key>UserName</key>
    <string>$SERVICE_USER</string>
    <key>WorkingDirectory</key>
    <string>$RUNTIME_STATE</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>$RUNTIME_HOME</string>
        <key>PATH</key>
        <string>/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
    <key>StandardErrorPath</key>
    <string>$RUNTIME_STATE/$PROGRAM.log</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>5</integer>
</dict>
</plist>
PLIST
else
    cat >"$INSTALLED_DEFINITION" <<UNIT || die "$WRITE_REFUSED"
[Unit]
Description=printobserver, a supervision layer between a 3D printer and an agent
Documentation=https://github.com/nickderobertis/printobserver
After=network-online.target
Wants=network-online.target

[Service]
Type=exec
User=$SERVICE_USER
Environment=HOME=$RUNTIME_HOME
ExecStart=$RUNTIME_BINARY server --config $RUNTIME_CONFIG
WorkingDirectory=$RUNTIME_STATE
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ReadWritePaths=$RUNTIME_STATE

[Install]
WantedBy=multi-user.target
UNIT
fi
chmod 0644 "$INSTALLED_DEFINITION" ||
    die "$INSTALLED_DEFINITION could not be made readable. Run this as root."

echo "install-service.sh: installed $INSTALLED_BINARY, $INSTALLED_CONFIG, $STATE_DIR \
and $INSTALLED_DEFINITION and started nothing; edit $INSTALLED_CONFIG, then run: \
$START_COMMAND" >&2
