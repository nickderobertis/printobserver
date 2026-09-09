#!/bin/sh
# Put the printobserver service in place, and start nothing.
#
# The second of the three commands of the end-user install path AGENTS.md's "The
# end-user install path" section states; `just check-repo` refuses a tree in
# which this path, or the unit name below, differs from what that section says.
#
# It places FOUR things and no more: the program, the configuration, the state
# directory, and the systemd unit. It does NOT enable the service and does NOT
# start it, and it must never be changed to — nor given an option that does.
# This service commands a 3D printer, so installing a package must not, as a
# side effect, start a process that can move a machine. Enabling and starting is
# the operator's own third command, which this script prints when it is done.
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
set -eu

UNIT_NAME="printobserver.service"
PROGRAM="printobserver"
SERVICE_USER=""
BINARY=""
ROOT=""

die() {
    echo "install-service.sh: $1" >&2
    exit 1
}

# Every value this script is given is written into a TOML document and into a
# systemd unit, neither of which has an escape for a quote, a backslash or a
# newline. So a value carrying one is refused here rather than producing a unit
# the service manager reads as something else.
plain() {
    [ -n "$2" ] || die "$1 is empty"
    case "$2" in
        *[\"\\]*)
            die "$1 carries a quote or a backslash, which a unit file and a TOML \
document have no escape for"
            ;;
    esac
    case "$2" in
        *"
"*)
            die "$1 carries a newline, which a unit file reads as the end of a setting"
            ;;
    esac
}

# A system user name, as `useradd` and a unit's `User=` take one: a letter or an
# underscore, then letters, digits, underscores and hyphens.
user_name() {
    case "$1" in
        [a-z_]*) ;;
        *) die "$1 is not a system user name: it does not begin with a letter or an \
underscore" ;;
    esac
    case "$1" in
        *[!a-z0-9_-]*)
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
            sed -n '2,25p' "$0"
            exit 0
            ;;
        *)
            die "$1 is not an option of this script"
            ;;
    esac
done

if [ -z "$BINARY" ]; then
    BINARY="$(command -v "$PROGRAM" || true)"
fi
[ -n "$BINARY" ] || die "no $PROGRAM program on PATH. Take one of the three routes \
AGENTS.md's install path states, or pass --binary."
[ -x "$BINARY" ] || die "$BINARY is not an executable program"

if [ -z "$SERVICE_USER" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        SERVICE_USER="$PROGRAM"
    else
        SERVICE_USER="$(id -un)"
        echo "install-service.sh: not running as root, so the service will run as \
$SERVICE_USER" >&2
    fi
    user_name "$SERVICE_USER"
fi

if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    [ "$(id -u)" -eq 0 ] || die "there is no user $SERVICE_USER and this is not root"
    useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER" ||
        die "the system user $SERVICE_USER could not be created"
fi

# The four places, spelled once. Everything below writes into one of them.
BIN_DIR="$ROOT/usr/local/lib/$PROGRAM"
CONF_DIR="$ROOT/etc/$PROGRAM"
STATE_DIR="$ROOT/var/lib/$PROGRAM"
UNIT_DIR="$ROOT/etc/systemd/system"

INSTALLED_BINARY="$BIN_DIR/$PROGRAM"
INSTALLED_CONFIG="$CONF_DIR/config.toml"
INSTALLED_UNIT="$UNIT_DIR/$UNIT_NAME"

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

mkdir -p "$BIN_DIR" "$CONF_DIR" "$UNIT_DIR" ||
    die "$ROOT/ could not be made writable for the install. Run this as root, or pass \
--root a directory you own."

install -m 0755 "$BINARY" "$INSTALLED_BINARY" ||
    die "$BINARY could not be installed to $INSTALLED_BINARY"

# 0700 and owned by the service's own user: this directory holds the whole
# record of what a printer did and what an agent decided, including the
# snapshots. Nothing else on the machine has any business reading it.
mkdir -p "$STATE_DIR" || die "$STATE_DIR could not be created"
chown "$SERVICE_USER" "$STATE_DIR" || die "$STATE_DIR could not be handed to $SERVICE_USER"
chmod 0700 "$STATE_DIR" || die "$STATE_DIR could not be made private"

# An existing configuration is left exactly as it is: a reinstall must not
# overwrite the operator's own values with a template's.
if [ -e "$INSTALLED_CONFIG" ]; then
    echo "install-service.sh: $INSTALLED_CONFIG is already there and was left alone" >&2
else
    cat >"$INSTALLED_CONFIG" <<CONFIG
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
    chown "$SERVICE_USER" "$INSTALLED_CONFIG"
    chmod 0600 "$INSTALLED_CONFIG"
fi

cat >"$INSTALLED_UNIT" <<UNIT
[Unit]
Description=printobserver, a supervision layer between a 3D printer and an agent
Documentation=https://github.com/nickderobertis/printobserver
After=network-online.target
Wants=network-online.target

[Service]
Type=exec
User=$SERVICE_USER
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
chmod 0644 "$INSTALLED_UNIT"

echo "install-service.sh: installed $INSTALLED_BINARY, $INSTALLED_CONFIG, \
$STATE_DIR and $INSTALLED_UNIT; nothing was started." >&2
echo "install-service.sh: edit $INSTALLED_CONFIG, then run: sudo systemctl enable \
--now $UNIT_NAME" >&2
