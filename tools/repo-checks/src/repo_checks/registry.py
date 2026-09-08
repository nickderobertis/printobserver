"""Every check, by the name `just check-repo` and the tests reach it under."""

from __future__ import annotations

from collections.abc import Callable

from repo_checks import (
    checks_ci,
    checks_integration,
    checks_release,
    checks_repo,
    checks_suppressions,
)
from repo_checks.model import Repo

Check = Callable[[Repo], list[str]]


def base_files(repo: Repo) -> list[str]:
    """The two files this repository was branched with are still there, intact."""
    import json

    findings: list[str] = []
    if not repo.exists("gh-secrets.json"):
        return ["gh-secrets.json is absent: it is the authoritative secret manifest"]
    if not repo.exists(".gitignore"):
        findings.append(".gitignore is absent")
    elif ".gh-secrets-state.json" not in repo.read(".gitignore"):
        findings.append(
            ".gitignore no longer ignores `.gh-secrets-state.json`, the per-machine "
            "bookkeeping the secret manifest's tooling writes"
        )
    manifest = json.loads(repo.read("gh-secrets.json"))
    if not manifest.get("secrets"):
        findings.append("gh-secrets.json declares no secrets")
    return findings


CHECKS: dict[str, Check] = {
    "agent-layer": checks_repo.agent_layer,
    "command-allowlist": checks_repo.command_allowlist,
    "recipes": checks_repo.recipe_set,
    "node-install": checks_repo.node_install,
    "workspace": checks_repo.workspace,
    "platforms": checks_ci.platforms,
    "install-path": checks_ci.install_path_section,
    "ci": checks_ci.continuous_integration,
    "integration-tier": checks_integration.integration_tier,
    "merge-model": checks_ci.merge_model,
    "secrets": checks_ci.secrets,
    "release-targets": checks_release.release_targets,
    "release-automation": checks_release.release_automation,
    "base-files": base_files,
    "suppressions": checks_suppressions.suppressions,
}

# `workflows` is run by `just lint-workflows` rather than by `just check-repo`,
# so that the workflow tier fails on its own findings rather than inside the
# repository-check tier.
WORKFLOW_CHECKS: dict[str, Check] = {"workflows": checks_ci.workflow_policy}

ALL: dict[str, Check] = {**CHECKS, **WORKFLOW_CHECKS}
