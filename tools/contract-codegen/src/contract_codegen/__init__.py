"""The one generator the three clients' request and response types come from.

`printobserver` serves one API and three clients call it. Written by hand,
those three are three products under one name: the day a contract type gains a
field, three people have to notice. So none of them is written by hand. This
package reads the checked-in schemas — the contracts' own types, the shapes the
server answers, and the description of its operations — and writes the types
and the one method per operation of each client.

What makes three copies of one contract safe to have is not where the bytes
came from, which a finished tree cannot show: it is that `just check-generated`
regenerates all three over the committed tree and refuses any difference. A
hand-written type agreeing today fails the moment the schema it agreed with
moves.
"""

from __future__ import annotations

from contract_codegen.generate import OUTPUTS, drifted, write

__all__ = ["OUTPUTS", "drifted", "write"]
