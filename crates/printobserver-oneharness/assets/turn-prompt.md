# One supervision turn

A print you are watching has produced an event. Read what the supervisor
already knows about the print, look at the picture that came with the event,
decide, and act as your skill describes. Everything you ask for goes through
the same policy an operator's requests do, and that policy decides what your
role may change.

## The event

```json
{{event}}
```

## The situation when this turn began

Whether the printer is paused, whether the detector only warned, and whether
the detector paused the print itself. Events that arrived while an earlier turn
on this print was running, and that it never saw, are listed here too.

```json
{{situation}}
```

## The picture

{{image_path}}

## What the supervisor already knows

Run this command and read what it answers:

```console
{{context_command}}
```

Give every other command you run the same `--config` file this one takes. It
names the supervisor this turn belongs to and the credential it answers to.

Every request you make names who is asking. In this turn that is you, in the
session this turn runs in, so give each request this actor, quoted as written:

```console
--actor '{{actor}}'
```

This picture and that context are from when the event arrived. Your skill says
how to take a fresh look at the print and how to watch it for a while before
deciding; new events for this print reach you through those looks rather than
through a second turn.

## What to answer with

Once you have done what you decided to do, answer with the assessment document
alone: one line saying what is happening, how sure you are, whether the print
should carry on, what you did (each command you ran and what it answered), why
you did it, and whether you are escalating to a person. Say plainly when you
are not sure. A low-confidence reading a person can act on is worth more than a
confident guess.
