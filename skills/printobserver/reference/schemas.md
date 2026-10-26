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

132 types. The set is every type any crate declares under
`schemas/<crate>/`, keyed by type name across every declaring crate: the
contract crate's shared vocabulary, the request and answer shapes the port
crates own, each domain's event payloads — marked with `x-event-kind`, the
kind each is written under — and the shapes the server answers. A port's own
error vocabulary is deliberately not in it: it reaches no process boundary, so
it emits no schema.

The set is read off the tree the contracts' generation target writes and prunes,
so a type entering or leaving it moves this document with it.

## The schemas

### AcknowledgementDisposition

Declared by `printobserver-core`.

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
      "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
      "type": "string"
    }
  ],
  "title": "AcknowledgementDisposition"
}
```

### ActionAnswer

Declared by `printobserver-server`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
          "type": "string"
        }
      ]
    },
    "ActionRecord": {
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
      "type": "object"
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
            "override_filament_check": {
              "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
              "type": "boolean"
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
        },
        {
          "additionalProperties": false,
          "description": "Close a print record printobserver left open, with the outcome the\noperator judges it ended with. It asks nothing of the machine and\ncharges no spool: the record's consumption was never seen to end.",
          "properties": {
            "action": {
              "const": "close_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "outcome": {
              "$ref": "#/$defs/PrintOutcome",
              "description": "How the print ended, in the operator's judgement."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "outcome",
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
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
          "type": "string"
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What one mutating request left behind it.",
  "properties": {
    "intervention": {
      "anyOf": [
        {
          "$ref": "#/$defs/Intervention"
        },
        {
          "type": "null"
        }
      ],
      "description": "The bounded intervention it opened, when it opened one."
    },
    "printer_refusal": {
      "description": "What the printer said, when the request reached it and it refused.",
      "type": [
        "string",
        "null"
      ]
    },
    "record": {
      "$ref": "#/$defs/ActionRecord",
      "description": "The record of the request and the decision taken on it. A rejected\nrequest's rejection is here, carrying its reason, the value asked for\nand the range allowed, so a caller can ask again inside the range."
    }
  },
  "required": [
    "record"
  ],
  "title": "ActionAnswer",
  "type": "object"
}
```

### ActionExecutedPayload

Declared by `printobserver-core`.

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
  "type": "object",
  "x-event-kind": "action_executed"
}
```

### ActionId

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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
    },
    {
      "const": "close_print",
      "description": "Close a print record printobserver left open.",
      "type": "string"
    },
    {
      "const": "register_spool",
      "description": "Register a spool.",
      "type": "string"
    },
    {
      "const": "load_spool",
      "description": "Load a spool onto a tool.",
      "type": "string"
    },
    {
      "const": "unload_spool",
      "description": "Unload a spool from its tool.",
      "type": "string"
    },
    {
      "const": "adjust_spool",
      "description": "Correct a spool's remaining figure.",
      "type": "string"
    },
    {
      "const": "retire_spool",
      "description": "Retire a spool.",
      "type": "string"
    }
  ],
  "title": "ActionKind"
}
```

### ActionRecord

