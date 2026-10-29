#!/bin/sh
# Put the printobserver program on your path, already built for your platform.
#
# The shell form of the third of the three ALTERNATIVE routes AGENTS.md's "The
# end-user install path" section states — `scripts/install.ps1` is the same
# route on Windows — and `just check-repo` refuses a tree in which this path
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
#   install.sh [--version vX.Y.Z] [--to DIR] [--client-only]
#              [--observation-interval-seconds N]
#
#   --version  install that release rather than the newest one.
#   --to       install into DIR rather than into ~/.local/bin.
#   --client-only  install the program alone, without a user server.
#   --observation-interval-seconds  positive interval for a new config.
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
CLIENT_ONLY=false
INTERVAL_LINE="# observation_interval_s = 120"
UNIT_NAME="printobserver.service"
LAUNCHD_LABEL="io.github.nickderobertis.printobserver"

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
        --client-only)
            CLIENT_ONLY=true
            shift
            ;;
        --observation-interval-seconds)
            [ $# -ge 2 ] || die "--observation-interval-seconds takes a number" "Pass a positive whole number of seconds."
            case "$2" in
                "" | 0* | *[!0123456789]*) die "invalid observation interval: $2" "Pass a positive whole number of seconds." ;;
            esac
            [ "${#2}" -le 19 ] || die "observation interval is too large" "Pass at most 9223372036854775807."
            if [ "${#2}" -eq 19 ]; then
                printf '%s\n%s\n' "$2" 9223372036854775807 | LC_ALL=C sort -c 2>/dev/null || die "observation interval is too large" "Pass at most 9223372036854775807."
            fi
            INTERVAL_LINE="observation_interval_s = $2"
            shift 2
            ;;
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
    Darwin/arm64) platform="macos-aarch64" ;;
    *)
        die "this is $system/$machine, which printobserver publishes no program for" \
            "The platforms this script installs for are linux-x86_64, linux-aarch64 and \
macos-aarch64; on Windows, run scripts/install.ps1 from PowerShell instead. On \
anything else, build it from source with \`cargo install printobserver\`."
        ;;
esac

