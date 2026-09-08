//! The action vocabulary is closed, and it has no escape hatch.
//!
//! Closed means two things here, and both are held at the type and at the wire
//! rather than at how the declaration is spelled: the variants are exactly the
//! ones the contract names, and the generated schema admits no tag outside
//! them. Nothing here reads the source's own text, because a check on spelling
//! can fail correct work over a comment while adding nothing the type and the
//! schema do not already carry.
//!
//! The three source forms a spelling check would have looked for are reached
//! here instead, or are nothing: a reserved discriminant **is** a variant, so
//! the walk over the declared variants refuses it for being one the contract
//! does not name; a catch-all is a variant carrying an unnamed payload, refused
//! by that same walk and by the schema's refusal of an undeclared tag; and a
//! commented-out arm is not a variant at all, changing neither the declared
//! type nor a byte on the wire.

use printobserver_types::contract::{TypeContract, declared, schema_of};
use printobserver_types::{ActionKind, PrintAction};
use serde_json::{Value, json};

/// Every variant the contract names, in the spelling the wire carries.
const STATED_VARIANTS: [&str; 10] = [
    "pause",
    "resume",
    "cancel",
    "start_print",
    "set_feedrate_factor",
    "set_flowrate_factor",
    "set_tool_target_c",
    "set_bed_target_c",
    "set_fan_percent",
    "acknowledge_failure",
];

/// The one free-text field the vocabulary declares, and the tag beside it.
const PERMITTED_TEXT_FIELDS: [&str; 2] = ["reason", "action"];

/// The contract for the action vocabulary.
fn action_contract() -> TypeContract {
    declared()
        .into_iter()
        .find(|entry| entry.name == "PrintAction")
        .expect("PrintAction is declared")
}

/// The arms one union schema declares.
fn arms(schema: &Value) -> Vec<Value> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("the vocabulary is a union")
        .clone()
}

/// The tag one arm declares.
fn tag_of(arm: &Value) -> String {
    arm.get("properties")
        .and_then(|properties| properties.get("action"))
        .and_then(|tag| tag.get("const"))
        .and_then(Value::as_str)
        .expect("every arm declares its tag")
        .to_owned()
}

/// The variants the generated vocabulary declares, in declaration order.
fn declared_variants(schema: &Value) -> Vec<String> {
    arms(schema).iter().map(tag_of).collect()
}

/// The vocabulary declares exactly the variants the contract names.
#[test]
fn the_vocabulary_declares_exactly_the_stated_variants() {
    assert_eq!(
        declared_variants(&schema_of::<PrintAction>()),
        STATED_VARIANTS.to_vec()
    );
}

/// The kind vocabulary names the same ten, so nothing can name a sixth policy.
#[test]
fn the_kind_vocabulary_names_the_same_ten() {
    let kinds: Vec<String> = arms(&schema_of::<ActionKind>())
        .iter()
        .map(|arm| {
            arm.get("const")
                .and_then(Value::as_str)
                .expect("a kind is a constant")
                .to_owned()
        })
        .collect();
    assert_eq!(kinds, STATED_VARIANTS.to_vec());
}

/// A fixture vocabulary carrying a reserved variant.
fn fixture_with_reserved() -> Value {
    let mut schema = schema_of::<PrintAction>();
    let mut declared = arms(&schema);
    declared.push(json!({
        "type": "object",
        "properties": { "action": { "const": "reserved", "type": "string" } },
        "required": ["action"]
    }));
    schema["oneOf"] = Value::Array(declared);
    schema
}

/// A fixture vocabulary carrying a catch-all variant.
fn fixture_with_catch_all() -> Value {
    let mut schema = schema_of::<PrintAction>();
    let mut declared = arms(&schema);
    declared.push(json!({
        "type": "object",
        "properties": {
            "action": { "const": "other", "type": "string" },
            "payload": { "type": "object" }
        },
        "required": ["action", "payload"]
    }));
    schema["oneOf"] = Value::Array(declared);
    schema
}

/// The walk refuses a vocabulary carrying a variant the contract does not name.
#[test]
fn the_variant_walk_refuses_a_reserved_or_catch_all_variant() {
    for (label, fixture) in [
        ("reserved", fixture_with_reserved()),
        ("catch-all", fixture_with_catch_all()),
    ] {
        let variants = declared_variants(&fixture);
        assert_ne!(
            variants,
            STATED_VARIANTS.to_vec(),
            "the {label} variant was not seen"
        );
        assert_eq!(variants.len(), STATED_VARIANTS.len() + 1);
    }
}

