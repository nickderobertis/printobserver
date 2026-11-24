# One supervision turn

The camera's image, when there is one, is attached; the situation's `image` says which and why.

## The event

```json
{{event}}
```

## The situation when this turn began

`null` is a fact nobody knew; `arrived_while_busy` lists, oldest first, events an earlier turn never saw.

```json
{{situation}}
```

## The print's facts

As this turn began: state, plan here, bounds, interventions, detector, unseen events newest first.

```json
{{facts}}
```

## The whole context

```console
{{context_command}}
```

## The commands you may run

```console
{{commands}}
```

## Your actor

For every command whose usage lists `--actor`:

```console
--actor '{{actor}}'
```
