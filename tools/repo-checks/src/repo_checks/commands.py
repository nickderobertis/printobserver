"""The commands the recipes and hooks run that do something rather than check it."""

from __future__ import annotations

import os
import re
import shutil
import sys
from pathlib import Path

from repo_checks.model import RELEASE, PolicyValueError, Repo, toolchain_tools
from repo_checks.platforms import PlatformError, descriptor
from repo_checks.shell import run

CONVENTIONAL = re.compile(r"^(?P<type>[a-z]+)(?:\([^)]+\))?!?: .+")


def install_tools(repo: Repo) -> int:
    """Put every tool `repo-policy.toml` declares on PATH, at the release it holds.

    A tool already on PATH is skipped when the policy holds it at no release, or
    when it answers the held one. A copy answering any other — a cache restored
    from before a bump, or an install that predates the pin — is replaced; and
    one still answering another once that install is done, because a copy
    earlier on PATH shadows it, is refused naming where it is rather than
    accepted. A declaration the installer cannot act on is refused before
    anything is installed.
    """
    try:
        tools = toolchain_tools(repo)
    except PolicyValueError as malformed:
        print(f"{malformed}. Correct it; nothing was installed.", file=sys.stderr)
        return 1
    for tool in tools:
        present = shutil.which(tool.command)
        if present is None:
            print(f"installing {tool.command}", file=sys.stderr)
        else:
            answered = _release_of(present) if tool.version is not None else None
            if tool.version is None or answered == tool.version:
                continue
            print(
                f"{tool.command} at {present} answers {answered or 'no release'}, not the held "
                f"{tool.version}: installing {tool.version}",
                file=sys.stderr,
            )
        result = run(tool.install_argv, cwd=repo.root, capture=False)
        if result.returncode != 0:
            print(
                f"failed to install {tool.command}. Run `{' '.join(tool.install_argv)}` by hand.",
                file=sys.stderr,
            )
            return 1
        if tool.version is None:
            continue
        installed = shutil.which(tool.command)
        answered = _release_of(installed) if installed is not None else None
        if answered != tool.version:
            print(
                f"{tool.command} at {installed} still answers {answered or 'no release'} after "
                f"installing {tool.version}: a copy earlier on PATH shadows the one installed. "
                f"Remove it, or put the installed one first on PATH.",
                file=sys.stderr,
            )
            return 1
    return 0


def _release_of(program: str) -> str | None:
    """The release `program` answers `--version` with, or none if it names none."""
    answer = run([program, "--version"], timeout=60)
    if answer.returncode != 0:
        return None
    found = RELEASE.search(answer.stdout)
    return found.group() if found else None


def tool_version(repo: Repo, command: str) -> int:
    """Print `version=<release>`: the release `repo-policy.toml` holds `command` at.

    The one line a workflow step appends to `GITHUB_OUTPUT`, so that a job
    installing the tool prebuilt installs the release the toolchain holds rather
    than a second statement of it.
    """
    try:
        declared = {tool.command: tool for tool in toolchain_tools(repo)}
    except PolicyValueError as malformed:
        print(f"{malformed}.", file=sys.stderr)
        return 1
    if command not in declared:
        print(
            f"`repo-policy.toml` declares no toolchain tool `{command}`. "
            f"Declare one of: {', '.join(sorted(declared))}.",
            file=sys.stderr,
        )
        return 1
    held = declared[command].version
    if held is None:
        print(
            f"`repo-policy.toml` holds `{command}` at no release. Add `version` to its "
            f"`[[toolchain.tool]]` entry.",
            file=sys.stderr,
        )
        return 1
    print(f"version={held}")
    return 0


def install_hooks(repo: Repo) -> int:
    """Point git at the committed hooks, where this tree is a git repository."""
    if not (repo.root / ".git").exists():
        return 0
    run(["git", "config", "core.hooksPath", ".githooks"], cwd=repo.root, check=True)
    return 0


