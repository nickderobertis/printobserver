# The schemas

Every JSON Schema this system's contracts generate, one entry per type in the
declared schema set. This document covers the schema set and the schemas
themselves.

**This document is generated.** Every entry below is exactly what that type
emits — nothing here is transcribed, and `just docs-generate` is what writes it.
A check refuses a tree in which an entry is not what the type generates, in which
a member of the set has no entry, or in which an entry names something the
contracts do not declare.

## The schema set

74 types. The set is every type `printobserver-types` declares as its
own, together with the six request and answer shapes the four port crates own —
the shapes their methods carry across a process boundary. A port's own error
vocabulary is deliberately not in it: it reaches no process boundary, so it emits
no schema.

The set is read off the tree the contracts' generation target writes and prunes,
so a type entering or leaving it moves this document with it.

## The schemas

### AcknowledgementDisposition

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What an operator's acknowledgement of a failure event asks for next.",
  "oneOf": [
    {
      "const": "continue",
      "description": "Carry on printing.",
      "type": "string"
    },
    {
      "const": "watch",
      "description": "Carry on printing, watched more closely.",
      "type": "string"
    },
    {
      "const": "stop",
      "description": "Stop the print.",
      "type": "string"
    }
  ],
  "title": "AcknowledgementDisposition"
}
```

### ActionExecutedPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "An accepted action reached the printer.",
  "properties": {
    "action_id": {
      "$ref": "#/$defs/ActionId",
      "description": "The action's identifier."
    },
    "intervention_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/InterventionId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The bounded intervention it opened, when it opened one."
    }
  },
  "required": [
    "action_id"
  ],
  "title": "ActionExecutedPayload",
  "type": "object"
}
```

### ActionId

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "ActionId",
  "type": "string"
}
```

### ActionKind

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
  "oneOf": [
    {
      "const": "pause",
      "description": "Pause the print.",
      "type": "string"
    },
    {
      "const": "resume",
      "description": "Resume the print.",
      "type": "string"
    },
    {
      "const": "cancel",
      "description": "Cancel the print.",
      "type": "string"
    },
    {
      "const": "start_print",
      "description": "Start a print of a named file.",
      "type": "string"
    },
    {
      "const": "set_feedrate_factor",
      "description": "Set the feedrate factor.",
      "type": "string"
    },
    {
      "const": "set_flowrate_factor",
      "description": "Set the flowrate factor.",
      "type": "string"
    },
    {
      "const": "set_tool_target_c",
      "description": "Set a tool's target temperature.",
      "type": "string"
    },
    {
      "const": "set_bed_target_c",
      "description": "Set the bed's target temperature.",
      "type": "string"
    },
    {
      "const": "set_fan_percent",
      "description": "Set the fan percentage.",
      "type": "string"
    },
    {
      "const": "acknowledge_failure",
      "description": "Acknowledge a failure event.",
      "type": "string"
    }
  ],
  "title": "ActionKind"
}
```

### ActionRecord

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRequest": {
      "additionalProperties": false,
      "description": "One request an actor made, at the instant it made it.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        },
        "requested_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When they asked."
        }
      },
      "required": [
        "action",
        "actor",
        "requested_at"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "ExecutionOutcome": {
      "description": "What happened when an accepted action reached the printer.",
      "oneOf": [
        {
          "const": "succeeded",
          "description": "The printer took it.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer did not, for this reason.",
          "properties": {
            "failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "failed"
          ],
          "type": "object"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One request, the decision taken on it, and what became of it.\n\n`executed_at` and `outcome` are both optional and are absent together: a\nrecord is written when the decision is taken, which is before \u2014 and, for a\nrejected request, instead of \u2014 anything reaching the printer.",
  "properties": {
    "decision": {
      "$ref": "#/$defs/PolicyDecision",
      "description": "The decision policy took on it."
    },
    "executed_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When it reached the printer, if it did."
    },
    "id": {
      "$ref": "#/$defs/ActionId",
      "description": "This record's identifier, minted by the store."
    },
    "outcome": {
      "anyOf": [
        {
          "$ref": "#/$defs/ExecutionOutcome"
        },
        {
          "type": "null"
        }
      ],
      "description": "What the printer made of it, if it reached the printer."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print this request was made against."
    },
    "request": {
      "$ref": "#/$defs/ActionRequest",
      "description": "The request itself."
    }
  },
  "required": [
    "id",
    "print_id",
    "request",
    "decision"
  ],
  "title": "ActionRecord",
  "type": "object"
}
```

### ActionRejectedPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Policy refused an action.",
  "properties": {
    "action_id": {
      "$ref": "#/$defs/ActionId",
      "description": "The action's identifier."
    },
    "decision": {
      "$ref": "#/$defs/PolicyDecision",
      "description": "The whole decision, carrying which rejection it was."
    }
  },
  "required": [
    "action_id",
    "decision"
  ],
  "title": "ActionRejectedPayload",
  "type": "object"
}
```

### ActionRequest

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One request an actor made, at the instant it made it.",
  "properties": {
    "action": {
      "$ref": "#/$defs/PrintAction",
      "description": "What was asked for."
    },
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who asked."
    },
    "requested_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When they asked."
    }
  },
  "required": [
    "action",
    "actor",
    "requested_at"
  ],
  "title": "ActionRequest",
  "type": "object"
}
```

### ActionRequestedPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "An actor asked for an action.",
  "properties": {
    "action": {
      "$ref": "#/$defs/PrintAction",
      "description": "What was asked for."
    },
    "action_id": {
      "$ref": "#/$defs/ActionId",
      "description": "The action's identifier."
    },
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who asked."
    }
  },
  "required": [
    "action_id",
    "action",
    "actor"
  ],
  "title": "ActionRequestedPayload",
  "type": "object"
}
```

### Actor

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Who asked for something.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "The supervising agent, naming its session.",
      "properties": {
        "agent": {
          "additionalProperties": false,
          "properties": {
            "session_name": {
              "description": "The supervision session the agent is acting in.",
              "type": "string"
            }
          },
          "required": [
            "session_name"
          ],
          "type": "object"
        }
      },
      "required": [
        "agent"
      ],
      "type": "object"
    },
    {
      "const": "operator",
      "description": "A person.",
      "type": "string"
    },
    {
      "const": "system",
      "description": "The supervisor itself.",
      "type": "string"
    }
  ],
  "title": "Actor"
}
```

### ActorClass

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "An actor class, which is what a safety envelope grants actions to.",
  "oneOf": [
    {
      "const": "agent",
      "description": "The supervising agent.",
      "type": "string"
    },
    {
      "const": "operator",
      "description": "A person.",
      "type": "string"
    },
    {
      "const": "system",
      "description": "The supervisor itself.",
      "type": "string"
    }
  ],
  "title": "ActorClass"
}
```

### Adjustable

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One thing an adjustment may change.",
  "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
  "title": "Adjustable",
  "type": "string"
}
```

### AgentAssessment

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
  "properties": {
    "confidence": {
      "$ref": "#/$defs/Confidence",
      "description": "How sure the agent is."
    },
    "did": {
      "description": "What the agent did.",
      "type": "string"
    },
    "escalating": {
      "description": "Whether the agent is escalating to a person.",
      "type": "boolean"
    },
    "should_continue": {
      "description": "Whether the print should carry on.",
      "type": "boolean"
    },
    "summary": {
      "description": "One line saying what is happening.",
      "type": "string"
    },
    "why": {
      "description": "Why it did it.",
      "type": "string"
    }
  },
  "required": [
    "summary",
    "confidence",
    "should_continue",
    "did",
    "why",
    "escalating"
  ],
  "title": "AgentAssessment",
  "type": "object"
}
```

### AgentAssessmentPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The agent wrote down what it made of a turn.",
  "properties": {
    "assessment": {
      "$ref": "#/$defs/AgentAssessment",
      "description": "What the agent answered with."
    },
    "session_name": {
      "description": "The session the turn ran in.",
      "type": "string"
    }
  },
  "required": [
    "session_name",
    "assessment"
  ],
  "title": "AgentAssessmentPayload",
  "type": "object"
}
```

### Confidence

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
  "oneOf": [
    {
      "const": "low",
      "description": "Not sure.",
      "type": "string"
    },
    {
      "const": "medium",
      "description": "Fairly sure.",
      "type": "string"
    },
    {
      "const": "high",
      "description": "Sure.",
      "type": "string"
    }
  ],
  "title": "Confidence"
}
```

### EffectiveBounds

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The envelope intersected with the print's manifest.\n\nThis is what context reports, so that the agent can see its own limits\nbefore it asks. Computing one is the supervision core's; this crate declares\nthe shape and nothing that produces it.",
  "properties": {
    "allowed": {
      "additionalProperties": false,
      "description": "The range each adjustable may be set to, inclusive.",
      "patternProperties": {
        "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
          "$ref": "#/$defs/Range"
        }
      },
      "type": "object"
    }
  },
  "required": [
    "allowed"
  ],
  "title": "EffectiveBounds",
  "type": "object"
}
```

### EventDraft

Declared by `printobserver-store-api`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from.",
      "oneOf": [
        {
          "const": "obico",
          "description": "Obico, over its webhook.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One event on its way into the store, before the store mints its identifier.",
  "oneOf": [
    {
      "description": "Obico reported a print failure.",
      "properties": {
        "kind": {
          "const": "obico_failure_alert",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoFailureAlertPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Obico sent a printer notification.",
      "properties": {
        "kind": {
          "const": "obico_printer_notification",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoPrinterNotificationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An external body arrived that could not be read.",
      "properties": {
        "kind": {
          "const": "malformed_external_event",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/MalformedExternalEventPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An actor asked for an action.",
      "properties": {
        "kind": {
          "const": "action_requested",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRequestedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An accepted action reached the printer.",
      "properties": {
        "kind": {
          "const": "action_executed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionExecutedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Policy refused an action.",
      "properties": {
        "kind": {
          "const": "action_rejected",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRejectedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A bounded intervention expired.",
      "properties": {
        "kind": {
          "const": "intervention_expired",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/InterventionExpiredPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was opened.",
      "properties": {
        "kind": {
          "const": "supervision_session_opened",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionOpenedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was closed.",
      "properties": {
        "kind": {
          "const": "supervision_session_closed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionClosedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "kind": {
          "const": "agent_assessment",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/AgentAssessmentPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An operator acknowledged an event.",
      "properties": {
        "kind": {
          "const": "operator_acknowledgement",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/OperatorAcknowledgementPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A port failed while an event was being handled.",
      "properties": {
        "kind": {
          "const": "port_failure",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/PortFailurePayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "kind": {
          "const": "startup_reconciliation",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/StartupReconciliationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    }
  ],
  "properties": {
    "print_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrintId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The print it belongs to, when it belongs to one."
    },
    "raw": {
      "anyOf": [
        {
          "$ref": "#/$defs/RawBytes"
        },
        {
          "type": "null"
        }
      ],
      "description": "The bytes exactly as received, for an externally sourced event."
    },
    "received_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was received."
    },
    "source": {
      "$ref": "#/$defs/EventSource",
      "description": "Where it came from."
    }
  },
  "required": [
    "source",
    "received_at"
  ],
  "title": "EventDraft",
  "type": "object"
}
```

### EventId

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one event.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "EventId",
  "type": "string"
}
```

### EventKind

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Which event this is, without its payload.\n\nEvery kind here has exactly one [`EventPayload`] variant, and the spellings\nare the same on the wire.",
  "oneOf": [
    {
      "const": "obico_failure_alert",
      "description": "Obico reported a print failure.",
      "type": "string"
    },
    {
      "const": "obico_printer_notification",
      "description": "Obico sent a printer notification.",
      "type": "string"
    },
    {
      "const": "malformed_external_event",
      "description": "An external body arrived that could not be read.",
      "type": "string"
    },
    {
      "const": "action_requested",
      "description": "An actor asked for an action.",
      "type": "string"
    },
    {
      "const": "action_executed",
      "description": "An accepted action reached the printer.",
      "type": "string"
    },
    {
      "const": "action_rejected",
      "description": "Policy refused an action.",
      "type": "string"
    },
    {
      "const": "intervention_expired",
      "description": "A bounded intervention expired.",
      "type": "string"
    },
    {
      "const": "supervision_session_opened",
      "description": "A supervision session was opened.",
      "type": "string"
    },
    {
      "const": "supervision_session_closed",
      "description": "A supervision session was closed.",
      "type": "string"
    },
    {
      "const": "agent_assessment",
      "description": "The agent wrote down what it made of a turn.",
      "type": "string"
    },
    {
      "const": "operator_acknowledgement",
      "description": "An operator acknowledged an event.",
      "type": "string"
    },
    {
      "const": "port_failure",
      "description": "A port failed while an event was being handled.",
      "type": "string"
    },
    {
      "const": "startup_reconciliation",
      "description": "A supervisor reconciled one thing the store held when it started.",
      "type": "string"
    }
  ],
  "title": "EventKind"
}
```

### EventPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What an event carries, tagged by the kind it belongs to.\n\nThe wire shape is the pair `kind` and `payload`, which is why an\n[`EventRecord`] carries this one value rather than two fields that could\ndisagree.",
  "oneOf": [
    {
      "description": "Obico reported a print failure.",
      "properties": {
        "kind": {
          "const": "obico_failure_alert",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoFailureAlertPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Obico sent a printer notification.",
      "properties": {
        "kind": {
          "const": "obico_printer_notification",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoPrinterNotificationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An external body arrived that could not be read.",
      "properties": {
        "kind": {
          "const": "malformed_external_event",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/MalformedExternalEventPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An actor asked for an action.",
      "properties": {
        "kind": {
          "const": "action_requested",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRequestedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An accepted action reached the printer.",
      "properties": {
        "kind": {
          "const": "action_executed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionExecutedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Policy refused an action.",
      "properties": {
        "kind": {
          "const": "action_rejected",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRejectedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A bounded intervention expired.",
      "properties": {
        "kind": {
          "const": "intervention_expired",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/InterventionExpiredPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was opened.",
      "properties": {
        "kind": {
          "const": "supervision_session_opened",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionOpenedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was closed.",
      "properties": {
        "kind": {
          "const": "supervision_session_closed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionClosedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "kind": {
          "const": "agent_assessment",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/AgentAssessmentPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An operator acknowledged an event.",
      "properties": {
        "kind": {
          "const": "operator_acknowledgement",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/OperatorAcknowledgementPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A port failed while an event was being handled.",
      "properties": {
        "kind": {
          "const": "port_failure",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/PortFailurePayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "kind": {
          "const": "startup_reconciliation",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/StartupReconciliationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    }
  ],
  "title": "EventPayload"
}
```

### EventRecord

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from.",
      "oneOf": [
        {
          "const": "obico",
          "description": "Obico, over its webhook.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "ImageId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one image.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ImageId",
      "type": "string"
    },
    "ImageRef": {
      "additionalProperties": false,
      "description": "The handle an image travels in context under.",
      "properties": {
        "id": {
          "$ref": "#/$defs/ImageId",
          "description": "The image's identifier."
        },
        "sha256": {
          "description": "The SHA-256 of its bytes, lowercase hexadecimal.",
          "type": "string"
        }
      },
      "required": [
        "id",
        "sha256"
      ],
      "type": "object"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One event, as the store holds it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
  "oneOf": [
    {
      "description": "Obico reported a print failure.",
      "properties": {
        "kind": {
          "const": "obico_failure_alert",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoFailureAlertPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Obico sent a printer notification.",
      "properties": {
        "kind": {
          "const": "obico_printer_notification",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoPrinterNotificationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An external body arrived that could not be read.",
      "properties": {
        "kind": {
          "const": "malformed_external_event",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/MalformedExternalEventPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An actor asked for an action.",
      "properties": {
        "kind": {
          "const": "action_requested",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRequestedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An accepted action reached the printer.",
      "properties": {
        "kind": {
          "const": "action_executed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionExecutedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Policy refused an action.",
      "properties": {
        "kind": {
          "const": "action_rejected",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRejectedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A bounded intervention expired.",
      "properties": {
        "kind": {
          "const": "intervention_expired",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/InterventionExpiredPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was opened.",
      "properties": {
        "kind": {
          "const": "supervision_session_opened",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionOpenedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was closed.",
      "properties": {
        "kind": {
          "const": "supervision_session_closed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionClosedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "kind": {
          "const": "agent_assessment",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/AgentAssessmentPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An operator acknowledged an event.",
      "properties": {
        "kind": {
          "const": "operator_acknowledgement",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/OperatorAcknowledgementPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A port failed while an event was being handled.",
      "properties": {
        "kind": {
          "const": "port_failure",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/PortFailurePayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "kind": {
          "const": "startup_reconciliation",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/StartupReconciliationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    }
  ],
  "properties": {
    "id": {
      "$ref": "#/$defs/EventId",
      "description": "This event's identifier, minted by the store."
    },
    "image": {
      "anyOf": [
        {
          "$ref": "#/$defs/ImageRef"
        },
        {
          "type": "null"
        }
      ],
      "description": "The image it arrived with, when it arrived with one."
    },
    "print_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrintId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The print it belongs to, when it belongs to one."
    },
    "raw": {
      "anyOf": [
        {
          "$ref": "#/$defs/RawBytes"
        },
        {
          "type": "null"
        }
      ],
      "description": "The bytes exactly as received, for an externally sourced event."
    },
    "received_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was received."
    },
    "source": {
      "$ref": "#/$defs/EventSource",
      "description": "Where it came from."
    }
  },
  "required": [
    "id",
    "source",
    "received_at"
  ],
  "title": "EventRecord",
  "type": "object"
}
```

### EventSource

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Where an event came from.",
  "oneOf": [
    {
      "const": "obico",
      "description": "Obico, over its webhook.",
      "type": "string"
    },
    {
      "const": "operator",
      "description": "A person.",
      "type": "string"
    },
    {
      "const": "agent",
      "description": "The supervising agent.",
      "type": "string"
    },
    {
      "const": "system",
      "description": "The supervisor itself.",
      "type": "string"
    }
  ],
  "title": "EventSource"
}
```

### ExecutionOutcome

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What happened when an accepted action reached the printer.",
  "oneOf": [
    {
      "const": "succeeded",
      "description": "The printer took it.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "The printer did not, for this reason.",
      "properties": {
        "failed": {
          "additionalProperties": false,
          "properties": {
            "reason": {
              "description": "Why it failed.",
              "type": "string"
            }
          },
          "required": [
            "reason"
          ],
          "type": "object"
        }
      },
      "required": [
        "failed"
      ],
      "type": "object"
    }
  ],
  "title": "ExecutionOutcome"
}
```

### FetchedImage

Declared by `printobserver-vision-api`.

```json
{
  "$defs": {
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One image, as it was retrieved.",
  "properties": {
    "bytes": {
      "$ref": "#/$defs/RawBytes",
      "description": "The image's bytes, exactly as served."
    },
    "content_type": {
      "description": "The content type it was served as.",
      "type": "string"
    }
  },
  "required": [
    "bytes",
    "content_type"
  ],
  "title": "FetchedImage",
  "type": "object"
}
```

### FileName

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
  "minLength": 1,
  "not": {
    "pattern": "^([.]{1,2}$|[A-Za-z]:)"
  },
  "pattern": "^[^/\\\\\u0000]+$",
  "title": "FileName",
  "type": "string"
}
```

### HeaterSnapshot

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Reported": {
      "additionalProperties": false,
      "description": "A value as a source reported it, flagged when it is outside the plausibility range this crate declares for its field.",
      "properties": {
        "out_of_range": {
          "type": "boolean"
        },
        "value": {
          "type": "number"
        }
      },
      "required": [
        "value",
        "out_of_range"
      ],
      "title": "Reported",
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One heater, as a source reported it.\n\nEvery field is optional, because a source that reports no heater at all\nreports none of these; each is a plausibility-ranged reported value in\ndegrees Celsius.",
  "properties": {
    "actual_c": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The temperature the heater is at."
    },
    "offset_c": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The offset applied to this heater's target."
    },
    "target_c": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The temperature the heater is driving towards."
    }
  },
  "title": "HeaterSnapshot",
  "type": "object"
}
```

### HistoryQuery

Declared by `printobserver-store-api`.

```json
{
  "$defs": {
    "EventKind": {
      "description": "Which event this is, without its payload.\n\nEvery kind here has exactly one [`EventPayload`] variant, and the spellings\nare the same on the wire.",
      "oneOf": [
        {
          "const": "obico_failure_alert",
          "description": "Obico reported a print failure.",
          "type": "string"
        },
        {
          "const": "obico_printer_notification",
          "description": "Obico sent a printer notification.",
          "type": "string"
        },
        {
          "const": "malformed_external_event",
          "description": "An external body arrived that could not be read.",
          "type": "string"
        },
        {
          "const": "action_requested",
          "description": "An actor asked for an action.",
          "type": "string"
        },
        {
          "const": "action_executed",
          "description": "An accepted action reached the printer.",
          "type": "string"
        },
        {
          "const": "action_rejected",
          "description": "Policy refused an action.",
          "type": "string"
        },
        {
          "const": "intervention_expired",
          "description": "A bounded intervention expired.",
          "type": "string"
        },
        {
          "const": "supervision_session_opened",
          "description": "A supervision session was opened.",
          "type": "string"
        },
        {
          "const": "supervision_session_closed",
          "description": "A supervision session was closed.",
          "type": "string"
        },
        {
          "const": "agent_assessment",
          "description": "The agent wrote down what it made of a turn.",
          "type": "string"
        },
        {
          "const": "operator_acknowledgement",
          "description": "An operator acknowledged an event.",
          "type": "string"
        },
        {
          "const": "port_failure",
          "description": "A port failed while an event was being handled.",
          "type": "string"
        },
        {
          "const": "startup_reconciliation",
          "description": "A supervisor reconciled one thing the store held when it started.",
          "type": "string"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Which of a print's events to read.\n\nAn empty `kinds` means every kind rather than no kinds, and an absent\n`limit` means [`DEFAULT_HISTORY_WINDOW`].",
  "properties": {
    "kinds": {
      "description": "The kinds to read; empty means every kind.",
      "items": {
        "$ref": "#/$defs/EventKind"
      },
      "type": "array"
    },
    "limit": {
      "description": "How many events to answer; absent means [`DEFAULT_HISTORY_WINDOW`].",
      "format": "uint32",
      "minimum": 0,
      "type": [
        "integer",
        "null"
      ]
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print to read the history of."
    },
    "since": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "The earliest instant to read from, inclusive."
    },
    "until": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "The latest instant to read to, inclusive."
    }
  },
  "required": [
    "print_id",
    "kinds"
  ],
  "title": "HistoryQuery",
  "type": "object"
}
```

### ImageId

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one image.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "ImageId",
  "type": "string"
}
```

### ImageRecord

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "ImageId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one image.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ImageId",
      "type": "string"
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One image, stored beside the event it arrived with.\n\n`relative_path` is relative to the configured state directory, so that the\nstore stays portable. Materializing an image resolves that to an absolute\npath; nothing in this system renders image bytes into JSON.",
  "properties": {
    "byte_len": {
      "description": "How many bytes it is.",
      "format": "int64",
      "type": "integer"
    },
    "content_type": {
      "description": "The content type it was served as.",
      "type": "string"
    },
    "event_id": {
      "$ref": "#/$defs/EventId",
      "description": "The event it arrived with."
    },
    "fetched_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was fetched."
    },
    "id": {
      "$ref": "#/$defs/ImageId",
      "description": "This image's identifier, minted by the store."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print it belongs to."
    },
    "relative_path": {
      "description": "Where it lives, relative to the configured state directory.",
      "type": "string"
    },
    "sha256": {
      "description": "The SHA-256 of its bytes, lowercase hexadecimal.",
      "type": "string"
    },
    "source_url": {
      "description": "Where it was fetched from, when it was fetched from somewhere.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "id",
    "print_id",
    "event_id",
    "fetched_at",
    "content_type",
    "byte_len",
    "sha256",
    "relative_path"
  ],
  "title": "ImageRecord",
  "type": "object"
}
```

### ImageRef

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ImageId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one image.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ImageId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The handle an image travels in context under.",
  "properties": {
    "id": {
      "$ref": "#/$defs/ImageId",
      "description": "The image's identifier."
    },
    "sha256": {
      "description": "The SHA-256 of its bytes, lowercase hexadecimal.",
      "type": "string"
    }
  },
  "required": [
    "id",
    "sha256"
  ],
  "title": "ImageRef",
  "type": "object"
}
```

### Intervention

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One adjustment made for a bounded time.\n\n`prior_value` is read from the printer snapshot taken before the change, and\nis absent when the printer reported none \u2014 in which case expiry restores\nnothing and the outcome says so rather than guessing a default.",
  "properties": {
    "action_id": {
      "$ref": "#/$defs/ActionId",
      "description": "The action that asked for it."
    },
    "adjustable": {
      "$ref": "#/$defs/Adjustable",
      "description": "What it changed."
    },
    "applied_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was applied."
    },
    "applied_value": {
      "description": "What it was changed to.",
      "format": "double",
      "type": "number"
    },
    "expires_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it stops standing."
    },
    "id": {
      "$ref": "#/$defs/InterventionId",
      "description": "This intervention's identifier, minted by the store."
    },
    "outcome": {
      "$ref": "#/$defs/InterventionOutcome",
      "description": "What became of it."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print it was made against."
    },
    "prior_value": {
      "description": "What the printer reported before the change, if it reported anything.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "restored_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the prior value was put back, if it was."
    }
  },
  "required": [
    "id",
    "print_id",
    "action_id",
    "adjustable",
    "applied_value",
    "applied_at",
    "expires_at",
    "outcome"
  ],
  "title": "Intervention",
  "type": "object"
}
```

### InterventionExpiredPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A bounded intervention expired.",
  "properties": {
    "adjustable": {
      "$ref": "#/$defs/Adjustable",
      "description": "What it had changed."
    },
    "intervention_id": {
      "$ref": "#/$defs/InterventionId",
      "description": "The intervention's identifier."
    },
    "outcome": {
      "$ref": "#/$defs/InterventionOutcome",
      "description": "What became of it."
    }
  },
  "required": [
    "intervention_id",
    "adjustable",
    "outcome"
  ],
  "title": "InterventionExpiredPayload",
  "type": "object"
}
```

### InterventionId

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "InterventionId",
  "type": "string"
}
```

### InterventionOutcome

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What became of a bounded change.",
  "oneOf": [
    {
      "const": "still_active",
      "description": "It is still in force.",
      "type": "string"
    },
    {
      "const": "restored",
      "description": "The prior value was put back.",
      "type": "string"
    },
    {
      "const": "restore_unavailable",
      "description": "There was no prior value to put back.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "Putting the prior value back failed.",
      "properties": {
        "restore_failed": {
          "additionalProperties": false,
          "properties": {
            "reason": {
              "description": "Why it failed.",
              "type": "string"
            }
          },
          "required": [
            "reason"
          ],
          "type": "object"
        }
      },
      "required": [
        "restore_failed"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Another intervention replaced it before it expired.",
      "properties": {
        "superseded": {
          "additionalProperties": false,
          "properties": {
            "by": {
              "$ref": "#/$defs/InterventionId",
              "description": "The intervention that replaced it."
            }
          },
          "required": [
            "by"
          ],
          "type": "object"
        }
      },
      "required": [
        "superseded"
      ],
      "type": "object"
    }
  ],
  "title": "InterventionOutcome"
}
```

### JobManifest

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
  "properties": {
    "allowed": {
      "additionalProperties": false,
      "description": "The range each named adjustable may take, inclusive.",
      "patternProperties": {
        "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
          "$ref": "#/$defs/Range"
        }
      },
      "type": "object"
    },
    "file_name": {
      "description": "The file this manifest is about, as the slicer named it.",
      "type": "string"
    },
    "material": {
      "description": "The material the job is sliced for.",
      "type": "string"
    },
    "metadata": {
      "additionalProperties": {
        "type": "string"
      },
      "description": "Whatever else the slicer recorded.",
      "type": "object"
    },
    "nozzle_diameter_mm": {
      "description": "The nozzle the job is sliced for, in millimetres.",
      "format": "double",
      "type": "number"
    },
    "slicer_profile": {
      "description": "The slicer profile the job was sliced with.",
      "type": "string"
    }
  },
  "required": [
    "file_name",
    "material",
    "nozzle_diameter_mm",
    "slicer_profile",
    "allowed",
    "metadata"
  ],
  "title": "JobManifest",
  "type": "object"
}
```

### JobSnapshot

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Reported": {
      "additionalProperties": false,
      "description": "A value as a source reported it, flagged when it is outside the plausibility range this crate declares for its field.",
      "properties": {
        "out_of_range": {
          "type": "boolean"
        },
        "value": {
          "type": "number"
        }
      },
      "required": [
        "value",
        "out_of_range"
      ],
      "title": "Reported",
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The job a printer reports it is running.\n\nEvery field but `state` is optional: an absent one means the source did not\nreport it.",
  "properties": {
    "completion": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "How far through the print is, as a fraction from zero to one.\n\nNormalized to a fraction here regardless of how the source expresses it."
    },
    "error": {
      "description": "The error the source reports, when it reports one.",
      "type": [
        "string",
        "null"
      ]
    },
    "estimated_print_time_s": {
      "description": "The whole print's estimated duration, in whole seconds.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "file_name": {
      "description": "The name of the file being printed, as the source reported it.",
      "type": [
        "string",
        "null"
      ]
    },
    "file_origin": {
      "description": "Where the file lives, in the source's own vocabulary.",
      "type": [
        "string",
        "null"
      ]
    },
    "print_time_left_s": {
      "description": "How long the print has left, in whole seconds.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "print_time_s": {
      "description": "How long the print has been running, in whole seconds.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "size_bytes": {
      "description": "The file's size in bytes.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "state": {
      "$ref": "#/$defs/PrinterState",
      "description": "The state the source reports the job to be in."
    }
  },
  "required": [
    "state"
  ],
  "title": "JobSnapshot",
  "type": "object"
}
```

### MalformedExternalEventPayload

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
  "properties": {
    "detail": {
      "description": "One line saying why the body could not be read.",
      "type": "string"
    }
  },
  "required": [
    "detail"
  ],
  "title": "MalformedExternalEventPayload",
  "type": "object"
}
```

### ManifestNarrowing

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One adjustable whose manifest range was wider than the envelope's.",
  "properties": {
    "adjustable": {
      "$ref": "#/$defs/Adjustable",
      "description": "The adjustable that was narrowed."
    },
    "applied": {
      "$ref": "#/$defs/Range",
      "description": "The range that stands."
    },
    "requested": {
      "$ref": "#/$defs/Range",
      "description": "The range the manifest asked for."
    }
  },
  "required": [
    "adjustable",
    "requested",
    "applied"
  ],
  "title": "ManifestNarrowing",
  "type": "object"
}
```

### NormalizedAlert

Declared by `printobserver-vision-api`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from.",
      "oneOf": [
        {
          "const": "obico",
          "description": "Obico, over its webhook.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One external body, read into this system's own event vocabulary.\n\n`kind` and `payload` are the one closed pair\n[`EventPayload`](printobserver_types::EventPayload) declares, so a\nnormalization naming one kind while carrying another's payload is\nunrepresentable here as it is in the store.",
  "oneOf": [
    {
      "description": "Obico reported a print failure.",
      "properties": {
        "kind": {
          "const": "obico_failure_alert",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoFailureAlertPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Obico sent a printer notification.",
      "properties": {
        "kind": {
          "const": "obico_printer_notification",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ObicoPrinterNotificationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An external body arrived that could not be read.",
      "properties": {
        "kind": {
          "const": "malformed_external_event",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/MalformedExternalEventPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An actor asked for an action.",
      "properties": {
        "kind": {
          "const": "action_requested",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRequestedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An accepted action reached the printer.",
      "properties": {
        "kind": {
          "const": "action_executed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionExecutedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "Policy refused an action.",
      "properties": {
        "kind": {
          "const": "action_rejected",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/ActionRejectedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A bounded intervention expired.",
      "properties": {
        "kind": {
          "const": "intervention_expired",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/InterventionExpiredPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was opened.",
      "properties": {
        "kind": {
          "const": "supervision_session_opened",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionOpenedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervision session was closed.",
      "properties": {
        "kind": {
          "const": "supervision_session_closed",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/SupervisionSessionClosedPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "kind": {
          "const": "agent_assessment",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/AgentAssessmentPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "An operator acknowledged an event.",
      "properties": {
        "kind": {
          "const": "operator_acknowledgement",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/OperatorAcknowledgementPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A port failed while an event was being handled.",
      "properties": {
        "kind": {
          "const": "port_failure",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/PortFailurePayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    },
    {
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "kind": {
          "const": "startup_reconciliation",
          "type": "string"
        },
        "payload": {
          "$ref": "#/$defs/StartupReconciliationPayload"
        }
      },
      "required": [
        "kind",
        "payload"
      ],
      "type": "object"
    }
  ],
  "properties": {
    "image_url": {
      "description": "The image this alert names, when it names one.",
      "type": [
        "string",
        "null"
      ]
    },
    "raw": {
      "$ref": "#/$defs/RawBytes",
      "description": "The bytes exactly as received."
    },
    "received_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was received."
    },
    "source": {
      "$ref": "#/$defs/EventSource",
      "description": "Where it came from."
    }
  },
  "required": [
    "source",
    "received_at",
    "raw"
  ],
  "title": "NormalizedAlert",
  "type": "object"
}
```

### ObicoEventType

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The `type` a printer notification's event object carries.\n\nThese are the producer's own spellings; the normalized vocabulary is\n[`ObicoNotificationType`](crate::ObicoNotificationType).",
  "oneOf": [
    {
      "const": "PrintStarted",
      "description": "A print started.",
      "type": "string"
    },
    {
      "const": "PrintDone",
      "description": "A print finished.",
      "type": "string"
    },
    {
      "const": "PrintCancelled",
      "description": "A print was cancelled.",
      "type": "string"
    },
    {
      "const": "PrintPaused",
      "description": "A print was paused.",
      "type": "string"
    },
    {
      "const": "PrintResumed",
      "description": "A print was resumed.",
      "type": "string"
    },
    {
      "const": "FilamentChange",
      "description": "The printer is waiting for a filament change.",
      "type": "string"
    },
    {
      "const": "HeaterCooledDown",
      "description": "A heater cooled down.",
      "type": "string"
    },
    {
      "const": "HeaterTargetReached",
      "description": "A heater reached its target.",
      "type": "string"
    }
  ],
  "title": "ObicoEventType"
}
```

### ObicoFailureAlert

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoFailureEvent": {
      "description": "The `event` object a failure alert carries.",
      "properties": {
        "is_warning": {
          "description": "Whether the producer called it a warning rather than a failure.",
          "type": "boolean"
        },
        "print_paused": {
          "description": "Whether the producer paused the print itself.",
          "type": "boolean"
        },
        "type": {
          "$ref": "#/$defs/ObicoFailureEventType",
          "description": "Always the producer's own `PrintFailure`."
        }
      },
      "required": [
        "type",
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoFailureEventType": {
      "description": "The one `type` a failure alert's event object carries.",
      "oneOf": [
        {
          "const": "PrintFailure",
          "description": "Obico's own spelling of a print failure.",
          "type": "string"
        }
      ]
    },
    "ObicoPrintInfo": {
      "description": "The `print` object, which the producer sends when there is a print.\n\nEach instant has **three** input states rather than two, and the option and\nthe enum carry one each: a number is [`ObicoTimestamp::Seconds`], the empty\nstring the producer sends for a print that has not started or ended is\n[`ObicoTimestamp::NotReported`], and a field the producer omits altogether\nis `None`. The last two both mean the producer reported no instant, and\nneither is refused and neither is an epoch date; they are held apart here\nonly so that a body round-trips back into exactly the form it arrived in.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/ObicoTimestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, absent when the producer omits the field."
        },
        "filename": {
          "description": "The file being printed.",
          "type": "string"
        },
        "id": {
          "description": "Obico's own identifier for the print.",
          "format": "int64",
          "type": "integer"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/ObicoTimestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, absent when the producer omits the field."
        }
      },
      "required": [
        "id",
        "filename"
      ],
      "type": "object"
    },
    "ObicoPrinterInfo": {
      "description": "The `printer` object both shapes carry.",
      "properties": {
        "id": {
          "description": "Obico's own identifier for the printer.",
          "format": "int64",
          "type": "integer"
        },
        "name": {
          "description": "The name Obico shows the printer under.",
          "type": "string"
        }
      },
      "required": [
        "id",
        "name"
      ],
      "type": "object"
    },
    "ObicoTimestamp": {
      "anyOf": [
        {
          "type": "number"
        },
        {
          "maxLength": 0,
          "type": "string"
        }
      ],
      "description": "A Unix timestamp number, or the empty string the producer sends when it has no instant to send.",
      "title": "ObicoTimestamp"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The whole body the producer sends for a print failure.",
  "properties": {
    "event": {
      "$ref": "#/$defs/ObicoFailureEvent",
      "description": "What happened."
    },
    "img_url": {
      "description": "Where the snapshot that shows it can be fetched.",
      "type": "string"
    },
    "print": {
      "$ref": "#/$defs/ObicoPrintInfo",
      "description": "Which print it happened to."
    },
    "printer": {
      "$ref": "#/$defs/ObicoPrinterInfo",
      "description": "Which printer it happened on."
    }
  },
  "required": [
    "event",
    "printer",
    "print",
    "img_url"
  ],
  "title": "ObicoFailureAlert",
  "type": "object"
}
```

### ObicoFailureAlertPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
  "properties": {
    "ended_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print ended, when Obico reported an instant for it."
    },
    "file_name": {
      "description": "The file being printed, when Obico named one.",
      "type": [
        "string",
        "null"
      ]
    },
    "is_warning": {
      "description": "Whether Obico called it a warning rather than a failure.",
      "type": "boolean"
    },
    "obico_print_id": {
      "description": "Obico's own identifier for the print, when it named one.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "print_paused": {
      "description": "Whether Obico paused the print itself.",
      "type": "boolean"
    },
    "started_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print started, when Obico reported an instant for it."
    }
  },
  "required": [
    "is_warning",
    "print_paused"
  ],
  "title": "ObicoFailureAlertPayload",
  "type": "object"
}
```

### ObicoFailureEvent

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoFailureEventType": {
      "description": "The one `type` a failure alert's event object carries.",
      "oneOf": [
        {
          "const": "PrintFailure",
          "description": "Obico's own spelling of a print failure.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The `event` object a failure alert carries.",
  "properties": {
    "is_warning": {
      "description": "Whether the producer called it a warning rather than a failure.",
      "type": "boolean"
    },
    "print_paused": {
      "description": "Whether the producer paused the print itself.",
      "type": "boolean"
    },
    "type": {
      "$ref": "#/$defs/ObicoFailureEventType",
      "description": "Always the producer's own `PrintFailure`."
    }
  },
  "required": [
    "type",
    "is_warning",
    "print_paused"
  ],
  "title": "ObicoFailureEvent",
  "type": "object"
}
```

### ObicoFailureEventType

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The one `type` a failure alert's event object carries.",
  "oneOf": [
    {
      "const": "PrintFailure",
      "description": "Obico's own spelling of a print failure.",
      "type": "string"
    }
  ],
  "title": "ObicoFailureEventType"
}
```

### ObicoNotificationEvent

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoEventType": {
      "description": "The `type` a printer notification's event object carries.\n\nThese are the producer's own spellings; the normalized vocabulary is\n[`ObicoNotificationType`](crate::ObicoNotificationType).",
      "oneOf": [
        {
          "const": "PrintStarted",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "PrintDone",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "PrintCancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "PrintPaused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "PrintResumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "FilamentChange",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "HeaterCooledDown",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "HeaterTargetReached",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The `event` object a printer notification carries.",
  "properties": {
    "is_warning": {
      "description": "Whether the producer called it a warning.",
      "type": "boolean"
    },
    "print_paused": {
      "description": "Whether the producer paused the print itself.",
      "type": "boolean"
    },
    "type": {
      "$ref": "#/$defs/ObicoEventType",
      "description": "Which notification it is, in the producer's own spelling."
    }
  },
  "required": [
    "type",
    "is_warning",
    "print_paused"
  ],
  "title": "ObicoNotificationEvent",
  "type": "object"
}
```

### ObicoNotificationType

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The kind of printer notification Obico sent, normalized.",
  "oneOf": [
    {
      "const": "started",
      "description": "A print started.",
      "type": "string"
    },
    {
      "const": "done",
      "description": "A print finished.",
      "type": "string"
    },
    {
      "const": "cancelled",
      "description": "A print was cancelled.",
      "type": "string"
    },
    {
      "const": "paused",
      "description": "A print was paused.",
      "type": "string"
    },
    {
      "const": "resumed",
      "description": "A print was resumed.",
      "type": "string"
    },
    {
      "const": "filament_change",
      "description": "The printer is waiting for a filament change.",
      "type": "string"
    },
    {
      "const": "heater_cooled",
      "description": "A heater cooled down.",
      "type": "string"
    },
    {
      "const": "heater_target",
      "description": "A heater reached its target.",
      "type": "string"
    }
  ],
  "title": "ObicoNotificationType"
}
```

### ObicoPrintInfo

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoTimestamp": {
      "anyOf": [
        {
          "type": "number"
        },
        {
          "maxLength": 0,
          "type": "string"
        }
      ],
      "description": "A Unix timestamp number, or the empty string the producer sends when it has no instant to send.",
      "title": "ObicoTimestamp"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The `print` object, which the producer sends when there is a print.\n\nEach instant has **three** input states rather than two, and the option and\nthe enum carry one each: a number is [`ObicoTimestamp::Seconds`], the empty\nstring the producer sends for a print that has not started or ended is\n[`ObicoTimestamp::NotReported`], and a field the producer omits altogether\nis `None`. The last two both mean the producer reported no instant, and\nneither is refused and neither is an epoch date; they are held apart here\nonly so that a body round-trips back into exactly the form it arrived in.",
  "properties": {
    "ended_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/ObicoTimestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print ended, absent when the producer omits the field."
    },
    "filename": {
      "description": "The file being printed.",
      "type": "string"
    },
    "id": {
      "description": "Obico's own identifier for the print.",
      "format": "int64",
      "type": "integer"
    },
    "started_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/ObicoTimestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print started, absent when the producer omits the field."
    }
  },
  "required": [
    "id",
    "filename"
  ],
  "title": "ObicoPrintInfo",
  "type": "object"
}
```

### ObicoPrinterInfo

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The `printer` object both shapes carry.",
  "properties": {
    "id": {
      "description": "Obico's own identifier for the printer.",
      "format": "int64",
      "type": "integer"
    },
    "name": {
      "description": "The name Obico shows the printer under.",
      "type": "string"
    }
  },
  "required": [
    "id",
    "name"
  ],
  "title": "ObicoPrinterInfo",
  "type": "object"
}
```

### ObicoPrinterNotification

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoEventType": {
      "description": "The `type` a printer notification's event object carries.\n\nThese are the producer's own spellings; the normalized vocabulary is\n[`ObicoNotificationType`](crate::ObicoNotificationType).",
      "oneOf": [
        {
          "const": "PrintStarted",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "PrintDone",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "PrintCancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "PrintPaused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "PrintResumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "FilamentChange",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "HeaterCooledDown",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "HeaterTargetReached",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoNotificationEvent": {
      "description": "The `event` object a printer notification carries.",
      "properties": {
        "is_warning": {
          "description": "Whether the producer called it a warning.",
          "type": "boolean"
        },
        "print_paused": {
          "description": "Whether the producer paused the print itself.",
          "type": "boolean"
        },
        "type": {
          "$ref": "#/$defs/ObicoEventType",
          "description": "Which notification it is, in the producer's own spelling."
        }
      },
      "required": [
        "type",
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoPrintInfo": {
      "description": "The `print` object, which the producer sends when there is a print.\n\nEach instant has **three** input states rather than two, and the option and\nthe enum carry one each: a number is [`ObicoTimestamp::Seconds`], the empty\nstring the producer sends for a print that has not started or ended is\n[`ObicoTimestamp::NotReported`], and a field the producer omits altogether\nis `None`. The last two both mean the producer reported no instant, and\nneither is refused and neither is an epoch date; they are held apart here\nonly so that a body round-trips back into exactly the form it arrived in.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/ObicoTimestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, absent when the producer omits the field."
        },
        "filename": {
          "description": "The file being printed.",
          "type": "string"
        },
        "id": {
          "description": "Obico's own identifier for the print.",
          "format": "int64",
          "type": "integer"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/ObicoTimestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, absent when the producer omits the field."
        }
      },
      "required": [
        "id",
        "filename"
      ],
      "type": "object"
    },
    "ObicoPrinterInfo": {
      "description": "The `printer` object both shapes carry.",
      "properties": {
        "id": {
          "description": "Obico's own identifier for the printer.",
          "format": "int64",
          "type": "integer"
        },
        "name": {
          "description": "The name Obico shows the printer under.",
          "type": "string"
        }
      },
      "required": [
        "id",
        "name"
      ],
      "type": "object"
    },
    "ObicoTimestamp": {
      "anyOf": [
        {
          "type": "number"
        },
        {
          "maxLength": 0,
          "type": "string"
        }
      ],
      "description": "A Unix timestamp number, or the empty string the producer sends when it has no instant to send.",
      "title": "ObicoTimestamp"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The whole body the producer sends for a printer notification.\n\n`print` and `img_url` are present together when the notification is about a\nprint and absent together when it is not; the absent form is a shape the\nproducer sends rather than an incomplete copy of the present one.",
  "properties": {
    "event": {
      "$ref": "#/$defs/ObicoNotificationEvent",
      "description": "What happened."
    },
    "img_url": {
      "description": "Where the snapshot can be fetched, when the notification carries one.",
      "type": [
        "string",
        "null"
      ]
    },
    "print": {
      "anyOf": [
        {
          "$ref": "#/$defs/ObicoPrintInfo"
        },
        {
          "type": "null"
        }
      ],
      "description": "Which print it is about, when it is about one."
    },
    "printer": {
      "$ref": "#/$defs/ObicoPrinterInfo",
      "description": "Which printer it happened on."
    }
  },
  "required": [
    "event",
    "printer"
  ],
  "title": "ObicoPrinterNotification",
  "type": "object"
}
```

### ObicoPrinterNotificationPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
  "properties": {
    "ended_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print ended, when Obico reported an instant for it."
    },
    "file_name": {
      "description": "The file being printed, when the notification is about one.",
      "type": [
        "string",
        "null"
      ]
    },
    "notification_type": {
      "$ref": "#/$defs/ObicoNotificationType",
      "description": "Which notification it is."
    },
    "obico_print_id": {
      "description": "Obico's own identifier for the print, when the notification is about one.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "started_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the print started, when Obico reported an instant for it."
    }
  },
  "required": [
    "notification_type"
  ],
  "title": "ObicoPrinterNotificationPayload",
  "type": "object"
}
```

### ObicoTimestamp

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "anyOf": [
    {
      "type": "number"
    },
    {
      "maxLength": 0,
      "type": "string"
    }
  ],
  "description": "A Unix timestamp number, or the empty string the producer sends when it has no instant to send.",
  "title": "ObicoTimestamp"
}
```

### OperatorAcknowledgementPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "An operator acknowledged an event.",
  "properties": {
    "acknowledged_event_id": {
      "$ref": "#/$defs/EventId",
      "description": "The event being acknowledged."
    },
    "disposition": {
      "$ref": "#/$defs/AcknowledgementDisposition",
      "description": "What the operator asked for next."
    }
  },
  "required": [
    "acknowledged_event_id",
    "disposition"
  ],
  "title": "OperatorAcknowledgementPayload",
  "type": "object"
}
```

### PolicyDecision

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The decision policy took on one request.",
  "oneOf": [
    {
      "const": "accepted",
      "description": "The request may proceed.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "The request may not, for this reason.",
      "properties": {
        "rejected": {
          "$ref": "#/$defs/RejectionReason"
        }
      },
      "required": [
        "rejected"
      ],
      "type": "object"
    }
  ],
  "title": "PolicyDecision"
}
```

### PortFailurePayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
  "properties": {
    "detail": {
      "description": "What the port said about it, in the port's own words.",
      "type": "string"
    },
    "event_id": {
      "$ref": "#/$defs/EventId",
      "description": "The event whose handling reached the failing call."
    },
    "site": {
      "$ref": "#/$defs/PortFailureSite",
      "description": "Where it failed."
    }
  },
  "required": [
    "event_id",
    "site",
    "detail"
  ],
  "title": "PortFailurePayload",
  "type": "object"
}
```

### PortFailureSite

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
  "oneOf": [
    {
      "const": "printer_snapshot",
      "description": "Reading the printer's own state.",
      "type": "string"
    },
    {
      "const": "printer_job",
      "description": "Reading the job the printer reports it is running.",
      "type": "string"
    },
    {
      "const": "image_write",
      "description": "Writing the image the event arrived with.",
      "type": "string"
    },
    {
      "const": "supervision_turn",
      "description": "Running the supervision turn the event prompted.",
      "type": "string"
    }
  ],
  "title": "PortFailureSite"
}
```

### PrintAction

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The whole vocabulary an actor may ask for, and there is no other.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "Pause the print.",
      "properties": {
        "action": {
          "const": "pause",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Resume the print.",
      "properties": {
        "action": {
          "const": "resume",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Cancel the print.",
      "properties": {
        "action": {
          "const": "cancel",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Start a print of a named file, with a manifest.",
      "properties": {
        "action": {
          "const": "start_print",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "file_name": {
          "$ref": "#/$defs/FileName",
          "description": "The file to print, validated as a name a file API can be asked for."
        },
        "manifest": {
          "$ref": "#/$defs/JobManifest",
          "description": "The manifest this print is bounded by."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "file_name",
        "manifest",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Set the feedrate factor, where one means one hundred percent.",
      "properties": {
        "action": {
          "const": "set_feedrate_factor",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "duration_s": {
          "description": "How long the change stands for, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "factor": {
          "description": "The multiplier asked for.",
          "format": "double",
          "type": "number"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "factor",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Set the flowrate factor, where one means one hundred percent.",
      "properties": {
        "action": {
          "const": "set_flowrate_factor",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "duration_s": {
          "description": "How long the change stands for, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "factor": {
          "description": "The multiplier asked for.",
          "format": "double",
          "type": "number"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "factor",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Set one tool's target temperature.",
      "properties": {
        "action": {
          "const": "set_tool_target_c",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "duration_s": {
          "description": "How long the change stands for, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "target_c": {
          "description": "The target temperature asked for, in degrees Celsius.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "action",
        "tool",
        "target_c",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Set the bed's target temperature.",
      "properties": {
        "action": {
          "const": "set_bed_target_c",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "duration_s": {
          "description": "How long the change stands for, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "target_c": {
          "description": "The target temperature asked for, in degrees Celsius.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "action",
        "target_c",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Set the part-cooling fan percentage.",
      "properties": {
        "action": {
          "const": "set_fan_percent",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "duration_s": {
          "description": "How long the change stands for, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "percent": {
          "description": "The percentage asked for.",
          "format": "double",
          "type": "number"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "percent",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Acknowledge a failure event, with a disposition.",
      "properties": {
        "action": {
          "const": "acknowledge_failure",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What to do next."
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "event_id",
        "disposition",
        "reason",
        "actor"
      ],
      "type": "object"
    }
  ],
  "title": "PrintAction"
}
```

### PrintContext

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EffectiveBounds": {
      "additionalProperties": false,
      "description": "The envelope intersected with the print's manifest.\n\nThis is what context reports, so that the agent can see its own limits\nbefore it asks. Computing one is the supervision core's; this crate declares\nthe shape and nothing that produces it.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each adjustable may be set to, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        }
      },
      "required": [
        "allowed"
      ],
      "type": "object"
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
      "oneOf": [
        {
          "description": "Obico reported a print failure.",
          "properties": {
            "kind": {
              "const": "obico_failure_alert",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ObicoFailureAlertPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "Obico sent a printer notification.",
          "properties": {
            "kind": {
              "const": "obico_printer_notification",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ObicoPrinterNotificationPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An external body arrived that could not be read.",
          "properties": {
            "kind": {
              "const": "malformed_external_event",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/MalformedExternalEventPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An actor asked for an action.",
          "properties": {
            "kind": {
              "const": "action_requested",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionRequestedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An accepted action reached the printer.",
          "properties": {
            "kind": {
              "const": "action_executed",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionExecutedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "Policy refused an action.",
          "properties": {
            "kind": {
              "const": "action_rejected",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionRejectedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A bounded intervention expired.",
          "properties": {
            "kind": {
              "const": "intervention_expired",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/InterventionExpiredPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervision session was opened.",
          "properties": {
            "kind": {
              "const": "supervision_session_opened",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/SupervisionSessionOpenedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervision session was closed.",
          "properties": {
            "kind": {
              "const": "supervision_session_closed",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/SupervisionSessionClosedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "The agent wrote down what it made of a turn.",
          "properties": {
            "kind": {
              "const": "agent_assessment",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/AgentAssessmentPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An operator acknowledged an event.",
          "properties": {
            "kind": {
              "const": "operator_acknowledgement",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/OperatorAcknowledgementPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A port failed while an event was being handled.",
          "properties": {
            "kind": {
              "const": "port_failure",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/PortFailurePayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervisor reconciled one thing the store held when it started.",
          "properties": {
            "kind": {
              "const": "startup_reconciliation",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/StartupReconciliationPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        }
      ],
      "properties": {
        "id": {
          "$ref": "#/$defs/EventId",
          "description": "This event's identifier, minted by the store."
        },
        "image": {
          "anyOf": [
            {
              "$ref": "#/$defs/ImageRef"
            },
            {
              "type": "null"
            }
          ],
          "description": "The image it arrived with, when it arrived with one."
        },
        "print_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/PrintId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The print it belongs to, when it belongs to one."
        },
        "raw": {
          "anyOf": [
            {
              "$ref": "#/$defs/RawBytes"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bytes exactly as received, for an externally sourced event."
        },
        "received_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was received."
        },
        "source": {
          "$ref": "#/$defs/EventSource",
          "description": "Where it came from."
        }
      },
      "required": [
        "id",
        "source",
        "received_at"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from.",
      "oneOf": [
        {
          "const": "obico",
          "description": "Obico, over its webhook.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "HeaterSnapshot": {
      "additionalProperties": false,
      "description": "One heater, as a source reported it.\n\nEvery field is optional, because a source that reports no heater at all\nreports none of these; each is a plausibility-ranged reported value in\ndegrees Celsius.",
      "properties": {
        "actual_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The temperature the heater is at."
        },
        "offset_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The offset applied to this heater's target."
        },
        "target_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The temperature the heater is driving towards."
        }
      },
      "type": "object"
    },
    "ImageId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one image.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ImageId",
      "type": "string"
    },
    "ImageRef": {
      "additionalProperties": false,
      "description": "The handle an image travels in context under.",
      "properties": {
        "id": {
          "$ref": "#/$defs/ImageId",
          "description": "The image's identifier."
        },
        "sha256": {
          "description": "The SHA-256 of its bytes, lowercase hexadecimal.",
          "type": "string"
        }
      },
      "required": [
        "id",
        "sha256"
      ],
      "type": "object"
    },
    "Intervention": {
      "additionalProperties": false,
      "description": "One adjustment made for a bounded time.\n\n`prior_value` is read from the printer snapshot taken before the change, and\nis absent when the printer reported none \u2014 in which case expiry restores\nnothing and the outcome says so rather than guessing a default.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action that asked for it."
        },
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it changed."
        },
        "applied_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was applied."
        },
        "applied_value": {
          "description": "What it was changed to.",
          "format": "double",
          "type": "number"
        },
        "expires_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it stops standing."
        },
        "id": {
          "$ref": "#/$defs/InterventionId",
          "description": "This intervention's identifier, minted by the store."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it was made against."
        },
        "prior_value": {
          "description": "What the printer reported before the change, if it reported anything.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "restored_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the prior value was put back, if it was."
        }
      },
      "required": [
        "id",
        "print_id",
        "action_id",
        "adjustable",
        "applied_value",
        "applied_at",
        "expires_at",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "JobSnapshot": {
      "additionalProperties": false,
      "description": "The job a printer reports it is running.\n\nEvery field but `state` is optional: an absent one means the source did not\nreport it.",
      "properties": {
        "completion": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "How far through the print is, as a fraction from zero to one.\n\nNormalized to a fraction here regardless of how the source expresses it."
        },
        "error": {
          "description": "The error the source reports, when it reports one.",
          "type": [
            "string",
            "null"
          ]
        },
        "estimated_print_time_s": {
          "description": "The whole print's estimated duration, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "file_name": {
          "description": "The name of the file being printed, as the source reported it.",
          "type": [
            "string",
            "null"
          ]
        },
        "file_origin": {
          "description": "Where the file lives, in the source's own vocabulary.",
          "type": [
            "string",
            "null"
          ]
        },
        "print_time_left_s": {
          "description": "How long the print has left, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_time_s": {
          "description": "How long the print has been running, in whole seconds.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "size_bytes": {
          "description": "The file's size in bytes.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "state": {
          "$ref": "#/$defs/PrinterState",
          "description": "The state the source reports the job to be in."
        }
      },
      "required": [
        "state"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ManifestNarrowing": {
      "additionalProperties": false,
      "description": "One adjustable whose manifest range was wider than the envelope's.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "The adjustable that was narrowed."
        },
        "applied": {
          "$ref": "#/$defs/Range",
          "description": "The range that stands."
        },
        "requested": {
          "$ref": "#/$defs/Range",
          "description": "The range the manifest asked for."
        }
      },
      "required": [
        "adjustable",
        "requested",
        "applied"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrintRecord": {
      "additionalProperties": false,
      "description": "One print, and the record supervision keys from.\n\nObico's own print id is carried beside this record's identifier rather than\nas it, because a print may be observed before Obico has one.",
      "properties": {
        "end_reason": {
          "description": "Why it ended, if it has.",
          "type": [
            "string",
            "null"
          ]
        },
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When it ended, if it has."
        },
        "file_name": {
          "description": "The file being printed, as the source reported it.",
          "type": [
            "string",
            "null"
          ]
        },
        "id": {
          "$ref": "#/$defs/PrintId",
          "description": "This print's identifier, minted by the store."
        },
        "narrowings": {
          "description": "Every manifest range this print narrowed to the envelope's.",
          "items": {
            "$ref": "#/$defs/ManifestNarrowing"
          },
          "type": "array"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when Obico has one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "opened_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the print was opened."
        },
        "state": {
          "$ref": "#/$defs/PrinterState",
          "description": "The state the print is in."
        }
      },
      "required": [
        "id",
        "state",
        "opened_at",
        "narrowings"
      ],
      "type": "object"
    },
    "PrinterSnapshot": {
      "additionalProperties": false,
      "description": "A printer, as a source reported it at one instant.",
      "properties": {
        "bed": {
          "anyOf": [
            {
              "$ref": "#/$defs/HeaterSnapshot"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bed heater, when the printer reports one."
        },
        "chamber": {
          "anyOf": [
            {
              "$ref": "#/$defs/HeaterSnapshot"
            },
            {
              "type": "null"
            }
          ],
          "description": "The chamber heater, when the printer reports one."
        },
        "connection": {
          "$ref": "#/$defs/PrinterState",
          "description": "The state the printer is in."
        },
        "fan_percent": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The part-cooling fan, in percent."
        },
        "feedrate_factor": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The feedrate multiplier, where one means one hundred percent."
        },
        "flowrate_factor": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The flowrate multiplier, where one means one hundred percent."
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "The instant this observation was taken."
        },
        "tools": {
          "description": "The tool heaters, indexed by tool number.",
          "items": {
            "$ref": "#/$defs/HeaterSnapshot"
          },
          "type": "array"
        }
      },
      "required": [
        "connection",
        "tools",
        "observed_at"
      ],
      "type": "object"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "Reported": {
      "additionalProperties": false,
      "description": "A value as a source reported it, flagged when it is outside the plausibility range this crate declares for its field.",
      "properties": {
        "out_of_range": {
          "type": "boolean"
        },
        "value": {
          "type": "number"
        }
      },
      "required": [
        "value",
        "out_of_range"
      ],
      "title": "Reported",
      "type": "object"
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Everything a supervision turn is given about one print.",
  "properties": {
    "bounds": {
      "$ref": "#/$defs/EffectiveBounds",
      "description": "The limits in force, so that the agent can see them before it asks."
    },
    "interventions": {
      "description": "The interventions still active.",
      "items": {
        "$ref": "#/$defs/Intervention"
      },
      "type": "array"
    },
    "job": {
      "anyOf": [
        {
          "$ref": "#/$defs/JobSnapshot"
        },
        {
          "type": "null"
        }
      ],
      "description": "The job, when a snapshot could be taken."
    },
    "latest_image": {
      "anyOf": [
        {
          "$ref": "#/$defs/ImageRef"
        },
        {
          "type": "null"
        }
      ],
      "description": "The most recent image, when there is one."
    },
    "manifest": {
      "anyOf": [
        {
          "$ref": "#/$defs/JobManifest"
        },
        {
          "type": "null"
        }
      ],
      "description": "The manifest, when the print has one."
    },
    "print": {
      "$ref": "#/$defs/PrintRecord",
      "description": "The print itself."
    },
    "printer": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrinterSnapshot"
        },
        {
          "type": "null"
        }
      ],
      "description": "The printer, when a snapshot could be taken."
    },
    "recent_events": {
      "description": "A bounded list of this print's events, newest first.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    }
  },
  "required": [
    "print",
    "bounds",
    "interventions",
    "recent_events"
  ],
  "title": "PrintContext",
  "type": "object"
}
```

### PrintId

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one print.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "PrintId",
  "type": "string"
}
```

### PrintRecord

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "ManifestNarrowing": {
      "additionalProperties": false,
      "description": "One adjustable whose manifest range was wider than the envelope's.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "The adjustable that was narrowed."
        },
        "applied": {
          "$ref": "#/$defs/Range",
          "description": "The range that stands."
        },
        "requested": {
          "$ref": "#/$defs/Range",
          "description": "The range the manifest asked for."
        }
      },
      "required": [
        "adjustable",
        "requested",
        "applied"
      ],
      "type": "object"
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One print, and the record supervision keys from.\n\nObico's own print id is carried beside this record's identifier rather than\nas it, because a print may be observed before Obico has one.",
  "properties": {
    "end_reason": {
      "description": "Why it ended, if it has.",
      "type": [
        "string",
        "null"
      ]
    },
    "ended_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When it ended, if it has."
    },
    "file_name": {
      "description": "The file being printed, as the source reported it.",
      "type": [
        "string",
        "null"
      ]
    },
    "id": {
      "$ref": "#/$defs/PrintId",
      "description": "This print's identifier, minted by the store."
    },
    "narrowings": {
      "description": "Every manifest range this print narrowed to the envelope's.",
      "items": {
        "$ref": "#/$defs/ManifestNarrowing"
      },
      "type": "array"
    },
    "obico_print_id": {
      "description": "Obico's own identifier for the print, when Obico has one.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "opened_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the print was opened."
    },
    "state": {
      "$ref": "#/$defs/PrinterState",
      "description": "The state the print is in."
    }
  },
  "required": [
    "id",
    "state",
    "opened_at",
    "narrowings"
  ],
  "title": "PrintRecord",
  "type": "object"
}
```

### PrinterSnapshot

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "HeaterSnapshot": {
      "additionalProperties": false,
      "description": "One heater, as a source reported it.\n\nEvery field is optional, because a source that reports no heater at all\nreports none of these; each is a plausibility-ranged reported value in\ndegrees Celsius.",
      "properties": {
        "actual_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The temperature the heater is at."
        },
        "offset_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The offset applied to this heater's target."
        },
        "target_c": {
          "anyOf": [
            {
              "$ref": "#/$defs/Reported"
            },
            {
              "type": "null"
            }
          ],
          "description": "The temperature the heater is driving towards."
        }
      },
      "type": "object"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Reported": {
      "additionalProperties": false,
      "description": "A value as a source reported it, flagged when it is outside the plausibility range this crate declares for its field.",
      "properties": {
        "out_of_range": {
          "type": "boolean"
        },
        "value": {
          "type": "number"
        }
      },
      "required": [
        "value",
        "out_of_range"
      ],
      "title": "Reported",
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A printer, as a source reported it at one instant.",
  "properties": {
    "bed": {
      "anyOf": [
        {
          "$ref": "#/$defs/HeaterSnapshot"
        },
        {
          "type": "null"
        }
      ],
      "description": "The bed heater, when the printer reports one."
    },
    "chamber": {
      "anyOf": [
        {
          "$ref": "#/$defs/HeaterSnapshot"
        },
        {
          "type": "null"
        }
      ],
      "description": "The chamber heater, when the printer reports one."
    },
    "connection": {
      "$ref": "#/$defs/PrinterState",
      "description": "The state the printer is in."
    },
    "fan_percent": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The part-cooling fan, in percent."
    },
    "feedrate_factor": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The feedrate multiplier, where one means one hundred percent."
    },
    "flowrate_factor": {
      "anyOf": [
        {
          "$ref": "#/$defs/Reported"
        },
        {
          "type": "null"
        }
      ],
      "description": "The flowrate multiplier, where one means one hundred percent."
    },
    "observed_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "The instant this observation was taken."
    },
    "tools": {
      "description": "The tool heaters, indexed by tool number.",
      "items": {
        "$ref": "#/$defs/HeaterSnapshot"
      },
      "type": "array"
    }
  },
  "required": [
    "connection",
    "tools",
    "observed_at"
  ],
  "title": "PrinterSnapshot",
  "type": "object"
}
```

### PrinterState

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
  "oneOf": [
    {
      "const": "operational",
      "description": "Connected and idle.",
      "type": "string"
    },
    {
      "const": "paused",
      "description": "Printing, but paused.",
      "type": "string"
    },
    {
      "const": "printing",
      "description": "Printing.",
      "type": "string"
    },
    {
      "const": "cancelling",
      "description": "Cancelling a print.",
      "type": "string"
    },
    {
      "const": "error",
      "description": "In an error state.",
      "type": "string"
    },
    {
      "const": "offline",
      "description": "Not reachable.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "A state this vocabulary does not name, in the source's own word for it.",
      "properties": {
        "unknown": {
          "type": "string"
        }
      },
      "required": [
        "unknown"
      ],
      "type": "object"
    }
  ],
  "title": "PrinterState"
}
```

### Range

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "An inclusive pair of 64-bit floats.",
  "properties": {
    "max": {
      "description": "The highest value the range admits, inclusive.",
      "format": "double",
      "type": "number"
    },
    "min": {
      "description": "The lowest value the range admits, inclusive.",
      "format": "double",
      "type": "number"
    }
  },
  "required": [
    "min",
    "max"
  ],
  "title": "Range",
  "type": "object"
}
```

### RawBytes

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "contentEncoding": "base64",
  "description": "Bytes exactly as received, base64-encoded.",
  "title": "RawBytes",
  "type": "string"
}
```

### RejectionReason

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "The value asked for is outside the range allowed for that adjustable.",
      "properties": {
        "out_of_bounds": {
          "additionalProperties": false,
          "properties": {
            "adjustable": {
              "$ref": "#/$defs/Adjustable",
              "description": "The adjustable that was asked for."
            },
            "allowed": {
              "$ref": "#/$defs/Range",
              "description": "The range that was allowed."
            },
            "requested": {
              "description": "The value that was asked for.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "adjustable",
            "requested",
            "allowed"
          ],
          "type": "object"
        }
      },
      "required": [
        "out_of_bounds"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "This actor class may not request this action at all.",
      "properties": {
        "actor_may_not_request": {
          "additionalProperties": false,
          "properties": {
            "action": {
              "$ref": "#/$defs/ActionKind",
              "description": "The action they asked for."
            },
            "actor_class": {
              "$ref": "#/$defs/ActorClass",
              "description": "The class of the actor that asked."
            }
          },
          "required": [
            "actor_class",
            "action"
          ],
          "type": "object"
        }
      },
      "required": [
        "actor_may_not_request"
      ],
      "type": "object"
    },
    {
      "const": "no_active_print",
      "description": "There is no active print to act on.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "The printer is not in a state this action is valid from.",
      "properties": {
        "invalid_from_state": {
          "additionalProperties": false,
          "properties": {
            "state": {
              "$ref": "#/$defs/PrinterState",
              "description": "The state the printer is in."
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        }
      },
      "required": [
        "invalid_from_state"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "The agent's minimum interval has not elapsed.",
      "properties": {
        "min_interval_not_elapsed": {
          "additionalProperties": false,
          "properties": {
            "interval_s": {
              "description": "The minimum interval, in whole seconds.",
              "format": "int64",
              "type": "integer"
            },
            "since_last_s": {
              "description": "How long it has been since the last agent action, in whole seconds.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "interval_s",
            "since_last_s"
          ],
          "type": "object"
        }
      },
      "required": [
        "min_interval_not_elapsed"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "The adjustable is not one this printer has.",
      "properties": {
        "unsupported_adjustable": {
          "additionalProperties": false,
          "properties": {
            "adjustable": {
              "$ref": "#/$defs/Adjustable",
              "description": "The adjustable this printer cannot express."
            }
          },
          "required": [
            "adjustable"
          ],
          "type": "object"
        }
      },
      "required": [
        "unsupported_adjustable"
      ],
      "type": "object"
    }
  ],
  "title": "RejectionReason"
}
```

### Reported

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A value as a source reported it, flagged when it is outside the plausibility range this crate declares for its field.",
  "properties": {
    "out_of_range": {
      "type": "boolean"
    },
    "value": {
      "type": "number"
    }
  },
  "required": [
    "value",
    "out_of_range"
  ],
  "title": "Reported",
  "type": "object"
}
```

### SafetyEnvelope

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Server configuration: what any actor may ask for at all.\n\nThis is the operator's safety envelope. It is a different contract from the\nplausibility ranges [`Reported`](crate::Reported) fields carry, is narrower\nby orders of magnitude, and the two are never intersected, compared or\nsubstituted for one another.",
  "properties": {
    "actions": {
      "additionalProperties": {
        "items": {
          "$ref": "#/$defs/ActionKind"
        },
        "type": "array"
      },
      "description": "The actions each actor class may request at all.",
      "type": "object"
    },
    "agent_min_interval_s": {
      "description": "The minimum interval between agent actions, in whole seconds.",
      "format": "int64",
      "type": "integer"
    },
    "allowed": {
      "additionalProperties": false,
      "description": "The range each adjustable may be set to, inclusive.",
      "patternProperties": {
        "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
          "$ref": "#/$defs/Range"
        }
      },
      "type": "object"
    }
  },
  "required": [
    "allowed",
    "actions",
    "agent_min_interval_s"
  ],
  "title": "SafetyEnvelope",
  "type": "object"
}
```

### SessionPhase

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Whether a turn opened a session or continued one.",
  "oneOf": [
    {
      "const": "created",
      "description": "This turn opened the session.",
      "type": "string"
    },
    {
      "const": "continued",
      "description": "This turn continued a session already open.",
      "type": "string"
    }
  ],
  "title": "SessionPhase"
}
```

### StartupOutcome

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
  "oneOf": [
    {
      "const": "print_adopted",
      "description": "A print left open was adopted as the print this supervisor is watching.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "A session left open was resumed rather than replaced.",
      "properties": {
        "session_resumed": {
          "additionalProperties": false,
          "properties": {
            "session_name": {
              "description": "The session's own name in the harness.",
              "type": "string"
            }
          },
          "required": [
            "session_name"
          ],
          "type": "object"
        }
      },
      "required": [
        "session_resumed"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "An intervention already past its expiry was expired on start.",
      "properties": {
        "intervention_expired": {
          "additionalProperties": false,
          "properties": {
            "adjustable": {
              "$ref": "#/$defs/Adjustable",
              "description": "What it had changed."
            },
            "intervention_id": {
              "$ref": "#/$defs/InterventionId",
              "description": "The intervention that had outlived its bound."
            },
            "outcome": {
              "$ref": "#/$defs/InterventionOutcome",
              "description": "What became of putting the prior value back."
            }
          },
          "required": [
            "intervention_id",
            "adjustable",
            "outcome"
          ],
          "type": "object"
        }
      },
      "required": [
        "intervention_expired"
      ],
      "type": "object"
    }
  ],
  "title": "StartupOutcome"
}
```

### StartupReconciliationPayload

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A supervisor reconciled one thing the store held when it started.",
  "properties": {
    "outcome": {
      "$ref": "#/$defs/StartupOutcome",
      "description": "What was reconciled."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print it is about."
    }
  },
  "required": [
    "print_id",
    "outcome"
  ],
  "title": "StartupReconciliationPayload",
  "type": "object"
}
```

### SupervisionSession

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The supervision session keyed to one print.",
  "properties": {
    "close_reason": {
      "description": "Why it was closed, if it was.",
      "type": [
        "string",
        "null"
      ]
    },
    "closed_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the session was closed, if it was."
    },
    "created_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the session was opened."
    },
    "harness_identity": {
      "description": "The identity the harness ran the session under.",
      "type": "string"
    },
    "last_turn_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the last turn ran."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print this session watches."
    },
    "session_name": {
      "description": "The session's own name in the harness.",
      "type": "string"
    }
  },
  "required": [
    "print_id",
    "session_name",
    "harness_identity",
    "created_at",
    "last_turn_at"
  ],
  "title": "SupervisionSession",
  "type": "object"
}
```

### SupervisionSessionClosedPayload

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A supervision session was closed.",
  "properties": {
    "close_reason": {
      "description": "Why it was closed.",
      "type": "string"
    },
    "session_name": {
      "description": "The session's own name in the harness.",
      "type": "string"
    }
  },
  "required": [
    "session_name",
    "close_reason"
  ],
  "title": "SupervisionSessionClosedPayload",
  "type": "object"
}
```

### SupervisionSessionOpenedPayload

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A supervision session was opened.",
  "properties": {
    "harness_identity": {
      "description": "The identity the harness ran it under.",
      "type": "string"
    },
    "session_name": {
      "description": "The session's own name in the harness.",
      "type": "string"
    }
  },
  "required": [
    "session_name",
    "harness_identity"
  ],
  "title": "SupervisionSessionOpenedPayload",
  "type": "object"
}
```

### Timestamp

Declared by `printobserver-types`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
  "format": "date-time",
  "title": "Timestamp",
  "type": "string"
}
```

### TurnOutcome

Declared by `printobserver-supervisor-api`.

```json
{
  "$defs": {
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "SessionPhase": {
      "description": "Whether a turn opened a session or continued one.",
      "oneOf": [
        {
          "const": "created",
          "description": "This turn opened the session.",
          "type": "string"
        },
        {
          "const": "continued",
          "description": "This turn continued a session already open.",
          "type": "string"
        }
      ]
    },
    "SupervisionSession": {
      "additionalProperties": false,
      "description": "The supervision session keyed to one print.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed, if it was.",
          "type": [
            "string",
            "null"
          ]
        },
        "closed_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the session was closed, if it was."
        },
        "created_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the session was opened."
        },
        "harness_identity": {
          "description": "The identity the harness ran the session under.",
          "type": "string"
        },
        "last_turn_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the last turn ran."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print this session watches."
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "print_id",
        "session_name",
        "harness_identity",
        "created_at",
        "last_turn_at"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What one supervision turn answered with.",
  "properties": {
    "assessment": {
      "$ref": "#/$defs/AgentAssessment",
      "description": "The agent's written record of the turn."
    },
    "phase": {
      "$ref": "#/$defs/SessionPhase",
      "description": "Whether this turn opened that session or continued it."
    },
    "session": {
      "$ref": "#/$defs/SupervisionSession",
      "description": "The session the turn ran in."
    }
  },
  "required": [
    "session",
    "phase",
    "assessment"
  ],
  "title": "TurnOutcome",
  "type": "object"
}
```

### TurnRequest

Declared by `printobserver-supervisor-api`.

```json
{
  "$defs": {
    "AcknowledgementDisposition": {
      "description": "What an operator's acknowledgement of a failure event asks for next.",
      "oneOf": [
        {
          "const": "continue",
          "description": "Carry on printing.",
          "type": "string"
        },
        {
          "const": "watch",
          "description": "Carry on printing, watched more closely.",
          "type": "string"
        },
        {
          "const": "stop",
          "description": "Stop the print.",
          "type": "string"
        }
      ]
    },
    "ActionExecutedPayload": {
      "additionalProperties": false,
      "description": "An accepted action reached the printer.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "intervention_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/InterventionId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bounded intervention it opened, when it opened one."
        }
      },
      "required": [
        "action_id"
      ],
      "type": "object"
    },
    "ActionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one requested action.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ActionId",
      "type": "string"
    },
    "ActionKind": {
      "description": "One action of the closed vocabulary, named without its payload.\n\nThis is what a safety envelope grants and what a policy rejection names; the\npayload lives on [`PrintAction`] itself.",
      "oneOf": [
        {
          "const": "pause",
          "description": "Pause the print.",
          "type": "string"
        },
        {
          "const": "resume",
          "description": "Resume the print.",
          "type": "string"
        },
        {
          "const": "cancel",
          "description": "Cancel the print.",
          "type": "string"
        },
        {
          "const": "start_print",
          "description": "Start a print of a named file.",
          "type": "string"
        },
        {
          "const": "set_feedrate_factor",
          "description": "Set the feedrate factor.",
          "type": "string"
        },
        {
          "const": "set_flowrate_factor",
          "description": "Set the flowrate factor.",
          "type": "string"
        },
        {
          "const": "set_tool_target_c",
          "description": "Set a tool's target temperature.",
          "type": "string"
        },
        {
          "const": "set_bed_target_c",
          "description": "Set the bed's target temperature.",
          "type": "string"
        },
        {
          "const": "set_fan_percent",
          "description": "Set the fan percentage.",
          "type": "string"
        },
        {
          "const": "acknowledge_failure",
          "description": "Acknowledge a failure event.",
          "type": "string"
        }
      ]
    },
    "ActionRejectedPayload": {
      "additionalProperties": false,
      "description": "Policy refused an action.",
      "properties": {
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "decision": {
          "$ref": "#/$defs/PolicyDecision",
          "description": "The whole decision, carrying which rejection it was."
        }
      },
      "required": [
        "action_id",
        "decision"
      ],
      "type": "object"
    },
    "ActionRequestedPayload": {
      "additionalProperties": false,
      "description": "An actor asked for an action.",
      "properties": {
        "action": {
          "$ref": "#/$defs/PrintAction",
          "description": "What was asked for."
        },
        "action_id": {
          "$ref": "#/$defs/ActionId",
          "description": "The action's identifier."
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who asked."
        }
      },
      "required": [
        "action_id",
        "action",
        "actor"
      ],
      "type": "object"
    },
    "Actor": {
      "description": "Who asked for something.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The supervising agent, naming its session.",
          "properties": {
            "agent": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The supervision session the agent is acting in.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "agent"
          ],
          "type": "object"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "ActorClass": {
      "description": "An actor class, which is what a safety envelope grants actions to.",
      "oneOf": [
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
      "type": "string"
    },
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n[`PrintAction`](crate::PrintAction) that policy rules on.",
      "properties": {
        "confidence": {
          "$ref": "#/$defs/Confidence",
          "description": "How sure the agent is."
        },
        "did": {
          "description": "What the agent did.",
          "type": "string"
        },
        "escalating": {
          "description": "Whether the agent is escalating to a person.",
          "type": "boolean"
        },
        "should_continue": {
          "description": "Whether the print should carry on.",
          "type": "boolean"
        },
        "summary": {
          "description": "One line saying what is happening.",
          "type": "string"
        },
        "why": {
          "description": "Why it did it.",
          "type": "string"
        }
      },
      "required": [
        "summary",
        "confidence",
        "should_continue",
        "did",
        "why",
        "escalating"
      ],
      "type": "object"
    },
    "AgentAssessmentPayload": {
      "additionalProperties": false,
      "description": "The agent wrote down what it made of a turn.",
      "properties": {
        "assessment": {
          "$ref": "#/$defs/AgentAssessment",
          "description": "What the agent answered with."
        },
        "session_name": {
          "description": "The session the turn ran in.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "assessment"
      ],
      "type": "object"
    },
    "Confidence": {
      "description": "How sure the agent is.\n\nA closed vocabulary rather than a number, because a number invites a\nprecision the agent does not have.",
      "oneOf": [
        {
          "const": "low",
          "description": "Not sure.",
          "type": "string"
        },
        {
          "const": "medium",
          "description": "Fairly sure.",
          "type": "string"
        },
        {
          "const": "high",
          "description": "Sure.",
          "type": "string"
        }
      ]
    },
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
      "oneOf": [
        {
          "description": "Obico reported a print failure.",
          "properties": {
            "kind": {
              "const": "obico_failure_alert",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ObicoFailureAlertPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "Obico sent a printer notification.",
          "properties": {
            "kind": {
              "const": "obico_printer_notification",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ObicoPrinterNotificationPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An external body arrived that could not be read.",
          "properties": {
            "kind": {
              "const": "malformed_external_event",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/MalformedExternalEventPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An actor asked for an action.",
          "properties": {
            "kind": {
              "const": "action_requested",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionRequestedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An accepted action reached the printer.",
          "properties": {
            "kind": {
              "const": "action_executed",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionExecutedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "Policy refused an action.",
          "properties": {
            "kind": {
              "const": "action_rejected",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/ActionRejectedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A bounded intervention expired.",
          "properties": {
            "kind": {
              "const": "intervention_expired",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/InterventionExpiredPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervision session was opened.",
          "properties": {
            "kind": {
              "const": "supervision_session_opened",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/SupervisionSessionOpenedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervision session was closed.",
          "properties": {
            "kind": {
              "const": "supervision_session_closed",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/SupervisionSessionClosedPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "The agent wrote down what it made of a turn.",
          "properties": {
            "kind": {
              "const": "agent_assessment",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/AgentAssessmentPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "An operator acknowledged an event.",
          "properties": {
            "kind": {
              "const": "operator_acknowledgement",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/OperatorAcknowledgementPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A port failed while an event was being handled.",
          "properties": {
            "kind": {
              "const": "port_failure",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/PortFailurePayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        },
        {
          "description": "A supervisor reconciled one thing the store held when it started.",
          "properties": {
            "kind": {
              "const": "startup_reconciliation",
              "type": "string"
            },
            "payload": {
              "$ref": "#/$defs/StartupReconciliationPayload"
            }
          },
          "required": [
            "kind",
            "payload"
          ],
          "type": "object"
        }
      ],
      "properties": {
        "id": {
          "$ref": "#/$defs/EventId",
          "description": "This event's identifier, minted by the store."
        },
        "image": {
          "anyOf": [
            {
              "$ref": "#/$defs/ImageRef"
            },
            {
              "type": "null"
            }
          ],
          "description": "The image it arrived with, when it arrived with one."
        },
        "print_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/PrintId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The print it belongs to, when it belongs to one."
        },
        "raw": {
          "anyOf": [
            {
              "$ref": "#/$defs/RawBytes"
            },
            {
              "type": "null"
            }
          ],
          "description": "The bytes exactly as received, for an externally sourced event."
        },
        "received_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was received."
        },
        "source": {
          "$ref": "#/$defs/EventSource",
          "description": "Where it came from."
        }
      },
      "required": [
        "id",
        "source",
        "received_at"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from.",
      "oneOf": [
        {
          "const": "obico",
          "description": "Obico, over its webhook.",
          "type": "string"
        },
        {
          "const": "operator",
          "description": "A person.",
          "type": "string"
        },
        {
          "const": "agent",
          "description": "The supervising agent.",
          "type": "string"
        },
        {
          "const": "system",
          "description": "The supervisor itself.",
          "type": "string"
        }
      ]
    },
    "FileName": {
      "description": "A file name a printer's own file API can be asked for: no path separator, no NUL byte, no `.` or `..` segment, no drive prefix, and not empty.",
      "minLength": 1,
      "not": {
        "pattern": "^([.]{1,2}$|[A-Za-z]:)"
      },
      "pattern": "^[^/\\\\\u0000]+$",
      "title": "FileName",
      "type": "string"
    },
    "ImageId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one image.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "ImageId",
      "type": "string"
    },
    "ImageRef": {
      "additionalProperties": false,
      "description": "The handle an image travels in context under.",
      "properties": {
        "id": {
          "$ref": "#/$defs/ImageId",
          "description": "The image's identifier."
        },
        "sha256": {
          "description": "The SHA-256 of its bytes, lowercase hexadecimal.",
          "type": "string"
        }
      },
      "required": [
        "id",
        "sha256"
      ],
      "type": "object"
    },
    "InterventionExpiredPayload": {
      "additionalProperties": false,
      "description": "A bounded intervention expired.",
      "properties": {
        "adjustable": {
          "$ref": "#/$defs/Adjustable",
          "description": "What it had changed."
        },
        "intervention_id": {
          "$ref": "#/$defs/InterventionId",
          "description": "The intervention's identifier."
        },
        "outcome": {
          "$ref": "#/$defs/InterventionOutcome",
          "description": "What became of it."
        }
      },
      "required": [
        "intervention_id",
        "adjustable",
        "outcome"
      ],
      "type": "object"
    },
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
    },
    "InterventionOutcome": {
      "description": "What became of a bounded change.",
      "oneOf": [
        {
          "const": "still_active",
          "description": "It is still in force.",
          "type": "string"
        },
        {
          "const": "restored",
          "description": "The prior value was put back.",
          "type": "string"
        },
        {
          "const": "restore_unavailable",
          "description": "There was no prior value to put back.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "Putting the prior value back failed.",
          "properties": {
            "restore_failed": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "description": "Why it failed.",
                  "type": "string"
                }
              },
              "required": [
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "restore_failed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Another intervention replaced it before it expired.",
          "properties": {
            "superseded": {
              "additionalProperties": false,
              "properties": {
                "by": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that replaced it."
                }
              },
              "required": [
                "by"
              ],
              "type": "object"
            }
          },
          "required": [
            "superseded"
          ],
          "type": "object"
        }
      ]
    },
    "JobManifest": {
      "additionalProperties": false,
      "description": "What a sliced job declares about itself and about what may be adjusted.\n\nAn adjustable the manifest does not name takes the envelope's own range; a\nmanifest range wider than the envelope's is narrowed to the envelope's and\nthe narrowing is recorded on the print. A manifest may only narrow.",
      "properties": {
        "allowed": {
          "additionalProperties": false,
          "description": "The range each named adjustable may take, inclusive.",
          "patternProperties": {
            "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$": {
              "$ref": "#/$defs/Range"
            }
          },
          "type": "object"
        },
        "file_name": {
          "description": "The file this manifest is about, as the slicer named it.",
          "type": "string"
        },
        "material": {
          "description": "The material the job is sliced for.",
          "type": "string"
        },
        "metadata": {
          "additionalProperties": {
            "type": "string"
          },
          "description": "Whatever else the slicer recorded.",
          "type": "object"
        },
        "nozzle_diameter_mm": {
          "description": "The nozzle the job is sliced for, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "slicer_profile": {
          "description": "The slicer profile the job was sliced with.",
          "type": "string"
        }
      },
      "required": [
        "file_name",
        "material",
        "nozzle_diameter_mm",
        "slicer_profile",
        "allowed",
        "metadata"
      ],
      "type": "object"
    },
    "MalformedExternalEventPayload": {
      "additionalProperties": false,
      "description": "An external body arrived that could not be read.\n\nThis kind always carries its `raw` bytes, and it exists so that an alert\nthis system cannot read is written down rather than dropped.",
      "properties": {
        "detail": {
          "description": "One line saying why the body could not be read.",
          "type": "string"
        }
      },
      "required": [
        "detail"
      ],
      "type": "object"
    },
    "ObicoFailureAlertPayload": {
      "additionalProperties": false,
      "description": "Obico reported a print failure.\n\nThe two instants are optional because Obico's own field for each is a Unix\ntimestamp number, an empty string, or absent, and the last two both mean the\nproducer reported no instant. An absent field here is that, never an epoch\ndate standing in for it.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when Obico named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "is_warning": {
          "description": "Whether Obico called it a warning rather than a failure.",
          "type": "boolean"
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when it named one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "print_paused": {
          "description": "Whether Obico paused the print itself.",
          "type": "boolean"
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "is_warning",
        "print_paused"
      ],
      "type": "object"
    },
    "ObicoNotificationType": {
      "description": "The kind of printer notification Obico sent, normalized.",
      "oneOf": [
        {
          "const": "started",
          "description": "A print started.",
          "type": "string"
        },
        {
          "const": "done",
          "description": "A print finished.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "A print was cancelled.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "A print was paused.",
          "type": "string"
        },
        {
          "const": "resumed",
          "description": "A print was resumed.",
          "type": "string"
        },
        {
          "const": "filament_change",
          "description": "The printer is waiting for a filament change.",
          "type": "string"
        },
        {
          "const": "heater_cooled",
          "description": "A heater cooled down.",
          "type": "string"
        },
        {
          "const": "heater_target",
          "description": "A heater reached its target.",
          "type": "string"
        }
      ]
    },
    "ObicoPrinterNotificationPayload": {
      "additionalProperties": false,
      "description": "Obico sent a printer notification.\n\nThe two instants are optional for the same reason\n[`ObicoFailureAlertPayload`]'s are, and are absent along with the rest of\nthe print's fields when the notification is about no print at all.",
      "properties": {
        "ended_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print ended, when Obico reported an instant for it."
        },
        "file_name": {
          "description": "The file being printed, when the notification is about one.",
          "type": [
            "string",
            "null"
          ]
        },
        "notification_type": {
          "$ref": "#/$defs/ObicoNotificationType",
          "description": "Which notification it is."
        },
        "obico_print_id": {
          "description": "Obico's own identifier for the print, when the notification is about one.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the print started, when Obico reported an instant for it."
        }
      },
      "required": [
        "notification_type"
      ],
      "type": "object"
    },
    "OperatorAcknowledgementPayload": {
      "additionalProperties": false,
      "description": "An operator acknowledged an event.",
      "properties": {
        "acknowledged_event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event being acknowledged."
        },
        "disposition": {
          "$ref": "#/$defs/AcknowledgementDisposition",
          "description": "What the operator asked for next."
        }
      },
      "required": [
        "acknowledged_event_id",
        "disposition"
      ],
      "type": "object"
    },
    "PolicyDecision": {
      "description": "The decision policy took on one request.",
      "oneOf": [
        {
          "const": "accepted",
          "description": "The request may proceed.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The request may not, for this reason.",
          "properties": {
            "rejected": {
              "$ref": "#/$defs/RejectionReason"
            }
          },
          "required": [
            "rejected"
          ],
          "type": "object"
        }
      ]
    },
    "PortFailurePayload": {
      "additionalProperties": false,
      "description": "A port failed while one event was being handled.\n\nThe event is named rather than implied, so that a reader holding an event's\nidentifier reaches every failure recorded while that event was being\nhandled. A failure recorded here is one the handling survived: the event is\nalready in the history by the time any of these sites is reached, and the\nloop goes on to handle the next event.",
      "properties": {
        "detail": {
          "description": "What the port said about it, in the port's own words.",
          "type": "string"
        },
        "event_id": {
          "$ref": "#/$defs/EventId",
          "description": "The event whose handling reached the failing call."
        },
        "site": {
          "$ref": "#/$defs/PortFailureSite",
          "description": "Where it failed."
        }
      },
      "required": [
        "event_id",
        "site",
        "detail"
      ],
      "type": "object"
    },
    "PortFailureSite": {
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
      "oneOf": [
        {
          "const": "printer_snapshot",
          "description": "Reading the printer's own state.",
          "type": "string"
        },
        {
          "const": "printer_job",
          "description": "Reading the job the printer reports it is running.",
          "type": "string"
        },
        {
          "const": "image_write",
          "description": "Writing the image the event arrived with.",
          "type": "string"
        },
        {
          "const": "supervision_turn",
          "description": "Running the supervision turn the event prompted.",
          "type": "string"
        }
      ]
    },
    "PrintAction": {
      "description": "The whole vocabulary an actor may ask for, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Pause the print.",
          "properties": {
            "action": {
              "const": "pause",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Resume the print.",
          "properties": {
            "action": {
              "const": "resume",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Cancel the print.",
          "properties": {
            "action": {
              "const": "cancel",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Start a print of a named file, with a manifest.",
          "properties": {
            "action": {
              "const": "start_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "file_name": {
              "$ref": "#/$defs/FileName",
              "description": "The file to print, validated as a name a file API can be asked for."
            },
            "manifest": {
              "$ref": "#/$defs/JobManifest",
              "description": "The manifest this print is bounded by."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "file_name",
            "manifest",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the feedrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_feedrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the flowrate factor, where one means one hundred percent.",
          "properties": {
            "action": {
              "const": "set_flowrate_factor",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "factor": {
              "description": "The multiplier asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "factor",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set one tool's target temperature.",
          "properties": {
            "action": {
              "const": "set_tool_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "action",
            "tool",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the bed's target temperature.",
          "properties": {
            "action": {
              "const": "set_bed_target_c",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "target_c": {
              "description": "The target temperature asked for, in degrees Celsius.",
              "format": "double",
              "type": "number"
            }
          },
          "required": [
            "action",
            "target_c",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Set the part-cooling fan percentage.",
          "properties": {
            "action": {
              "const": "set_fan_percent",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "duration_s": {
              "description": "How long the change stands for, in whole seconds.",
              "format": "int64",
              "type": [
                "integer",
                "null"
              ]
            },
            "percent": {
              "description": "The percentage asked for.",
              "format": "double",
              "type": "number"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "percent",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Acknowledge a failure event, with a disposition.",
          "properties": {
            "action": {
              "const": "acknowledge_failure",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "disposition": {
              "$ref": "#/$defs/AcknowledgementDisposition",
              "description": "What to do next."
            },
            "event_id": {
              "$ref": "#/$defs/EventId",
              "description": "The event being acknowledged."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "event_id",
            "disposition",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "PrinterState": {
      "description": "The state a source reports a printer or a print to be in.\n\nThe `unknown` arm exists so that a state nobody anticipated is recorded\ncarrying the source's own word for it rather than lost.",
      "oneOf": [
        {
          "const": "operational",
          "description": "Connected and idle.",
          "type": "string"
        },
        {
          "const": "paused",
          "description": "Printing, but paused.",
          "type": "string"
        },
        {
          "const": "printing",
          "description": "Printing.",
          "type": "string"
        },
        {
          "const": "cancelling",
          "description": "Cancelling a print.",
          "type": "string"
        },
        {
          "const": "error",
          "description": "In an error state.",
          "type": "string"
        },
        {
          "const": "offline",
          "description": "Not reachable.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A state this vocabulary does not name, in the source's own word for it.",
          "properties": {
            "unknown": {
              "type": "string"
            }
          },
          "required": [
            "unknown"
          ],
          "type": "object"
        }
      ]
    },
    "Range": {
      "additionalProperties": false,
      "description": "An inclusive pair of 64-bit floats.",
      "properties": {
        "max": {
          "description": "The highest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        },
        "min": {
          "description": "The lowest value the range admits, inclusive.",
          "format": "double",
          "type": "number"
        }
      },
      "required": [
        "min",
        "max"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "RejectionReason": {
      "description": "Why a request was refused.\n\nEach rejection is a distinct variant, so a consumer distinguishes them by\nmatching rather than by reading a message.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "The value asked for is outside the range allowed for that adjustable.",
          "properties": {
            "out_of_bounds": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable that was asked for."
                },
                "allowed": {
                  "$ref": "#/$defs/Range",
                  "description": "The range that was allowed."
                },
                "requested": {
                  "description": "The value that was asked for.",
                  "format": "double",
                  "type": "number"
                }
              },
              "required": [
                "adjustable",
                "requested",
                "allowed"
              ],
              "type": "object"
            }
          },
          "required": [
            "out_of_bounds"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "This actor class may not request this action at all.",
          "properties": {
            "actor_may_not_request": {
              "additionalProperties": false,
              "properties": {
                "action": {
                  "$ref": "#/$defs/ActionKind",
                  "description": "The action they asked for."
                },
                "actor_class": {
                  "$ref": "#/$defs/ActorClass",
                  "description": "The class of the actor that asked."
                }
              },
              "required": [
                "actor_class",
                "action"
              ],
              "type": "object"
            }
          },
          "required": [
            "actor_may_not_request"
          ],
          "type": "object"
        },
        {
          "const": "no_active_print",
          "description": "There is no active print to act on.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "The printer is not in a state this action is valid from.",
          "properties": {
            "invalid_from_state": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer is in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "invalid_from_state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The agent's minimum interval has not elapsed.",
          "properties": {
            "min_interval_not_elapsed": {
              "additionalProperties": false,
              "properties": {
                "interval_s": {
                  "description": "The minimum interval, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                },
                "since_last_s": {
                  "description": "How long it has been since the last agent action, in whole seconds.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "interval_s",
                "since_last_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "min_interval_not_elapsed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "The adjustable is not one this printer has.",
          "properties": {
            "unsupported_adjustable": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "The adjustable this printer cannot express."
                }
              },
              "required": [
                "adjustable"
              ],
              "type": "object"
            }
          },
          "required": [
            "unsupported_adjustable"
          ],
          "type": "object"
        }
      ]
    },
    "StartupOutcome": {
      "description": "What one restart put back the way it found it.\n\nA supervisor that has been restarted adopts whatever the store holds rather\nthan starting empty, and each of these is one of those adoptions. They are\nrecorded rather than merely done, because a print that carried on across a\nrestart and one that was started again look identical afterwards unless the\nhistory says which happened.",
      "oneOf": [
        {
          "const": "print_adopted",
          "description": "A print left open was adopted as the print this supervisor is watching.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A session left open was resumed rather than replaced.",
          "properties": {
            "session_resumed": {
              "additionalProperties": false,
              "properties": {
                "session_name": {
                  "description": "The session's own name in the harness.",
                  "type": "string"
                }
              },
              "required": [
                "session_name"
              ],
              "type": "object"
            }
          },
          "required": [
            "session_resumed"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "An intervention already past its expiry was expired on start.",
          "properties": {
            "intervention_expired": {
              "additionalProperties": false,
              "properties": {
                "adjustable": {
                  "$ref": "#/$defs/Adjustable",
                  "description": "What it had changed."
                },
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention that had outlived its bound."
                },
                "outcome": {
                  "$ref": "#/$defs/InterventionOutcome",
                  "description": "What became of putting the prior value back."
                }
              },
              "required": [
                "intervention_id",
                "adjustable",
                "outcome"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_expired"
          ],
          "type": "object"
        }
      ]
    },
    "StartupReconciliationPayload": {
      "additionalProperties": false,
      "description": "A supervisor reconciled one thing the store held when it started.",
      "properties": {
        "outcome": {
          "$ref": "#/$defs/StartupOutcome",
          "description": "What was reconciled."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it is about."
        }
      },
      "required": [
        "print_id",
        "outcome"
      ],
      "type": "object"
    },
    "SupervisionSessionClosedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was closed.",
      "properties": {
        "close_reason": {
          "description": "Why it was closed.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "close_reason"
      ],
      "type": "object"
    },
    "SupervisionSessionOpenedPayload": {
      "additionalProperties": false,
      "description": "A supervision session was opened.",
      "properties": {
        "harness_identity": {
          "description": "The identity the harness ran it under.",
          "type": "string"
        },
        "session_name": {
          "description": "The session's own name in the harness.",
          "type": "string"
        }
      },
      "required": [
        "session_name",
        "harness_identity"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What one supervision turn is asked to consider.",
  "properties": {
    "context_command": {
      "description": "The command the turn runs to read the print's context.",
      "type": "string"
    },
    "event": {
      "$ref": "#/$defs/EventRecord",
      "description": "The event that prompted the turn."
    },
    "image_path": {
      "description": "The absolute path of the image to look at, when there is one.",
      "type": [
        "string",
        "null"
      ]
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print the turn is about."
    }
  },
  "required": [
    "print_id",
    "event",
    "context_command"
  ],
  "title": "TurnRequest",
  "type": "object"
}
```
