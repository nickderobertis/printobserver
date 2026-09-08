//! The three shapes the self-hosted Obico plugin sends, committed beside the
//! types that model them.
//!
//! The producer sends three shapes, and a fixture per shape is what the fast
//! vision tier replays and the scheduled Obico tier reconciles a live server
//! against. The third is not an incomplete copy of the second: it is the shape
//! the producer sends when there is no print, and its two absences are the
//! whole reason it is committed.
//!
//! These samples are this repository's *claim* about an external producer.
//! What reconciles that claim against the real one is the scheduled Obico tier,
//! which drives a live self-hosted Obico and fails naming any field that has
//! moved; nothing here can establish that, and nothing here pretends to.

use std::path::PathBuf;

use printobserver_types::contract::schema_of;
use printobserver_types::{ObicoFailureAlert, ObicoPrinterNotification};
use serde_json::Value;

/// Where the committed samples live.
fn sample(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("samples")
        .join("obico")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The failure alert the producer sends.
fn failure_alert() -> Value {
    sample("failure-alert.json")
}

/// The printer notification the producer sends about a print.
fn about_a_print() -> Value {
    sample("printer-notification-about-a-print.json")
}

/// The printer notification the producer sends when there is no print.
fn not_about_a_print() -> Value {
    sample("printer-notification-not-about-a-print.json")
}

/// Parse one value as the failure alert and serialize it back.
fn round_trip_alert(value: Value) -> Result<Value, String> {
    let parsed: ObicoFailureAlert =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    serde_json::to_value(parsed).map_err(|error| error.to_string())
}

/// Parse one value as the printer notification and serialize it back.
fn round_trip_notification(value: Value) -> Result<Value, String> {
    let parsed: ObicoPrinterNotification =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    serde_json::to_value(parsed).map_err(|error| error.to_string())
}

/// Each sample parses into its type and back without loss.
#[test]
fn each_sample_parses_into_its_type_and_back_without_loss() {
    assert_eq!(round_trip_alert(failure_alert()), Ok(failure_alert()));
    assert_eq!(
        round_trip_notification(about_a_print()),
        Ok(about_a_print())
    );
    assert_eq!(
        round_trip_notification(not_about_a_print()),
        Ok(not_about_a_print())
    );
}

/// Each sample satisfies the schema its own type generates.
#[test]
fn each_sample_satisfies_the_schema_its_type_generates() {
    let alert = jsonschema::validator_for(&schema_of::<ObicoFailureAlert>())
        .expect("the alert schema compiles");
    assert!(alert.is_valid(&failure_alert()));

    let notification = jsonschema::validator_for(&schema_of::<ObicoPrinterNotification>())
        .expect("the notification schema compiles");
    assert!(notification.is_valid(&about_a_print()));
    assert!(notification.is_valid(&not_about_a_print()));
}

/// The failure alert carries every optional field its type declares.
///
/// A numeric `started_at` and an empty-string `ended_at`, so both timestamp
/// forms the producer sends appear in the committed corpus.
#[test]
fn the_failure_alert_carries_every_field_and_both_timestamp_forms() {
    let alert = failure_alert();
    assert!(
        alert
            .pointer("/print/started_at")
            .is_some_and(Value::is_number)
    );
    assert_eq!(
        alert.pointer("/print/ended_at").and_then(Value::as_str),
        Some("")
    );
    assert!(alert.pointer("/img_url").is_some_and(Value::is_string));
    assert_eq!(
        alert.pointer("/event/type").and_then(Value::as_str),
        Some("PrintFailure")
    );
}

/// The about-a-print notification carries `print` and `img_url`, and the other
/// carries neither, which is what the producer sends in each form.
#[test]
fn the_two_notifications_differ_in_exactly_the_two_optional_fields() {
    let about = about_a_print();
    assert!(
        about.get("print").is_some(),
        "the about-a-print form carries no print"
    );
    assert!(
        about.get("img_url").is_some(),
        "the about-a-print form carries no img_url"
    );

    let not_about = not_about_a_print();
    assert!(
        not_about.get("print").is_none(),
        "the not-about form carries a print"
    );
    assert!(
        not_about.get("img_url").is_none(),
        "the not-about form carries an img_url"
    );

    let required: Vec<&str> = vec!["event", "printer"];
    for shape in [&about, &not_about] {
        for name in &required {
            assert!(
                shape.get(*name).is_some(),
                "a notification carries no {name}"
            );
        }
    }
}

/// No field of any sample is a placeholder or an empty shape standing in.
#[test]
fn no_sample_carries_a_placeholder() {
    /// The one field the producer legitimately sends as an empty string.
    const DELIBERATELY_EMPTY: [&str; 2] = ["started_at", "ended_at"];

    fn walk(name: &str, value: &Value, path: &str) {
        match value {
            Value::Object(fields) => {
                assert!(
                    !fields.is_empty(),
                    "{path} is an empty object standing in for a shape"
                );
                for (key, held) in fields {
                    walk(key, held, &format!("{path}/{key}"));
                }
            }
            Value::Array(items) => {
                assert!(
                    !items.is_empty(),
                    "{path} is an empty list standing in for a shape"
                );
            }
            Value::String(text) => {
                assert!(
                    !text.is_empty() || DELIBERATELY_EMPTY.contains(&name),
                    "{path} is an empty string standing in for a value"
                );
                assert!(!text.contains("TODO"), "{path} is a placeholder");
            }
            Value::Null => panic!("{path} is null, which stands in for a field the producer sends"),
            Value::Bool(_) | Value::Number(_) => {}
        }
    }

    for (label, shape) in [
        ("failure-alert", failure_alert()),
        ("about-a-print", about_a_print()),
        ("not-about-a-print", not_about_a_print()),
    ] {
        walk(label, &shape, label);
    }
}

/// Which of the producer's three shapes a sample is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// The failure alert.
    FailureAlert,
    /// The printer notification about a print.
    AboutAPrint,
    /// The printer notification that is about no print.
    NotAboutAPrint,
}

