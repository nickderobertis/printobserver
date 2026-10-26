//! Events written under an earlier payload shape, read into the one their kind
//! carries now.
//!
//! The log is never rewritten: a row keeps the text it was written with, and a
//! store reads it through [`upgraded`] so every reader — the history, the
//! timeline and this crate's own — sees one shape per kind. Each earlier shape
//! is recognised by a field only it carries, and a payload that is not one of
//! them, or that one of them cannot be read into whole, is answered as it was
//! stored rather than refused or guessed at: the open log still holds it.
//!
//! The shapes read here are the ones printobserver 0.4.3 wrote:
//!
//! * `filament_consumed` with the charge's spool, grams, diameter, density and
//!   figures as independent fields, now one `charged` calculation;
//! * `filament_change_signalled` with `position_read_at_signal` beside its
//!   position, now one `position` carrying both;
//! * `spool_registered` whose spool carries `loaded_on_tool`, now a
//!   `lifecycle`;
//! * `charge_pended` whose pending charge repeats its print and tool beside a
//!   tagged key, now a print-segment key alone;
//! * `preflight_checked` and `print_started` whose filament check carries each
//!   tool's figures as independent fields beside its verdict, now one `result`;
//! * `spool_command_rejected` whose refusal is a sentence alone, now a tagged
//!   `cause` beside it, read from the sentence 0.4.3 wrote for each cause.

use printobserver_types::EventBody;
use printobserver_types::serde_json::{Map, Value, json};

use crate::filament::DEFAULT_DIAMETER_MM;

/// One stored event, read into the payload shape its kind carries now.
#[must_use]
pub fn upgraded(mut body: EventBody) -> EventBody {
    let payload = core::mem::take(&mut body.payload);
    body.payload = match body.kind.as_str() {
        "filament_consumed" => filament_consumed(payload),
        "filament_change_signalled" => filament_change_signalled(payload),
        "spool_registered" => at_field(payload, "spool", spool),
        "charge_pended" => at_field(payload, "pending", pending),
        "preflight_checked" => at_field(payload, "preflight", preflight),
        "print_started" => at_field(payload, "preflight", preflight),
        "spool_command_rejected" => spool_command_rejected(payload),
        _ => payload,
    };
    body
}

/// A payload whose one field is read by `read`, or the payload as it was.
fn at_field(mut payload: Value, field: &str, read: fn(Value) -> Option<Value>) -> Value {
    if let Some(slot) = payload.get_mut(field)
        && let Some(read) = read(slot.clone())
    {
        *slot = read;
    }
    payload
}

/// Remove each of `fields` from an object, answering what each held.
fn taken(object: &mut Map<String, Value>, fields: &[&str]) -> Vec<Option<Value>> {
    fields.iter().map(|field| object.remove(*field)).collect()
}

/// A `filament_consumed` with its calculation in one `charged`.
fn filament_consumed(payload: Value) -> Value {
    let Value::Object(mut object) = payload.clone() else {
        return payload;
    };
    if object.contains_key("charged") {
        return payload;
    }
    let figures = taken(
        &mut object,
        &[
            "spool_id",
            "grams",
            "diameter_mm",
            "density_g_cm3",
            "remaining_before_g",
            "remaining_after_g",
        ],
    );
    match figures.as_slice() {
        [None, ..] => {}
        [
            Some(spool_id),
            Some(grams),
            Some(diameter_mm),
            Some(density_g_cm3),
            Some(remaining_before_g),
            Some(remaining_after_g),
        ] => {
            object.insert(
                "charged".to_owned(),
                json!({
                    "spool_id": spool_id,
                    "grams": grams,
                    "diameter_mm": diameter_mm,
                    "density_g_cm3": density_g_cm3,
                    "remaining_before_g": remaining_before_g,
                    "remaining_after_g": remaining_after_g,
                }),
            );
        }
        // A spool named with half a calculation is no charge this shape can
        // hold, so the row is answered as it was written.
        _ => return payload,
    }
    Value::Object(object)
}

/// A `filament_change_signalled` whose position carries its own freshness.
fn filament_change_signalled(payload: Value) -> Value {
    let Value::Object(mut object) = payload.clone() else {
        return payload;
    };
    let Some(read_at_signal) = object.remove("position_read_at_signal") else {
        return payload;
    };
    if let Some(position) = object.remove("position") {
        object.insert(
            "position".to_owned(),
            json!({ "position": position, "read_at_signal": read_at_signal }),
        );
    }
    Value::Object(object)
}