Declared by `printobserver-core`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
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
            "override_filament_check": {
              "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
              "type": "boolean"
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
        },
        {
          "additionalProperties": false,
          "description": "Close a print record printobserver left open, with the outcome the\noperator judges it ended with. It asks nothing of the machine and\ncharges no spool: the record's consumption was never seen to end.",
          "properties": {
            "action": {
              "const": "close_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "outcome": {
              "$ref": "#/$defs/PrintOutcome",
              "description": "How the print ended, in the operator's judgement."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "outcome",
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
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
          "type": "string"
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
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

Declared by `printobserver-core`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
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
  "type": "object",
  "x-event-kind": "action_rejected"
}
```

### ActionRequest

Declared by `printobserver-core`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
            "override_filament_check": {
              "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
              "type": "boolean"
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
        },
        {
          "additionalProperties": false,
          "description": "Close a print record printobserver left open, with the outcome the\noperator judges it ended with. It asks nothing of the machine and\ncharges no spool: the record's consumption was never seen to end.",
          "properties": {
            "action": {
              "const": "close_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "outcome": {
              "$ref": "#/$defs/PrintOutcome",
              "description": "How the print ended, in the operator's judgement."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "outcome",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
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

Declared by `printobserver-core`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
            "override_filament_check": {
              "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
              "type": "boolean"
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
        },
        {
          "additionalProperties": false,
          "description": "Close a print record printobserver left open, with the outcome the\noperator judges it ended with. It asks nothing of the machine and\ncharges no spool: the record's consumption was never seen to end.",
          "properties": {
            "action": {
              "const": "close_print",
              "type": "string"
            },
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "outcome": {
              "$ref": "#/$defs/PrintOutcome",
              "description": "How the print ended, in the operator's judgement."
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            }
          },
          "required": [
            "action",
            "outcome",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
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
  "type": "object",
  "x-event-kind": "action_requested"
}
```

### Actor

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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

Declared by `printobserver-printer-api`.

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

Declared by `printobserver-supervisor-api`.

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
  "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n`PrintAction` that policy rules on.",
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

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n`PrintAction` that policy rules on.",
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
  "type": "object",
  "x-event-kind": "agent_assessment"
}
```

### CameraLookPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
  "description": "Somebody took a fresh look at the print.\n\nWritten down before the camera is asked for a frame, so that a frame the\ncamera would not give is recorded against a look that is already in the\nhistory; the frame itself, when there is one, is this event's image.",
  "properties": {
    "delivered": {
      "default": [],
      "description": "The events for the print that arrived while it waited, and that it\nhanded to whoever looked, oldest first.",
      "items": {
        "$ref": "#/$defs/EventId"
      },
      "type": "array"
    },
    "waited_s": {
      "description": "How long the look waited for something to happen before it was taken,\nin whole seconds.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    }
  },
  "required": [
    "waited_s"
  ],
  "title": "CameraLookPayload",
  "type": "object",
  "x-event-kind": "camera_look"
}
```

### ChargeKey

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The durable identity of one debit, so that a replay can never debit twice.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "One segment of a print this system watched, on one tool.",
      "properties": {
        "key": {
          "const": "print",
          "type": "string"
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print."
        },
        "segment": {
          "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
          "format": "uint32",
          "minimum": 0,
          "type": "integer"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "key",
        "print_id",
        "tool",
        "segment"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "One print this system never saw, on one tool.",
      "properties": {
        "file_name": {
          "description": "The file that was printed.",
          "type": "string"
        },
        "generation": {
          "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
          "format": "uint32",
          "minimum": 0,
          "type": "integer"
        },
        "key": {
          "const": "unobserved",
          "type": "string"
        },
        "origin": {
          "description": "Where the file lives, in the printer's own vocabulary.",
          "type": "string"
        },
        "print_number": {
          "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
          "format": "int64",
          "type": "integer"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "key",
        "origin",
        "file_name",
        "generation",
        "print_number",
        "tool"
      ],
      "type": "object"
    }
  ],
  "title": "ChargeKey"
}
```

### ChargeMethod

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "How the length a charge debits was worked out.",
  "oneOf": [
    {
      "const": "slicer_estimate",
      "description": "A completed print: the length the file's analysis gives.",
      "type": "string"
    },
    {
      "const": "gcode_extrusion",
      "description": "The extrusion the file's own moves add up to, up to a position in it.",
      "type": "string"
    },
    {
      "const": "file_fraction",
      "description": "The analysed length scaled by how far into the file the print got; the\nfallback used only when the file itself could not be read.",
      "type": "string"
    },
    {
      "const": "unobserved_estimate",
      "description": "A completed print this system never saw: the file's analysed length.",
      "type": "string"
    }
  ],
  "title": "ChargeMethod"
}
```

### ChargePendedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ChargeMethod": {
      "description": "How the length a charge debits was worked out.",
      "oneOf": [
        {
          "const": "slicer_estimate",
          "description": "A completed print: the length the file's analysis gives.",
          "type": "string"
        },
        {
          "const": "gcode_extrusion",
          "description": "The extrusion the file's own moves add up to, up to a position in it.",
          "type": "string"
        },
        {
          "const": "file_fraction",
          "description": "The analysed length scaled by how far into the file the print got; the\nfallback used only when the file itself could not be read.",
          "type": "string"
        },
        {
          "const": "unobserved_estimate",
          "description": "A completed print this system never saw: the file's analysed length.",
          "type": "string"
        }
      ]
    },
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
      ],
      "type": "object"
    },
    "PendingCharge": {
      "additionalProperties": false,
      "description": "Filament a tool fed after a filament change nobody has confirmed, held until\nsomebody says which spool fed it.\n\nA length rather than grams, because the grams follow the spool that takes\nit.",
      "properties": {
        "analysed_length_mm": {
          "description": "The file's analysed length for the tool, in millimetres, when it had one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "charge_key": {
          "$ref": "#/$defs/ChargeKey",
          "description": "The key of the print segment the filament was fed in."
        },
        "length_mm": {
          "description": "How much was fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "method": {
          "$ref": "#/$defs/ChargeMethod",
          "description": "How that length was worked out."
        },
        "note": {
          "description": "Anything a reader needs to make sense of the figure.",
          "type": [
            "string",
            "null"
          ]
        },
        "pended_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was held."
        },
        "position": {
          "anyOf": [
            {
              "$ref": "#/$defs/FilePosition"
            },
            {
              "type": "null"
            }
          ],
          "description": "How far into the file the segment ran, when that was read."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it was fed in."
        },
        "tool": {
          "description": "The tool that fed it.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "charge_key",
        "print_id",
        "tool",
        "length_mm",
        "method",
        "pended_at"
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
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Filament a tool fed after an unconfirmed filament change was held for the\nnext spool loaded onto that tool, or confirmed on it.",
  "properties": {
    "pending": {
      "$ref": "#/$defs/PendingCharge",
      "description": "What was held."
    }
  },
  "required": [
    "pending"
  ],
  "title": "ChargePendedPayload",
  "type": "object",
  "x-event-kind": "charge_pended"
}
```

### ClearedMark

Declared by `printobserver-core`.

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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One mark a spool command cleared, and the spool it was cleared from.",
  "properties": {
    "reason": {
      "$ref": "#/$defs/UnverifiedReason",
      "description": "The reason that no longer stands."
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool the mark was cleared from."
    }
  },
  "required": [
    "spool_id",
    "reason"
  ],
  "title": "ClearedMark",
  "type": "object"
}
```

### Confidence

Declared by `printobserver-supervisor-api`.

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

### ContextAnswer

Declared by `printobserver-server`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
        "filament": {
          "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
          "items": {
            "$ref": "#/$defs/ToolFilament"
          },
          "type": "array"
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
        "file_position_bytes": {
          "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
          "format": "int64",
          "type": [
            "integer",
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
    "PrintContext": {
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
      "type": "object"
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
      "description": "One print, and the record supervision keys from.\n\nThe provider's own print id is carried beside this record's identifier\nrather than as it, because a print may be observed before the provider\nthat reports it has one.",
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
        "job_print_time_s": {
          "description": "The longest running time, in whole seconds, the printer reported for\nthis print's job at a read that found it printing rather than paused.\n\nThe time a job has spent printing only grows, pause or no pause, so a\njob of this print's file reporting less \u2014 by more than\n`listing::JOB_IDENTITY_TOLERANCE_S` \u2014 is a later job. Absent until a\nread finds the job printing.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "job_started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the printer's job this print records began, as the printer's own\nrunning time put it at the latest read that found the job running or\npaused: the instant of that read, less the running time it reported.\n\n`OctoPrint` counts a pause into the running time until the job resumes\nand then takes the pause back out, so a pause moves this later: a read\nputting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one\nrecorded resets it, rather than it being fixed once. Absent when no read has\nfound this print's job reporting a running time \u2014 a print opened by an\nalert or a start before any read, and every print recorded before this\nwas."
        },
        "narrowings": {
          "description": "Every manifest range this print narrowed to the envelope's.",
          "items": {
            "$ref": "#/$defs/ManifestNarrowing"
          },
          "type": "array"
        },
        "opened_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the print was opened."
        },
        "provider_print_id": {
          "description": "The provider's own identifier for the print, whichever provider\nreported it, when one has.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What a context read answers.\n\nThe latest image is **materialized** here rather than left as the handle\n[`PrintContext`] carries: the one thing a supervision turn does with an\nimage is look at it, and a handle is not something anything can open. The\npath is absolute on this server's own filesystem, exactly as\n[`ImageAnswer`]'s is, and is absent when there is no image or when the\nrecord is intact and the file is not.",
  "properties": {
    "context": {
      "$ref": "#/$defs/PrintContext",
      "description": "Everything a supervision turn is given about the print."
    },
    "image_path": {
      "description": "Where the latest image's bytes are, absolute on this host.",
      "type": [
        "string",
        "null"
      ]
    }
  },
  "required": [
    "context"
  ],
  "title": "ContextAnswer",
  "type": "object"
}
```

### Detection

Declared by `printobserver-vision-api`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What a provider's failure detector did about a print, when an alert is one\nof its detections.\n\nNamed for what it is rather than for the provider, like [`ProviderPrint`]:\nwhether the detector only warned, whether it paused the print itself, and\nthe provider's own identifier for the printer, which is what telling it the\ndetection was handled is addressed to.",
  "properties": {
    "paused_the_print": {
      "description": "Whether the detector paused the print itself before alerting.",
      "type": "boolean"
    },
    "provider_printer_id": {
      "description": "The provider's own identifier for the printer the detection is about.",
      "format": "int64",
      "type": "integer"
    },
    "warning": {
      "description": "Whether the detector called it a warning rather than a failure.",
      "type": "boolean"
    }
  },
  "required": [
    "warning",
    "paused_the_print",
    "provider_printer_id"
  ],
  "title": "Detection",
  "type": "object"
}
```

### EffectiveBounds

Declared by `printobserver-core`.

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

### ErrorAnswer

Declared by `printobserver-server`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What this server answers when it will not do what it was asked.",
  "properties": {
    "error": {
      "description": "One line saying why, in the words of whatever refused it.",
      "type": "string"
    }
  },
  "required": [
    "error"
  ],
  "title": "ErrorAnswer",
  "type": "object"
}
```

### EventBody

Declared by `printobserver-types`.

```json
{
  "$defs": {
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The kind and the payload, as the log holds them.\n\nThe pair `{\"kind\": .., \"payload\": ..}`: the kind is the name a payload type\ndeclares, and the payload is that type's value, held as JSON so that a body\nunder a kind this crate has never heard of is carried rather than refused.",
  "properties": {
    "kind": {
      "$ref": "#/$defs/EventKind",
      "description": "Which event this is."
    },
    "payload": {
      "description": "What it carries, in the form its kind declares."
    }
  },
  "required": [
    "kind",
    "payload"
  ],
  "title": "EventBody",
  "type": "object"
}
```

### EventDraft

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
      "type": "string"
    },
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "One event on its way into the store, before the store mints its identifier.",
  "properties": {
    "kind": {
      "$ref": "#/$defs/EventKind",
      "description": "Which event this is."
    },
    "payload": {
      "description": "What it carries, in the form its kind declares."
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
    "source",
    "received_at",
    "kind",
    "payload"
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
  "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
  "pattern": "^[a-z][a-z0-9_]*$",
  "title": "EventKind",
  "type": "string"
}
```

### EventRecord

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
    "kind": {
      "$ref": "#/$defs/EventKind",
      "description": "Which event this is."
    },
    "payload": {
      "description": "What it carries, in the form its kind declares."
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
    "received_at",
    "kind",
    "payload"
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
  "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
  "title": "EventSource",
  "type": "string"
}
```

### ExecutionOutcome

Declared by `printobserver-core`.

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

### FilamentChangeSignalledPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
      ],
      "type": "object"
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
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
  "description": "A filament change was signalled during a print.",
  "properties": {
    "position": {
      "anyOf": [
        {
          "$ref": "#/$defs/FilePosition"
        },
        {
          "type": "null"
        }
      ],
      "description": "The position the spool was charged up to, when one was known."
    },
    "position_read_at_signal": {
      "description": "Whether that position was read when the signal was handled, rather than\nbeing the last one recorded; never with no position at all.",
      "type": "boolean"
    },
    "source_kind": {
      "$ref": "#/$defs/EventKind",
      "description": "The kind of the alert that carried the signal."
    },
    "spool_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/SpoolId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The spool that was on it, when one was tracked."
    },
    "tool": {
      "description": "The tool the change was signalled for, when it is known.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "source_kind",
    "position_read_at_signal"
  ],
  "title": "FilamentChangeSignalledPayload",
  "type": "object",
  "x-event-kind": "filament_change_signalled"
}
```

### FilamentConsumedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ChargeMethod": {
      "description": "How the length a charge debits was worked out.",
      "oneOf": [
        {
          "const": "slicer_estimate",
          "description": "A completed print: the length the file's analysis gives.",
          "type": "string"
        },
        {
          "const": "gcode_extrusion",
          "description": "The extrusion the file's own moves add up to, up to a position in it.",
          "type": "string"
        },
        {
          "const": "file_fraction",
          "description": "The analysed length scaled by how far into the file the print got; the\nfallback used only when the file itself could not be read.",
          "type": "string"
        },
        {
          "const": "unobserved_estimate",
          "description": "A completed print this system never saw: the file's analysed length.",
          "type": "string"
        }
      ]
    },
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
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
  "description": "Filament was charged against a spool, or recorded uncharged.",
  "properties": {
    "analysed_length_mm": {
      "description": "The file's analysed length for the tool, in millimetres, when it had one.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "charge_key": {
      "$ref": "#/$defs/ChargeKey",
      "description": "The durable identity of this debit."
    },
    "density_g_cm3": {
      "description": "The density the grams were worked out with, in grams per cubic\ncentimetre.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "diameter_mm": {
      "description": "The diameter the grams were worked out with, in millimetres.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "grams": {
      "description": "What that length weighs, in grams, when a spool gave its diameter and\ndensity.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "length_mm": {
      "description": "How much was fed, in millimetres.",
      "format": "double",
      "type": "number"
    },
    "method": {
      "$ref": "#/$defs/ChargeMethod",
      "description": "How the length was worked out."
    },
    "note": {
      "description": "Anything a reader needs to make sense of the figure.",
      "type": [
        "string",
        "null"
      ]
    },
    "position": {
      "anyOf": [
        {
          "$ref": "#/$defs/FilePosition"
        },
        {
          "type": "null"
        }
      ],
      "description": "How far into the file the charge runs, when that was read."
    },
    "remaining_after_g": {
      "description": "The spool's figure after it, in grams.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "remaining_before_g": {
      "description": "The spool's figure before the charge, in grams.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "spool_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/SpoolId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The spool charged; absent when nothing was tracked on the tool, and the\nfilament was recorded uncharged."
    },
    "tool": {
      "description": "The tool that fed it.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "charge_key",
    "tool",
    "method",
    "length_mm"
  ],
  "title": "FilamentConsumedPayload",
  "type": "object",
  "x-event-kind": "filament_consumed"
}
```

### FileFacts

Declared by `printobserver-printer-api`.

```json
{
  "$defs": {
    "FilePrint": {
      "additionalProperties": false,
      "description": "The last time a file was printed, and whether that print completed.",
      "properties": {
        "at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was printed."
        },
        "success": {
          "description": "Whether that print completed.",
          "type": "boolean"
        }
      },
      "required": [
        "at",
        "success"
      ],
      "type": "object"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One file the printer holds, as its source describes it.",
  "properties": {
    "failures": {
      "description": "How many prints of the file did not, by the source's own tally.",
      "format": "int64",
      "type": "integer"
    },
    "filament": {
      "description": "The filament the source's analysis of the file says each tool uses.\n\nAbsent when the file has no analysis: one still pending, or a file the\nsource never analyses, such as one on an SD card.",
      "items": {
        "$ref": "#/$defs/ToolFilament"
      },
      "type": [
        "array",
        "null"
      ]
    },
    "file_name": {
      "description": "The file's name, as the source reported it.",
      "type": "string"
    },
    "last_print": {
      "anyOf": [
        {
          "$ref": "#/$defs/FilePrint"
        },
        {
          "type": "null"
        }
      ],
      "description": "The file's most recent print, when it has been printed."
    },
    "origin": {
      "description": "Where the file lives, in the source's own vocabulary.",
      "type": "string"
    },
    "size_bytes": {
      "description": "The file's size in bytes, when the source reported it.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "successes": {
      "description": "How many prints of the file completed, by the source's own tally.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "file_name",
    "origin",
    "successes",
    "failures"
  ],
  "title": "FileFacts",
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

### FilePosition

Declared by `printobserver-core`.

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
  "description": "How far into its file a print had got, and when that was read.",
  "properties": {
    "bytes": {
      "description": "The byte the printer had read the file up to.",
      "format": "int64",
      "type": "integer"
    },
    "observed_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the position was read."
    },
    "size_bytes": {
      "description": "The file's size in bytes, when it was known.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    }
  },
  "required": [
    "bytes",
    "observed_at"
  ],
  "title": "FilePosition",
  "type": "object"
}
```

### FilePrint

Declared by `printobserver-printer-api`.

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
  "description": "The last time a file was printed, and whether that print completed.",
  "properties": {
    "at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was printed."
    },
    "success": {
      "description": "Whether that print completed.",
      "type": "boolean"
    }
  },
  "required": [
    "at",
    "success"
  ],
  "title": "FilePrint",
  "type": "object"
}
```

### FileTallyRebaselinedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "RebaselineDisposition": {
      "description": "What became of the prints a file's new tally generation counts.",
      "oneOf": [
        {
          "const": "accounted",
          "description": "They were accounted as prints this system never saw.",
          "type": "string"
        },
        {
          "const": "unverified",
          "description": "It could not be told which of them this system already settled, so the\nloaded spool was marked unverified rather than charged on a guess.",
          "type": "string"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The printer's tally for a file went down \u2014 the file was deleted and\nuploaded again \u2014 so the file's tally generation advanced, and the new count\nis that generation's growth from zero.\n\nThe rebaseline itself marks no spool; only an `unverified` disposition does.",
  "properties": {
    "disposition": {
      "$ref": "#/$defs/RebaselineDisposition",
      "description": "What became of the prints the new generation counts."
    },
    "file_name": {
      "description": "The file whose tally went down.",
      "type": "string"
    },
    "new_failures": {
      "description": "The failed prints the new tally counts.",
      "format": "int64",
      "type": "integer"
    },
    "new_generation": {
      "description": "The generation it is in now.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "new_successes": {
      "description": "The completed prints the new tally counts.",
      "format": "int64",
      "type": "integer"
    },
    "old_failures": {
      "description": "The failed prints the old tally counted.",
      "format": "int64",
      "type": "integer"
    },
    "old_generation": {
      "description": "The generation the tally was in.",
      "format": "uint32",
      "minimum": 0,
      "type": "integer"
    },
    "old_successes": {
      "description": "The completed prints the old tally counted.",
      "format": "int64",
      "type": "integer"
    },
    "origin": {
      "description": "Where it lives, in the printer's own vocabulary.",
      "type": "string"
    }
  },
  "required": [
    "file_name",
    "origin",
    "old_generation",
    "new_generation",
    "old_successes",
    "old_failures",
    "new_successes",
    "new_failures",
    "disposition"
  ],
  "title": "FileTallyRebaselinedPayload",
  "type": "object",
  "x-event-kind": "file_tally_rebaselined"
}
```

### HeaterSnapshot

Declared by `printobserver-printer-api`.

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

### HistoryAnswer

Declared by `printobserver-server`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "What a history read answers.",
  "properties": {
    "events": {
      "description": "The print's events, newest first.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    }
  },
  "required": [
    "events"
  ],
  "title": "HistoryAnswer",
  "type": "object"
}
```

### HistoryQuery

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
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

### ImageAnswer

Declared by `printobserver-server`.

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
    "ImageRecord": {
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
      "type": "object"
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
  "description": "What an image read answers.\n\nThe path is absolute on this server's own filesystem, and is the whole of\nthe answer about the bytes: nothing here renders them, encodes them or\nserves them. It is absent when the record is intact and the file is not,\nwhich is a different answer from there being no such image.",
  "properties": {
    "path": {
      "description": "Where its bytes are, absolute on this host.",
      "type": [
        "string",
        "null"
      ]
    },
    "record": {
      "$ref": "#/$defs/ImageRecord",
      "description": "The record."
    }
  },
  "required": [
    "record"
  ],
  "title": "ImageAnswer",
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

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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
  "type": "object",
  "x-event-kind": "intervention_expired"
}
```

### InterventionId

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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

Declared by `printobserver-core`.

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

Declared by `printobserver-printer-api`.

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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
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
    "filament": {
      "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
      "items": {
        "$ref": "#/$defs/ToolFilament"
      },
      "type": "array"
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
    "file_position_bytes": {
      "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
      "format": "int64",
      "type": [
        "integer",
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

### LoadedSpool

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One spool loaded on one tool, as a print started.",
  "properties": {
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool loaded on it."
    },
    "tool": {
      "description": "The tool, in the printer's own numbering.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "tool",
    "spool_id"
  ],
  "title": "LoadedSpool",
  "type": "object"
}
```

### Look

Declared by `printobserver-core`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
        "filament": {
          "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
          "items": {
            "$ref": "#/$defs/ToolFilament"
          },
          "type": "array"
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
        "file_position_bytes": {
          "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
          "format": "int64",
          "type": [
            "integer",
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
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
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "One fresh look at a print.\n\nAn answer rather than a record, so it admits fields beside its own the way\nevery answer of the server does: a client that could not open the frame's\npath says so in a field of its own beside the rest of the answer.",
  "properties": {
    "arrived": {
      "description": "The events for this print that arrived while the look waited, oldest\nfirst; the look returned early when there were any.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    },
    "detector_paused": {
      "description": "Whether the print is paused and the pause is the detector's, which is\nwhat an adjustment asked for now would be applied under.",
      "type": "boolean"
    },
    "event": {
      "$ref": "#/$defs/EventRecord",
      "description": "The look itself, as it was written into the print's history, carrying\nthe frame as its image when there is one."
    },
    "frame": {
      "anyOf": [
        {
          "$ref": "#/$defs/ImageRef"
        },
        {
          "type": "null"
        }
      ],
      "description": "The frame the camera gave, absent when no camera is configured or it\ngave none \u2014 which the history records as a port failure."
    },
    "image_path": {
      "description": "The absolute path the frame's bytes are at, on the supervisor's host.",
      "type": [
        "string",
        "null"
      ]
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
      "description": "The job the printer reports now, absent when it could not be read."
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
      "description": "The printer's state now, absent when it could not be read."
    }
  },
  "required": [
    "event",
    "detector_paused"
  ],
  "title": "Look",
  "type": "object"
}
```

### MalformedExternalEventPayload

Declared by `printobserver-vision-api`.

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
  "type": "object",
  "x-event-kind": "malformed_external_event"
}
```

### ManifestAnswer

Declared by `printobserver-server`.

```json
{
  "$defs": {
    "Adjustable": {
      "description": "One thing an adjustment may change.",
      "pattern": "^(feedrate|flowrate|bed_target|fan|tool_target:-?[0-9]+)$",
      "title": "Adjustable",
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
  "description": "What a manifest read or write answers.",
  "properties": {
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
    "narrowings": {
      "description": "Every range this manifest asked wider than the envelope allows, narrowed\nto the envelope's \u2014 recorded on the print, so nobody has to wonder later\nwhich bound applied.",
      "items": {
        "$ref": "#/$defs/ManifestNarrowing"
      },
      "type": "array"
    }
  },
  "required": [
    "narrowings"
  ],
  "title": "ManifestAnswer",
  "type": "object"
}
```

### ManifestNarrowing

Declared by `printobserver-core`.

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
    "Detection": {
      "additionalProperties": false,
      "description": "What a provider's failure detector did about a print, when an alert is one\nof its detections.\n\nNamed for what it is rather than for the provider, like [`ProviderPrint`]:\nwhether the detector only warned, whether it paused the print itself, and\nthe provider's own identifier for the printer, which is what telling it the\ndetection was handled is addressed to.",
      "properties": {
        "paused_the_print": {
          "description": "Whether the detector paused the print itself before alerting.",
          "type": "boolean"
        },
        "provider_printer_id": {
          "description": "The provider's own identifier for the printer the detection is about.",
          "format": "int64",
          "type": "integer"
        },
        "warning": {
          "description": "Whether the detector called it a warning rather than a failure.",
          "type": "boolean"
        }
      },
      "required": [
        "warning",
        "paused_the_print",
        "provider_printer_id"
      ],
      "type": "object"
    },
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
      "type": "string"
    },
    "PrinterSignal": {
      "description": "Something an alert says the printer itself signalled, beside what the\ndetector saw.\n\nNamed for what the printer did rather than for the provider that relayed\nit, like [`Detection`], so the supervision domain acts on it without knowing\nwhose alert carried it.",
      "oneOf": [
        {
          "const": "filament_change",
          "description": "The printer asked for its filament to be changed.",
          "type": "string"
        }
      ]
    },
    "ProviderPrint": {
      "additionalProperties": false,
      "description": "The provider's own identifier for the print an alert is about.\n\nHeld in the provider's representation \u2014 a 64-bit integer, which is what\nevery provider this system has met uses \u2014 and named for what it is rather\nthan for the provider, so that the supervision domain correlates on it\nwithout knowing whose it is.",
      "properties": {
        "file_name": {
          "description": "The file the provider named, when it named one.",
          "type": [
            "string",
            "null"
          ]
        },
        "id": {
          "description": "The provider's own identifier for the print.",
          "format": "int64",
          "type": "integer"
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
          "description": "When the provider says the print started, when it said.\n\nWhat tells this print's job from a later job of the same file that the\nsupervisor found before any alert about it arrived."
        }
      },
      "required": [
        "id"
      ],
      "type": "object"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "One external body, read into an event under the adapter's own kind.\n\n`kind` and `payload` are the [`EventBody`] flattened into this shape, so an\nalert's wire form is the pair the store holds. `print` is what the\nsupervision domain correlates on; an alert about no print carries none.",
  "properties": {
    "detection": {
      "anyOf": [
        {
          "$ref": "#/$defs/Detection"
        },
        {
          "type": "null"
        }
      ],
      "description": "What the provider's detector did, when this alert is one of its\ndetections."
    },
    "image_url": {
      "description": "The image this alert names, when it names one.",
      "type": [
        "string",
        "null"
      ]
    },
    "kind": {
      "$ref": "#/$defs/EventKind",
      "description": "Which event this is."
    },
    "payload": {
      "description": "What it carries, in the form its kind declares."
    },
    "print": {
      "anyOf": [
        {
          "$ref": "#/$defs/ProviderPrint"
        },
        {
          "type": "null"
        }
      ],
      "description": "The provider's own identifier for the print this alert is about, and\nthe file it named, when the alert is about a print at all."
    },
    "raw": {
      "$ref": "#/$defs/RawBytes",
      "description": "The bytes exactly as received."
    },
    "received_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was received."
    },
    "signal": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrinterSignal"
        },
        {
          "type": "null"
        }
      ],
      "description": "What the printer itself signalled, when the alert relays a signal."
    },
    "source": {
      "$ref": "#/$defs/EventSource",
      "description": "Where it came from."
    }
  },
  "required": [
    "source",
    "received_at",
    "kind",
    "payload",
    "raw"
  ],
  "title": "NormalizedAlert",
  "type": "object"
}
```

### ObicoEventType

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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
  "type": "object",
  "x-event-kind": "obico_failure_alert"
}
```

### ObicoFailureEvent

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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

Declared by `printobserver-obico`.

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
  "type": "object",
  "x-event-kind": "obico_printer_notification"
}
```

### ObicoTimestamp

Declared by `printobserver-obico`.

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

Declared by `printobserver-core`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
  "type": "object",
  "x-event-kind": "operator_acknowledgement"
}
```

### PendingCharge

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ChargeMethod": {
      "description": "How the length a charge debits was worked out.",
      "oneOf": [
        {
          "const": "slicer_estimate",
          "description": "A completed print: the length the file's analysis gives.",
          "type": "string"
        },
        {
          "const": "gcode_extrusion",
          "description": "The extrusion the file's own moves add up to, up to a position in it.",
          "type": "string"
        },
        {
          "const": "file_fraction",
          "description": "The analysed length scaled by how far into the file the print got; the\nfallback used only when the file itself could not be read.",
          "type": "string"
        },
        {
          "const": "unobserved_estimate",
          "description": "A completed print this system never saw: the file's analysed length.",
          "type": "string"
        }
      ]
    },
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
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
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "Filament a tool fed after a filament change nobody has confirmed, held until\nsomebody says which spool fed it.\n\nA length rather than grams, because the grams follow the spool that takes\nit.",
  "properties": {
    "analysed_length_mm": {
      "description": "The file's analysed length for the tool, in millimetres, when it had one.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "charge_key": {
      "$ref": "#/$defs/ChargeKey",
      "description": "The key of the print segment the filament was fed in."
    },
    "length_mm": {
      "description": "How much was fed, in millimetres.",
      "format": "double",
      "type": "number"
    },
    "method": {
      "$ref": "#/$defs/ChargeMethod",
      "description": "How that length was worked out."
    },
    "note": {
      "description": "Anything a reader needs to make sense of the figure.",
      "type": [
        "string",
        "null"
      ]
    },
    "pended_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When it was held."
    },
    "position": {
      "anyOf": [
        {
          "$ref": "#/$defs/FilePosition"
        },
        {
          "type": "null"
        }
      ],
      "description": "How far into the file the segment ran, when that was read."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print it was fed in."
    },
    "tool": {
      "description": "The tool that fed it.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "charge_key",
    "print_id",
    "tool",
    "length_mm",
    "method",
    "pended_at"
  ],
  "title": "PendingCharge",
  "type": "object"
}
```

### PendingTaken

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
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
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One pending charge a spool command took.",
  "properties": {
    "charge_key": {
      "$ref": "#/$defs/ChargeKey",
      "description": "The key it is charged under."
    },
    "print_id": {
      "$ref": "#/$defs/PrintId",
      "description": "The print it was fed in."
    }
  },
  "required": [
    "charge_key",
    "print_id"
  ],
  "title": "PendingTaken",
  "type": "object"
}
```

### PeriodicObservationPayload

Declared by `printobserver-core`.

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
        "filament": {
          "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
          "items": {
            "$ref": "#/$defs/ToolFilament"
          },
          "type": "array"
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
        "file_position_bytes": {
          "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
          "format": "int64",
          "type": [
            "integer",
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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The supervisor looked in on an active print on its own schedule, with no\ndetector having said anything about it.\n\nWhat a failure detector never alerts on \u2014 a progress that stopped moving, a\nheater whose target fell to nothing mid-print, a fan cut at a bridge, a wall\ngoing thin \u2014 is still on the machine and in the camera's frame, so every\nactive print is given one of these each interval and a turn on it. It\ncarries what the printer reported as it was taken; the frame the camera gave\nat the same moment, untagged by any detector, is this event's image.",
  "properties": {
    "interval_s": {
      "description": "How often these are taken, in whole seconds.",
      "format": "uint64",
      "minimum": 0,
      "type": "integer"
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
      "description": "The job the printer reported as the observation was taken, absent when\nit could not be read."
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
      "description": "The printer's state as the observation was taken, absent when it could\nnot be read \u2014 which the history records as a port failure."
    }
  },
  "required": [
    "interval_s"
  ],
  "title": "PeriodicObservationPayload",
  "type": "object",
  "x-event-kind": "periodic_observation"
}
```

### PolicyDecision

Declared by `printobserver-core`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
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

Declared by `printobserver-core`.

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
      "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::records::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::records::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
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
        },
        {
          "const": "camera_look",
          "description": "Taking a fresh frame from the camera, for a look at the print or for a\nperiodic observation of it.",
          "type": "string"
        },
        {
          "const": "detector_acknowledgement",
          "description": "Telling the detector that paused the print its detection was handled.",
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
  "type": "object",
  "x-event-kind": "port_failure"
}
```

### PortFailureSite

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Where a port failed while an event was being handled.\n\nA closed set of exactly the sites at which a failure has nowhere else to be\nrecorded. The printer's action methods record theirs on the\n[`ActionRecord`](crate::records::ActionRecord) the request minted, and a restoring\ncall records its own on the [`Intervention`](crate::records::Intervention) it was\nexpiring; those are not sites here, because a second record of them would be\na second version of one fact.",
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
    },
    {
      "const": "camera_look",
      "description": "Taking a fresh frame from the camera, for a look at the print or for a\nperiodic observation of it.",
      "type": "string"
    },
    {
      "const": "detector_acknowledgement",
      "description": "Telling the detector that paused the print its detection was handled.",
      "type": "string"
    }
  ],
  "title": "PortFailureSite"
}
```

### Preflight

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "PreflightTool": {
      "additionalProperties": false,
      "description": "What a filament check found for one tool.",
      "properties": {
        "buffer_g": {
          "description": "The margin asked for beyond that, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "marks": {
          "description": "The loaded spool's marks at the check, oldest first, whatever the\nverdict, so a spool both short and unverified says both.",
          "items": {
            "$ref": "#/$defs/UnverifiedMark"
          },
          "type": "array"
        },
        "remaining_g": {
          "description": "What the spool holds, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "required_g": {
          "description": "What the print needs from it, in grams, when the file's analysis says.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "spool_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/SpoolId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The spool loaded on it, when one is."
        },
        "spool_name": {
          "description": "That spool's name, beside its identifier, so a reader need not look it\nup.",
          "type": [
            "string",
            "null"
          ]
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "What the check found for this tool."
        }
      },
      "required": [
        "tool",
        "verdict"
      ],
      "type": "object"
    },
    "PreflightVerdict": {
      "description": "What a filament check found, for one tool or for a whole print.",
      "oneOf": [
        {
          "const": "passed",
          "description": "The spool holds what the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unchecked",
          "description": "No tracked spool is loaded on the tool, so nothing was checked.",
          "type": "string"
        },
        {
          "const": "insufficient",
          "description": "The spool holds less than the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unverified_spool",
          "description": "The spool's figure is in doubt.",
          "type": "string"
        },
        {
          "const": "no_analysis",
          "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
          "type": "string"
        },
        {
          "const": "overridden",
          "description": "The start went ahead despite the check, because the actor said so.",
          "type": "string"
        },
        {
          "const": "warned",
          "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What a filament check found for one print.",
  "properties": {
    "checked_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the check was made."
    },
    "file_name": {
      "description": "The file the print is of.",
      "type": "string"
    },
    "tools": {
      "description": "What it found for each tool.",
      "items": {
        "$ref": "#/$defs/PreflightTool"
      },
      "type": "array"
    },
    "verdict": {
      "$ref": "#/$defs/PreflightVerdict",
      "description": "The worst of the tools' verdicts, in the order insufficient,\nunverified spool, no analysis, warned, overridden, unchecked, passed."
    }
  },
  "required": [
    "file_name",
    "tools",
    "verdict",
    "checked_at"
  ],
  "title": "Preflight",
  "type": "object"
}
```

