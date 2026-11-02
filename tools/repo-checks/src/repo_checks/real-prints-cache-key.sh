#!/usr/bin/env bash
# The Actions cache key of the real-print cases' Git LFS objects, as the line a
# workflow step appends to `GITHUB_OUTPUT`: `key=real-prints-lfs-<digest>`.
#
# The digest is over the distinct object ids the pointers at HEAD name under
# `tests/real-prints/`, as `git lfs ls-files --long` lists them, so nothing is
# downloaded, and the key moves when one object does and not when any other
# file does. Run in the repository whose cases it keys; `just
# real-prints-cache-key` runs it there.
set -euo pipefail

oids="$(git lfs ls-files --long --include 'tests/real-prints/**' | cut -d ' ' -f 1 | LC_ALL=C sort -u)"
if [ -z "$oids" ]; then
    echo "git-lfs lists no object under tests/real-prints/ at HEAD; run just bootstrap, which installs git-lfs" >&2
    exit 1
fi
echo "key=real-prints-lfs-$(printf '%s\n' "$oids" | git hash-object --stdin)"