/// A spool record with a `lifecycle` in place of its loaded tool and
/// retirement.
fn spool(record: Value) -> Option<Value> {
    let Value::Object(mut object) = record else {
        return None;
    };
    if object.contains_key("lifecycle") {
        return None;
    }
    let loaded = object.remove("loaded_on_tool");
    let lifecycle = match (object.remove("retired_at"), loaded) {
        (Some(at), _) => json!({ "state": "retired", "at": at }),
        (None, Some(tool)) => json!({ "state": "loaded", "tool": tool }),
        (None, None) => json!({ "state": "shelved" }),
    };
    object.insert("lifecycle".to_owned(), lifecycle);
    Some(Value::Object(object))
}

/// A pending charge keyed by its print segment alone, when the print and tool
/// it repeated are the ones its key names.
fn pending(held: Value) -> Option<Value> {
    let Value::Object(mut object) = held else {
        return None;
    };
    let Some(Value::Object(mut key)) = object.remove("charge_key") else {
        return None;
    };
    if key.remove("key")? != json!("print") {
        return None;
    }
    let print_id = object.remove("print_id")?;
    let tool = object.remove("tool")?;
    if key.get("print_id") != Some(&print_id) || key.get("tool") != Some(&tool) {
        return None;
    }
    object.insert("charge_key".to_owned(), Value::Object(key));
    Some(Value::Object(object))
}

/// A filament check whose every tool carries one tagged `result`.
fn preflight(check: Value) -> Option<Value> {
    let Value::Object(mut object) = check else {
        return None;
    };
    let Some(Value::Array(tools)) = object.remove("tools") else {
        return None;
    };
    let tools = tools
        .into_iter()
        .map(preflight_tool)
        .collect::<Option<_>>()?;
    object.insert("tools".to_owned(), Value::Array(tools));
    Some(Value::Object(object))
}

/// One tool of a filament check, its figures folded into its verdict's
/// result.
fn preflight_tool(tool: Value) -> Option<Value> {
    let Value::Object(mut object) = tool else {
        return None;
    };
    if object.contains_key("result") {
        return Some(Value::Object(object));
    }
    let verdict = object.remove("verdict")?;
    let [
        spool_id,
        spool_name,
        required_g,
        buffer_g,
        remaining_g,
        marks,
    ] = <[_; 6]>::try_from(taken(
        &mut object,
        &[
            "spool_id",
            "spool_name",
            "required_g",
            "buffer_g",
            "remaining_g",
            "marks",
        ],
    ))
    .ok()?;
    let marks = marks.unwrap_or_else(|| json!([]));
    let held = |name: &str| -> Option<Map<String, Value>> {
        let mut found = Map::new();
        found.insert(
            "spool".to_owned(),
            json!({ "id": spool_id.clone()?, "name": spool_name.clone()? }),
        );
        found.insert("remaining_g".to_owned(), remaining_g.clone()?);
        found.insert("verdict".to_owned(), json!(name));
        Some(found)
    };
    let need = match (&required_g, &buffer_g) {
        (Some(required_g), Some(buffer_g)) => {
            Some(json!({ "required_g": required_g, "buffer_g": buffer_g }))
        }
        _ => None,
    };
    let marked = marks.as_array().is_some_and(|marks| !marks.is_empty());
    let objection = |name: &str| -> Option<Value> {
        let mut found = held(name)?;
        match name {
            "insufficient" => {
                found.insert("need".to_owned(), need.clone()?);
                if marked {
                    found.insert("marks".to_owned(), marks.clone());
                }
            }
            "unverified_spool" => {
                if let Some(need) = &need {
                    found.insert("need".to_owned(), need.clone());
                }
                found.insert("marks".to_owned(), marks.clone());
            }
            _ => {}
        }
        Some(Value::Object(found))
    };
    let result = match verdict.as_str()? {
        "unchecked" => json!({ "verdict": "unchecked" }),
        "passed" => {
            let mut found = held("passed")?;
            found.insert("need".to_owned(), need.clone()?);
            Value::Object(found)
        }
        name @ ("insufficient" | "unverified_spool" | "no_analysis") => objection(name)?,
        name @ ("overridden" | "warned") => {
            // A check let through kept the figures and marks it objected on,
            // and the rule it objected by: short before doubted before
            // unanalysed.
            let short = match (&required_g, &buffer_g, &remaining_g) {
                (Some(required), Some(buffer), Some(remaining)) => {
                    remaining.as_f64()? < required.as_f64()? + buffer.as_f64()?
                }
                _ => false,
            };
            let objected = if short {
                "insufficient"
            } else if marked {
                "unverified_spool"
            } else {
                "no_analysis"
            };
            json!({ "verdict": name, "objection": objection(objected)? })
        }
        _ => return None,
    };
    object.insert("result".to_owned(), result);
    Some(Value::Object(object))
}