### PreflightAnswer

Declared by `printobserver-server`.

```json
{
  "$defs": {
    "Preflight": {
      "additionalProperties": false,
      "description": "What a filament check found for one print.",
      "properties": {
        "checked_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the check was made."
        },
        "file_name": {
          "description": "The file the print is of.",
          "type": "string"
        },
        "tools": {
          "description": "What it found for each tool.",
          "items": {
            "$ref": "#/$defs/PreflightTool"
          },
          "type": "array"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "The worst of the tools' verdicts, in the order insufficient,\nunverified spool, no analysis, warned, overridden, unchecked, passed."
        }
      },
      "required": [
        "file_name",
        "tools",
        "verdict",
        "checked_at"
      ],
      "type": "object"
    },
    "PreflightTool": {
      "additionalProperties": false,
      "description": "What a filament check found for one tool.",
      "properties": {
        "buffer_g": {
          "description": "The margin asked for beyond that, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "marks": {
          "description": "The loaded spool's marks at the check, oldest first, whatever the\nverdict, so a spool both short and unverified says both.",
          "items": {
            "$ref": "#/$defs/UnverifiedMark"
          },
          "type": "array"
        },
        "remaining_g": {
          "description": "What the spool holds, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "required_g": {
          "description": "What the print needs from it, in grams, when the file's analysis says.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "spool_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/SpoolId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The spool loaded on it, when one is."
        },
        "spool_name": {
          "description": "That spool's name, beside its identifier, so a reader need not look it\nup.",
          "type": [
            "string",
            "null"
          ]
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "What the check found for this tool."
        }
      },
      "required": [
        "tool",
        "verdict"
      ],
      "type": "object"
    },
    "PreflightVerdict": {
      "description": "What a filament check found, for one tool or for a whole print.",
      "oneOf": [
        {
          "const": "passed",
          "description": "The spool holds what the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unchecked",
          "description": "No tracked spool is loaded on the tool, so nothing was checked.",
          "type": "string"
        },
        {
          "const": "insufficient",
          "description": "The spool holds less than the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unverified_spool",
          "description": "The spool's figure is in doubt.",
          "type": "string"
        },
        {
          "const": "no_analysis",
          "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
          "type": "string"
        },
        {
          "const": "overridden",
          "description": "The start went ahead despite the check, because the actor said so.",
          "type": "string"
        },
        {
          "const": "warned",
          "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What a filament check answers.",
  "properties": {
    "preflight": {
      "$ref": "#/$defs/Preflight",
      "description": "What the check found, read now."
    }
  },
  "required": [
    "preflight"
  ],
  "title": "PreflightAnswer",
  "type": "object"
}
```

### PreflightCheckedPayload

Declared by `printobserver-core`.

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
    "Preflight": {
      "additionalProperties": false,
      "description": "What a filament check found for one print.",
      "properties": {
        "checked_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the check was made."
        },
        "file_name": {
          "description": "The file the print is of.",
          "type": "string"
        },
        "tools": {
          "description": "What it found for each tool.",
          "items": {
            "$ref": "#/$defs/PreflightTool"
          },
          "type": "array"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "The worst of the tools' verdicts, in the order insufficient,\nunverified spool, no analysis, warned, overridden, unchecked, passed."
        }
      },
      "required": [
        "file_name",
        "tools",
        "verdict",
        "checked_at"
      ],
      "type": "object"
    },
    "PreflightTool": {
      "additionalProperties": false,
      "description": "What a filament check found for one tool.",
      "properties": {
        "buffer_g": {
          "description": "The margin asked for beyond that, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "marks": {
          "description": "The loaded spool's marks at the check, oldest first, whatever the\nverdict, so a spool both short and unverified says both.",
          "items": {
            "$ref": "#/$defs/UnverifiedMark"
          },
          "type": "array"
        },
        "remaining_g": {
          "description": "What the spool holds, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "required_g": {
          "description": "What the print needs from it, in grams, when the file's analysis says.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "spool_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/SpoolId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The spool loaded on it, when one is."
        },
        "spool_name": {
          "description": "That spool's name, beside its identifier, so a reader need not look it\nup.",
          "type": [
            "string",
            "null"
          ]
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "What the check found for this tool."
        }
      },
      "required": [
        "tool",
        "verdict"
      ],
      "type": "object"
    },
    "PreflightVerdict": {
      "description": "What a filament check found, for one tool or for a whole print.",
      "oneOf": [
        {
          "const": "passed",
          "description": "The spool holds what the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unchecked",
          "description": "No tracked spool is loaded on the tool, so nothing was checked.",
          "type": "string"
        },
        {
          "const": "insufficient",
          "description": "The spool holds less than the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unverified_spool",
          "description": "The spool's figure is in doubt.",
          "type": "string"
        },
        {
          "const": "no_analysis",
          "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
          "type": "string"
        },
        {
          "const": "overridden",
          "description": "The start went ahead despite the check, because the actor said so.",
          "type": "string"
        },
        {
          "const": "warned",
          "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The filament check before a start was made.",
  "properties": {
    "action_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/ActionId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The start it was made for, when it was made for one."
    },
    "preflight": {
      "$ref": "#/$defs/Preflight",
      "description": "What the check found."
    }
  },
  "required": [
    "preflight"
  ],
  "title": "PreflightCheckedPayload",
  "type": "object",
  "x-event-kind": "preflight_checked"
}
```

### PreflightTool

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "PreflightVerdict": {
      "description": "What a filament check found, for one tool or for a whole print.",
      "oneOf": [
        {
          "const": "passed",
          "description": "The spool holds what the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unchecked",
          "description": "No tracked spool is loaded on the tool, so nothing was checked.",
          "type": "string"
        },
        {
          "const": "insufficient",
          "description": "The spool holds less than the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unverified_spool",
          "description": "The spool's figure is in doubt.",
          "type": "string"
        },
        {
          "const": "no_analysis",
          "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
          "type": "string"
        },
        {
          "const": "overridden",
          "description": "The start went ahead despite the check, because the actor said so.",
          "type": "string"
        },
        {
          "const": "warned",
          "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "What a filament check found for one tool.",
  "properties": {
    "buffer_g": {
      "description": "The margin asked for beyond that, in grams.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "marks": {
      "description": "The loaded spool's marks at the check, oldest first, whatever the\nverdict, so a spool both short and unverified says both.",
      "items": {
        "$ref": "#/$defs/UnverifiedMark"
      },
      "type": "array"
    },
    "remaining_g": {
      "description": "What the spool holds, in grams.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "required_g": {
      "description": "What the print needs from it, in grams, when the file's analysis says.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "spool_id": {
      "anyOf": [
        {
          "$ref": "#/$defs/SpoolId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The spool loaded on it, when one is."
    },
    "spool_name": {
      "description": "That spool's name, beside its identifier, so a reader need not look it\nup.",
      "type": [
        "string",
        "null"
      ]
    },
    "tool": {
      "description": "The tool, in the printer's own numbering.",
      "format": "int64",
      "type": "integer"
    },
    "verdict": {
      "$ref": "#/$defs/PreflightVerdict",
      "description": "What the check found for this tool."
    }
  },
  "required": [
    "tool",
    "verdict"
  ],
  "title": "PreflightTool",
  "type": "object"
}
```

### PreflightVerdict

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What a filament check found, for one tool or for a whole print.",
  "oneOf": [
    {
      "const": "passed",
      "description": "The spool holds what the print needs and its buffer.",
      "type": "string"
    },
    {
      "const": "unchecked",
      "description": "No tracked spool is loaded on the tool, so nothing was checked.",
      "type": "string"
    },
    {
      "const": "insufficient",
      "description": "The spool holds less than the print needs and its buffer.",
      "type": "string"
    },
    {
      "const": "unverified_spool",
      "description": "The spool's figure is in doubt.",
      "type": "string"
    },
    {
      "const": "no_analysis",
      "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
      "type": "string"
    },
    {
      "const": "overridden",
      "description": "The start went ahead despite the check, because the actor said so.",
      "type": "string"
    },
    {
      "const": "warned",
      "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
      "type": "string"
    }
  ],
  "title": "PreflightVerdict"
}
```

### PrintAction

Declared by `printobserver-core`.

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
          "description": "The print should stop. Recorded for whoever may cancel it: the\nacknowledgement itself asks nothing of the machine.",
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
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
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
        "override_filament_check": {
          "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
          "type": "boolean"
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
    },
    {
      "additionalProperties": false,
      "description": "Close a print record printobserver left open, with the outcome the\noperator judges it ended with. It asks nothing of the machine and\ncharges no spool: the record's consumption was never seen to end.",
      "properties": {
        "action": {
          "const": "close_print",
          "type": "string"
        },
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "outcome": {
          "$ref": "#/$defs/PrintOutcome",
          "description": "How the print ended, in the operator's judgement."
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        }
      },
      "required": [
        "action",
        "outcome",
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

Declared by `printobserver-core`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
        "filament": {
          "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
          "items": {
            "$ref": "#/$defs/ToolFilament"
          },
          "type": "array"
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
        "file_position_bytes": {
          "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
          "format": "int64",
          "type": [
            "integer",
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
    "PrintRecord": {
      "additionalProperties": false,
      "description": "One print, and the record supervision keys from.\n\nThe provider's own print id is carried beside this record's identifier\nrather than as it, because a print may be observed before the provider\nthat reports it has one.",
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
        "job_print_time_s": {
          "description": "The longest running time, in whole seconds, the printer reported for\nthis print's job at a read that found it printing rather than paused.\n\nThe time a job has spent printing only grows, pause or no pause, so a\njob of this print's file reporting less \u2014 by more than\n`listing::JOB_IDENTITY_TOLERANCE_S` \u2014 is a later job. Absent until a\nread finds the job printing.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "job_started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the printer's job this print records began, as the printer's own\nrunning time put it at the latest read that found the job running or\npaused: the instant of that read, less the running time it reported.\n\n`OctoPrint` counts a pause into the running time until the job resumes\nand then takes the pause back out, so a pause moves this later: a read\nputting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one\nrecorded resets it, rather than it being fixed once. Absent when no read has\nfound this print's job reporting a running time \u2014 a print opened by an\nalert or a start before any read, and every print recorded before this\nwas."
        },
        "narrowings": {
          "description": "Every manifest range this print narrowed to the envelope's.",
          "items": {
            "$ref": "#/$defs/ManifestNarrowing"
          },
          "type": "array"
        },
        "opened_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the print was opened."
        },
        "provider_print_id": {
          "description": "The provider's own identifier for the print, whichever provider\nreported it, when one has.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
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

### PrintEndedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
      ],
      "type": "object"
    },
    "PrintOutcome": {
      "description": "How a print ended.",
      "oneOf": [
        {
          "const": "completed",
          "description": "It ran to its end.",
          "type": "string"
        },
        {
          "const": "cancelled",
          "description": "Somebody cancelled it.",
          "type": "string"
        },
        {
          "const": "failed",
          "description": "It stopped on an error.",
          "type": "string"
        },
        {
          "const": "interrupted",
          "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
          "type": "string"
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
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A print ended.",
  "properties": {
    "last_position": {
      "anyOf": [
        {
          "$ref": "#/$defs/FilePosition"
        },
        {
          "type": "null"
        }
      ],
      "description": "How far into its file it last read, when that was read."
    },
    "outcome": {
      "$ref": "#/$defs/PrintOutcome",
      "description": "How it ended."
    },
    "reason": {
      "description": "Why it ended, in the words the end was recorded with.",
      "type": "string"
    },
    "state": {
      "$ref": "#/$defs/PrinterState",
      "description": "The state the printer was in as it ended."
    }
  },
  "required": [
    "outcome",
    "state",
    "reason"
  ],
  "title": "PrintEndedPayload",
  "type": "object",
  "x-event-kind": "print_ended"
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

### PrintOutcome

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "How a print ended.",
  "oneOf": [
    {
      "const": "completed",
      "description": "It ran to its end.",
      "type": "string"
    },
    {
      "const": "cancelled",
      "description": "Somebody cancelled it.",
      "type": "string"
    },
    {
      "const": "failed",
      "description": "It stopped on an error.",
      "type": "string"
    },
    {
      "const": "interrupted",
      "description": "It stopped with no cancel, no error the printer reported and no\nfinish. Charged as a failed print is: what the file's moves extruded up\nto the last file position seen.",
      "type": "string"
    }
  ],
  "title": "PrintOutcome"
}
```

