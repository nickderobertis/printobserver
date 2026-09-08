//! Writing and checking the schemas one crate declares.
//!
//! The `printobserver-types:schemas` graph target runs the `schemas` test of
//! every crate in the schema set: with `PRINTOBSERVER_SCHEMAS=write` it writes
//! them, and without it refuses a tree whose checked-in schema no longer
//! matches what the types generate.

use std::path::PathBuf;

use printobserver_types::serde_json::{self, Value};

/// Where the schemas one crate declares are checked in.
pub fn schema_dir(crate_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join(crate_name)
}

/// One schema's text, as it is written and as it is compared.
fn text_of(schema: &Value) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("a schema serializes");
    text.push('\n');
    text
}

/// Write, or check, the schemas one crate declares.
///
/// Answers every way the checked-in directory differs from what the types
/// generate: a schema that is absent, one that has drifted, and a file no
/// declared type generates.
pub fn reconcile(crate_name: &str, entries: &[(String, Value)]) -> Vec<String> {
    let directory = schema_dir(crate_name);
    if std::env::var("PRINTOBSERVER_SCHEMAS").is_ok_and(|mode| mode == "write") {
        std::fs::create_dir_all(&directory).expect("the schema directory is writable");
        for existing in std::fs::read_dir(&directory).expect("the schema directory is readable") {
            let path = existing.expect("a readable directory entry").path();
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            if !entries.iter().any(|(file, _)| *file == name) {
                std::fs::remove_file(&path).expect("a stale schema is removable");
            }
        }
        for (file, schema) in entries {
            std::fs::write(directory.join(file), text_of(schema)).expect("a schema is writable");
        }
    }

    let mut findings = Vec::new();
    for (file, schema) in entries {
        let path = directory.join(file);
        match std::fs::read_to_string(&path) {
            Err(error) => findings.push(format!("{}: {error}", path.display())),
            Ok(found) if found != text_of(schema) => {
                findings.push(format!("{} is not what the types generate", path.display()));
            }
            Ok(_) => {}
        }
    }
    match std::fs::read_dir(&directory) {
        Err(error) => findings.push(format!("{}: {error}", directory.display())),
        Ok(listing) => {
            for existing in listing {
                let path = existing.expect("a readable directory entry").path();
                let name = path
                    .file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned();
                if !entries.iter().any(|(file, _)| *file == name) {
                    findings.push(format!(
                        "{} is a schema no declared type generates",
                        path.display()
                    ));
                }
            }
        }
    }
    findings
}
