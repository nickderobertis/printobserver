# obico-env

The rules `obico_env.py` and `obico_tier.py` are held to, and why. The root
`AGENTS.md`'s "The scheduled Obico tier" says what the tier proves, why it runs
on a schedule rather than on every change, and how to run it by hand —
`just check-repo` reads that section and its schedule block from the root. This
file holds what only this tier's code carries.

## The capture is what is compared

Nothing in the tier compares a body the capture did not produce — the
alteration tests in `tools/obico-env/tests/test_reconciliation.py` run each alteration
through the capture rather than past the comparator, which is what makes them
proof of that. What is compared is the *shape*: the set of fields and the JSON
type of each, because the ids, the file name and the instants differ on every
run by design and are not what the sample claims. A field the producer added,
renamed, removed or retyped moves the shape and fails the tier naming it.

## The trigger starts a fresh print

One thing about the trigger — the second step of the root's walk — a reader will
otherwise meet as a mystery: Obico alerts on a print **once** and suppresses
every alert after it, which is right — a printer that alerted on the same failed
print every ten seconds would be unusable. So the trigger finishes an already-alerted print and starts a fresh one, which is what
happens between two real failures anyway. A stack that has run this tier several
times therefore carries several finished prints, and a tier that skipped this
would capture nothing on its second run and blame the network.
