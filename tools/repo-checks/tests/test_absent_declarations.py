"""What each check says when the thing it reads is absent or malformed.

A check that crashed on a missing file would leave the gate reporting an error
nobody can act on; each of these asserts it names what is missing instead.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from repo_checks.checks_ci import install_path_section, merge_model, platforms, secrets
from repo_checks.checks_release import release_targets
from repo_checks.checks_repo import agent_layer, recipe_set, workspace
from repo_checks.model import Repo
from repo_checks.registry import base_files
from treecopy import Tree

BLOCKS = ("composition-record", "supported-platforms", "required-checks")


def _drop_block(tree: Tree, name: str) -> None:
    text = tree.read("AGENTS.md")
    start = text.index(f"[//]: # (BEGIN {name})")
    end = text.index(f"[//]: # (END {name})") + len(f"[//]: # (END {name})")
    tree.write("AGENTS.md", text[:start] + text[end:])


def test_a_tree_without_agents_md_is_refused(tree: Callable[[], Tree]) -> None:
    """The always-loaded instruction layer is the first thing a check looks for."""
    broken = tree()
    broken.remove("AGENTS.md")

    assert agent_layer(broken.repo) == ["AGENTS.md is absent"]


def test_a_missing_composition_block_is_refused(tree: Callable[[], Tree]) -> None:
    """A record nobody can find is a record nobody wrote."""
    broken = tree()
    _drop_block(broken, "composition-record")

    findings = agent_layer(broken.repo)

    assert any("no `composition-record` marker block" in f for f in findings), findings


def test_a_missing_platform_block_is_refused(tree: Callable[[], Tree]) -> None:
    """Without the list there is no source for the matrices."""
    broken = tree()
    _drop_block(broken, "supported-platforms")

    findings = platforms(broken.repo)

    assert any("no `supported-platforms` marker block" in f for f in findings), findings


def test_a_missing_required_checks_block_is_refused(tree: Callable[[], Tree]) -> None:
    """The merge model has to say what is required on the path to the base branch."""
    broken = tree()
    _drop_block(broken, "required-checks")

    findings = merge_model(broken.repo)

    assert any("no `required-checks` marker block" in f for f in findings), findings


def test_dropping_the_no_direct_push_statement_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """How a change reaches the base branch is part of the recorded model."""
    broken = tree()
    broken.write("AGENTS.md", broken.read("AGENTS.md").replace("takes no direct push", "is open"))

    findings = merge_model(broken.repo)

    assert any("no direct push" in f for f in findings), findings


def test_a_missing_check_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """There is no gate without the recipe that is the gate."""
    broken = tree()
    text = broken.read("justfile")
    start = text.index("check:\n")
    end = text.index("# Rewrite every project's sources")
    broken.write("justfile", text[:start] + text[end:])

    assert recipe_set(broken.repo) == ["the justfile declares no `check` recipe"]


def test_a_missing_bootstrap_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """A gate a clean clone cannot reach is a gate nobody can run."""
    broken = tree()
    text = broken.read("justfile")
    start = text.index("bootstrap:\n")
    end = text.index("# Install the tools")
    broken.write("justfile", text[:start] + text[end:])

    findings = recipe_set(broken.repo)

    assert any("no `bootstrap` recipe" in f for f in findings), findings


def test_a_declared_tier_with_no_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier the policy declares and the justfile does not define is a hole."""
    broken = tree()
    broken.edit("repo-policy.toml", '    "build",\n', '    "build",\n    "smoke",\n')

    findings = recipe_set(broken.repo)

    assert any("but no recipe defines it" in f for f in findings), findings


def test_a_crate_without_a_source_file_is_refused(tree: Callable[[], Tree]) -> None:
    """A crate with no root module has no module comment to read."""
    broken = tree()
    broken.remove("crates/printobserver-sdk/src/lib.rs")

    findings = workspace(broken.repo)

    assert any("has no `src/lib.rs`" in f for f in findings), findings


def test_a_platform_specific_dependency_still_counts(tree: Callable[[], Tree]) -> None:
    """A forbidden edge hidden under `[target.'cfg(...)'.dependencies]` is still an edge."""
    broken = tree()
    broken.write(
        "crates/printobserver-core/Cargo.toml",
        broken.read("crates/printobserver-core/Cargo.toml")
        + '\n[target."cfg(unix)".dependencies]\n'
        'printobserver-obico = { path = "../printobserver-obico" }\n',
    )

    findings = workspace(broken.repo)

    assert any("printobserver-obico" in f for f in findings), findings


