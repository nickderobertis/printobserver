//! The checked-in schemas are what these types generate, and nothing else.
//!
//! This is the generation half of the `printobserver-types:schemas` graph
//! target: run with `PRINTOBSERVER_SCHEMAS=write` it writes every schema this
//! crate declares, and run without it refuses a tree whose checked-in schema no
//! longer matches what the types generate. The same target runs the same test
//! in each of the four port crates, so one target generates every schema in the
//! set and one target refuses any drift in it.

use std::path::{Path, PathBuf};

use printobserver_types::contract::declared;
use serde_json::Value;

/// The repository root, from this crate's own directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Where the schemas one crate declares are checked in.
fn schema_dir(root: &Path, crate_name: &str) -> PathBuf {
    root.join("schemas").join(crate_name)
}

/// The lock the whole checked-in schema tree is read and written under.
///
/// Every test in this file reads that tree, and one journey in
/// `printobserver-oneharness` **writes** to it: it changes the checked-in
/// assessment schema on disk, drives an answer that was accepted before, and
/// puts the artifact back — which is what proves the port reads that artifact at
/// run time rather than validating against a copy of its bytes. The two suites
/// run at the same time, because `nx run-many` drives one project's tests while
/// another's are still going, so both sides take this lock and neither ever sees
/// the other's half-done tree.
///
/// The lock is the operating system's own, so the kernel releases it when the
/// handle goes — a test that panics, or is killed, leaves nothing behind. The
/// file sits under `target`, which is per-worktree and ignored, so two checkouts
/// on one machine never block each other.
///
/// `repo-policy.toml`'s `supervisor.schema_lock` is where the name comes from,
/// and `just check-repo` holds every holder it declares to that one name: two
/// suites that locked two different files would be back to no lock at all.
fn schema_lock() -> std::fs::File {
    let directory = repo_root().join("target");
    std::fs::create_dir_all(&directory).expect("the target directory is writable");
    let file = std::fs::File::create(directory.join("printobserver-schemas.lock"))
        .expect("the schema lock file is creatable");
    file.lock().expect("the schema lock is takeable");
    file
}

/// Whether this run writes the schemas rather than checking them.
fn writing() -> bool {
    std::env::var("PRINTOBSERVER_SCHEMAS").is_ok_and(|mode| mode == "write")
}

/// Write the schemas once, whichever test in this binary runs first.
///
/// The tests of one binary run in parallel, so a test that read the checked-in
/// schemas while another was writing them would read a half-written tree.
fn ensure_written() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if writing() {
            write(
                &schema_dir(&repo_root(), "printobserver-types"),
                &generated(),
            );
        }
    });
}

/// The schemas this crate declares, by the file name each is written under.
fn generated() -> Vec<(String, Value)> {
    declared()
        .into_iter()
        .map(|entry| (format!("{}.json", entry.name), entry.schema()))
        .collect()
}

/// One schema's text, as it is written and as it is compared.
fn text_of(schema: &Value) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("a schema serializes");
    text.push('\n');
    text
}