### PrintRecord

Declared by `printobserver-core`.

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
  "description": "One print, and the record supervision keys from.\n\nThe provider's own print id is carried beside this record's identifier\nrather than as it, because a print may be observed before the provider\nthat reports it has one.",
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
    "job_print_time_s": {
      "description": "The longest running time, in whole seconds, the printer reported for\nthis print's job at a read that found it printing rather than paused.\n\nThe time a job has spent printing only grows, pause or no pause, so a\njob of this print's file reporting less \u2014 by more than\n`listing::JOB_IDENTITY_TOLERANCE_S` \u2014 is a later job. Absent until a\nread finds the job printing.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "job_started_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the printer's job this print records began, as the printer's own\nrunning time put it at the latest read that found the job running or\npaused: the instant of that read, less the running time it reported.\n\n`OctoPrint` counts a pause into the running time until the job resumes\nand then takes the pause back out, so a pause moves this later: a read\nputting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one\nrecorded resets it, rather than it being fixed once. Absent when no read has\nfound this print's job reporting a running time \u2014 a print opened by an\nalert or a start before any read, and every print recorded before this\nwas."
    },
    "narrowings": {
      "description": "Every manifest range this print narrowed to the envelope's.",
      "items": {
        "$ref": "#/$defs/ManifestNarrowing"
      },
      "type": "array"
    },
    "opened_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the print was opened."
    },
    "provider_print_id": {
      "description": "The provider's own identifier for the print, whichever provider\nreported it, when one has.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
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

### PrintStartedPayload

Declared by `printobserver-core`.

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
    "LoadedSpool": {
      "additionalProperties": false,
      "description": "One spool loaded on one tool, as a print started.",
      "properties": {
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool loaded on it."
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "tool",
        "spool_id"
      ],
      "type": "object"
    },
    "Preflight": {
      "additionalProperties": false,
      "description": "What a filament check found for one print.",
      "properties": {
        "checked_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the check was made."
        },
        "file_name": {
          "description": "The file the print is of.",
          "type": "string"
        },
        "tools": {
          "description": "What it found for each tool.",
          "items": {
            "$ref": "#/$defs/PreflightTool"
          },
          "type": "array"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "The worst of the tools' verdicts, in the order insufficient,\nunverified spool, no analysis, warned, overridden, unchecked, passed."
        }
      },
      "required": [
        "file_name",
        "tools",
        "verdict",
        "checked_at"
      ],
      "type": "object"
    },
    "PreflightTool": {
      "additionalProperties": false,
      "description": "What a filament check found for one tool.",
      "properties": {
        "buffer_g": {
          "description": "The margin asked for beyond that, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "marks": {
          "description": "The loaded spool's marks at the check, oldest first, whatever the\nverdict, so a spool both short and unverified says both.",
          "items": {
            "$ref": "#/$defs/UnverifiedMark"
          },
          "type": "array"
        },
        "remaining_g": {
          "description": "What the spool holds, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "required_g": {
          "description": "What the print needs from it, in grams, when the file's analysis says.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "spool_id": {
          "anyOf": [
            {
              "$ref": "#/$defs/SpoolId"
            },
            {
              "type": "null"
            }
          ],
          "description": "The spool loaded on it, when one is."
        },
        "spool_name": {
          "description": "That spool's name, beside its identifier, so a reader need not look it\nup.",
          "type": [
            "string",
            "null"
          ]
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "verdict": {
          "$ref": "#/$defs/PreflightVerdict",
          "description": "What the check found for this tool."
        }
      },
      "required": [
        "tool",
        "verdict"
      ],
      "type": "object"
    },
    "PreflightVerdict": {
      "description": "What a filament check found, for one tool or for a whole print.",
      "oneOf": [
        {
          "const": "passed",
          "description": "The spool holds what the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unchecked",
          "description": "No tracked spool is loaded on the tool, so nothing was checked.",
          "type": "string"
        },
        {
          "const": "insufficient",
          "description": "The spool holds less than the print needs and its buffer.",
          "type": "string"
        },
        {
          "const": "unverified_spool",
          "description": "The spool's figure is in doubt.",
          "type": "string"
        },
        {
          "const": "no_analysis",
          "description": "`OctoPrint`'s analysis of the file had still not appeared when the\nstart's wait for it elapsed, or has not appeared yet when the check is\nonly read.",
          "type": "string"
        },
        {
          "const": "overridden",
          "description": "The start went ahead despite the check, because the actor said so.",
          "type": "string"
        },
        {
          "const": "warned",
          "description": "The check is set to warn, and let an insufficient, unverified or\nunanalysed start go ahead.",
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A print started.",
  "properties": {
    "file_name": {
      "description": "The file being printed, when it is known.",
      "type": [
        "string",
        "null"
      ]
    },
    "loaded": {
      "description": "The spool loaded on each tool as it started.",
      "items": {
        "$ref": "#/$defs/LoadedSpool"
      },
      "type": "array"
    },
    "preflight": {
      "anyOf": [
        {
          "$ref": "#/$defs/Preflight"
        },
        {
          "type": "null"
        }
      ],
      "description": "The filament check made before it started, when one was."
    },
    "started_by": {
      "anyOf": [
        {
          "$ref": "#/$defs/ActionId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The action that started it; absent when the print was observed rather\nthan started through this system."
    }
  },
  "required": [
    "loaded"
  ],
  "title": "PrintStartedPayload",
  "type": "object",
  "x-event-kind": "print_started"
}
```

### PrintUntrackedPayload

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A print that was already open when spool tracking was installed: it is\nnever charged, and this says why it carries no charge.",
  "properties": {
    "reason": {
      "description": "Why the print carries no charge.",
      "type": "string"
    }
  },
  "required": [
    "reason"
  ],
  "title": "PrintUntrackedPayload",
  "type": "object",
  "x-event-kind": "print_untracked"
}
```

### PrinterSignal

Declared by `printobserver-vision-api`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "Something an alert says the printer itself signalled, beside what the\ndetector saw.\n\nNamed for what the printer did rather than for the provider that relayed\nit, like [`Detection`], so the supervision domain acts on it without knowing\nwhose alert carried it.",
  "oneOf": [
    {
      "const": "filament_change",
      "description": "The printer asked for its filament to be changed.",
      "type": "string"
    }
  ],
  "title": "PrinterSignal"
}
```

### PrinterSnapshot

Declared by `printobserver-printer-api`.

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

### PrintsAnswer

Declared by `printobserver-server`.

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
    "PrintRecord": {
      "additionalProperties": false,
      "description": "One print, and the record supervision keys from.\n\nThe provider's own print id is carried beside this record's identifier\nrather than as it, because a print may be observed before the provider\nthat reports it has one.",
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
        "job_print_time_s": {
          "description": "The longest running time, in whole seconds, the printer reported for\nthis print's job at a read that found it printing rather than paused.\n\nThe time a job has spent printing only grows, pause or no pause, so a\njob of this print's file reporting less \u2014 by more than\n`listing::JOB_IDENTITY_TOLERANCE_S` \u2014 is a later job. Absent until a\nread finds the job printing.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "job_started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the printer's job this print records began, as the printer's own\nrunning time put it at the latest read that found the job running or\npaused: the instant of that read, less the running time it reported.\n\n`OctoPrint` counts a pause into the running time until the job resumes\nand then takes the pause back out, so a pause moves this later: a read\nputting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one\nrecorded resets it, rather than it being fixed once. Absent when no read has\nfound this print's job reporting a running time \u2014 a print opened by an\nalert or a start before any read, and every print recorded before this\nwas."
        },
        "narrowings": {
          "description": "Every manifest range this print narrowed to the envelope's.",
          "items": {
            "$ref": "#/$defs/ManifestNarrowing"
          },
          "type": "array"
        },
        "opened_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the print was opened."
        },
        "provider_print_id": {
          "description": "The provider's own identifier for the print, whichever provider\nreported it, when one has.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
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
  "description": "What the prints read answers: every print, and the one the printer is\nrunning.\n\nAn open print's recorded `state` is the state it was opened in, which is\n`printing` whether or not the machine is paused now; what the machine is\ndoing is the status read's answer.",
  "properties": {
    "active": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrintId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The print the printer's current job belongs to, present exactly when the\nprinter reports a job it is printing or has paused. Absent when the\nprinter could not be read."
    },
    "prints": {
      "description": "Every print this server holds, ended or not, most recently opened first.",
      "items": {
        "$ref": "#/$defs/PrintRecord"
      },
      "type": "array"
    }
  },
  "required": [
    "prints"
  ],
  "title": "PrintsAnswer",
  "type": "object"
}
```

### ProviderPrint

Declared by `printobserver-vision-api`.

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
  "description": "The provider's own identifier for the print an alert is about.\n\nHeld in the provider's representation \u2014 a 64-bit integer, which is what\nevery provider this system has met uses \u2014 and named for what it is rather\nthan for the provider, so that the supervision domain correlates on it\nwithout knowing whose it is.",
  "properties": {
    "file_name": {
      "description": "The file the provider named, when it named one.",
      "type": [
        "string",
        "null"
      ]
    },
    "id": {
      "description": "The provider's own identifier for the print.",
      "format": "int64",
      "type": "integer"
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
      "description": "When the provider says the print started, when it said.\n\nWhat tells this print's job from a later job of the same file that the\nsupervisor found before any alert about it arrived."
    }
  },
  "required": [
    "id"
  ],
  "title": "ProviderPrint",
  "type": "object"
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

### RebaselineDisposition

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What became of the prints a file's new tally generation counts.",
  "oneOf": [
    {
      "const": "accounted",
      "description": "They were accounted as prints this system never saw.",
      "type": "string"
    },
    {
      "const": "unverified",
      "description": "It could not be told which of them this system already settled, so the\nloaded spool was marked unverified rather than charged on a guess.",
      "type": "string"
    }
  ],
  "title": "RebaselineDisposition"
}
```

### RejectionReason

Declared by `printobserver-core`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
      "type": "string"
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
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
    },
    {
      "additionalProperties": false,
      "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
      "properties": {
        "insufficient_filament": {
          "additionalProperties": false,
          "properties": {
            "buffer_g": {
              "description": "The margin asked for beyond that, in grams.",
              "format": "double",
              "type": "number"
            },
            "remaining_g": {
              "description": "What the spool holds, in grams.",
              "format": "double",
              "type": "number"
            },
            "required_g": {
              "description": "What the print needs from it, in grams.",
              "format": "double",
              "type": "number"
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool loaded on it."
            },
            "spool_name": {
              "description": "That spool's name.",
              "type": "string"
            },
            "tool": {
              "description": "The tool whose spool is short.",
              "format": "int64",
              "type": "integer"
            },
            "unverified": {
              "anyOf": [
                {
                  "$ref": "#/$defs/UnverifiedReason"
                },
                {
                  "type": "null"
                }
              ],
              "description": "The spool's oldest mark, when the short spool is also unverified."
            }
          },
          "required": [
            "tool",
            "spool_id",
            "spool_name",
            "required_g",
            "buffer_g",
            "remaining_g"
          ],
          "type": "object"
        }
      },
      "required": [
        "insufficient_filament"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A start was refused because the spool on one tool is unverified.",
      "properties": {
        "unverified_spool": {
          "additionalProperties": false,
          "properties": {
            "reason": {
              "$ref": "#/$defs/UnverifiedReason",
              "description": "The spool's oldest mark."
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool loaded on it."
            },
            "spool_name": {
              "description": "That spool's name.",
              "type": "string"
            },
            "tool": {
              "description": "The tool whose spool is unverified.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "tool",
            "spool_id",
            "spool_name",
            "reason"
          ],
          "type": "object"
        }
      },
      "required": [
        "unverified_spool"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
      "properties": {
        "analysis_unavailable": {
          "additionalProperties": false,
          "properties": {
            "file_name": {
              "description": "The file that has no analysis.",
              "type": "string"
            },
            "waited_s": {
              "description": "How long the start waited for one, in whole seconds.",
              "format": "uint64",
              "minimum": 0,
              "type": "integer"
            }
          },
          "required": [
            "file_name",
            "waited_s"
          ],
          "type": "object"
        }
      },
      "required": [
        "analysis_unavailable"
      ],
      "type": "object"
    },
    {
      "const": "print_already_ended",
      "description": "A close was refused because the print record has already ended.",
      "type": "string"
    },
    {
      "additionalProperties": false,
      "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
      "properties": {
        "print_still_active": {
          "additionalProperties": false,
          "properties": {
            "state": {
              "$ref": "#/$defs/PrinterState",
              "description": "The state the printer reported that job in."
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        }
      },
      "required": [
        "print_still_active"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
      "properties": {
        "intervention_still_active": {
          "additionalProperties": false,
          "properties": {
            "intervention_id": {
              "$ref": "#/$defs/InterventionId",
              "description": "The intervention still standing."
            }
          },
          "required": [
            "intervention_id"
          ],
          "type": "object"
        }
      },
      "required": [
        "intervention_still_active"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A spool command could not be carried out.",
      "properties": {
        "spool_refused": {
          "additionalProperties": false,
          "properties": {
            "detail": {
              "description": "Why, in a sentence naming what to do instead.",
              "type": "string"
            }
          },
          "required": [
            "detail"
          ],
          "type": "object"
        }
      },
      "required": [
        "spool_refused"
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

Declared by `printobserver-core`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
  "description": "Server configuration: what any actor may ask for at all.\n\nThis is the operator's safety envelope. It is a different contract from the\nplausibility ranges [`Reported`](printobserver_types::Reported) fields carry, is narrower\nby orders of magnitude, and the two are never intersected, compared or\nsubstituted for one another.",
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
      "description": "The minimum interval between two agent actions that change the machine,\nin whole seconds. An acknowledgement neither waits on it nor restarts it.",
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

Declared by `printobserver-supervisor-api`.

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

### SpoolAdjustedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ClearedMark": {
      "additionalProperties": false,
      "description": "One mark a spool command cleared, and the spool it was cleared from.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "The reason that no longer stands."
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool the mark was cleared from."
        }
      },
      "required": [
        "spool_id",
        "reason"
      ],
      "type": "object"
    },
    "PendingTaken": {
      "additionalProperties": false,
      "description": "One pending charge a spool command took.",
      "properties": {
        "charge_key": {
          "$ref": "#/$defs/ChargeKey",
          "description": "The key it is charged under."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it was fed in."
        }
      },
      "required": [
        "charge_key",
        "print_id"
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool's remaining figure was corrected.",
  "properties": {
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who corrected it."
    },
    "cleared_marks": {
      "description": "Every mark the correction cleared.",
      "items": {
        "$ref": "#/$defs/ClearedMark"
      },
      "type": "array"
    },
    "gross_g": {
      "description": "The scale reading the figure was worked out from, when it was given as\none.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "reason": {
      "description": "Why.",
      "type": "string"
    },
    "remaining_after_g": {
      "description": "Its figure after, in grams.",
      "format": "double",
      "type": "number"
    },
    "remaining_before_g": {
      "description": "Its figure before, in grams, after any pending charge it took.",
      "format": "double",
      "type": "number"
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool corrected."
    },
    "took_pending": {
      "description": "The pending charges the correction took first, each also its own\n`filament_consumed` event under its key.",
      "items": {
        "$ref": "#/$defs/PendingTaken"
      },
      "type": "array"
    }
  },
  "required": [
    "spool_id",
    "remaining_before_g",
    "remaining_after_g",
    "actor",
    "reason"
  ],
  "title": "SpoolAdjustedPayload",
  "type": "object",
  "x-event-kind": "spool_adjusted"
}
```

### SpoolAnswer

Declared by `printobserver-server`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "SpoolRecord": {
      "additionalProperties": false,
      "description": "One spool the operator registered, and what is known of it now.",
      "properties": {
        "color": {
          "description": "The filament's colour, when the operator gave one.",
          "type": [
            "string",
            "null"
          ]
        },
        "density_g_cm3": {
          "description": "The material's density, in grams per cubic centimetre; above zero.",
          "format": "double",
          "type": "number"
        },
        "diameter_mm": {
          "description": "The filament's diameter, in millimetres; above zero.",
          "format": "double",
          "type": "number"
        },
        "id": {
          "$ref": "#/$defs/SpoolId",
          "description": "This spool's identifier, minted when it was registered."
        },
        "initial_g": {
          "description": "The net filament on the spool when it was full, in grams; above zero.",
          "format": "double",
          "type": "number"
        },
        "loaded_on_tool": {
          "description": "The tool the spool is loaded on, when it is loaded.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "material": {
          "description": "The material, as given and trimmed, such as `PLA`; never empty.",
          "type": "string"
        },
        "name": {
          "description": "The operator's own label for the spool; never empty.",
          "type": "string"
        },
        "registered_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was registered."
        },
        "remaining_g": {
          "description": "The filament left, in grams.\n\nAlways the initial figure less every charge plus every correction. A\ncharge is never refused or clamped, so this may go below zero.",
          "format": "double",
          "type": "number"
        },
        "retired_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the spool was retired, if it has been."
        },
        "tare_g": {
          "description": "The empty spool's own weight, in grams, when the operator gave it.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "trust": {
          "$ref": "#/$defs/SpoolTrust",
          "description": "How far the remaining figure can be trusted."
        },
        "updated_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When anything about the spool last changed."
        }
      },
      "required": [
        "id",
        "name",
        "material",
        "diameter_mm",
        "density_g_cm3",
        "initial_g",
        "remaining_g",
        "trust",
        "registered_at",
        "updated_at"
      ],
      "type": "object"
    },
    "SpoolTrust": {
      "description": "How far a spool's remaining figure can be trusted.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Every charge against the spool was taken exactly.",
          "properties": {
            "state": {
              "const": "verified",
              "type": "string"
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "At least one thing happened that could not be charged exactly.",
          "properties": {
            "marks": {
              "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
              "items": {
                "$ref": "#/$defs/UnverifiedMark"
              },
              "minItems": 1,
              "type": "array"
            },
            "state": {
              "const": "unverified",
              "type": "string"
            }
          },
          "required": [
            "state",
            "marks"
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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What one spool's read answers: the spool, and the newest of the printer's\nown events that name it.",
  "properties": {
    "recent": {
      "description": "The newest timeline events naming it, newest first.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    },
    "spool": {
      "$ref": "#/$defs/SpoolRecord",
      "description": "The spool, retired or not."
    }
  },
  "required": [
    "spool",
    "recent"
  ],
  "title": "SpoolAnswer",
  "type": "object"
}
```

### SpoolCommand

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "The whole vocabulary of the spool ledger, and there is no other.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "Register a spool.",
      "properties": {
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "color": {
          "description": "The filament's colour.",
          "type": [
            "string",
            "null"
          ]
        },
        "command": {
          "const": "register_spool",
          "type": "string"
        },
        "density_g_cm3": {
          "description": "The material's density, in grams per cubic centimetre; the known\ndensity of the material when absent, and a material with none known\nis refused.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "diameter_mm": {
          "description": "The filament's diameter, in millimetres; 1.75 when absent.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "initial_g": {
          "description": "The net filament on the spool when full, in grams.",
          "format": "double",
          "type": "number"
        },
        "material": {
          "description": "The material, such as `PLA`.",
          "type": "string"
        },
        "name": {
          "description": "The operator's own label for it.",
          "type": "string"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "remaining_g": {
          "description": "The filament on it now, in grams; the full figure when absent.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "tare_g": {
          "description": "The empty spool's own weight, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "command",
        "name",
        "material",
        "initial_g",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Load a spool onto a tool.\n\nOnto an occupied tool it is a swap, and the spool there is unloaded in\nthe same write. Naming the spool already on the tool confirms it was\nnever swapped, which is accepted only while that tool holds a pending\ncharge or that spool holds a filament-change mark.",
      "properties": {
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "command": {
          "const": "load_spool",
          "type": "string"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool to load."
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "command",
        "spool_id",
        "tool",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Unload a spool from its tool.",
      "properties": {
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "command": {
          "const": "unload_spool",
          "type": "string"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool to unload."
        }
      },
      "required": [
        "command",
        "spool_id",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Correct a spool's remaining figure, from exactly one of the two; a\ncommand naming both or neither is refused as `spool_refused`.",
      "properties": {
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "command": {
          "const": "adjust_spool",
          "type": "string"
        },
        "gross_g": {
          "description": "A scale reading of the spool, in grams, which needs its tare.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "remaining_g": {
          "description": "The filament left, in grams.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool to correct."
        }
      },
      "required": [
        "command",
        "spool_id",
        "reason",
        "actor"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "Retire a spool, after which it refuses every command.",
      "properties": {
        "actor": {
          "$ref": "#/$defs/Actor",
          "description": "Who is asking."
        },
        "command": {
          "const": "retire_spool",
          "type": "string"
        },
        "reason": {
          "description": "Why the actor is asking for this.",
          "type": "string"
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool to retire."
        }
      },
      "required": [
        "command",
        "spool_id",
        "reason",
        "actor"
      ],
      "type": "object"
    }
  ],
  "title": "SpoolCommand"
}
```

### SpoolCommandAnswer

Declared by `printobserver-server`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "SpoolRecord": {
      "additionalProperties": false,
      "description": "One spool the operator registered, and what is known of it now.",
      "properties": {
        "color": {
          "description": "The filament's colour, when the operator gave one.",
          "type": [
            "string",
            "null"
          ]
        },
        "density_g_cm3": {
          "description": "The material's density, in grams per cubic centimetre; above zero.",
          "format": "double",
          "type": "number"
        },
        "diameter_mm": {
          "description": "The filament's diameter, in millimetres; above zero.",
          "format": "double",
          "type": "number"
        },
        "id": {
          "$ref": "#/$defs/SpoolId",
          "description": "This spool's identifier, minted when it was registered."
        },
        "initial_g": {
          "description": "The net filament on the spool when it was full, in grams; above zero.",
          "format": "double",
          "type": "number"
        },
        "loaded_on_tool": {
          "description": "The tool the spool is loaded on, when it is loaded.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "material": {
          "description": "The material, as given and trimmed, such as `PLA`; never empty.",
          "type": "string"
        },
        "name": {
          "description": "The operator's own label for the spool; never empty.",
          "type": "string"
        },
        "registered_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was registered."
        },
        "remaining_g": {
          "description": "The filament left, in grams.\n\nAlways the initial figure less every charge plus every correction. A\ncharge is never refused or clamped, so this may go below zero.",
          "format": "double",
          "type": "number"
        },
        "retired_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the spool was retired, if it has been."
        },
        "tare_g": {
          "description": "The empty spool's own weight, in grams, when the operator gave it.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "trust": {
          "$ref": "#/$defs/SpoolTrust",
          "description": "How far the remaining figure can be trusted."
        },
        "updated_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When anything about the spool last changed."
        }
      },
      "required": [
        "id",
        "name",
        "material",
        "diameter_mm",
        "density_g_cm3",
        "initial_g",
        "remaining_g",
        "trust",
        "registered_at",
        "updated_at"
      ],
      "type": "object"
    },
    "SpoolTrust": {
      "description": "How far a spool's remaining figure can be trusted.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Every charge against the spool was taken exactly.",
          "properties": {
            "state": {
              "const": "verified",
              "type": "string"
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "At least one thing happened that could not be charged exactly.",
          "properties": {
            "marks": {
              "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
              "items": {
                "$ref": "#/$defs/UnverifiedMark"
              },
              "minItems": 1,
              "type": "array"
            },
            "state": {
              "const": "unverified",
              "type": "string"
            }
          },
          "required": [
            "state",
            "marks"
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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What a spool command answers, accepted or refused.\n\nA refusal is the same shape and deliberately so: the decision carries the\ngrant or the ledger's own reason, and the events are the record the refusal\nwrote.",
  "properties": {
    "decision": {
      "$ref": "#/$defs/PolicyDecision",
      "description": "The decision taken on the command."
    },
    "events": {
      "description": "Every event it wrote, in the order they were written.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    },
    "spool": {
      "anyOf": [
        {
          "$ref": "#/$defs/SpoolRecord"
        },
        {
          "type": "null"
        }
      ],
      "description": "The spool it was about, as the command left it, when there is one."
    }
  },
  "required": [
    "decision",
    "events"
  ],
  "title": "SpoolCommandAnswer",
  "type": "object"
}
```

### SpoolCommandRejectedPayload

Declared by `printobserver-core`.

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
        },
        {
          "const": "close_print",
          "description": "Close a print record printobserver left open.",
          "type": "string"
        },
        {
          "const": "register_spool",
          "description": "Register a spool.",
          "type": "string"
        },
        {
          "const": "load_spool",
          "description": "Load a spool onto a tool.",
          "type": "string"
        },
        {
          "const": "unload_spool",
          "description": "Unload a spool from its tool.",
          "type": "string"
        },
        {
          "const": "adjust_spool",
          "description": "Correct a spool's remaining figure.",
          "type": "string"
        },
        {
          "const": "retire_spool",
          "description": "Retire a spool.",
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
    "InterventionId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one bounded intervention.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "InterventionId",
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
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool holds less than the\nprint needs and its buffer.",
          "properties": {
            "insufficient_filament": {
              "additionalProperties": false,
              "properties": {
                "buffer_g": {
                  "description": "The margin asked for beyond that, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "remaining_g": {
                  "description": "What the spool holds, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "required_g": {
                  "description": "What the print needs from it, in grams.",
                  "format": "double",
                  "type": "number"
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is short.",
                  "format": "int64",
                  "type": "integer"
                },
                "unverified": {
                  "anyOf": [
                    {
                      "$ref": "#/$defs/UnverifiedReason"
                    },
                    {
                      "type": "null"
                    }
                  ],
                  "description": "The spool's oldest mark, when the short spool is also unverified."
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "required_g",
                "buffer_g",
                "remaining_g"
              ],
              "type": "object"
            }
          },
          "required": [
            "insufficient_filament"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because the spool on one tool is unverified.",
          "properties": {
            "unverified_spool": {
              "additionalProperties": false,
              "properties": {
                "reason": {
                  "$ref": "#/$defs/UnverifiedReason",
                  "description": "The spool's oldest mark."
                },
                "spool_id": {
                  "$ref": "#/$defs/SpoolId",
                  "description": "The spool loaded on it."
                },
                "spool_name": {
                  "description": "That spool's name.",
                  "type": "string"
                },
                "tool": {
                  "description": "The tool whose spool is unverified.",
                  "format": "int64",
                  "type": "integer"
                }
              },
              "required": [
                "tool",
                "spool_id",
                "spool_name",
                "reason"
              ],
              "type": "object"
            }
          },
          "required": [
            "unverified_spool"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A start was refused because `OctoPrint`'s analysis of the file had not\nappeared when the start's wait for it elapsed.",
          "properties": {
            "analysis_unavailable": {
              "additionalProperties": false,
              "properties": {
                "file_name": {
                  "description": "The file that has no analysis.",
                  "type": "string"
                },
                "waited_s": {
                  "description": "How long the start waited for one, in whole seconds.",
                  "format": "uint64",
                  "minimum": 0,
                  "type": "integer"
                }
              },
              "required": [
                "file_name",
                "waited_s"
              ],
              "type": "object"
            }
          },
          "required": [
            "analysis_unavailable"
          ],
          "type": "object"
        },
        {
          "const": "print_already_ended",
          "description": "A close was refused because the print record has already ended.",
          "type": "string"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because the printer shows a job of the record's\nfile printing, paused or cancelling.",
          "properties": {
            "print_still_active": {
              "additionalProperties": false,
              "properties": {
                "state": {
                  "$ref": "#/$defs/PrinterState",
                  "description": "The state the printer reported that job in."
                }
              },
              "required": [
                "state"
              ],
              "type": "object"
            }
          },
          "required": [
            "print_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A close was refused because an intervention for the print has neither\nexpired nor been restored.",
          "properties": {
            "intervention_still_active": {
              "additionalProperties": false,
              "properties": {
                "intervention_id": {
                  "$ref": "#/$defs/InterventionId",
                  "description": "The intervention still standing."
                }
              },
              "required": [
                "intervention_id"
              ],
              "type": "object"
            }
          },
          "required": [
            "intervention_still_active"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A spool command could not be carried out.",
          "properties": {
            "spool_refused": {
              "additionalProperties": false,
              "properties": {
                "detail": {
                  "description": "Why, in a sentence naming what to do instead.",
                  "type": "string"
                }
              },
              "required": [
                "detail"
              ],
              "type": "object"
            }
          },
          "required": [
            "spool_refused"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolCommand": {
      "description": "The whole vocabulary of the spool ledger, and there is no other.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Register a spool.",
          "properties": {
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "color": {
              "description": "The filament's colour.",
              "type": [
                "string",
                "null"
              ]
            },
            "command": {
              "const": "register_spool",
              "type": "string"
            },
            "density_g_cm3": {
              "description": "The material's density, in grams per cubic centimetre; the known\ndensity of the material when absent, and a material with none known\nis refused.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            },
            "diameter_mm": {
              "description": "The filament's diameter, in millimetres; 1.75 when absent.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            },
            "initial_g": {
              "description": "The net filament on the spool when full, in grams.",
              "format": "double",
              "type": "number"
            },
            "material": {
              "description": "The material, such as `PLA`.",
              "type": "string"
            },
            "name": {
              "description": "The operator's own label for it.",
              "type": "string"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "remaining_g": {
              "description": "The filament on it now, in grams; the full figure when absent.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            },
            "tare_g": {
              "description": "The empty spool's own weight, in grams.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            }
          },
          "required": [
            "command",
            "name",
            "material",
            "initial_g",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Load a spool onto a tool.\n\nOnto an occupied tool it is a swap, and the spool there is unloaded in\nthe same write. Naming the spool already on the tool confirms it was\nnever swapped, which is accepted only while that tool holds a pending\ncharge or that spool holds a filament-change mark.",
          "properties": {
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "command": {
              "const": "load_spool",
              "type": "string"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool to load."
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "command",
            "spool_id",
            "tool",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Unload a spool from its tool.",
          "properties": {
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "command": {
              "const": "unload_spool",
              "type": "string"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool to unload."
            }
          },
          "required": [
            "command",
            "spool_id",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Correct a spool's remaining figure, from exactly one of the two; a\ncommand naming both or neither is refused as `spool_refused`.",
          "properties": {
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "command": {
              "const": "adjust_spool",
              "type": "string"
            },
            "gross_g": {
              "description": "A scale reading of the spool, in grams, which needs its tare.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "remaining_g": {
              "description": "The filament left, in grams.",
              "format": "double",
              "type": [
                "number",
                "null"
              ]
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool to correct."
            }
          },
          "required": [
            "command",
            "spool_id",
            "reason",
            "actor"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "Retire a spool, after which it refuses every command.",
          "properties": {
            "actor": {
              "$ref": "#/$defs/Actor",
              "description": "Who is asking."
            },
            "command": {
              "const": "retire_spool",
              "type": "string"
            },
            "reason": {
              "description": "Why the actor is asking for this.",
              "type": "string"
            },
            "spool_id": {
              "$ref": "#/$defs/SpoolId",
              "description": "The spool to retire."
            }
          },
          "required": [
            "command",
            "spool_id",
            "reason",
            "actor"
          ],
          "type": "object"
        }
      ]
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool command was refused.",
  "properties": {
    "command": {
      "$ref": "#/$defs/SpoolCommand",
      "description": "The command, as it was asked for."
    },
    "decision": {
      "$ref": "#/$defs/PolicyDecision",
      "description": "The whole decision, carrying which refusal it was."
    }
  },
  "required": [
    "command",
    "decision"
  ],
  "title": "SpoolCommandRejectedPayload",
  "type": "object",
  "x-event-kind": "spool_command_rejected"
}
```

### SpoolId

Declared by `printobserver-core`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
  "format": "uuid",
  "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
  "title": "SpoolId",
  "type": "string"
}
```

### SpoolLoadedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ClearedMark": {
      "additionalProperties": false,
      "description": "One mark a spool command cleared, and the spool it was cleared from.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "The reason that no longer stands."
        },
        "spool_id": {
          "$ref": "#/$defs/SpoolId",
          "description": "The spool the mark was cleared from."
        }
      },
      "required": [
        "spool_id",
        "reason"
      ],
      "type": "object"
    },
    "PendingTaken": {
      "additionalProperties": false,
      "description": "One pending charge a spool command took.",
      "properties": {
        "charge_key": {
          "$ref": "#/$defs/ChargeKey",
          "description": "The key it is charged under."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it was fed in."
        }
      },
      "required": [
        "charge_key",
        "print_id"
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool was loaded onto a tool, or confirmed as never having left it.",
  "properties": {
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who loaded it."
    },
    "cleared_marks": {
      "description": "Every mark the load cleared, on either spool.",
      "items": {
        "$ref": "#/$defs/ClearedMark"
      },
      "type": "array"
    },
    "reason": {
      "description": "Why.",
      "type": "string"
    },
    "replaced": {
      "anyOf": [
        {
          "$ref": "#/$defs/SpoolId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The spool the load swapped off the tool, when it was occupied."
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool loaded."
    },
    "took_pending": {
      "description": "The pending charges the load took, each also its own\n`filament_consumed` event under its key.",
      "items": {
        "$ref": "#/$defs/PendingTaken"
      },
      "type": "array"
    },
    "tool": {
      "description": "The tool, in the printer's own numbering.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "spool_id",
    "tool",
    "actor",
    "reason"
  ],
  "title": "SpoolLoadedPayload",
  "type": "object",
  "x-event-kind": "spool_loaded"
}
```

### SpoolRecord

Declared by `printobserver-core`.

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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "SpoolTrust": {
      "description": "How far a spool's remaining figure can be trusted.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Every charge against the spool was taken exactly.",
          "properties": {
            "state": {
              "const": "verified",
              "type": "string"
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "At least one thing happened that could not be charged exactly.",
          "properties": {
            "marks": {
              "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
              "items": {
                "$ref": "#/$defs/UnverifiedMark"
              },
              "minItems": 1,
              "type": "array"
            },
            "state": {
              "const": "unverified",
              "type": "string"
            }
          },
          "required": [
            "state",
            "marks"
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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One spool the operator registered, and what is known of it now.",
  "properties": {
    "color": {
      "description": "The filament's colour, when the operator gave one.",
      "type": [
        "string",
        "null"
      ]
    },
    "density_g_cm3": {
      "description": "The material's density, in grams per cubic centimetre; above zero.",
      "format": "double",
      "type": "number"
    },
    "diameter_mm": {
      "description": "The filament's diameter, in millimetres; above zero.",
      "format": "double",
      "type": "number"
    },
    "id": {
      "$ref": "#/$defs/SpoolId",
      "description": "This spool's identifier, minted when it was registered."
    },
    "initial_g": {
      "description": "The net filament on the spool when it was full, in grams; above zero.",
      "format": "double",
      "type": "number"
    },
    "loaded_on_tool": {
      "description": "The tool the spool is loaded on, when it is loaded.",
      "format": "int64",
      "type": [
        "integer",
        "null"
      ]
    },
    "material": {
      "description": "The material, as given and trimmed, such as `PLA`; never empty.",
      "type": "string"
    },
    "name": {
      "description": "The operator's own label for the spool; never empty.",
      "type": "string"
    },
    "registered_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the spool was registered."
    },
    "remaining_g": {
      "description": "The filament left, in grams.\n\nAlways the initial figure less every charge plus every correction. A\ncharge is never refused or clamped, so this may go below zero.",
      "format": "double",
      "type": "number"
    },
    "retired_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When the spool was retired, if it has been."
    },
    "tare_g": {
      "description": "The empty spool's own weight, in grams, when the operator gave it.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    },
    "trust": {
      "$ref": "#/$defs/SpoolTrust",
      "description": "How far the remaining figure can be trusted."
    },
    "updated_at": {
      "$ref": "#/$defs/Timestamp",
      "description": "When anything about the spool last changed."
    }
  },
  "required": [
    "id",
    "name",
    "material",
    "diameter_mm",
    "density_g_cm3",
    "initial_g",
    "remaining_g",
    "trust",
    "registered_at",
    "updated_at"
  ],
  "title": "SpoolRecord",
  "type": "object"
}
```

### SpoolRegisteredPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "SpoolRecord": {
      "additionalProperties": false,
      "description": "One spool the operator registered, and what is known of it now.",
      "properties": {
        "color": {
          "description": "The filament's colour, when the operator gave one.",
          "type": [
            "string",
            "null"
          ]
        },
        "density_g_cm3": {
          "description": "The material's density, in grams per cubic centimetre; above zero.",
          "format": "double",
          "type": "number"
        },
        "diameter_mm": {
          "description": "The filament's diameter, in millimetres; above zero.",
          "format": "double",
          "type": "number"
        },
        "id": {
          "$ref": "#/$defs/SpoolId",
          "description": "This spool's identifier, minted when it was registered."
        },
        "initial_g": {
          "description": "The net filament on the spool when it was full, in grams; above zero.",
          "format": "double",
          "type": "number"
        },
        "loaded_on_tool": {
          "description": "The tool the spool is loaded on, when it is loaded.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "material": {
          "description": "The material, as given and trimmed, such as `PLA`; never empty.",
          "type": "string"
        },
        "name": {
          "description": "The operator's own label for the spool; never empty.",
          "type": "string"
        },
        "registered_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was registered."
        },
        "remaining_g": {
          "description": "The filament left, in grams.\n\nAlways the initial figure less every charge plus every correction. A\ncharge is never refused or clamped, so this may go below zero.",
          "format": "double",
          "type": "number"
        },
        "retired_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the spool was retired, if it has been."
        },
        "tare_g": {
          "description": "The empty spool's own weight, in grams, when the operator gave it.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "trust": {
          "$ref": "#/$defs/SpoolTrust",
          "description": "How far the remaining figure can be trusted."
        },
        "updated_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When anything about the spool last changed."
        }
      },
      "required": [
        "id",
        "name",
        "material",
        "diameter_mm",
        "density_g_cm3",
        "initial_g",
        "remaining_g",
        "trust",
        "registered_at",
        "updated_at"
      ],
      "type": "object"
    },
    "SpoolTrust": {
      "description": "How far a spool's remaining figure can be trusted.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Every charge against the spool was taken exactly.",
          "properties": {
            "state": {
              "const": "verified",
              "type": "string"
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "At least one thing happened that could not be charged exactly.",
          "properties": {
            "marks": {
              "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
              "items": {
                "$ref": "#/$defs/UnverifiedMark"
              },
              "minItems": 1,
              "type": "array"
            },
            "state": {
              "const": "unverified",
              "type": "string"
            }
          },
          "required": [
            "state",
            "marks"
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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool was registered.",
  "properties": {
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who registered it."
    },
    "reason": {
      "description": "Why.",
      "type": "string"
    },
    "spool": {
      "$ref": "#/$defs/SpoolRecord",
      "description": "The spool, as it was registered."
    }
  },
  "required": [
    "spool",
    "actor",
    "reason"
  ],
  "title": "SpoolRegisteredPayload",
  "type": "object",
  "x-event-kind": "spool_registered"
}
```

### SpoolRetiredPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool was retired.",
  "properties": {
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who retired it."
    },
    "reason": {
      "description": "Why.",
      "type": "string"
    },
    "remaining_g": {
      "description": "Its figure as it was retired, in grams.",
      "format": "double",
      "type": "number"
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool retired."
    }
  },
  "required": [
    "spool_id",
    "remaining_g",
    "actor",
    "reason"
  ],
  "title": "SpoolRetiredPayload",
  "type": "object",
  "x-event-kind": "spool_retired"
}
```

### SpoolTrust

Declared by `printobserver-core`.

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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "How far a spool's remaining figure can be trusted.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "Every charge against the spool was taken exactly.",
      "properties": {
        "state": {
          "const": "verified",
          "type": "string"
        }
      },
      "required": [
        "state"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "At least one thing happened that could not be charged exactly.",
      "properties": {
        "marks": {
          "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
          "items": {
            "$ref": "#/$defs/UnverifiedMark"
          },
          "minItems": 1,
          "type": "array"
        },
        "state": {
          "const": "unverified",
          "type": "string"
        }
      },
      "required": [
        "state",
        "marks"
      ],
      "type": "object"
    }
  ],
  "title": "SpoolTrust"
}
```

### SpoolUnloadedPayload

Declared by `printobserver-core`.

```json
{
  "$defs": {
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool was unloaded from its tool.",
  "properties": {
    "actor": {
      "$ref": "#/$defs/Actor",
      "description": "Who unloaded it."
    },
    "reason": {
      "description": "Why.",
      "type": "string"
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool unloaded."
    },
    "tool": {
      "description": "The tool it left.",
      "format": "int64",
      "type": "integer"
    }
  },
  "required": [
    "spool_id",
    "tool",
    "actor",
    "reason"
  ],
  "title": "SpoolUnloadedPayload",
  "type": "object",
  "x-event-kind": "spool_unloaded"
}
```

### SpoolUnverifiedPayload

Declared by `printobserver-core`.

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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "A spool was marked as one whose figure is in doubt.",
  "properties": {
    "reason": {
      "$ref": "#/$defs/UnverifiedReason",
      "description": "Why its figure is in doubt."
    },
    "spool_id": {
      "$ref": "#/$defs/SpoolId",
      "description": "The spool marked."
    }
  },
  "required": [
    "spool_id",
    "reason"
  ],
  "title": "SpoolUnverifiedPayload",
  "type": "object",
  "x-event-kind": "spool_unverified"
}
```

### SpoolsAnswer

Declared by `printobserver-server`.

```json
{
  "$defs": {
    "ChargeKey": {
      "description": "The durable identity of one debit, so that a replay can never debit twice.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "One segment of a print this system watched, on one tool.",
          "properties": {
            "key": {
              "const": "print",
              "type": "string"
            },
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print."
            },
            "segment": {
              "description": "Which segment: zero from the print's start, and one more each time\na mid-print swap or a filament-change signal closes the current one.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "print_id",
            "tool",
            "segment"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "One print this system never saw, on one tool.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "generation": {
              "description": "The file's tally generation, which advances each time the printer's\ntally for the file is reset.",
              "format": "uint32",
              "minimum": 0,
              "type": "integer"
            },
            "key": {
              "const": "unobserved",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "print_number": {
              "description": "The print's ordinal in the file's tally \u2014 completed and failed\nprints counted up to and including this one \u2014 within that\ngeneration.",
              "format": "int64",
              "type": "integer"
            },
            "tool": {
              "description": "The tool, in the printer's own numbering.",
              "format": "int64",
              "type": "integer"
            }
          },
          "required": [
            "key",
            "origin",
            "file_name",
            "generation",
            "print_number",
            "tool"
          ],
          "type": "object"
        }
      ]
    },
    "ChargeMethod": {
      "description": "How the length a charge debits was worked out.",
      "oneOf": [
        {
          "const": "slicer_estimate",
          "description": "A completed print: the length the file's analysis gives.",
          "type": "string"
        },
        {
          "const": "gcode_extrusion",
          "description": "The extrusion the file's own moves add up to, up to a position in it.",
          "type": "string"
        },
        {
          "const": "file_fraction",
          "description": "The analysed length scaled by how far into the file the print got; the\nfallback used only when the file itself could not be read.",
          "type": "string"
        },
        {
          "const": "unobserved_estimate",
          "description": "A completed print this system never saw: the file's analysed length.",
          "type": "string"
        }
      ]
    },
    "FilePosition": {
      "additionalProperties": false,
      "description": "How far into its file a print had got, and when that was read.",
      "properties": {
        "bytes": {
          "description": "The byte the printer had read the file up to.",
          "format": "int64",
          "type": "integer"
        },
        "observed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the position was read."
        },
        "size_bytes": {
          "description": "The file's size in bytes, when it was known.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        }
      },
      "required": [
        "bytes",
        "observed_at"
      ],
      "type": "object"
    },
    "PendingCharge": {
      "additionalProperties": false,
      "description": "Filament a tool fed after a filament change nobody has confirmed, held until\nsomebody says which spool fed it.\n\nA length rather than grams, because the grams follow the spool that takes\nit.",
      "properties": {
        "analysed_length_mm": {
          "description": "The file's analysed length for the tool, in millimetres, when it had one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "charge_key": {
          "$ref": "#/$defs/ChargeKey",
          "description": "The key of the print segment the filament was fed in."
        },
        "length_mm": {
          "description": "How much was fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "method": {
          "$ref": "#/$defs/ChargeMethod",
          "description": "How that length was worked out."
        },
        "note": {
          "description": "Anything a reader needs to make sense of the figure.",
          "type": [
            "string",
            "null"
          ]
        },
        "pended_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was held."
        },
        "position": {
          "anyOf": [
            {
              "$ref": "#/$defs/FilePosition"
            },
            {
              "type": "null"
            }
          ],
          "description": "How far into the file the segment ran, when that was read."
        },
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print it was fed in."
        },
        "tool": {
          "description": "The tool that fed it.",
          "format": "int64",
          "type": "integer"
        }
      },
      "required": [
        "charge_key",
        "print_id",
        "tool",
        "length_mm",
        "method",
        "pended_at"
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
    "SpoolId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one registered spool.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "SpoolId",
      "type": "string"
    },
    "SpoolRecord": {
      "additionalProperties": false,
      "description": "One spool the operator registered, and what is known of it now.",
      "properties": {
        "color": {
          "description": "The filament's colour, when the operator gave one.",
          "type": [
            "string",
            "null"
          ]
        },
        "density_g_cm3": {
          "description": "The material's density, in grams per cubic centimetre; above zero.",
          "format": "double",
          "type": "number"
        },
        "diameter_mm": {
          "description": "The filament's diameter, in millimetres; above zero.",
          "format": "double",
          "type": "number"
        },
        "id": {
          "$ref": "#/$defs/SpoolId",
          "description": "This spool's identifier, minted when it was registered."
        },
        "initial_g": {
          "description": "The net filament on the spool when it was full, in grams; above zero.",
          "format": "double",
          "type": "number"
        },
        "loaded_on_tool": {
          "description": "The tool the spool is loaded on, when it is loaded.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "material": {
          "description": "The material, as given and trimmed, such as `PLA`; never empty.",
          "type": "string"
        },
        "name": {
          "description": "The operator's own label for the spool; never empty.",
          "type": "string"
        },
        "registered_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was registered."
        },
        "remaining_g": {
          "description": "The filament left, in grams.\n\nAlways the initial figure less every charge plus every correction. A\ncharge is never refused or clamped, so this may go below zero.",
          "format": "double",
          "type": "number"
        },
        "retired_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the spool was retired, if it has been."
        },
        "tare_g": {
          "description": "The empty spool's own weight, in grams, when the operator gave it.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        },
        "trust": {
          "$ref": "#/$defs/SpoolTrust",
          "description": "How far the remaining figure can be trusted."
        },
        "updated_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When anything about the spool last changed."
        }
      },
      "required": [
        "id",
        "name",
        "material",
        "diameter_mm",
        "density_g_cm3",
        "initial_g",
        "remaining_g",
        "trust",
        "registered_at",
        "updated_at"
      ],
      "type": "object"
    },
    "SpoolTrust": {
      "description": "How far a spool's remaining figure can be trusted.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "Every charge against the spool was taken exactly.",
          "properties": {
            "state": {
              "const": "verified",
              "type": "string"
            }
          },
          "required": [
            "state"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "At least one thing happened that could not be charged exactly.",
          "properties": {
            "marks": {
              "description": "Every reason the figure is in doubt, oldest first, at most one per\ndistinct reason; never empty.",
              "items": {
                "$ref": "#/$defs/UnverifiedMark"
              },
              "minItems": 1,
              "type": "array"
            },
            "state": {
              "const": "unverified",
              "type": "string"
            }
          },
          "required": [
            "state",
            "marks"
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
    },
    "UnverifiedMark": {
      "additionalProperties": false,
      "description": "One reason a spool's figure is in doubt, and since when.",
      "properties": {
        "reason": {
          "$ref": "#/$defs/UnverifiedReason",
          "description": "Why the figure is in doubt."
        },
        "since": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the spool was marked for it."
        }
      },
      "required": [
        "reason",
        "since"
      ],
      "type": "object"
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What the spools read answers.\n\n`at_a_glance` is one line per spool, in the order `spools` lists them,\ncomposed here so that what is left on each spool is the first thing a\nreader of either rendering meets rather than a figure inside a whole\nrecord. Its name is what puts it first: both renderings walk an answer's\nfields in name order, ahead of `pending` and `spools`.",
  "properties": {
    "at_a_glance": {
      "description": "One line per spool in `spools`, in the same order.",
      "items": {
        "type": "string"
      },
      "type": "array"
    },
    "pending": {
      "description": "Every charge held after a filament change nobody has confirmed, oldest\nfirst: what the next spool loaded on that tool is charged.",
      "items": {
        "$ref": "#/$defs/PendingCharge"
      },
      "type": "array"
    },
    "spools": {
      "description": "Every spool, retired ones only when asked for, most recently registered\nfirst.",
      "items": {
        "$ref": "#/$defs/SpoolRecord"
      },
      "type": "array"
    }
  },
  "required": [
    "at_a_glance",
    "spools",
    "pending"
  ],
  "title": "SpoolsAnswer",
  "type": "object"
}
```

### StartupOutcome

Declared by `printobserver-server`.

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

Declared by `printobserver-server`.

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
  "type": "object",
  "x-event-kind": "startup_reconciliation"
}
```

### StatusAnswer

Declared by `printobserver-server`.

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
        "filament": {
          "description": "The filament the source's analysis of the file says each tool uses;\nempty when the source reported none.",
          "items": {
            "$ref": "#/$defs/ToolFilament"
          },
          "type": "array"
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
        "file_position_bytes": {
          "description": "How far into the file the printer has read, in bytes, when the source\nreported it.",
          "format": "int64",
          "type": [
            "integer",
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
    "PrintRecord": {
      "additionalProperties": false,
      "description": "One print, and the record supervision keys from.\n\nThe provider's own print id is carried beside this record's identifier\nrather than as it, because a print may be observed before the provider\nthat reports it has one.",
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
        "job_print_time_s": {
          "description": "The longest running time, in whole seconds, the printer reported for\nthis print's job at a read that found it printing rather than paused.\n\nThe time a job has spent printing only grows, pause or no pause, so a\njob of this print's file reporting less \u2014 by more than\n`listing::JOB_IDENTITY_TOLERANCE_S` \u2014 is a later job. Absent until a\nread finds the job printing.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
        },
        "job_started_at": {
          "anyOf": [
            {
              "$ref": "#/$defs/Timestamp"
            },
            {
              "type": "null"
            }
          ],
          "description": "When the printer's job this print records began, as the printer's own\nrunning time put it at the latest read that found the job running or\npaused: the instant of that read, less the running time it reported.\n\n`OctoPrint` counts a pause into the running time until the job resumes\nand then takes the pause back out, so a pause moves this later: a read\nputting it further than `listing::JOB_IDENTITY_TOLERANCE_S` from the one\nrecorded resets it, rather than it being fixed once. Absent when no read has\nfound this print's job reporting a running time \u2014 a print opened by an\nalert or a start before any read, and every print recorded before this\nwas."
        },
        "narrowings": {
          "description": "Every manifest range this print narrowed to the envelope's.",
          "items": {
            "$ref": "#/$defs/ManifestNarrowing"
          },
          "type": "array"
        },
        "opened_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When the print was opened."
        },
        "provider_print_id": {
          "description": "The provider's own identifier for the print, whichever provider\nreported it, when one has.",
          "format": "int64",
          "type": [
            "integer",
            "null"
          ]
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
    },
    "ToolFilament": {
      "additionalProperties": false,
      "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
      "properties": {
        "length_mm": {
          "description": "The length of filament fed, in millimetres.",
          "format": "double",
          "type": "number"
        },
        "tool": {
          "description": "The tool, in the printer's own numbering.",
          "format": "int64",
          "type": "integer"
        },
        "volume_cm3": {
          "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
          "format": "double",
          "type": [
            "number",
            "null"
          ]
        }
      },
      "required": [
        "tool",
        "length_mm"
      ],
      "type": "object"
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "What a status read answers.",
  "properties": {
    "interventions": {
      "description": "The bounded interventions still in force.",
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
    "print": {
      "$ref": "#/$defs/PrintRecord",
      "description": "The print."
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
    "session": {
      "anyOf": [
        {
          "$ref": "#/$defs/SupervisionSession"
        },
        {
          "type": "null"
        }
      ],
      "description": "The supervision session watching it, when one is."
    }
  },
  "required": [
    "print",
    "interventions"
  ],
  "title": "StatusAnswer",
  "type": "object"
}
```

### SupervisionSession

Declared by `printobserver-supervisor-api`.

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

Declared by `printobserver-supervisor-api`.

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
  "type": "object",
  "x-event-kind": "supervision_session_closed"
}
```

### SupervisionSessionOpenedPayload

Declared by `printobserver-supervisor-api`.

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
  "type": "object",
  "x-event-kind": "supervision_session_opened"
}
```

### TimelineAnswer

Declared by `printobserver-server`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "PrintId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one print.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "PrintId",
      "type": "string"
    },
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "What a printer history read answers: one page of the printer's own events.",
  "properties": {
    "events": {
      "description": "The events, newest first.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    },
    "next": {
      "anyOf": [
        {
          "$ref": "#/$defs/EventId"
        },
        {
          "type": "null"
        }
      ],
      "description": "The cursor for the following, older page, given as `before`; absent\nwhen there is none."
    }
  },
  "required": [
    "events"
  ],
  "title": "TimelineAnswer",
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

### ToolFilament

Declared by `printobserver-printer-api`.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "The filament one tool feeds, as a length and, when it is known, a volume.",
  "properties": {
    "length_mm": {
      "description": "The length of filament fed, in millimetres.",
      "format": "double",
      "type": "number"
    },
    "tool": {
      "description": "The tool, in the printer's own numbering.",
      "format": "int64",
      "type": "integer"
    },
    "volume_cm3": {
      "description": "The volume of filament fed, in cubic centimetres, when the source\nreported one.",
      "format": "double",
      "type": [
        "number",
        "null"
      ]
    }
  },
  "required": [
    "tool",
    "length_mm"
  ],
  "title": "ToolFilament",
  "type": "object"
}
```

### TurnOutcome

Declared by `printobserver-supervisor-api`.

```json
{
  "$defs": {
    "AgentAssessment": {
      "additionalProperties": false,
      "description": "The agent's written record of one supervision turn.\n\nThis is deliberately not how the agent acts: acting is a\n`PrintAction` that policy rules on.",
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
    "EventId": {
      "description": "A lowercase hyphenated version 7 UUID identifying one event.",
      "format": "uuid",
      "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$",
      "title": "EventId",
      "type": "string"
    },
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
      "type": "string"
    },
    "Timestamp": {
      "description": "An instant in UTC, as an RFC 3339 string with a zero offset.",
      "format": "date-time",
      "title": "Timestamp",
      "type": "string"
    },
    "TurnSituation": {
      "additionalProperties": false,
      "description": "What the supervisor knew about the moment one turn began, beside its event.\n\nFacts rather than instructions: whether the print is paused, and whether\nthe detector paused it itself, are exactly what decides whether an agent's\nadjustment would be applied to a moving print or held until it moves again.\nEvery field is written out, a fact nobody knows as `null`, so that the\nprompt it fills has one shape whatever was known.",
      "properties": {
        "arrived_while_busy": {
          "default": [],
          "description": "Events for this print that arrived while its previous turn was running\nand that turn never took, oldest first. The turn's own event is the\nnewest of them, and is not repeated here.",
          "items": {
            "$ref": "#/$defs/EventRecord"
          },
          "type": "array"
        },
        "detector_paused_the_print": {
          "default": null,
          "description": "Whether the detector paused the print itself, null when the event is\nnot one of its detections.",
          "type": [
            "boolean",
            "null"
          ]
        },
        "detector_warned": {
          "default": null,
          "description": "Whether the detector only warned, null when the event is not one of its\ndetections.",
          "type": [
            "boolean",
            "null"
          ]
        },
        "printer_state": {
          "anyOf": [
            {
              "$ref": "#/$defs/PrinterState"
            },
            {
              "type": "null"
            }
          ],
          "default": null,
          "description": "The printer's state when the turn began, in the printer contract's own\nvocabulary \u2014 a state it does not name as `{\"unknown\": \"<the printer's\nword>\"}` \u2014 null when the printer could not be read."
        }
      },
      "type": "object"
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
    },
    "situation": {
      "$ref": "#/$defs/TurnSituation",
      "default": {
        "arrived_while_busy": [],
        "detector_paused_the_print": null,
        "detector_warned": null,
        "printer_state": null
      },
      "description": "What the supervisor knew about the moment the turn began."
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

### TurnSituation

Declared by `printobserver-supervisor-api`.

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
    "EventKind": {
      "description": "The name one kind of event is written down under: lowercase `snake_case`, declared by the domain that owns the event.",
      "pattern": "^[a-z][a-z0-9_]*$",
      "title": "EventKind",
      "type": "string"
    },
    "EventRecord": {
      "description": "One event, as the store holds it and the server serves it.\n\n`raw` holds the bytes exactly as received for an externally sourced event\nand is absent for an internally raised one \u2014 it is what makes the history\nauditable when a normalization turns out to be wrong. `print_id` is\noptional, because an externally sourced event may name no print this system\nknows.",
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
        "kind": {
          "$ref": "#/$defs/EventKind",
          "description": "Which event this is."
        },
        "payload": {
          "description": "What it carries, in the form its kind declares."
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
        "received_at",
        "kind",
        "payload"
      ],
      "type": "object"
    },
    "EventSource": {
      "description": "Where an event came from, as the bare string the domain that raised it declares for itself.",
      "title": "EventSource",
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
    "RawBytes": {
      "contentEncoding": "base64",
      "description": "Bytes exactly as received, base64-encoded.",
      "title": "RawBytes",
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
  "description": "What the supervisor knew about the moment one turn began, beside its event.\n\nFacts rather than instructions: whether the print is paused, and whether\nthe detector paused it itself, are exactly what decides whether an agent's\nadjustment would be applied to a moving print or held until it moves again.\nEvery field is written out, a fact nobody knows as `null`, so that the\nprompt it fills has one shape whatever was known.",
  "properties": {
    "arrived_while_busy": {
      "default": [],
      "description": "Events for this print that arrived while its previous turn was running\nand that turn never took, oldest first. The turn's own event is the\nnewest of them, and is not repeated here.",
      "items": {
        "$ref": "#/$defs/EventRecord"
      },
      "type": "array"
    },
    "detector_paused_the_print": {
      "default": null,
      "description": "Whether the detector paused the print itself, null when the event is\nnot one of its detections.",
      "type": [
        "boolean",
        "null"
      ]
    },
    "detector_warned": {
      "default": null,
      "description": "Whether the detector only warned, null when the event is not one of its\ndetections.",
      "type": [
        "boolean",
        "null"
      ]
    },
    "printer_state": {
      "anyOf": [
        {
          "$ref": "#/$defs/PrinterState"
        },
        {
          "type": "null"
        }
      ],
      "default": null,
      "description": "The printer's state when the turn began, in the printer contract's own\nvocabulary \u2014 a state it does not name as `{\"unknown\": \"<the printer's\nword>\"}` \u2014 null when the printer could not be read."
    }
  },
  "title": "TurnSituation",
  "type": "object"
}
```

### UnaccountedPrintPayload

Declared by `printobserver-core`.

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
  "description": "A print this system never saw was found in the printer's own tally.",
  "properties": {
    "charged": {
      "description": "Whether it was charged against a spool.",
      "type": "boolean"
    },
    "file_name": {
      "description": "The file that was printed.",
      "type": "string"
    },
    "printed_at": {
      "anyOf": [
        {
          "$ref": "#/$defs/Timestamp"
        },
        {
          "type": "null"
        }
      ],
      "description": "When it was printed, when the printer says."
    },
    "success": {
      "description": "Whether it completed.",
      "type": "boolean"
    }
  },
  "required": [
    "file_name",
    "success",
    "charged"
  ],
  "title": "UnaccountedPrintPayload",
  "type": "object",
  "x-event-kind": "unaccounted_print"
}
```

### UnverifiedMark

Declared by `printobserver-core`.

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
    },
    "UnverifiedReason": {
      "description": "Why a spool's figure is in doubt.",
      "oneOf": [
        {
          "additionalProperties": false,
          "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print nothing could be charged for."
            },
            "reason": {
              "const": "uncharged_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A print this system never saw ran and did not complete.",
          "properties": {
            "file_name": {
              "description": "The file that was printed.",
              "type": "string"
            },
            "printed_at": {
              "$ref": "#/$defs/Timestamp",
              "description": "When it was printed."
            },
            "reason": {
              "const": "unaccounted_print",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "printed_at"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
          "properties": {
            "print_id": {
              "$ref": "#/$defs/PrintId",
              "description": "The print the change was signalled during."
            },
            "reason": {
              "const": "filament_change_unconfirmed_position",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "print_id"
          ],
          "type": "object"
        },
        {
          "additionalProperties": false,
          "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
          "properties": {
            "file_name": {
              "description": "The file whose tally was reset.",
              "type": "string"
            },
            "origin": {
              "description": "Where the file lives, in the printer's own vocabulary.",
              "type": "string"
            },
            "reason": {
              "const": "tally_reset",
              "type": "string"
            }
          },
          "required": [
            "reason",
            "file_name",
            "origin"
          ],
          "type": "object"
        }
      ]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "additionalProperties": false,
  "description": "One reason a spool's figure is in doubt, and since when.",
  "properties": {
    "reason": {
      "$ref": "#/$defs/UnverifiedReason",
      "description": "Why the figure is in doubt."
    },
    "since": {
      "$ref": "#/$defs/Timestamp",
      "description": "When the spool was marked for it."
    }
  },
  "required": [
    "reason",
    "since"
  ],
  "title": "UnverifiedMark",
  "type": "object"
}
```

### UnverifiedReason

Declared by `printobserver-core`.

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
  "description": "Why a spool's figure is in doubt.",
  "oneOf": [
    {
      "additionalProperties": false,
      "description": "A print ran on the spool's tool and nothing could be charged, because\nthe file had no analysis \u2014 an SD-card file, for one.",
      "properties": {
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print nothing could be charged for."
        },
        "reason": {
          "const": "uncharged_print",
          "type": "string"
        }
      },
      "required": [
        "reason",
        "print_id"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A print this system never saw ran and did not complete.",
      "properties": {
        "file_name": {
          "description": "The file that was printed.",
          "type": "string"
        },
        "printed_at": {
          "$ref": "#/$defs/Timestamp",
          "description": "When it was printed."
        },
        "reason": {
          "const": "unaccounted_print",
          "type": "string"
        }
      },
      "required": [
        "reason",
        "file_name",
        "printed_at"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A filament change was signalled and nobody has said which spool is now\nloaded. The spool was charged up to a position read when the signal was\nhandled.",
      "properties": {
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print the change was signalled during."
        },
        "reason": {
          "const": "filament_change_unconfirmed",
          "type": "string"
        }
      },
      "required": [
        "reason",
        "print_id"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A filament change was signalled, nobody has said which spool is now\nloaded, and no fresh position could be read at the signal: the spool was\ncharged only up to the last recorded position, so its figure may be\noverstated.",
      "properties": {
        "print_id": {
          "$ref": "#/$defs/PrintId",
          "description": "The print the change was signalled during."
        },
        "reason": {
          "const": "filament_change_unconfirmed_position",
          "type": "string"
        }
      },
      "required": [
        "reason",
        "print_id"
      ],
      "type": "object"
    },
    {
      "additionalProperties": false,
      "description": "A file's print tally was reset, and it cannot be told which of the\nprints since the re-upload were already settled.",
      "properties": {
        "file_name": {
          "description": "The file whose tally was reset.",
          "type": "string"
        },
        "origin": {
          "description": "Where the file lives, in the printer's own vocabulary.",
          "type": "string"
        },
        "reason": {
          "const": "tally_reset",
          "type": "string"
        }
      },
      "required": [
        "reason",
        "file_name",
        "origin"
      ],
      "type": "object"
    }
  ],
  "title": "UnverifiedReason"
}
```

### operations

Declared by `printobserver-server`.

```json
{
  "description_version": 2,
  "media_type": "application/json",
  "operations": [
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "prints",
      "parameters": [],
      "path": "/v1/prints",
      "responses": [
        {
          "answer": "success",
          "type": "PrintsAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "status",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/status",
      "responses": [
        {
          "answer": "success",
          "type": "StatusAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": "image_path",
      "method": "GET",
      "name": "context",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/context",
      "responses": [
        {
          "answer": "success",
          "type": "ContextAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": "path",
      "method": "GET",
      "name": "image",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "image_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/images/{image_id}",
      "responses": [
        {
          "answer": "success",
          "type": "ImageAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "history",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "integer",
          "located": "query",
          "name": "limit",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "integer"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/history",
      "responses": [
        {
          "answer": "success",
          "type": "HistoryAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "manifest_get",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/manifest",
      "responses": [
        {
          "answer": "success",
          "type": "ManifestAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": "image_path",
      "method": "GET",
      "name": "look",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "integer",
          "located": "query",
          "name": "wait_s",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "integer"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/look",
      "responses": [
        {
          "answer": "success",
          "type": "Look"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "write",
      "image_path_field": null,
      "method": "PUT",
      "name": "manifest_set",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "manifest",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/JobManifest"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/manifest",
      "responses": [
        {
          "answer": "success",
          "type": "ManifestAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "spools",
      "parameters": [
        {
          "kind": "boolean",
          "located": "query",
          "name": "include_retired",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "boolean"
          }
        }
      ],
      "path": "/v1/spools",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolsAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "spool",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "spool_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/spools/{spool_id}",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "printer_history",
      "parameters": [
        {
          "kind": "text",
          "located": "query",
          "name": "kinds",
          "repeated": true,
          "required": false,
          "shape": {
            "items": {
              "type": "string"
            },
            "type": "array"
          }
        },
        {
          "kind": "text",
          "located": "query",
          "name": "since",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "query",
          "name": "until",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "query",
          "name": "spool_id",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "query",
          "name": "before",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "integer",
          "located": "query",
          "name": "limit",
          "repeated": false,
          "required": false,
          "shape": {
            "type": "integer"
          }
        }
      ],
      "path": "/v1/history",
      "responses": [
        {
          "answer": "success",
          "type": "TimelineAnswer"
        }
      ]
    },
    {
      "accepts": null,
      "answers": "application/json",
      "effect": "read",
      "image_path_field": null,
      "method": "GET",
      "name": "preflight",
      "parameters": [
        {
          "kind": "text",
          "located": "query",
          "name": "file_name",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        }
      ],
      "path": "/v1/preflight",
      "responses": [
        {
          "answer": "success",
          "type": "PreflightAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:pause",
      "image_path_field": null,
      "method": "POST",
      "name": "pause",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/pause",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:resume",
      "image_path_field": null,
      "method": "POST",
      "name": "resume",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/resume",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:cancel",
      "image_path_field": null,
      "method": "POST",
      "name": "cancel",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/cancel",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:start_print",
      "image_path_field": null,
      "method": "POST",
      "name": "start_print",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "file_name",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/FileName",
            "description": "The file to print, validated as a name a file API can be asked for."
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "manifest",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/JobManifest",
            "description": "The manifest this print is bounded by."
          }
        },
        {
          "kind": "boolean",
          "located": "body",
          "name": "override_filament_check",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "Start despite an insufficient or unverified spool, or a file\n`OctoPrint` has not analysed; the start's own reason is the\noverride's reason.",
            "type": "boolean"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/start_print",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:set_feedrate_factor",
      "image_path_field": null,
      "method": "POST",
      "name": "set_feedrate_factor",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "duration_s",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "How long the change stands for, in whole seconds.",
            "format": "int64",
            "type": [
              "integer",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "factor",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The multiplier asked for.",
            "format": "double",
            "type": "number"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/set_feedrate_factor",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:set_flowrate_factor",
      "image_path_field": null,
      "method": "POST",
      "name": "set_flowrate_factor",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "duration_s",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "How long the change stands for, in whole seconds.",
            "format": "int64",
            "type": [
              "integer",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "factor",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The multiplier asked for.",
            "format": "double",
            "type": "number"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/set_flowrate_factor",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:set_tool_target_c",
      "image_path_field": null,
      "method": "POST",
      "name": "set_tool_target_c",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "duration_s",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "How long the change stands for, in whole seconds.",
            "format": "int64",
            "type": [
              "integer",
              "null"
            ]
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "target_c",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The target temperature asked for, in degrees Celsius.",
            "format": "double",
            "type": "number"
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "tool",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The tool, in the printer's own numbering.",
            "format": "int64",
            "type": "integer"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/set_tool_target_c",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:set_bed_target_c",
      "image_path_field": null,
      "method": "POST",
      "name": "set_bed_target_c",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "duration_s",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "How long the change stands for, in whole seconds.",
            "format": "int64",
            "type": [
              "integer",
              "null"
            ]
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "target_c",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The target temperature asked for, in degrees Celsius.",
            "format": "double",
            "type": "number"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/set_bed_target_c",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:set_fan_percent",
      "image_path_field": null,
      "method": "POST",
      "name": "set_fan_percent",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "duration_s",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "How long the change stands for, in whole seconds.",
            "format": "int64",
            "type": [
              "integer",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "percent",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The percentage asked for.",
            "format": "double",
            "type": "number"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/set_fan_percent",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:acknowledge_failure",
      "image_path_field": null,
      "method": "POST",
      "name": "acknowledge_failure",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "disposition",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/AcknowledgementDisposition",
            "description": "What to do next."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "event_id",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/EventId",
            "description": "The event being acknowledged."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/acknowledge_failure",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:close_print",
      "image_path_field": null,
      "method": "POST",
      "name": "close_print",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "print_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "outcome",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/PrintOutcome",
            "description": "How the print ended, in the operator's judgement."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/prints/{print_id}/actions/close_print",
      "responses": [
        {
          "answer": "success",
          "type": "ActionAnswer"
        },
        {
          "answer": "rejected",
          "type": "ActionAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:register_spool",
      "image_path_field": null,
      "method": "POST",
      "name": "register_spool",
      "parameters": [
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "color",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The filament's colour.",
            "type": [
              "string",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "density_g_cm3",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The material's density, in grams per cubic centimetre; the known\ndensity of the material when absent, and a material with none known\nis refused.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "diameter_mm",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The filament's diameter, in millimetres; 1.75 when absent.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "initial_g",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The net filament on the spool when full, in grams.",
            "format": "double",
            "type": "number"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "material",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The material, such as `PLA`.",
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "name",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The operator's own label for it.",
            "type": "string"
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "remaining_g",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The filament on it now, in grams; the full figure when absent.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "tare_g",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The empty spool's own weight, in grams.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        }
      ],
      "path": "/v1/spools/actions/register_spool",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolCommandAnswer"
        },
        {
          "answer": "rejected",
          "type": "SpoolCommandAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:load_spool",
      "image_path_field": null,
      "method": "POST",
      "name": "load_spool",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "spool_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        },
        {
          "kind": "integer",
          "located": "body",
          "name": "tool",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "The tool, in the printer's own numbering.",
            "format": "int64",
            "type": "integer"
          }
        }
      ],
      "path": "/v1/spools/{spool_id}/actions/load_spool",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolCommandAnswer"
        },
        {
          "answer": "rejected",
          "type": "SpoolCommandAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:unload_spool",
      "image_path_field": null,
      "method": "POST",
      "name": "unload_spool",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "spool_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/spools/{spool_id}/actions/unload_spool",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolCommandAnswer"
        },
        {
          "answer": "rejected",
          "type": "SpoolCommandAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:adjust_spool",
      "image_path_field": null,
      "method": "POST",
      "name": "adjust_spool",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "spool_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "gross_g",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "A scale reading of the spool, in grams, which needs its tare.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        },
        {
          "kind": "number",
          "located": "body",
          "name": "remaining_g",
          "repeated": false,
          "required": false,
          "shape": {
            "description": "The filament left, in grams.",
            "format": "double",
            "type": [
              "number",
              "null"
            ]
          }
        }
      ],
      "path": "/v1/spools/{spool_id}/actions/adjust_spool",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolCommandAnswer"
        },
        {
          "answer": "rejected",
          "type": "SpoolCommandAnswer"
        }
      ]
    },
    {
      "accepts": "application/json",
      "answers": "application/json",
      "effect": "mutating:retire_spool",
      "image_path_field": null,
      "method": "POST",
      "name": "retire_spool",
      "parameters": [
        {
          "kind": "text",
          "located": "path",
          "name": "spool_id",
          "repeated": false,
          "required": true,
          "shape": {
            "type": "string"
          }
        },
        {
          "kind": "structured",
          "located": "body",
          "name": "actor",
          "repeated": false,
          "required": true,
          "shape": {
            "$ref": "#/$defs/Actor",
            "description": "Who is asking."
          }
        },
        {
          "kind": "text",
          "located": "body",
          "name": "reason",
          "repeated": false,
          "required": true,
          "shape": {
            "description": "Why the actor is asking for this.",
            "type": "string"
          }
        }
      ],
      "path": "/v1/spools/{spool_id}/actions/retire_spool",
      "responses": [
        {
          "answer": "success",
          "type": "SpoolCommandAnswer"
        },
        {
          "answer": "rejected",
          "type": "SpoolCommandAnswer"
        }
      ]
    }
  ],
  "version_prefix": "/v1"
}
```
