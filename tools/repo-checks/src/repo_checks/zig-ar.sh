#!/bin/sh
# zig as the archiver for a Windows-target lint pass on a Unix host; see zig-cc.sh.
set -eu
exec "$PRINTOBSERVER_ZIG" ar "$@"