/// Write every schema, removing any file the declared set no longer names.
fn write(directory: &Path, entries: &[(String, Value)]) {
    std::fs::create_dir_all(directory).expect("the schema directory is writable");
    for existing in std::fs::read_dir(directory).expect("the schema directory is readable") {
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

/// Every way a checked-in schema directory differs from what the types generate.
fn drift(directory: &Path, entries: &[(String, Value)]) -> Vec<String> {
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
    let Ok(listing) = std::fs::read_dir(directory) else {
        findings.push(format!("{} is not a directory", directory.display()));
        return findings;
    };
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
    findings
}

/// The checked-in schemas of this crate are what its types generate.
#[test]
fn the_checked_in_schemas_are_what_the_types_generate() {
    let _lock = schema_lock();
    ensure_written();
    let directory = schema_dir(&repo_root(), "printobserver-types");
    let entries = generated();
    assert!(!entries.is_empty(), "this crate declares no type at all");
    let findings = drift(&directory, &entries);
    assert!(
        findings.is_empty(),
        "the checked-in schemas have drifted:\n{}",
        findings.join("\n")
    );
}

/// The six shapes the port crates own, and the crate each is declared by.
const PORT_OWNED_SHAPES: [(&str, &str); 6] = [
    ("printobserver-vision-api", "NormalizedAlert"),
    ("printobserver-vision-api", "FetchedImage"),
    ("printobserver-supervisor-api", "TurnRequest"),
    ("printobserver-supervisor-api", "TurnOutcome"),
    ("printobserver-store-api", "EventDraft"),
    ("printobserver-store-api", "HistoryQuery"),
];

/// The four port error vocabularies, which cross no process boundary.
const PORT_ERRORS: [(&str, &str); 4] = [
    ("printobserver-printer-api", "PrinterError"),
    ("printobserver-vision-api", "VisionError"),
    ("printobserver-supervisor-api", "SupervisorError"),
    ("printobserver-store-api", "StoreError"),
];

/// Every way a tree falls short of carrying the whole schema set.
///
/// The set is wider than this crate's own declarations, because six of the
/// types that cross a process boundary are the ports' own, so this reads both:
/// a type cannot fall out of the set by being declared in a port crate rather
/// than here, and a port's error vocabulary — which reaches no process boundary
/// — cannot slip into it.
fn schema_set_findings(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for entry in declared() {
        let path = schema_dir(root, "printobserver-types").join(format!("{}.json", entry.name));
        if !path.is_file() {
            findings.push(format!("{} emits no checked-in schema", entry.name));
        }
    }
    for (crate_name, type_name) in PORT_OWNED_SHAPES {
        let path = schema_dir(root, crate_name).join(format!("{type_name}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            findings.push(format!("{type_name} emits no checked-in schema"));
            continue;
        };
        let schema: Value = serde_json::from_str(&text).expect("the schema is JSON");
        if schema.get("title").and_then(Value::as_str) != Some(type_name) {
            findings.push(format!("{} is not {type_name}'s schema", path.display()));
        }
    }
    for (crate_name, error_name) in PORT_ERRORS {
        let path = schema_dir(root, crate_name).join(format!("{error_name}.json"));
        if path.exists() {
            findings.push(format!(
                "{error_name} emits a schema, and a port error reaches no process boundary"
            ));
        }
    }
    findings
}

/// Every type in the schema set has a checked-in schema, the six included.
#[test]
fn every_type_in_the_schema_set_has_a_checked_in_schema() {
    let _lock = schema_lock();
    ensure_written();
    let findings = schema_set_findings(&repo_root());
    assert!(
        findings.is_empty(),
        "the schema set is incomplete:\n{}",
        findings.join("\n")
    );
}

/// A scratch copy of the checked-in schemas, for driving the drift check.
struct ScratchTree {
    /// Where the copy lives.
    root: PathBuf,
}

impl ScratchTree {
    /// Copy the checked-in schemas of the five crates into a scratch tree.
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "printobserver-schema-drift-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (crate_name, _) in PORT_OWNED_SHAPES
            .into_iter()
            .chain([("printobserver-types", "")])
        {
            let from = schema_dir(&repo_root(), crate_name);
            let to = schema_dir(&root, crate_name);
            std::fs::create_dir_all(&to).expect("the scratch tree is writable");
            for entry in std::fs::read_dir(&from).expect("the schema directory is readable") {
                let path = entry.expect("a readable directory entry").path();
                let name = path.file_name().expect("a file name").to_owned();
                std::fs::copy(&path, to.join(name)).expect("a schema is copyable");
            }
        }
        Self { root }
    }

    /// Alter one checked-in schema, as a drifted tree carries.
    fn alter(&self, crate_name: &str, type_name: &str) {
        let path = schema_dir(&self.root, crate_name).join(format!("{type_name}.json"));
        let mut schema: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                .expect("the schema is JSON");
        schema["description"] = Value::String("a description the types do not generate".to_owned());
        std::fs::write(&path, text_of(&schema)).expect("the scratch schema is writable");
    }
}

impl Drop for ScratchTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The drift check accepts the matching tree and refuses the altered one.
///
/// Driven for one of the six shapes a port crate declares as well as for a type
/// this crate declares, so that the check is shown to reach both crates.
#[test]
fn the_drift_check_refuses_an_altered_schema_in_either_crate() {
    let _lock = schema_lock();
    ensure_written();
    let entries = generated();

    let scratch = ScratchTree::new("types");
    assert!(
        drift(&schema_dir(&scratch.root, "printobserver-types"), &entries).is_empty(),
        "the matching tree was refused"
    );
    scratch.alter("printobserver-types", "PrintRecord");
    let findings = drift(&schema_dir(&scratch.root, "printobserver-types"), &entries);
    assert_eq!(
        findings.len(),
        1,
        "the altered type-crate schema was not refused: {findings:?}"
    );
    assert!(findings[0].contains("PrintRecord.json"));

    let port = ScratchTree::new("vision");
    let committed =
        schema_dir(&repo_root(), "printobserver-vision-api").join("NormalizedAlert.json");
    let generated_port: Value =
        serde_json::from_str(&std::fs::read_to_string(&committed).expect("readable"))
            .expect("the schema is JSON");
    let port_entries = vec![("NormalizedAlert.json".to_owned(), generated_port)];
    let port_dir = schema_dir(&port.root, "printobserver-vision-api");
    assert_eq!(
        drift(&port_dir, &port_entries),
        vec![format!(
            "{} is a schema no declared type generates",
            port_dir.join("FetchedImage.json").display()
        )],
        "the matching port tree was refused for the wrong reason"
    );
    port.alter("printobserver-vision-api", "NormalizedAlert");
    let findings = drift(&port_dir, &port_entries);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("NormalizedAlert.json")
                && finding.contains("not what the types generate")),
        "the altered port schema was not refused: {findings:?}"
    );
}

/// The reading refuses a tree in which one of the six emits no schema.
///
/// Driven for one of the six the port crates own, because that is how a type
/// falls out of the set without this crate's own declarations changing.
#[test]
fn the_schema_set_reading_refuses_a_tree_missing_one_of_the_six() {
    let _lock = schema_lock();
    ensure_written();
    let scratch = ScratchTree::new("missing-six");
    assert!(
        schema_set_findings(&scratch.root).is_empty(),
        "the matching tree was refused"
    );
    let removed =
        schema_dir(&scratch.root, "printobserver-vision-api").join("NormalizedAlert.json");
    std::fs::remove_file(&removed).expect("the scratch schema is removable");
    assert_eq!(
        schema_set_findings(&scratch.root),
        vec!["NormalizedAlert emits no checked-in schema".to_owned()]
    );
}
