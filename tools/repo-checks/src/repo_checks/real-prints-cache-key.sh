#!/usr/bin/env bash
# The Actions cache key of the real-print cases' Git LFS objects, as the line a
# workflow step appends to `GITHUB_OUTPUT`: `key=real-prints-lfs-<digest>`.
#
# The digest is over the distinct object ids the pointers at HEAD name under
# `tests/real-prints/`, as `git lfs ls-files --long` lists them, so nothing is
# downloaded, and the key moves when one object does and not when any other
# file does. Run in the repository whose cases it keys; `just
# real-prints-cache-key` runs it there. Every failure exits non-zero naming its
# cause and what to do, and prints no key: a key line is only ever a whole one.
set -euo pipefail

if ! git lfs version >/dev/null 2>&1; then
    echo "git-lfs is not installed, so the cases' object ids cannot be read; run just bootstrap, which installs it" >&2
    exit 1
fi
if ! listing="$(git lfs ls-files --long --include 'tests/real-prints/**')"; then
    echo "git lfs ls-files could not list the cases at HEAD; run this inside the repository whose cases it keys" >&2
    exit 1
fi
malformed="$(printf '%s\n' "$listing" | grep -Ev '^[0-9a-f]{64} [*-] .+$' | grep -v '^$' || true)"
if [ -n "$malformed" ]; then
    echo "git lfs ls-files answered a line that is no listing of an object id and a path: $(printf '%s\n' "$malformed" | head -n 1); check the git-lfs release just bootstrap installs" >&2
    exit 1
fi
oids="$(printf '%s\n' "$listing" | cut -d ' ' -f 1 | LC_ALL=C sort -u | grep -v '^$' || true)"
if [ -z "$oids" ]; then
    echo "git-lfs lists no object under tests/real-prints/ at HEAD; commit the cases as LFS pointers first" >&2
    exit 1
fi
if ! digest="$(printf '%s\n' "$oids" | git hash-object --stdin)" || [ -z "$digest" ]; then
    echo "git hash-object could not digest the object ids; check that git works in this repository" >&2
    exit 1
fi
echo "key=real-prints-lfs-$digest"
