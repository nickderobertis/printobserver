//! The type crate declares what is its own, and nothing that is another's.
//!
//! Two readings, both off this crate's own sources rather than a table
//! maintained beside them. The first holds the declared set to the sources:
//! every type that can emit a schema is registered, so none falls silently out
//! of the schema set. The second holds the sources to the rest of the schema
//! tree: no type this crate declares is one another crate checks a schema in
//! for — which is how a type that moved to its owning domain stays moved, and
//! how a domain's type cannot quietly reappear here. The set of other crates'
//! types is read off `schemas/<crate>/` rather than listed, because a list of
//! other domains' types is exactly the thing a contract crate must not hold.
//!
//! The four port crates' own surfaces — which methods each trait declares,
//! which variants each error carries, what a parameter's closure reaches —
//! are each port's to assert and live in that port's `surface` test, over the
//! same reader.

#[path = "support/surface.rs"]
pub mod surface;

use std::path::PathBuf;

use surface::{crate_sources, declared_type_names, parse, schema_emitting_types};

/// The checked-in schema tree.
fn schema_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
}

/// Every type name another crate checks a schema in for, with that crate.
fn types_other_crates_check_in() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for directory in std::fs::read_dir(schema_root()).expect("the schema tree is readable") {
        let directory = directory.expect("a readable directory entry").path();
        let crate_name = directory
            .file_name()
            .expect("a directory name")
            .to_string_lossy()
            .into_owned();
        if crate_name == "printobserver-types" || !directory.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&directory).expect("a schema directory is readable") {
            let path = file.expect("a readable directory entry").path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
                && let Some(stem) = path.file_stem()
            {
                found.push((stem.to_string_lossy().into_owned(), crate_name.clone()));
            }
        }
    }
    found.sort();
    found
}

/// Every declared name that another crate checks a schema in for.
fn claimed_elsewhere(declared: &[String], elsewhere: &[(String, String)]) -> Vec<String> {
    elsewhere
        .iter()
        .filter(|(name, _)| declared.contains(name))
        .map(|(name, crate_name)| format!("{name} (checked in by {crate_name})"))
        .collect()
}

/// The type crate declares no type another crate checks a schema in for.
#[test]
fn the_type_crate_declares_no_type_another_crate_checks_in() {
    let declared: Vec<String> = crate_sources("printobserver-types")
        .iter()
        .flat_map(declared_type_names)
        .collect();
    assert!(
        declared.iter().any(|name| name == "EventRecord"),
        "the reader found no declarations"
    );
    let elsewhere = types_other_crates_check_in();
    assert!(
        elsewhere.len() > 1,
        "the reader found no other crate's schemas"
    );
    let claimed = claimed_elsewhere(&declared, &elsewhere);
    assert!(
        claimed.is_empty(),
        "the type crate declares types the crate that owns them checks in: {claimed:?}"
    );
}

/// The ownership reading refuses a crate declaring another crate's type.
///
/// The fixture declares whichever type another crate checks in first, read
/// off the tree rather than named here, so this test privileges no domain.
#[test]
fn the_ownership_reading_refuses_a_crate_declaring_another_crates_type() {
    let elsewhere = types_other_crates_check_in();
    let (name, crate_name) = elsewhere.first().expect("another crate checks a schema in");
    let fixture = format!("pub struct {name} {{\n    pub field: String,\n}}\n");
    let declared = declared_type_names(&parse(&fixture));
    assert_eq!(
        claimed_elsewhere(&declared, &elsewhere),
        vec![format!("{name} (checked in by {crate_name})")],
        "a declaration of another crate's type was not seen"
    );
}

/// Every type that can emit a schema is in the declared set, and no other.
///
/// A type that derives `JsonSchema` and is not registered would fall silently
/// out of the schema set and out of every corpus these tests walk, so the set
/// is read off the declarations rather than trusted to a list.
#[test]
fn the_declared_set_holds_every_type_that_can_emit_a_schema() {
    let mut emitting: Vec<String> = crate_sources("printobserver-types")
        .iter()
        .flat_map(schema_emitting_types)
        .collect();
    emitting.sort();
    emitting.dedup();

    let mut registered: Vec<String> = printobserver_types::contract::declared()
        .into_iter()
        .map(|entry| entry.name.to_owned())
        .collect();
    registered.sort();

    assert_eq!(emitting, registered);
}
