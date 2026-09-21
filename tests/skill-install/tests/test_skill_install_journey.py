"""The Agent Skill, installed by the real `gh skill install` from a copy of this tree.

`skills/printobserver/` is what a host running the supervisor takes its skill
from, and what anybody's own agent pulls from GitHub. Both take it with `gh
skill install`, which installs the skill's directory alone, drops every
symlink in it, and rewrites the installed `SKILL.md`'s frontmatter. So this
takes a copy of the finished tree's working files and installs the skill out of
it with the real `gh`, in an environment holding **no GitHub credentials** — an
empty configuration directory and no token variable, as on a printer host where
nobody signed in — and holds what arrives to what is committed: no symlink, the
same files byte for byte but for `SKILL.md`, whose prose is the committed
prose, and every link the installed skill carries opening from its own
directory.

`gh skill publish --dry-run` exits 0 whether or not validation passed, so its
verdict is read from what it prints; a copy with a broken frontmatter is run
through it too, which is what shows that reading sees a refusal when there is
one.

This is a project of its own and not one of `just check`'s tiers: it needs
GitHub CLI at the release `repo-policy.toml` holds, which the gate's runners are
not given, so its `test-skill-install` target is run by the `skill-install` job
alone. Reached without that `gh`, it refuses, naming what is missing — it never
skips.
"""

from __future__ import annotations

import os
import re
import shutil
from pathlib import Path

import pytest
from repo_checks.docs import links_of, skill_prose
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo, toolchain_tools
from repo_checks.shell import run

#: The repository this journey copies.
REPO_ROOT = Path(__file__).resolve().parents[3]

#: The skill, as `gh skill install` names it.
SKILL = "printobserver"

#: The committed skill's directory.
SKILL_DIRECTORY = REPO_ROOT / "skills" / SKILL

#: The variables through which `gh` would find a credential, every one removed.
TOKEN_VARIABLES = ("GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN")

#: A line in which `gh skill publish --dry-run` reports a refusal.
PUBLISH_REFUSAL = re.compile(r"\berror\b|validation failed", re.IGNORECASE)


def _held_release() -> str:
    """The release `repo-policy.toml` holds gh at."""
    held = {tool.command: tool.version for tool in toolchain_tools(Repo(REPO_ROOT))}
    release = held.get("gh")
    if release is None:
        message = "`repo-policy.toml` holds gh at no release, and this journey is about one"
        raise AssertionError(message)
    return release


@pytest.fixture(scope="module")
def gh() -> str:
    """The `gh` on PATH, refusing — never skipping — where there is none with `gh skill`."""
    release = _held_release()
    found = shutil.which("gh")
    if found is None:
        pytest.fail(
            f"`gh` is not on PATH. This journey runs the real `gh skill install`, which needs "
            f"GitHub CLI {release}: install it with `just install-tools gh`, or `just "
            f"install-gh {release}`, and put ~/.local/bin on PATH.",
            pytrace=False,
        )
    version = run([found, "--version"], timeout=60).stdout.strip()
    answered = run([found, "skill", "--help"], timeout=60)
    if answered.returncode != 0:
        pytest.fail(
            f"`gh skill` is missing from {found} ({version or 'no version'}): `gh skill` "
            f"arrived in GitHub CLI 2.100.0, and this repository holds gh at {release}. "
            f"Install it with `just install-tools gh`.",
            pytrace=False,
        )
    if f"gh version {release} " not in f"{version} ":
        pytest.fail(
            f"{found} answers {version.splitlines()[0] if version else 'no version'}, and this "
            f"journey is a claim about gh {release}, the release `repo-policy.toml` holds. "
            f"Install it with `just install-tools gh`.",
            pytrace=False,
        )
    return found


def _copy_of_the_tree(destination: Path) -> Path:
    """The finished tree's working files: every file a clone would carry once it lands.

    `--others --exclude-standard` includes what the change has added and not yet
    committed, because `gh skill install --from-local` reads the working tree,
    and excludes everything `.gitignore` covers.
    """
    listing = run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=REPO_ROOT,
        check=True,
    ).stdout
    for name in (entry for entry in listing.split("\0") if entry):
        source = REPO_ROOT / name
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            target.symlink_to(source.readlink(), target_is_directory=source.is_dir())
        elif source.is_file():
            shutil.copy2(source, target)
    return destination


def _signed_out(configuration: Path) -> dict[str, str]:
    """This process's environment with every way `gh` could find a credential removed."""
    environment = dict(os.environ)
    environment.pop("VIRTUAL_ENV", None)
    for variable in TOKEN_VARIABLES:
        environment.pop(variable, None)
    environment["GH_CONFIG_DIR"] = str(configuration)
    environment["GH_PROMPT_DISABLED"] = "1"
    return environment


