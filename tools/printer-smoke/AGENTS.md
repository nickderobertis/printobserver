# printer-smoke

The rules `printer_smoke.py` is held to on its way out of a run, and why. The
root `AGENTS.md` holds the repository-wide rules; this file holds the ones only
this script carries: how it leaves the machine.

**It cleans up on every exit path it has** — a completed run as much as a failed
or an interrupted one, since a run that finishes without restoring what it
changed leaves the machine altered exactly as a crashed one does. The restore
comes *before* the cancel, and that ordering is load-bearing: an adjustment is
valid from a printing or a paused machine and from no other state, so a run that
cancelled first could never put back what it changed. One adjustable that cannot
be put back does not cost the ones after it: every one is attempted, and what
could not be restored is collected rather than raised at the first.

**And a run that could not put everything back says so and exits non-zero.**
What the cleanup managed is not taken on trust: afterwards the machine is read
once more, and every value still carrying this run's own — and a printer not
left operational — is printed as `LEFT CHANGED` and makes the run fail, whether
or not any verification point did. A green report over a machine still holding a
modified feedrate is the worst answer this program can give, and it is worse
than the failure it would be hiding. Where a verification point *did* fail, that
failure is what is reported first and the cleanup is reported beneath it: a
cleanup that could not finish never replaces the cause a reader needs.

**A command that never answers is that command's failure and nothing more.** A
run that hung, or one whose program could not be started at all, comes back as
an exit no answer carries rather than as an exception out of the middle of the
cleanup — because letting one out there would abandon the restorations after it
and the cancellation with them, on a machine this run has already moved. So the
bound one command is given (`PRINTOBSERVER_SMOKE_COMMAND_TIMEOUT_S`, two minutes
by default) is enforced where the command is run, a restoration that never
answers costs that adjustable and no other, and a cancellation is still asked
for over a machine whose state could not be read — a print that is not running
refuses it and nothing moves, while one that is running is this run's own and
must not be left behind. And where the last look at the machine is the thing
that did not answer, that is `UNVERIFIED` and it fails the run too: a machine
nothing could see is not one this test may report green on.