def commit_msg(repo: Repo, message_file: Path) -> int:
    """Refuse an authored commit subject that is not a Conventional Commit.

    The subject git writes for a merge it is completing is not one anybody
    authored, so it is admitted as written; everything else is held to the
    declared type list.
    """
    subject = message_file.read_text(encoding="utf-8").splitlines()[0].strip()
    if subject.startswith("#") or not subject:
        return 0
    if _a_merge_is_in_progress(repo):
        return 0
    return _rule_on_subject(repo, subject, "commit subject")


def _a_merge_is_in_progress(repo: Repo) -> bool:
    """Whether git is part-way through a merge it is writing this commit for.

    Publishing a branch of this repository merges the base into it first, and
    git — not a person — writes `Merge remote-tracking branch 'origin/main'
    into <branch>` for that commit. Holding a subject nobody typed to
    Conventional Commits refused the merge, so no branch could be published
    once `main` had moved under it.

    What distinguishes that commit is its *state*, not its wording: git writes
    `MERGE_HEAD` into the git directory when a merge starts and removes it once
    the merge commit is made, so it is present exactly while git is completing
    one and absent for an ordinary commit whatever its subject says. Reading
    the subject instead would hand anybody a bypass of the whole convention by
    typing one word, because `Merge branch 'main'` typed by a person is
    textually identical to what git generates.

    `run` drops the variables naming a repository, so this asks about the tree
    the hook was pointed at rather than about whichever repository an outer
    git invocation happened to name. A tree that is no git repository
    at all answers no, and its subject is ruled on as usual.
    """
    located = run(["git", "rev-parse", "--verify", "--quiet", "MERGE_HEAD"], cwd=repo.root)
    return located.returncode == 0


def pr_title(repo: Repo) -> int:
    """Refuse a pull-request title that is not a Conventional Commit subject."""
    import os

    title = os.environ.get("PR_TITLE", "").strip()
    if not title:
        print("PR_TITLE is empty. Pass the pull-request title in the environment.", file=sys.stderr)
        return 1
    return _rule_on_subject(repo, title, "pull-request title")


def _rule_on_subject(repo: Repo, subject: str, what: str) -> int:
    """Hold one subject to the type list `repo-policy.toml` declares."""
    admitted = [
        *repo.policy["commits"]["release_types"],
        *repo.policy["commits"]["non_release_types"],
    ]
    match = CONVENTIONAL.match(subject)
    if not match:
        print(
            f"{what} is not a Conventional Commit: {subject!r}\n"
            f"Use `<type>(<scope>): <subject>`, with type one of "
            f"{', '.join(sorted(admitted))}.",
            file=sys.stderr,
        )
        return 1
    if match["type"] not in admitted:
        print(
            f"{what} uses type `{match['type']}`, which `repo-policy.toml` does not "
            f"admit. Use one of {', '.join(sorted(admitted))}.",
            file=sys.stderr,
        )
        return 1
    return 0


def docs_schemas_write(repo: Repo) -> int:
    """Write the generated schema document from the schema set the contracts write.

    The one way that document is produced. It is regenerated rather than edited,
    and `just check-repo` refuses a tree whose committed document is not what
    this writes.
    """
    from repo_checks.docs import docs_policy, schema_document_text

    policy = docs_policy(repo)
    target = repo.path(policy.schema_document)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(schema_document_text(repo, policy), encoding="utf-8")
    return 0


def _total_of(report: str, column: int) -> str:
    """The figure in `column` of a coverage report's `TOTAL` row, or `unknown`.

    `cargo llvm-cov report --summary-only` and `coverage report` both end in a
    `TOTAL` row; the line-coverage percentage is the ninth figure after the name in the first
    and the last column of the second.
    """
    for line in reversed(report.splitlines()):
        words = line.split()
        if words[:1] == ["TOTAL"] and len(words) > column:
            return words[column]
    return "unknown"


#: The `gate.coverage.exemptions.<platform>` fields a per-target exemption states.
EXEMPTION_FIELDS = ("target", "toolchain", "diagnostics", "reference")


