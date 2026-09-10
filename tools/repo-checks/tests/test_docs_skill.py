"""The skill is short, carries what it owes, and restates nothing.

The check itself is where those rules live, and it runs over the committed tree
on every `just check-repo`. What this suite establishes beside it is that the
tree it ships passes them, and that the two bounds it enforces are the two this
repository declares rather than ones the check chose for itself.
"""

from __future__ import annotations

from repo_checks.checks_docs import skill
from repo_checks.docs import docs_policy
from repo_checks.expect import accepted, equal
from repo_checks.model import Repo


def test_the_committed_skill_is_accepted(committed: Repo) -> None:
    """The skill this repository ships passes every rule the check carries."""
    accepted(skill(committed))


def test_the_check_enforces_the_bounds_this_repository_declares(committed: Repo) -> None:
    """The two numbers are 6,000 characters and 150 lines.

    A check free to choose its own budget could hold the skill to one no skill
    could exceed while buying none of the discipline the bounds are for. The
    skill is sent as the system prompt of every supervision turn, so every
    character of it is paid on every turn of every print.
    """
    policy = docs_policy(committed)

    equal(policy.skill_max_characters, 6000, describing="the character bound")
    equal(policy.skill_max_lines, 150, describing="the line bound")