def test_a_target_pointing_at_a_missing_manifest_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A declaration that points nowhere cannot be published from."""
    broken = tree()
    broken.edit(
        "release-targets.toml",
        'manifest = "crates/printobserver-sdk/Cargo.toml"',
        'manifest = "crates/printobserver-gone/Cargo.toml"',
    )

    findings = release_targets(broken.repo)

    assert any("at a missing" in f for f in findings), findings


def test_a_tree_without_release_plz_is_refused(tree: Callable[[], Tree]) -> None:
    """Release automation that is not configured is not automation."""
    broken = tree()
    broken.remove("release-plz.toml")

    findings = release_targets(broken.repo)

    assert any("not configured" in f for f in findings), findings


def test_a_release_rule_that_releases_everything_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Without a rule, a generated release commit opens the next release."""
    broken = tree()
    text = broken.read("release-plz.toml")
    start = text.index("release_commits = ")
    end = text.index("\n", start) + 1
    broken.write("release-plz.toml", text[:start] + text[end:])

    findings = release_targets(broken.repo)

    assert any("every commit would release" in f for f in findings), findings


def test_a_changelog_that_drops_a_releasing_type_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A release whose changelog omits what changed says nothing to a consumer."""
    broken = tree()
    broken.edit("release-plz.toml", '{ message = "^feat", group = "Added" },\n', "")

    findings = release_targets(broken.repo)

    assert any("does not group `feat`" in f for f in findings), findings


def test_a_missing_commit_msg_hook_is_refused(tree: Callable[[], Tree]) -> None:
    """Something has to rule on a commit subject before it becomes history."""
    broken = tree()
    broken.remove(".githooks/commit-msg")

    findings = release_targets(broken.repo)

    assert any("no hook rules on a commit subject" in f for f in findings), findings


def test_a_hook_carrying_its_own_type_list_is_refused(tree: Callable[[], Tree]) -> None:
    """A second copy of the list is a second thing that can disagree with the first."""
    broken = tree()
    broken.write(".githooks/commit-msg", "#!/usr/bin/env bash\ngrep -Eq '^(feat|fix): ' \"$1\"\n")

    findings = release_targets(broken.repo)

    assert any("can disagree" in f for f in findings), findings


def test_a_tree_without_the_secret_manifest_is_refused(tmp_path: Path) -> None:
    """The manifest is the authoritative list; nothing else names a secret."""
    root = tmp_path / "tree"
    root.mkdir()

    assert base_files(Repo(root)) == [
        "gh-secrets.json is absent: it is the authoritative secret manifest"
    ]


def test_a_manifest_declaring_no_secrets_is_refused(tmp_path: Path) -> None:
    """An empty manifest would make every workflow reference a finding."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "gh-secrets.json").write_text('{"secrets": []}', encoding="utf-8")

    findings = base_files(Repo(root))

    assert any("declares no secrets" in f for f in findings), findings
    assert any(".gitignore is absent" in f for f in findings), findings


def test_a_workflow_without_a_release_job_leaves_the_secret_check_saying_so(
    tree: Callable[[], Tree],
) -> None:
    """The credential rules only make sense once a release path exists."""
    broken = tree()
    broken.remove(".github/workflows/release-plz.yml")

    findings = secrets(broken.repo)

    assert any("no committed workflow performs releases" in f for f in findings), findings


def test_a_section_with_a_route_stating_no_command_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A route with nothing to paste is a route nobody can take."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("```console\npip install printobserver-cli\n```")
    end = start + len("```console\npip install printobserver-cli\n```")
    broken.write("AGENTS.md", text[:start] + text[end:])

    findings = install_path_section(broken.repo)

    assert any("states no command" in f for f in findings), findings


def test_a_matrix_running_a_platform_on_the_wrong_runner_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The list names the runner too, so a matrix cannot quietly move a platform."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "            runner: ubuntu-24.04-arm\n",
        "            runner: ubuntu-22.04\n",
    )

    findings = platforms(broken.repo)

    assert any("but AGENTS.md declares" in f for f in findings), findings


def test_an_allowlist_entry_missing_a_field_is_refused(tree: Callable[[], Tree]) -> None:
    """An entry that names no site anchors to nothing."""
    from repo_checks.checks_suppressions import suppressions

    broken = tree()
    broken.write(
        "suppressions.toml",
        broken.read("suppressions.toml")
        + '\n[[suppression]]\nrule = "dead_code"\nfile = "x.rs"\nsite = ""\nreason = "why"\n',
    )

    findings = suppressions(broken.repo)

    assert any("names no site" in f for f in findings), findings
