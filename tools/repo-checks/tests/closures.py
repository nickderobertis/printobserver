"""The files each check reads, so that a copy for it carries those and no others.

One entry per check `repo_checks.registry` names, by that name. A check absent
here reads the whole tree — `suppressions` scans every committed file — and a
copy for it is a whole one.

What an entry claims is proven rather than trusted twice over. `test_closures`
runs every check here over a copy of its closure and over a whole copy, and
requires the same findings from both; and `treecopy` watches every read under a
closure copy, so a test whose check opens a committed file its closure left out
fails, naming that file.
"""

from __future__ import annotations

from treecopy import Closure

POLICY = "repo-policy.toml"
AGENTS = "AGENTS.md"
JUSTFILE = "justfile"
WORKFLOWS = ".github/workflows/"
PROJECTS = "**/project.json"
SCHEMAS = "schemas/"
SKILL = "skills/"
RELEASE_TARGETS = "release-targets.toml"
RELEASE_SOURCES = "tools/release-artifacts/src/"
SDKS = ("crates/printobserver-sdk/", "npm/printobserver-sdk/", "python/printobserver-sdk/")
REAL_PRINTS = ("tests/real-prints/", SCHEMAS)

#: What each check reads, by the name the registry gives it.
CHECKS: dict[str, Closure] = {
    "agent-layer": Closure((AGENTS, "CLAUDE.md")),
    "command-allowlist": Closure((".claude/settings.json", JUSTFILE, POLICY, PROJECTS)),
    "powershell-providers": Closure((POLICY, RELEASE_SOURCES)),
    "powershell-ascii": Closure(("**/*.ps1",)),
    "recipes": Closure((JUSTFILE, POLICY, PROJECTS)),
    "python-typecheck-platforms": Closure((POLICY, PROJECTS)),
    "node-install": Closure((JUSTFILE,)),
    "workspace": Closure((POLICY, "Cargo.toml", "crates/*/Cargo.toml", "crates/*/src/lib.rs")),
    "graph-edges": Closure(
        (PROJECTS, "**/*.py", "**/Cargo.toml", "**/package.json", "pyproject.toml")
    ),
    "module-names": Closure(("pyproject.toml", "**/*.py")),
    "octoprint-client": Closure((POLICY, "crates/**/*.rs", "crates/*/Cargo.toml")),
    "supervisor-spawn-free": Closure((POLICY, "crates/printobserver-oneharness/src/")),
    "supervisor-schema-source": Closure(
        (POLICY, "crates/printobserver-oneharness/**/*.json", SCHEMAS)
    ),
    "supervisor-prompt-template": Closure(
        (POLICY, "crates/printobserver-oneharness/assets/", SCHEMAS, SKILL)
    ),
    "supervisor-schema-lock": Closure(
        (
            POLICY,
            "crates/printobserver-oneharness/tests/",
            "crates/printobserver-supervisor-api/tests/",
            "tools/repo-checks/tests/treecopy.py",
        )
    ),
    "platforms": Closure((WORKFLOWS, AGENTS, JUSTFILE, RELEASE_TARGETS, POLICY)),
    "platform-facts": Closure(
        (AGENTS, POLICY, "rust-toolchain.toml", "npm/printobserver-cli/bin/", "scripts/")
    ),
    "unmatrixed-jobs": Closure((WORKFLOWS, AGENTS, JUSTFILE, POLICY)),
    "platform-dispatch": Closure((WORKFLOWS, AGENTS, POLICY)),
    "sharded-jobs": Closure((WORKFLOWS, AGENTS, JUSTFILE, POLICY)),
    "install-path": Closure((WORKFLOWS, AGENTS, "README.md", POLICY)),
    "ci": Closure((WORKFLOWS, AGENTS, JUSTFILE, POLICY)),
    "artifact-jobs": Closure((WORKFLOWS, AGENTS, JUSTFILE, RELEASE_TARGETS, POLICY)),
    "install-script": Closure((AGENTS, "scripts/")),
    "install-path-not-narrowed": Closure((AGENTS, POLICY)),
    "integration-tier": Closure((PROJECTS, WORKFLOWS, AGENTS, JUSTFILE, "nx.json", POLICY)),
    "obico-tier": Closure((WORKFLOWS, AGENTS, JUSTFILE, POLICY)),
    "install-proof": Closure(
        (WORKFLOWS, AGENTS, JUSTFILE, RELEASE_TARGETS, POLICY, RELEASE_SOURCES)
    ),
    "smoke-payload": Closure((POLICY, "tools/printer-smoke/gcode/")),
    "smoke-selection": Closure(
        (PROJECTS, WORKFLOWS, AGENTS, JUSTFILE, POLICY, "tools/printer-smoke/printer_smoke.py")
    ),
    "real-prints-schema": Closure(REAL_PRINTS),
    "real-prints-files": Closure(REAL_PRINTS),
    "real-prints-images": Closure(REAL_PRINTS),
    "real-prints-scenarios": Closure(REAL_PRINTS),
    "real-prints-operations": Closure(REAL_PRINTS),
    "real-prints-service-config": Closure(REAL_PRINTS),
    "ingress-answer-bound": Closure((AGENTS, POLICY, "crates/printobserver-server/src/config.rs")),
    "service-install": Closure(
        (AGENTS, POLICY, "scripts/", SCHEMAS, "crates/printobserver/src/windows_service.rs")
    ),
    "merge-model": Closure((WORKFLOWS, AGENTS, JUSTFILE, POLICY)),
    "secrets": Closure((WORKFLOWS, AGENTS, "gh-secrets.json", JUSTFILE, POLICY)),
    # The generator lays each client out with the formatter its gate holds it
    # to, run over the copy, so each formatter's own configuration comes along.
    "generated-clients": Closure(
        (SCHEMAS, *SDKS, "rustfmt.toml", "pyproject.toml", "biome.json", "package.json")
    ),
    "client-surface": Closure((SCHEMAS, *SDKS)),
    "response-shapes": Closure((SCHEMAS, POLICY)),
    "journey-completeness": Closure((SCHEMAS, *SDKS)),
    "release-targets": Closure(
        (
            "**/Cargo.toml",
            "**/package.json",
            "**/pyproject.toml",
            ".githooks/",
            AGENTS,
            "release-plz.toml",
            RELEASE_TARGETS,
            POLICY,
        )
    ),
    "release-declaration": Closure((RELEASE_TARGETS, POLICY, AGENTS, "uv.lock")),
    "release-automation": Closure((WORKFLOWS, AGENTS, RELEASE_TARGETS, POLICY)),
    "release-gating": Closure((WORKFLOWS, JUSTFILE, POLICY, RELEASE_SOURCES)),
    "release-dispatch": Closure((WORKFLOWS, JUSTFILE, POLICY, RELEASE_SOURCES)),
    "release-tracking": Closure((WORKFLOWS, JUSTFILE, POLICY)),
    "publish-credentials": Closure((WORKFLOWS, "gh-secrets.json", POLICY)),
    "base-files": Closure((".gitignore", "gh-secrets.json")),
    "docs-skill": Closure((SCHEMAS, POLICY, SKILL)),
    "docs-skill-directory": Closure((POLICY, SKILL)),
    "docs-link-symlinks": Closure(("**/*.md", SKILL)),
    "docs-reference": Closure(
        (
            "**/*.rs",
            "crates/*/Cargo.toml",
            SKILL,
            WORKFLOWS,
            SCHEMAS,
            POLICY,
            "tools/printer-smoke/",
        )
    ),
    "docs-schemas": Closure((SCHEMAS, POLICY, SKILL)),
    "docs-platforms": Closure(("**/*.md", SKILL)),
    "workflows": Closure((WORKFLOWS, ".claude/settings.json", AGENTS, POLICY)),
}


def closure(*checks: str, also: tuple[str, ...] = ()) -> Closure:
    """What a module driving these checks reads, with whatever else its own tests read.

    Raises:
        KeyError: If a name is no check this table declares a closure for.
    """
    joined = Closure(also)
    for name in checks:
        joined |= CHECKS[name]
    return joined
