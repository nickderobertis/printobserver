"""How one name on the wire is spelled in each of the three clients.

Spelled once here rather than three times in the emitters, because the check
that holds the three clients to the server's own operation list has to spell
them the same way the emitters did — and a method a check cannot find is a
method the check reports as missing.
"""

from __future__ import annotations

import re

#: Every word Rust reserves. A field of a contract type spelled as one of these
#: is emitted raw (`r#type`), which serde still reads under its own name.
RUST_KEYWORDS = frozenset(
    [
        "as",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "async",
        "await",
        "abstract",
        "become",
        "box",
        "do",
        "final",
        "macro",
        "override",
        "priv",
        "typeof",
        "unsized",
        "virtual",
        "yield",
        "try",
        "gen",
    ]
)

#: Every word TypeScript reserves that a contract field could collide with.
TYPESCRIPT_KEYWORDS = frozenset(
    [
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
    ]
)

#: Every word Python reserves that a contract field could collide with. A
#: `TypedDict` key that is one of these is written in the functional form,
#: which takes the key as a string.
PYTHON_KEYWORDS = frozenset(
    [
        "False",
        "None",
        "True",
        "and",
        "as",
        "assert",
        "async",
        "await",
        "break",
        "class",
        "continue",
        "def",
        "del",
        "elif",
        "else",
        "except",
        "finally",
        "for",
        "from",
        "global",
        "if",
        "import",
        "in",
        "is",
        "lambda",
        "nonlocal",
        "not",
        "or",
        "pass",
        "raise",
        "return",
        "try",
        "while",
        "with",
        "yield",
        "match",
        "case",
    ]
)

_WORD = re.compile(r"[^0-9a-zA-Z]+")


def pascal(name: str) -> str:
    """`set_feedrate_factor` as a type or variant identifier."""
    return "".join(word[:1].upper() + word[1:] for word in _WORD.split(name) if word)


def camel(name: str) -> str:
    """`set_feedrate_factor` as the JavaScript spelling of a method."""
    spelled = pascal(name)
    return spelled[:1].lower() + spelled[1:]


def rust_identifier(name: str) -> str:
    """One wire name as a Rust binding, raw where Rust reserves it."""
    return f"r#{name}" if name in RUST_KEYWORDS else name


def typescript_property(name: str) -> str:
    """One wire name as a TypeScript property, quoted where it must be."""
    if re.fullmatch(r"[A-Za-z_$][A-Za-z0-9_$]*", name) and name not in TYPESCRIPT_KEYWORDS:
        return name
    return f'"{name}"'


def typescript_member(name: str) -> str:
    """How one wire name is reached on an object being built.

    A plain name is reached with a dot and anything else with a subscript,
    which is what the linter this repository runs over the Node client asks
    for.
    """
    spelled = typescript_property(name)
    return f"[{spelled}]" if spelled.startswith('"') else f".{spelled}"


def python_identifier(name: str) -> bool:
    """Whether one wire name can be a Python identifier at all."""
    return name.isidentifier() and name not in PYTHON_KEYWORDS


def method_name(operation: str, language: str) -> str:
    """What one operation is called in one client.

    Two of the three spell a method the way the operation is spelled, and the
    third spells it the way its own ecosystem spells every other method. That
    is the whole of the difference between the three surfaces.

    Raises:
        ValueError: If the language is not one of the three clients'.
    """
    if language in {"rust", "python"}:
        return operation
    if language == "typescript":
        return camel(operation)
    msg = f"there is no `{language}` client for an operation to be named in"
    raise ValueError(msg)
