#!/bin/sh
# zig as the C compiler for a Windows-target lint pass on a Unix host.
#
# cc-rs hands a clang-like compiler the Rust triple as `--target=`, which zig
# does not read; that argument is dropped and zig's own target named instead.
# `PRINTOBSERVER_ZIG` is the zig program and `PRINTOBSERVER_ZIG_TARGET` its
# target, both set by `repo_checks.windows_lint` for the one cargo run it makes.
set -eu
for arg in "$@"; do
  shift
  case "$arg" in
    --target=*) ;;
    *) set -- "$@" "$arg" ;;
  esac
done
exec "$PRINTOBSERVER_ZIG" cc -target "$PRINTOBSERVER_ZIG_TARGET" "$@"