# Refuse an isolated install before downloading or changing any file.
if [ "$CLIENT_ONLY" = false ]; then
    case "$system" in
        Linux)
            if [ -e /etc/systemd/system/printobserver.service ] || [ -e /usr/lib/systemd/system/printobserver.service ]; then
                die "the system printobserver.service is installed" "Run sudo sh scripts/migrate-service-to-per-user.sh before installing per-user, or use --client-only."
            fi
            if registration="$(systemctl show --property=LoadState --value "$UNIT_NAME" 2>&1)"; then
                [ "$registration" = not-found ] || die "the system printobserver.service registration is not absent (LoadState: $registration)" "Run sudo sh scripts/migrate-service-to-per-user.sh before installing per-user, or use --client-only."
            else
                die "the system unit registration could not be queried: $registration" "Run systemctl show --property=LoadState --value $UNIT_NAME, correct its reported error and retry."
            fi
            CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
            RUNTIME_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/printobserver"
            DEFINITION="$CONFIG_HOME/systemd/user/$UNIT_NAME"
            ;;
        Darwin)
            if [ -e "/Library/LaunchDaemons/$LAUNCHD_LABEL.plist" ]; then
                die "the LaunchDaemon is installed" "Run sudo sh scripts/migrate-service-to-per-user.sh before installing per-user, or use --client-only."
            fi
            if registration="$(launchctl print "system/$LAUNCHD_LABEL" 2>&1)"; then
                die "the LaunchDaemon is installed" "Run sudo sh scripts/migrate-service-to-per-user.sh before installing per-user, or use --client-only."
            else
                registration_status=$?
                [ "$registration_status" -eq 113 ] || die "the LaunchDaemon registration could not be queried (exit $registration_status): $registration" "Run launchctl print system/$LAUNCHD_LABEL, correct its reported error and retry."
            fi
            CONFIG_HOME="$HOME/Library/Application Support"
            RUNTIME_STATE="$CONFIG_HOME/printobserver/state"
            DEFINITION="$HOME/Library/LaunchAgents/$LAUNCHD_LABEL.plist"
            ;;
    esac
    RUNTIME_CONFIG="$CONFIG_HOME/printobserver/config.toml"
    case "$DIRECTORY" in /*) ;; *) DIRECTORY="$PWD/$DIRECTORY" ;; esac
    for value in "$DIRECTORY" "$RUNTIME_CONFIG" "$RUNTIME_STATE" "$DEFINITION"; do
        if ! printf '%s\n' "$value" | LC_ALL=C awk '/[[:cntrl:]]/ { exit 1 }'; then
            die "an install path carries a control character" "Check HOME, --to and XDG paths; use paths without those characters."
        fi
        case "$value" in
            *[\"\\%\&\<\>\$]* | *"
"*) die "a path carries a quote, backslash, dollar, percent, XML markup or newline: $value" "Use paths without those characters." ;;
            /*) ;;
            *) die "a configuration or state path is relative: $value" "Set HOME and XDG directories to absolute paths." ;;
        esac
        ancestor="$value"
        while [ "$ancestor" != / ]; do
            [ ! -L "$ancestor" ] || die "an install path contains a symbolic link: $ancestor" "Replace that link with a real user-owned path and retry."
            ancestor="$(dirname "$ancestor")"
        done
    done
fi

asset="$PROGRAM-$platform.tar.gz"
if [ -n "$VERSION" ]; then
    from="$RELEASE_BASE/download/$VERSION"
    which="release $VERSION"
else
    from="$RELEASE_BASE/latest/download"
    which="the newest release"
fi

work="$(mktemp -d)" || die "a temporary download directory could not be created" "Free space in TMPDIR or select a writable TMPDIR, then retry."
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

# Query failures are not proof that a job is absent. launchctl distinguishes
# an unloaded label (113) from a loaded job and an unavailable GUI domain.
agent_loaded() {
    agent_status=0
    agent_answer="$(launchctl print "$1" 2>&1)" || agent_status=$?
    case "$agent_status" in
        0) return 0 ;;
        113) return 1 ;;
        *) die "the LaunchAgent could not be queried (exit $agent_status): $agent_answer; nothing was replaced" "Run launchctl print $1, correct its reported error and retry." ;;
    esac
}

# Downloads have verified before an upgrade interrupts the running server.
if [ "$CLIENT_ONLY" = false ] && [ -f "$DEFINITION" ]; then
    case "$system" in
        Linux) systemctl --user stop "$UNIT_NAME" || die "the user unit could not be stopped; nothing was replaced" "Run systemctl --user status printobserver.service, correct its reported error and retry." ;;
        Darwin)
            agent_target="gui/$(id -u)/$LAUNCHD_LABEL"
            if agent_loaded "$agent_target"; then
                launchctl bootout "$agent_target" || die "the LaunchAgent could not be stopped; nothing was replaced" "Run launchctl print $agent_target, correct its reported error and retry."
                remaining=60
                while agent_loaded "$agent_target"; do
                    [ "$remaining" -gt 0 ] || die "the LaunchAgent has not unloaded after a minute; nothing was replaced" "Run launchctl print $agent_target, resolve the pending stop and retry."
                    sleep 1
                    remaining=$((remaining - 1))
                done
            fi
            ;;

    esac
fi

mkdir -p "$DIRECTORY" || die \
    "$DIRECTORY could not be created" \
    "Nothing was installed. Pass \`--to\` a directory you can write to."

chmod 0755 "$work/$PROGRAM"
mv -f "$work/$PROGRAM" "$DIRECTORY/$PROGRAM" || die \
    "$PROGRAM could not be put in $DIRECTORY" \
    "Nothing was installed. Pass \`--to\` a directory you can write to."

if [ "$CLIENT_ONLY" = true ]; then

case "$system" in
    Darwin) start="sudo launchctl bootstrap system /Library/LaunchDaemons/io.github.nickderobertis.$PROGRAM.plist" ;;
    *) start="sudo systemctl enable --now $PROGRAM.service" ;;
esac
path_note=""
case ":$PATH:" in
    *":$DIRECTORY:"*) ;;
    *)
        path_note="; $DIRECTORY is not on your PATH, so add it or run $DIRECTORY/$PROGRAM by its whole name"
        ;;
esac
echo "install.sh: installed $DIRECTORY/$PROGRAM from $which; next, put the service in \
place with \`curl -fsSL https://raw.githubusercontent.com/$OWNER/$REPOSITORY/main/scripts/install-service.sh | sudo sh\` \
and then start it with \`$start\`$path_note" >&2

    exit 0
fi

umask 077
mkdir -p "$(dirname "$RUNTIME_CONFIG")" "$RUNTIME_STATE" "$(dirname "$DEFINITION")" || die "the user configuration, state or manager directory could not be created" "Check ownership and free space under $HOME, then rerun this installer."
chmod 0700 "$RUNTIME_STATE" || die "$RUNTIME_STATE could not be made private" "Make sure you own this directory, then rerun this installer."
if [ ! -e "$RUNTIME_CONFIG" ]; then
    cat >"$RUNTIME_CONFIG" <<CONFIG || die "$RUNTIME_CONFIG could not be written" "Check its directory ownership and free space, then rerun this installer."
# printobserver's one configuration file.
#
# Every value below is validated when the service starts, and a value that
# cannot work is refused naming this file's own field for it. Fill in the three
# marked FILL IN, install the skill and restart the user server afterwards.

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
# The agent's skill, which this program does not carry. Install it here, as
# yourself:
#   gh skill install nickderobertis/printobserver printobserver --dir $RUNTIME_STATE/skills
skill_path = "$RUNTIME_STATE/skills/printobserver/SKILL.md"
# How often every active print is looked in on, and its agent given a turn,
# whether or not the detector has alerted. Left out, every 120 seconds: an
# eight-hour print is 240 turns. Raise it to spend less; zero is refused.
$INTERVAL_LINE

[ingress]
# FILL IN: the shared secret Obico's webhook notification plugin must carry.
# Anything that can post to the ingress can pause a printer.
shared_secret = ""
# The answer bound is left out on purpose. The default this program ships is
# already below the timeout Obico posts under, and writing that number here
# would be a second copy of it to keep right; see AGENTS.md, "The Obico ingress
# answer bound".

# This per-user server keeps only the credential verifier in this file.
# Supervision runs as your account and can read your client configuration.
# As yourself, unprivileged, run
#   printobserver credential issue
# which writes the credential into your own client configuration and prints
# one line to put here, above the first [table] header:
#   api.credential_verifier = "sha256:..."
# then restart the service. Until that line is here the service supervises and
# refuses every operator request. Each supervision turn is handed a credential
# of its own in its environment, so nothing else is needed for the agent.

# Optional: a fresh look at the print. A URL answering one still image, such as
# go2rtc's frame.jpeg for the printer's camera. Left out, a supervising agent's
# looks carry the printer's state but no frame.
# [camera]
# snapshot_url = "http://127.0.0.1:1984/api/frame.jpeg?src=camera"

# Optional: Obico's own API, so that a print Obico paused, and the agent adjusted
# and the supervisor resumed, is acknowledged to Obico, which re-arms its
# detection for the rest of the print. Give both keys or neither; the token is
# an OAuth2 access token from the Obico instance's own administration.
# [obico]
# url = "http://127.0.0.1:3334"
# access_token = ""

# How a start checks the spools loaded against what its file needs, by
# OctoPrint's own analysis of it. Every key is optional; left out, a start is
# refused while the spool on a tool holds less than the print needs plus 10%
# (and at least 5 g), while its figure is unverified, or while OctoPrint has
# no analysis of the file after waiting 300 seconds for one. "warn" lets such
# a start go ahead and records that it warned; "off" checks nothing. An
# operator may start one anyway with "--override-filament-check true".
# [spools]
# preflight = "refuse"
# buffer_percent = 10.0
# buffer_min_g = 5.0
# analysis_wait_seconds = 300

# What any actor may ask for at all. A manifest may narrow these; nothing may
# widen them. The system resumes a print the detector paused once the agent's
# adjustment to it is in, and puts back a bounded adjustment when it expires.
[safety]
agent_min_interval_s = 30

[safety.allowed]
feedrate = { min = 0.5, max = 1.5 }
flowrate = { min = 0.9, max = 1.1 }
fan = { min = 0.0, max = 100.0 }
bed_target = { min = 0.0, max = 110.0 }
"tool_target:0" = { min = 0.0, max = 260.0 }

# Who may ask for what. The spool commands (registering, loading,
# unloading, correcting and retiring a spool) and closing a print record left
# open are the operator's, and the agent is granted none of them: a
# configuration granting the agent close_print is refused at start.
[safety.actions]
operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
            "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
            "set_fan_percent", "acknowledge_failure", "close_print",
            "register_spool", "load_spool", "unload_spool", "adjust_spool",
            "retire_spool"]
agent = ["pause", "set_feedrate_factor", "set_flowrate_factor",
         "set_fan_percent", "acknowledge_failure"]
system = ["resume", "set_feedrate_factor", "set_flowrate_factor",
          "set_tool_target_c", "set_bed_target_c", "set_fan_percent"]
CONFIG
    # llmlint: ignore[changed_behavior_has_e2e] New config mode failure: The file was just created by this user in a validated writable directory; failure here requires a filesystem failure or concurrent replacement. Real directory/write refusals and manager recovery are covered, without replacing the filesystem tools.
    chmod 0600 "$RUNTIME_CONFIG" || die "$RUNTIME_CONFIG could not be made private" "Run chmod 0600 on this file as its owner, then retry."
fi

if [ "$system" = Linux ]; then
    cat >"$DEFINITION" <<UNIT || die "$DEFINITION could not be written" "Check its directory ownership and free space, then rerun this installer."
[Unit]
Description=printobserver supervision
[Service]
Type=exec
ExecStart="$DIRECTORY/$PROGRAM" server --config "$RUNTIME_CONFIG"
WorkingDirectory=$RUNTIME_STATE
Restart=on-failure
RestartSec=5
[Install]
WantedBy=default.target
UNIT
    # llmlint: ignore[changed_behavior_has_e2e] Linux definition mode failure: The file was just created by this user in a validated writable directory; failure here requires a filesystem failure or concurrent replacement. Real directory/write refusals and manager recovery are covered, without replacing the filesystem tools.
    chmod 0644 "$DEFINITION" || die "$DEFINITION could not be made readable" "Run chmod 0644 on this file as its owner, then retry."
    systemctl --user daemon-reload || die "the user manager could not reload its units" "Run systemctl --user status and check that your user manager is running, then retry."
    # llmlint: ignore[boundary_inputs_validated] Lingering account authorization: The contract deliberately selects USER in this unprivileged per-user installer. logind authorizes enabling lingering for the selected account and refuses another account without authorization, so forging USER grants no permission to enable lingering for someone else. The installer preserves that authorization boundary and reports refusal without escalating.
    if ! loginctl enable-linger "${USER:-$(id -un)}"; then
        echo "install.sh: lingering was refused; the server runs only while you are logged in. Enable it with: loginctl enable-linger \"${USER:-$(id -un)}\"" >&2
    fi
    systemctl --user enable --now "$UNIT_NAME" || die "the user unit could not be enabled and started" "Run systemctl --user status printobserver.service, correct its reported error and rerun this installer."
else
    cat >"$DEFINITION" <<PLIST || die "$DEFINITION could not be written" "Check its directory ownership and free space, then rerun this installer."
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$LAUNCHD_LABEL</string>
<key>ProgramArguments</key><array><string>$DIRECTORY/$PROGRAM</string><string>server</string><string>--config</string><string>$RUNTIME_CONFIG</string></array>
<key>WorkingDirectory</key><string>$RUNTIME_STATE</string>
<key>EnvironmentVariables</key><dict><key>HOME</key><string>$HOME</string><key>PATH</key><string>$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin</string></dict>
<key>StandardErrorPath</key><string>$RUNTIME_STATE/printobserver.log</string>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>5</integer>
</dict></plist>
PLIST
    # llmlint: ignore[changed_behavior_has_e2e] macOS definition mode failure: The file was just created by this user in a validated writable directory; failure here requires a filesystem failure or concurrent replacement. Real directory/write refusals and manager recovery are covered, without replacing the filesystem tools.
    chmod 0644 "$DEFINITION" || die "$DEFINITION could not be made readable" "Run chmod 0644 on this file as its owner, then retry."
    launchctl bootstrap "gui/$(id -u)" "$DEFINITION" || die "the LaunchAgent could not be loaded" "Run launchctl print gui/$(id -u)/$LAUNCHD_LABEL; correct its reported error in your logged-in GUI session and retry."
    echo "install.sh: the server runs only while you are logged in; auto-login covers a dedicated machine." >&2
fi
# llmlint: ignore[tool_output_is_signal] The install contract requires credential issue, the verifier paste, skill installation and sign-in as actionable next steps; each following line is one of those steps.
printf '%s\n' "install.sh: installed and started $DIRECTORY/$PROGRAM; next, as yourself:" \
    "  \"$DIRECTORY/$PROGRAM\" credential issue" \
    "  Paste its api.credential_verifier line above the first table in $RUNTIME_CONFIG; edit the printer settings there." \
    "  gh skill install nickderobertis/printobserver printobserver --dir \"$RUNTIME_STATE/skills\"" \
    "  \"$DIRECTORY/$PROGRAM\" sign-in" >&2
