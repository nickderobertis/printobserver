"""How the real-print suite reads a shell line an agent wrote, as its compound-command report does.

The table is the start replays' own, kept identical so that both read one line one way.
"""

from __future__ import annotations

import pytest
from repo_checks.expect import equal
from shell_lines import commands, is_compound


@pytest.mark.parametrize(
    ("line", "compound"),
    [
        ("printobserver look --json", False),
        ("printobserver start-print --reason 'clear; a && b'", False),
        ("cd /tmp && printobserver prints", True),
        ("printobserver prints; printobserver look", True),
        ("printobserver look | head", True),
        ("printobserver look\nprintobserver prints", True),
        ("ls -la && cat x", False),
        ("printobserver start-print --reason a\\;b", False),
        ('"printobserver" look; echo done', True),
        ("echo $(printobserver prints)", True),
        # A substitution runs its command inside the one around it, quoted or not.
        ("echo `printobserver prints`", True),
        ('echo "$(printobserver prints)"', True),
        ('echo "`printobserver prints`"', True),
        ("echo $(echo x; printobserver prints)", True),
        ('printobserver start-print --reason "the `smooth-pei` sheet"', True),
        # The same words as literal text run nothing.
        ("echo 'printobserver prints'", False),
        ('echo "printobserver prints"', False),
        ("echo '`printobserver prints`'", False),
        ("echo '$(printobserver prints)'", False),
        ('echo "\\`printobserver prints\\`"', False),
        ("grep printobserver notes.txt", False),
        # A program run through another is run inside that other command.
        ("env X=1 printobserver prints", True),
        ("sudo -u root printobserver prints", True),
        ("nohup printobserver prints", True),
        ("timeout 30 printobserver look", True),
        ("sh -c 'printobserver prints'", True),
        ('bash -c "printobserver prints"', True),
        ("X=1 printobserver prints", False),
        ("bash -lc 'printobserver prints'", True),
        ("env FOO=1 nice -n 5 printobserver prints", True),
        ("printobserver look 2>&1", False),
        ("printobserver look &> out", False),
        ("X=1 printobserver prints; echo done", True),
        ("/usr/local/bin/printobserver prints | head", True),
        ("echo 'printobserver prints; echo'", False),
        ('printobserver start-print --reason "clear (smooth PEI)"', False),
        ("printobserver look --reason 'a `b` c'", False),
        ("find / -name x 2>/dev/null; which printobserver", False),
        ("(printobserver prints); echo done", True),
        # A group runs its commands in a shell of their own, one command or several.
        ("(printobserver prints)", True),
        ("( printobserver prints )", True),
        ("{ printobserver prints; }", True),
        ("printobserver start-print --reason 'a { b } (c)'", False),
        ("printobserver look # ; echo done", False),
        ("printobserver look --reason 'unclosed", False),
        ("env X=1 printobserver prints && echo done", True),
    ],
)
def test_a_compound_command_runs_the_program_beside_or_inside_another(
    line: str, *, compound: bool
) -> None:
    """Operators, substitutions and wrappers run another command; literal quoted text does not."""
    equal(is_compound(line), compound, describing=line)


def test_every_command_a_line_runs_is_read_wrappers_and_substitutions_included() -> None:
    """A wrapper's program is read as the command it runs, as is a shell's line."""
    equal(commands("env echo hello"), [["env"], ["echo", "hello"]])
    equal(commands("timeout 30 ls -l"), [["timeout", "30"], ["ls", "-l"]])
    equal(commands("sh -c 'ls; pwd'"), [["sh", "-c", "ls; pwd"], ["ls"], ["pwd"]])
    equal(commands("echo `ls`"), [["echo", "__substitution__"], ["ls"]])