/// The text-typed fields one arm declares, beyond the ones permitted.
fn offending_text_fields(arm: &Value) -> Vec<String> {
    let Some(Value::Object(properties)) = arm.get("properties") else {
        return Vec::new();
    };
    properties
        .iter()
        .filter(|(name, _)| !PERMITTED_TEXT_FIELDS.contains(&name.as_str()))
        .filter(|(_, property)| {
            let bare_string = property.get("type") == Some(&Value::from("string"));
            let bytes = property.get("$ref") == Some(&Value::from("#/$defs/RawBytes"));
            bare_string || bytes
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// No variant declares a string or byte-sequence field beyond the one `reason`.
///
/// So a G-code field, a command field or any other free-form instruction is
/// refused by the name it appears under rather than by reading its purpose. The
/// start-a-print variant's file name is the validated newtype rather than a
/// string, and is not reached by this walk for that reason.
#[test]
fn no_variant_declares_a_free_form_text_field() {
    let schema = schema_of::<PrintAction>();
    for arm in arms(&schema) {
        let offending = offending_text_fields(&arm);
        assert!(
            offending.is_empty(),
            "the {} arm declares {offending:?}",
            tag_of(&arm)
        );
    }
    let start = arms(&schema)
        .into_iter()
        .find(|arm| tag_of(arm) == "start_print")
        .expect("the vocabulary declares start_print");
    assert_eq!(
        start
            .pointer("/properties/file_name/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/FileName"),
        "the file name is not the validated newtype"
    );
}

/// A fixture arm carrying a G-code field is refused by the same walk.
#[test]
fn the_text_field_walk_refuses_a_fixture_carrying_an_instruction() {
    let arm = json!({
        "type": "object",
        "properties": {
            "action": { "const": "run_gcode", "type": "string" },
            "gcode": { "type": "string" },
            "reason": { "type": "string" }
        }
    });
    assert_eq!(offending_text_fields(&arm), vec!["gcode".to_owned()]);

    let arm = json!({
        "type": "object",
        "properties": {
            "action": { "const": "send", "type": "string" },
            "body": { "$ref": "#/$defs/RawBytes" }
        }
    });
    assert_eq!(offending_text_fields(&arm), vec!["body".to_owned()]);
}

/// Every declared variant round-trips, and the corpus covers all ten.
#[test]
fn every_declared_variant_round_trips() {
    let entry = action_contract();
    let mut seen: Vec<String> = Vec::new();
    for value in entry.samples() {
        let round = entry
            .round_trip(value.clone())
            .unwrap_or_else(|error| panic!("{value}: {error}"));
        assert_eq!(round, value);
        seen.push(
            value
                .get("action")
                .and_then(Value::as_str)
                .expect("an action carries its tag")
                .to_owned(),
        );
    }
    seen.sort();
    seen.dedup();
    let mut stated = STATED_VARIANTS.to_vec();
    stated.sort_unstable();
    assert_eq!(
        seen, stated,
        "the corpus does not drive every declared variant"
    );
}

/// The values the generated schema must refuse, beside the ones it must admit.
fn refused_corpus(schema: &Value, admitted: &[Value]) -> Vec<Value> {
    let mut refused = Vec::new();
    for value in admitted {
        let mut extra = value.clone();
        extra["gcode"] = Value::String("G28".to_owned());
        refused.push(extra);

        let mut wrong = value.clone();
        wrong["reason"] = json!(17);
        refused.push(wrong);
    }
    let neighbour = |tag: &str| {
        json!({
            "action": tag,
            "reason": "the chamber needs clearing",
            "actor": { "agent": { "session_name": "print-0191f0a0" } }
        })
    };
    refused.push(neighbour("Filtration"));
    refused.push(neighbour("SetFiltration"));
    refused.push(neighbour("set_filtration"));
    refused.push(json!({
        "action": "run_gcode",
        "gcode": "G28",
        "reason": "home the printer",
        "actor": { "operator": null }
    }));
    refused.push(json!({
        "action": "run_command",
        "command": "M112",
        "reason": "stop the printer",
        "actor": "operator"
    }));
    assert!(schema.is_object(), "the schema is an object");
    refused
}

/// The generated schema admits each declared variant and refuses the rest.
#[test]
fn the_generated_schema_admits_the_vocabulary_and_nothing_else() {
    let schema = schema_of::<PrintAction>();
    let validator = jsonschema::validator_for(&schema).expect("the generated schema compiles");
    let entry = action_contract();
    let admitted = entry.samples();
    assert_eq!(
        admitted.len(),
        STATED_VARIANTS.len() + 1,
        "the corpus does not carry one value per declared variant"
    );
    for value in &admitted {
        assert!(validator.is_valid(value), "the schema refused {value}");
    }
    for value in refused_corpus(&schema, &admitted) {
        assert!(!validator.is_valid(&value), "the schema admitted {value}");
    }
}

/// Every declared type's schema admits its own canonical values.
///
/// The schemas are generated from the types, so a value one type emits is a
/// value its own schema admits; a schema that refused it would be a schema
/// transcribed rather than generated.
#[test]
fn every_generated_schema_admits_its_own_canonical_values() {
    for entry in declared() {
        let schema = entry.schema();
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|error| panic!("{}'s schema does not compile: {error}", entry.name));
        for value in entry.samples() {
            assert!(
                validator.is_valid(&value),
                "{}'s schema refused {value}",
                entry.name
            );
        }
    }
}