/// Every way one sample falls short of what its shape must carry.
///
/// The producer's two notification shapes differ in exactly which optional
/// fields it sends, so what makes a sample complete is stated per shape.
fn completeness_findings(shape: Shape, value: &Value) -> Vec<String> {
    let mut findings = Vec::new();
    let (schema, round_tripped) = match shape {
        Shape::FailureAlert => (
            schema_of::<ObicoFailureAlert>(),
            round_trip_alert(value.clone()),
        ),
        Shape::AboutAPrint | Shape::NotAboutAPrint => (
            schema_of::<ObicoPrinterNotification>(),
            round_trip_notification(value.clone()),
        ),
    };
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    if !validator.is_valid(value) {
        findings.push(format!(
            "{shape:?} omits a field its type declares as required"
        ));
    }
    match round_tripped {
        Err(error) => findings.push(format!("{shape:?} does not parse: {error}")),
        Ok(round) if round != *value => {
            findings.push(format!(
                "{shape:?} carries a field its type does not declare"
            ));
        }
        Ok(_) => {}
    }
    for name in ["print", "img_url"] {
        let carried = value.get(name).is_some();
        match shape {
            Shape::AboutAPrint if !carried => {
                findings.push(format!("the about-a-print form carries no {name}"));
            }
            Shape::NotAboutAPrint if carried => {
                findings.push(format!("the not-about-a-print form carries a {name}"));
            }
            Shape::FailureAlert if !carried => {
                findings.push(format!("the failure alert carries no {name}"));
            }
            _ => {}
        }
    }
    findings
}

/// Each committed sample is complete for the shape it is.
#[test]
fn each_committed_sample_is_complete_for_its_shape() {
    for (shape, value) in [
        (Shape::FailureAlert, failure_alert()),
        (Shape::AboutAPrint, about_a_print()),
        (Shape::NotAboutAPrint, not_about_a_print()),
    ] {
        let findings = completeness_findings(shape, &value);
        assert!(findings.is_empty(), "{shape:?}: {}", findings.join("; "));
    }
}

/// The completeness check refuses each fixture the contract names.
#[test]
fn the_completeness_check_refuses_every_fixture_it_must() {
    let mut without_required = failure_alert();
    without_required
        .as_object_mut()
        .expect("an object")
        .remove("printer")
        .expect("the sample carries a printer");
    assert!(
        !completeness_findings(Shape::FailureAlert, &without_required).is_empty(),
        "a sample omitting a field its type declares as required was accepted"
    );

    let mut with_undeclared = failure_alert();
    with_undeclared["obico_version"] = Value::String("2.0".to_owned());
    assert!(
        !completeness_findings(Shape::FailureAlert, &with_undeclared).is_empty(),
        "a sample carrying a field its type does not declare was accepted"
    );

    for missing in ["print", "img_url"] {
        let mut incomplete = about_a_print();
        incomplete
            .as_object_mut()
            .expect("an object")
            .remove(missing);
        assert!(
            !completeness_findings(Shape::AboutAPrint, &incomplete).is_empty(),
            "the about-a-print form was accepted without {missing}"
        );
    }

    for extra in ["print", "img_url"] {
        let mut overfull = not_about_a_print();
        overfull[extra] = about_a_print()[extra].clone();
        assert!(
            !completeness_findings(Shape::NotAboutAPrint, &overfull).is_empty(),
            "the not-about-a-print form was accepted carrying a {extra}"
        );
    }
}
