"""Wrapping a description the contracts wrote for a comment in a client.

None of the three formatters that run over the generated clients reflows a
comment, so a description written as one long sentence stays one long line in
three languages unless it is wrapped here. Wrapping is done once, on the text
rather than on the comment, so the three clients break the same sentence in
the same places.
"""

from __future__ import annotations

import textwrap

#: How wide a wrapped comment is, before the comment marker and the
#: indentation in front of it. Short enough that the deepest indentation any
#: emitter reaches still leaves the line inside the width the formatters use.
WIDTH = 74

#: What a shape the contracts say nothing about is documented as. Every public
#: item of the Rust client needs a sentence, and an empty one is a lint.
UNSAID = "One shape of the contracts, which say nothing more about it."


def wrapped(text: str, indent: str = "") -> list[str]:
    """One description as comment lines, blank lines kept as paragraph breaks."""
    width = max(WIDTH - len(indent), 32)
    lines: list[str] = []
    for paragraph in (text or UNSAID).split("\n"):
        if not paragraph.strip():
            lines.append("")
            continue
        lines.extend(
            textwrap.wrap(
                paragraph,
                width=width,
                break_long_words=False,
                break_on_hyphens=False,
            )
        )
    return lines
