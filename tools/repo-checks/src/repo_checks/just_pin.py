"""The `just` release the recipes are held to, and the parse that proves a host's `just` reads them.

`repo-policy.toml`'s `toolchain.just.version` is the one statement of that
release. `.tool-versions` pins it for asdf and mise, and
`scripts/session-setup.sh` installs no older one where neither is; the
`just-pin` check holds both to the policy.

A pin can still name a release that cannot read the committed `justfile` — the
one this repository carried did, and a fresh host selecting it failed at a
syntax location saying nothing about the pin. So `just bootstrap` starts with
`check-justfile`, which hands the `justfile` to the `just` on PATH, the one the
pin selects on an asdf or mise host, and refuses one that cannot parse it
naming its release, the pinned one and what it said.
"""

from __future__ import annotations

import re
import shutil
import sys

from repo_checks.model import RELEASE, Repo, policy_table
from repo_checks.shell import run

TOOL_VERSIONS = ".tool-versions"
SESSION_SETUP = "scripts/session-setup.sh"
JUSTFILE = "justfile"

#: The line of `scripts/session-setup.sh` naming the oldest `just` it installs.
SESSION_FLOOR = re.compile(r'^readonly JUST_MIN="(?P<release>[^"]*)"$', re.MULTILINE)


def held(repo: Repo) -> str | None:
    """The `just` release `repo-policy.toml` holds the recipes to, or none it names as one."""
    table = policy_table(repo, "toolchain").get("just")
    version = table.get("version") if isinstance(table, dict) else None
    return version if isinstance(version, str) and RELEASE.fullmatch(version) else None


def tool_versions_pin(repo: Repo) -> str | None:
    """The release `.tool-versions` pins `just` at, or none where it pins none."""
    if not repo.exists(TOOL_VERSIONS):
        return None
    for line in repo.read(TOOL_VERSIONS).splitlines():
        match line.split("#", 1)[0].split():
            case ["just", release, *_]:
                return release
            case _:
                continue
    return None


def just_pin(repo: Repo) -> list[str]:
    """`.tool-versions` and the session installer name the release the policy holds `just` at."""
    version = held(repo)
    if version is None:
        return [
            "`repo-policy.toml` holds `just` at no release: declare `[toolchain.just]` with "
            "`version` set to the oldest release that parses the committed `justfile`"
        ]
    findings: list[str] = []
    pinned = tool_versions_pin(repo)
    if pinned != version:
        findings.append(
            f"`{TOOL_VERSIONS}` pins `just` at {pinned or 'no release'}, not the "
            f"{version} `repo-policy.toml` holds it at: write `just {version}` there"
        )
    floor = SESSION_FLOOR.search(repo.read(SESSION_SETUP)) if repo.exists(SESSION_SETUP) else None
    if floor is None or floor["release"] != version:
        findings.append(
            f"`{SESSION_SETUP}` installs `just` no older than "
            f"{floor['release'] if floor else 'no release it names'}, not the {version} "
            f'`repo-policy.toml` holds it at: set `readonly JUST_MIN="{version}"` there'
        )
    return findings


def check_justfile(repo: Repo) -> int:
    """Refuse a `just` on PATH that cannot parse the committed `justfile`.

    What is refused is the parse, never the release: a newer `just` that reads
    the file is as good as the pinned one, and refusing it would fail every host
    whose package manager serves the newest. The refusal names the release it
    found, the one the policy holds, and the parser's own words.
    """
    version = held(repo)
    if version is None:
        print(just_pin(repo)[0], file=sys.stderr)
        return 1
    found = shutil.which("just")
    if found is None:
        print(
            f"no `just` is on PATH to parse the `justfile` with. Install just {version}, the "
            f"release `{TOOL_VERSIONS}` pins, and run this again.",
            file=sys.stderr,
        )
        return 1
    parsed = run(
        [found, "--justfile", str(repo.path(JUSTFILE)), "--summary"], cwd=repo.root, timeout=60
    )
    if parsed.returncode == 0:
        return 0
    answered = run([found, "--version"], timeout=60)
    release = RELEASE.search(answered.stdout) if answered.returncode == 0 else None
    print(
        f"just {release.group() if release else '(no release it names)'} at {found} cannot "
        f"parse the `justfile`. `{TOOL_VERSIONS}` pins just {version}, the oldest release "
        f"that parses it: install that release (`asdf install just {version}`, `mise install "
        f"just@{version}` or `uv tool install rust-just=={version}`), put it first on PATH, "
        f"and run this again. What it said:\n{parsed.stderr.rstrip()}",
        file=sys.stderr,
    )
    return 1
