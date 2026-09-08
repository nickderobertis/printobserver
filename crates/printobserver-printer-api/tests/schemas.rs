//! This port declares no type that crosses a process boundary.
//!
//! The `printobserver-types:schemas` graph target runs this test in every crate
//! of the schema set, and this crate's part of that set is empty: it declares
//! one trait and one error vocabulary, and an error reaches a client as the
//! server's own declared error shape rather than as a port type. So this crate
//! emits no schema, and a checked-in schema under its name is drift in the
//! other direction.

use std::path::PathBuf;

/// Where a schema of this crate would be checked in.
fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("printobserver-printer-api")
}

/// This crate emits no schema, so it has no checked-in schema directory.
#[test]
fn this_port_emits_no_schema() {
    let directory = schema_dir();
    assert!(
        !directory.exists(),
        "{} exists, and this port declares no type that crosses a process boundary",
        directory.display()
    );
}
