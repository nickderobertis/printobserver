# One supervision turn

A print you are watching has produced an event. The camera's image of it, when
there is one, is attached to this message, and the situation's `image` line says
which image it is and why.

## The event

```json
{{event}}
```

## The situation when this turn began

Whether the printer is paused, whether the detector only warned, and whether
the detector paused the print itself; `null` is a fact nobody knew. Events for
this print that arrived while an earlier turn on it was running, and that it
never saw, are listed here too, oldest first.

```json
{{situation}}
```

## The print's facts

The print's context as it stood when this turn began: its state, the plan at
the current position, the bounds in force, the active interventions, the
detector's status, and the events you have not been shown, newest first.

```json
{{facts}}
```

## The whole context

The command that reads the print's whole context, events included:

```console
{{context_command}}
```

## The commands you may run

Each as the program prints its usage:

```console
{{commands}}
```

## Your actor

The value every command whose usage lists `--actor` takes in this turn, quoted
as written:

```console
--actor '{{actor}}'
```