/// A `spool_command_rejected` whose spool refusal carries its cause.
fn spool_command_rejected(payload: Value) -> Value {
    let read = || -> Option<Value> {
        let mut upgraded = payload.clone();
        let command = payload.get("command")?;
        let refused = upgraded
            .get_mut("decision")?
            .get_mut("rejected")?
            .get_mut("spool_refused")?
            .as_object_mut()?;
        let detail = refused.remove("detail")?;
        let cause = refusal_cause(detail.as_str()?, command)?;
        refused.insert("cause".to_owned(), cause);
        refused.insert("message".to_owned(), detail);
        Some(upgraded)
    };
    read().unwrap_or(payload)
}

/// The cause a refusal sentence of 0.4.3's names, read with the command it
/// refused.
fn refusal_cause(detail: &str, command: &Value) -> Option<Value> {
    let field = |name: &str| command.get(name).cloned();
    let cause = |name: &str| json!({ "cause": name });
    let with = |name: &str, key: &str, value: Option<Value>| -> Option<Value> {
        let mut found = cause(name);
        found
            .as_object_mut()?
            .insert(key.to_owned(), value.filter(|value| !value.is_null())?);
        Some(found)
    };
    let figure = |name: &str, value: Option<Value>| -> Option<Value> {
        let mut found = with("invalid_adjustment_figure", "value", value)?;
        found
            .as_object_mut()?
            .insert("field".to_owned(), json!(name));
        Some(found)
    };
    let spool_id = || field("spool_id");
    if detail.starts_with("there is no spool ") {
        return with("unknown_spool", "spool_id", spool_id());
    }
    if detail.contains(" is retired and takes no further command") {
        return with("retired_spool", "spool_id", spool_id());
    }
    if detail.contains("nothing is pending there to confirm") {
        return with("nothing_to_confirm", "tool", field("tool"));
    }
    if detail.contains(" is not loaded on any tool") {
        return with("not_loaded", "spool_id", spool_id());
    }
    if detail.starts_with("there is no tool ") {
        return with("unknown_tool", "tool", field("tool"));
    }
    if detail.contains(" has no tare") {
        return with("tare_required", "spool_id", spool_id());
    }
    if detail.starts_with("a correction names exactly one of") {
        let both = field("remaining_g").is_some() && field("gross_g").is_some();
        return Some(if both {
            json!({ "cause": "conflicting_fields", "fields": ["remaining_g", "gross_g"] })
        } else {
            json!({ "cause": "missing_field", "field": "remaining_g" })
        });
    }
    if detail == "the figure given is not a weight in grams" {
        return match field("gross_g") {
            Some(gross) => figure("gross_g", Some(gross)),
            None => figure("remaining_g", field("remaining_g")),
        };
    }
    if detail == "a spool needs a name to be known by" {
        return Some(json!({ "cause": "missing_field", "field": "name" }));
    }
    if detail == "a spool needs its material, such as PLA" {
        return Some(json!({ "cause": "missing_field", "field": "material" }));
    }
    if detail.contains(" has no known density") {
        let material = field("material")?.as_str()?.trim().to_owned();
        return Some(json!({ "cause": "density_required", "material": material }));
    }
    if detail.starts_with("a diameter of ") {
        return figure(
            "diameter_mm",
            Some(field("diameter_mm").unwrap_or_else(|| json!(DEFAULT_DIAMETER_MM))),
        );
    }
    if detail.starts_with("a density of ") {
        return figure("density_g_cm3", field("density_g_cm3"));
    }
    if detail.starts_with("a full spool of ") {
        return figure("initial_g", field("initial_g"));
    }
    if detail == "the remaining figure is not a number" {
        return figure("remaining_g", field("remaining_g"));
    }
    if detail == "an empty spool's weight is at least zero grams" {
        return figure("tare_g", field("tare_g"));
    }
    if detail.starts_with("the ledger refused the change on ") {
        return Some(cause("ledger_refused"));
    }
    None
}
