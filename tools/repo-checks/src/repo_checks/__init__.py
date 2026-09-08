"""This repository's own deterministic checks.

Every check here refuses on shape rather than on judgment: it reads the
committed tree against the rules `repo-policy.toml` and `AGENTS.md` declare and
names what disagrees. Nothing here asks whether a decision was a *good* one —
that is for whoever reviews the change.

Run them with `just check-repo`, or one at a time with
`uv run -q python -m repo_checks <name>`.
"""

__all__ = ["__doc__"]
