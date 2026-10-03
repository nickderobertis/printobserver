# The intervention policy

What may be asked for, what happens when it may not, and what happens to a
change that was only meant to stand for a while. This document covers where the
bounds come from, what a rejection carries, how a bounded intervention is
restored, what expiry does when no prior value was available, and what happens
to a print the detector paused.

Every claim here is stated against the thing it describes: the fields a
rejection carries are the fields the contracts' rejection type declares, the
source named for the bounds is the file the server reads them from, and the
restoration and expiry behaviours each name the journey that asserts them. A
check reads all four beside the tree; the detector's pause names its journeys
too.

## Where the bounds come from

Two things are intersected, and they are never confused with each other.

**The operator's safety envelope** is server configuration. The server reads it
from its own configuration file in `crates/printobserver-server/src/config.rs`,
which refuses to start on an envelope that admits nothing. It declares three
things: the range each adjustable may be set to, the actions each actor class
may request at all, and the minimum interval between two agent actions that
change the machine. Acknowledging a failure changes nothing there, whatever its
disposition — `stop` included, which records that the print should stop and
cancels nothing — so an acknowledgement neither waits on that interval nor
starts it again. An adjustment asked for while the print is paused does not
wait on it either, because nothing it does can show until the print moves;
pausing, resuming and cancelling still do.

**The print's manifest** is attached when a print is started and may be replaced
while it runs. A manifest may only ever *narrow*. An adjustable it names
narrower than the envelope takes the manifest's range; one it names wider is
narrowed to the envelope's, and that narrowing is recorded on the print so
nobody has to wonder later which bound applied; one it is silent about takes the
envelope's, because inheriting the envelope is what silence means. An adjustable
the *envelope* does not name is no bound at all — it is not one this printer has,
and a request for it is refused as such rather than acquiring a bound from
nowhere.

What comes out is the **effective bounds**, and that is what a context read
reports, so an actor can see its own limits before it asks rather than by being
refused. The plausibility ranges the contracts declare for what a printer may
*report* are a different contract entirely, orders of magnitude wider, and the
two are never intersected, compared or substituted for one another.

## What a rejection carries

A rejection is an answer rather than a transport failure: the body is the same
action record every accepted request answers with, carrying the decision the
policy took. Each reason is a distinct variant, so a consumer distinguishes them
by matching rather than by reading a message, and each carries exactly what a
second, acceptable request would be composed from.

- `out_of_bounds` — the value is outside the range allowed. It carries
  `adjustable`, `requested` and `allowed`, which is the whole of what is needed
  to ask again inside the range.
- `actor_may_not_request` — this actor class may not ask for this action at all.
  It carries `actor_class` and `action`. Asking again will not help; this is a
  configuration decision.
- `no_active_print` — there is no active print to act on. It carries nothing,
  because there is nothing to narrow.
- `invalid_from_state` — the printer is not in a state this action is valid
  from. It carries `state`.
- `min_interval_not_elapsed` — the agent's minimum interval has not passed. It
  carries `interval_s` and `since_last_s`, so the caller knows how long to wait.
- `unsupported_adjustable` — the adjustable is not one this printer has. It
  carries `adjustable`.

The command-line program exits with its rejected status on all of these and prints
the record,
so the reason, the value asked for and the range allowed are in front of
whoever — or whatever — is composing the next request.

## How a bounded intervention is restored

An adjustment given a duration opens a **bounded intervention**. The supervisor
reads the value the printer was holding for that adjustable *before* the change
and keeps it as the intervention's prior value, and the intervention expires on
the injected clock alone: nothing has to ask it to, and no client has to come
back.

At expiry the prior value goes back, through the same policy the original
request passed and as an action of its own carrying its own reason — so a
restoration that would now be out of bounds is refused and recorded rather than
applied behind the policy's back. An adjustment that supersedes an earlier one
carries the *earlier* one's prior value forward, so restoring still reaches the
value the print started from rather than an intermediate one.

`an_intervention_expires_on_the_clock_alone_and_restores_the_prior_value` in
`crates/printobserver-core/tests/journeys/interventions.rs` is the journey that
asserts it, by advancing the clock and nothing else.

If the restoration is refused — by the policy or by the machine — the
intervention is **settled** rather than left active, because the expiry did
happen. It reads back carrying the applied value the printer was left holding
beside the prior value it should have been returned to, so the record says what
the machine is actually holding now. Nothing retries.

## What expiry does when no prior value was available

The printer does not always report a value for the adjustable being changed. A
fan whose percentage the machine does not report, or a heater it reports no
target for, leaves the intervention with **no prior value at all**.

When such an intervention expires, nothing is sent to the machine and the
outcome is recorded as *restore unavailable*. Writing a plausible default would
be inventing a value nobody observed and then driving a heater to it, which is
the one thing worse than leaving it where it is. The record says the change
expired and that there was nothing to put back, which is what an operator needs
in order to decide.

`an_intervention_with_no_prior_value_restores_nothing_and_says_so` in
`crates/printobserver-core/tests/journeys/interventions.rs` is the journey that
asserts it, and it asserts the machine was asked for nothing as well as the
outcome that was recorded.

## What happens to a print the detector paused

A failure detector that pauses a print reacts faster than any turn, so its
pause is left standing while the agent looks. When the agent asks for an
adjustment while the detector's pause holds the print, the adjustment is
applied at once, and the supervisor then resumes the print **as the system
actor, through the same policy** — twenty seconds after the agent's last such
adjustment, or when its turn ends, whichever comes first — so that two or three
changes reach the print together as it moves again. The agent is not granted
resume and does not need it.

A resume the policy refuses, because the safety envelope does not grant the
system resume, leaves the print paused, and the refusal is in the record like
any other. Once the print has resumed, the detector is told its detection was
handled — for Obico, an acknowledgement through its own API with the overwrite
`FAILED` — which is what re-arms it for the rest of the print; an
acknowledgement it refuses is recorded as a port failure against the
detection's own event. An agent that acknowledges the detection with the
disposition `stop` has decided a person should look, and the pause is then left
alone; so is a print somebody else resumed or cancelled.

`an_adjustment_during_the_turn_resumes_the_print_when_the_turn_ends`,
`an_adjustment_outside_a_turn_resumes_the_print_after_the_grace`,
`acknowledging_stop_leaves_the_pause_for_a_person` and
`a_system_not_granted_resume_leaves_the_print_paused` in
`crates/printobserver-core/tests/journeys/detector_pause.rs` are the journeys
that assert it.