def _regular_files(root: Path) -> dict[str, Path]:
    """Every regular file under `root`, by its path relative to it."""
    return {
        path.relative_to(root).as_posix(): path
        for path in sorted(root.rglob("*"))
        if path.is_file() and not path.is_symlink()
    }


def _symlinks(root: Path) -> list[str]:
    """Every symlink under `root`, directory or file."""
    found: list[str] = []
    for current, directories, files in os.walk(root, followlinks=False):
        found.extend(
            (Path(current) / name).relative_to(root).as_posix()
            for name in [*directories, *files]
            if (Path(current) / name).is_symlink()
        )
    return found


def _refusals(said: str) -> list[str]:
    """Every line of a dry run's output that reports a refusal."""
    return [line for line in said.splitlines() if PUBLISH_REFUSAL.search(line)]


def test_the_skill_installs_whole_from_a_copy_of_the_tree_with_no_credentials(
    gh: str, tmp_path: Path
) -> None:
    """What `gh skill install` puts down is the committed skill, and every link in it opens."""
    tree = _copy_of_the_tree(tmp_path / "tree")
    configuration = tmp_path / "gh-config"
    configuration.mkdir()
    environment = _signed_out(configuration)
    signed_in = run([gh, "auth", "status"], cwd=tmp_path, env=environment, timeout=60)
    # The state a printer host is in: `gh` holds no credential at all.
    failing(
        (signed_in.returncode, signed_in.stdout + signed_in.stderr),
        naming="not logged into any GitHub hosts",
    )

    out = tmp_path / "installed"
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    installed = run(
        [gh, "skill", "install", str(tree), SKILL, "--from-local", "--dir", str(out)],
        cwd=tmp_path,
        env=environment,
        timeout=300,
    )
    passing(
        (installed.returncode, installed.stdout + installed.stderr),
        describing="`gh skill install --from-local` with no GitHub credentials",
    )

    skill = out / SKILL
    truth(skill.is_dir(), describing=f"`gh skill install` to have written {skill}")
    equal(_symlinks(skill), [], describing="the symlinks in the installed skill")

    committed = _regular_files(tree / "skills" / SKILL)
    arrived = _regular_files(skill)
    equal(sorted(arrived), sorted(committed), describing="the files the install carried")
    for name, path in committed.items():
        if name == "SKILL.md":
            continue
        truth(
            arrived[name].read_bytes() == path.read_bytes(),
            describing=f"the installed `{name}` to be the committed one byte for byte",
        )
    committed_prose = skill_prose((SKILL_DIRECTORY / "SKILL.md").read_text(encoding="utf-8"))
    installed_prose = skill_prose((skill / "SKILL.md").read_text(encoding="utf-8"))
    equal(
        installed_prose.prose,
        committed_prose.prose,
        describing="the installed SKILL.md's prose against the committed prose",
    )

    links = links_of(installed_prose.prose)
    truth(bool(links), describing="the installed skill to link to its reference documents")
    for target in links:
        opened = skill / target
        truth(
            opened.is_file()
            and not opened.is_symlink()
            and opened.resolve().is_relative_to(skill.resolve()),
            describing=f"the installed skill's link `{target}` to open from its own directory",
        )


def test_publish_validation_finds_no_error_in_the_skill(gh: str, tmp_path: Path) -> None:
    """The dry run's verdict, read from what it prints, and shown to see a refusal."""
    configuration = tmp_path / "gh-config"
    configuration.mkdir()
    environment = _signed_out(configuration)

    tree = _copy_of_the_tree(tmp_path / "tree")
    validated = run([gh, "skill", "publish", "--dry-run"], cwd=tree, env=environment, timeout=300)
    said = validated.stdout + validated.stderr
    equal(_refusals(said), [], describing=f"what `gh skill publish --dry-run` refused:\n{said}")
    contains(said, "Dry run complete", describing="the dry run to have finished")

    broken = _copy_of_the_tree(tmp_path / "broken")
    written = broken / "skills" / SKILL / "SKILL.md"
    written.write_text(
        written.read_text(encoding="utf-8").replace("name: printobserver", "name: Print--Observer"),
        encoding="utf-8",
    )
    refused = run([gh, "skill", "publish", "--dry-run"], cwd=broken, env=environment, timeout=300)
    refusing = refused.stdout + refused.stderr
    truth(
        bool(_refusals(refusing)),
        describing=f"the dry run over a skill named against the rules to refuse it:\n{refusing}",
    )
