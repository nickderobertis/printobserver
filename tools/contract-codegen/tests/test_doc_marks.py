"""Every one of the contracts' type names a description says is put in code marks.

Driven through the reader the generator writes every client from, over a copy
of the checked-in schemas whose one description names a type everywhere a
sentence can: standing alone, beside punctuation, already in code marks, as
part of a longer word, and inside the rustdoc link the reader takes apart.
Only the first two are marked, which is what a client's documentation linter
holds a comment to; the committed clients being exactly what the generator
writes is `test_correspondence`'s.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

from conftest import REPO_ROOT
from contract_codegen.model import Struct
from contract_codegen.schemas import SCHEMAS_DIR, load
from repo_checks.expect import equal, truth

#: The type whose field's description the journey rewrites, and that field.
TYPE = "ImageRecord"
FIELD = "byte_len"

#: The sentence written, and what the reader hands every client for it. The
#: type's own name is left as prose inside its own documentation, which is the
#: reader's rule rather than this sentence's, so the names here are others.
SAID = (
    "ActionRecord, (ActionRecord) and ActionKind. stand alone; `ActionRecord` is "
    "marked already, ActionRecords and MyActionKind are other words, and "
    "[`PrintRecord`](crate::PrintRecord) is a link. ImageRecord is this type."
)
MARKED = (
    "`ActionRecord`, (`ActionRecord`) and `ActionKind`. stand alone; `ActionRecord` is "
    "marked already, ActionRecords and MyActionKind are other words, and "
    "`PrintRecord` is a link. ImageRecord is this type."
)


def test_a_type_name_is_marked_as_code_wherever_it_stands_alone(tmp_path: Path) -> None:
    """A name alone or beside punctuation is marked, and nowhere else is."""
    shutil.copytree(REPO_ROOT / SCHEMAS_DIR, tmp_path / SCHEMAS_DIR)
    path = tmp_path / SCHEMAS_DIR / "printobserver-core" / f"{TYPE}.json"
    schema = json.loads(path.read_text(encoding="utf-8"))
    schema["properties"][FIELD]["description"] = SAID
    path.write_text(json.dumps(schema, indent=2), encoding="utf-8")

    declared = [one for one in load(tmp_path).declarations if one.name == TYPE]

    equal(len(declared), 1, describing=f"the declarations named {TYPE}")
    struct = declared[0]
    truth(isinstance(struct, Struct), describing=f"{TYPE} is read as a struct")
    if isinstance(struct, Struct):
        docs = {field.name: field.doc for field in struct.fields}
        equal(docs.get(FIELD), MARKED, describing=f"the documentation {TYPE}.{FIELD} carries")