def _exempt(repo: Repo, floors: dict, stderr: str) -> str | list[str]:
    """Whether this platform's unreadable Rust profile is exempt by policy.

    Answers the one line the exemption prints where it applies, and otherwise
    the refusals — each a sentence naming what the entry lacks. An exemption
    is an entry under `gate.coverage.exemptions` keyed by the platform this
    gate runs as (`PRINTOBSERVER_PLATFORM`); it names the Rust target
    `AGENTS.md`'s supported-platform list gives that platform and no other, the
    toolchain release whose profile reader refuses, every diagnostic that
    refusal prints, and an upstream issue describing the refusal on that
    target. A missing platform variable, or a platform with no entry, is no
    exemption and no refusal: the floor was missed.
    """
    platform_id = os.environ.get("PRINTOBSERVER_PLATFORM", "")
    exemption = (floors.get("exemptions") or {}).get(platform_id)
    if exemption is None:
        return []
    refusals = [
        f"the coverage exemption for `{platform_id}` states no `{field}`"
        for field in EXEMPTION_FIELDS
        if not exemption.get(field)
    ]
    reference = str(exemption.get("reference", ""))
    if reference and not reference.startswith("https://github.com/"):
        refusals.append(
            f"the coverage exemption for `{platform_id}` names `{reference}` as its "
            f"reference, and an exemption is held to an upstream issue on GitHub"
        )
    try:
        target = descriptor(repo, platform_id).target
    except PlatformError:
        refusals.append(
            f"the coverage exemption for `{platform_id}` names a platform AGENTS.md's "
            f"supported-platform list does not"
        )
        return refusals
    if exemption.get("target") and exemption["target"] != target:
        refusals.append(
            f"the coverage exemption for `{platform_id}` names target `{exemption['target']}`, "
            f"and that platform's Rust target is `{target}`"
        )
    if refusals:
        return refusals
    missing = [d for d in exemption["diagnostics"] if d not in stderr]
    if missing:
        return [
            f"the coverage exemption for `{platform_id}` did not apply: the profile reader "
            f"did not print {missing!r}, so the floor was missed rather than unreadable"
        ]
    return (
        f"no readable profile on {target}, exempt by policy: "
        f"{exemption['toolchain']}; {exemption['reference']}"
    )


def coverage(repo: Repo) -> int:
    """Fail the build below the line-coverage floors `repo-policy.toml` records.

    Pass or fail, one line at the end states each ecosystem's measured total
    beside its floor, so every platform's figure is in its log. A platform whose
    toolchain cannot read the profiles its own instrumentation writes states
    that as a distinct outcome, under an exemption `_exempt` holds to policy.
    """
    floors = repo.policy["gate"]["coverage"]
    failed = False

    rust = run(
        ["cargo", "llvm-cov", "report", "--summary-only", f"--fail-under-lines={floors['rust']}"],
        cwd=repo.root,
    )
    rust_total = _total_of(rust.stdout, 9)
    if rust.returncode != 0:
        exempt = _exempt(repo, floors, rust.stderr)
        if isinstance(exempt, str):
            rust_total = "no readable profile, exempt"
            print(exempt)
        else:
            print(rust.stdout, file=sys.stderr)
            print(rust.stderr, file=sys.stderr)
            for refusal in exempt:
                print(refusal, file=sys.stderr)
            print(
                f"Rust line coverage is below the {floors['rust']}% floor. Add tests that "
                f"drive the uncovered lines, or explain the floor change in AGENTS.md.",
                file=sys.stderr,
            )
            failed = True

    python = run(["uv", "run", "-q", "coverage", "combine"], cwd=repo.root)
    if python.returncode not in (0, 1):
        print(python.stderr, file=sys.stderr)
        failed = True
    report = run(
        ["uv", "run", "-q", "coverage", "report", f"--fail-under={floors['python']}"],
        cwd=repo.root,
    )
    if report.returncode != 0:
        print(report.stdout, file=sys.stderr)
        print(
            f"Python line coverage is below the {floors['python']}% floor. Add tests "
            f"that drive the uncovered lines.",
            file=sys.stderr,
        )
        failed = True

    print(
        f"coverage: rust lines {rust_total} (floor {floors['rust']}%), "
        f"python lines {_total_of(report.stdout, -1)} (floor {floors['python']}%)"
    )
    return 1 if failed else 0
