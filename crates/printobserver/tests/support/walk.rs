//! The cross-product every client command is driven over.
//!
//! # What the product is over, and why it is finite
//!
//! For each client command: every mutually exclusive form of each of its own
//! values, crossed with machine-readable output present and absent, crossed
//! with the configuration file given explicitly and left to the environment.
//! Those two are the global options that do not terminate the program; the
//! other two terminate and are driven on their own.
//!
//! It is finite and small because the surface is closed: the forms come from
//! the command's own fields, and the globals are fixed at four. A command that
//! grew an option or a form would grow this product rather than escape it, and
//! [`covers`] is what refuses a surface it does not cover.
//!
//! # The values are distinct per command
//!
//! No two commands drive the same reason or the same payload value, so a
//! command that substituted a fixed value of its own — or sent one belonging to
//! its neighbour — fails on the request the server received and again on the
//! record read back. Within one command they are the **same** across forms,
//! which is what makes the forms interchangeable rather than merely each
//! individually functional.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use printobserver::surface::{CONFIG_OPTION, JSON_OPTION, Supply, surface};
use printobserver_server::{Located, ValueKind};
use printobserver_types::serde_json::{Value, json};

use crate::machine::Reports;
use crate::world::{CREDENTIAL, World};

/// One command of the walk: what it is, what it drives, and what the machine
/// has to be doing for it.
#[derive(Debug, Clone)]
pub struct Driven {
    /// The command, as the program's own surface declares it.
    pub command: printobserver::surface::Command,
    /// Every value it drives, by the name the request carries it as.
    pub values: BTreeMap<String, String>,
    /// What the machine reports before it is asked.
    pub reports: Reports,
}

impl Driven {
    /// The operation it calls.
    ///
    /// # Panics
    ///
    /// Panics for a command that calls none, which the walk holds none of.
    pub fn operation(&self) -> printobserver_server::Operation {
        self.command.operation.expect("a client command")
    }

    /// The value it drives for one field, as the caller typed it.
    pub fn text(&self, field: &str) -> String {
        self.values
            .get(field)
            .unwrap_or_else(|| panic!("`{}` drives no `{field}`", self.command.name))
            .clone()
    }
}

/// One invocation of one command.
#[derive(Debug, Clone)]
pub struct Invocation {
    /// The arguments, in the order they are given.
    pub arguments: Vec<String>,
    /// The environment it runs under.
    pub environment: Vec<(String, String)>,
    /// Whether it asked for machine-readable output.
    pub machine_readable: bool,
    /// Whether it named a configuration file.
    pub explicit_config: bool,
    /// Every option it gave, which is what the coverage check reads.
    pub options: BTreeSet<String>,
}

/// Every client command, in the one order a single print admits.
///
/// Pausing is valid from printing and resuming from paused, so the order is
/// what a print actually goes through; and the manifest read comes after the
/// write, because a start attaches a manifest of its own.
pub fn walk(world: &World) -> Vec<Driven> {
    let print = world.print_id.clone();
    let mut driven = Vec::new();
    for (name, reports, values) in ordered(world) {
        let command = printobserver::surface::command(&name)
            .unwrap_or_else(|| panic!("this program has a `{name}` command"));
        let mut carried: BTreeMap<String, String> = values.into_iter().collect();
        for field in &command.fields {
            if field.parameter.located == Located::Path && field.parameter.name == "print_id" {
                carried.insert(field.parameter.name.clone(), print.clone());
            }
        }
        driven.push(Driven {
            command,
            values: carried,
            reports,
        });
    }
    driven
}

/// The file this world's printer can be asked for.
fn printable(world: &World) -> String {
    world.printable_file()
}

/// The reason each command gives, distinct from every other command's.
pub fn reason_of(command: &str) -> String {
    format!("this walk is asking `{command}` for exactly this")
}

/// The manifest a command that carries one drives.
///
/// The slicer profile names the command that drove it, so a manifest that
/// arrived from a neighbouring command is one this walk can see.
pub fn manifest_of(command: &str, feedrate_max: f64, file: &str) -> Value {
    json!({
        "file_name": file,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": format!("the profile `{command}` drove"),
        "allowed": { "feedrate": { "min": 0.9, "max": feedrate_max } },
        "metadata": {},
    })
}

/// Every command of the walk, with the values distinct to it.
fn ordered(world: &World) -> Vec<Entry> {
    let mut found = vec![
        named("status", Reports::Printing, vec![]),
        named("context", Reports::Printing, vec![]),
        named(
            "image",
            Reports::Printing,
            vec![("image_id".to_owned(), world.image_id.clone())],
        ),
        named(
            "history",
            Reports::Printing,
            vec![("limit".to_owned(), "7".to_owned())],
        ),
    ];
    found.extend(actions(world));
    found.push(named(
        "manifest-set",
        Reports::Operational,
        vec![
            ("reason".to_owned(), reason_of("manifest-set")),
            (
                "manifest".to_owned(),
                manifest_of("manifest-set", 1.4, &printable(world)).to_string(),
            ),
        ],
    ));
    found.push(named("manifest-get", Reports::Operational, vec![]));
    found
}

/// One entry of the walk: a command, what the machine reports for it, and the
/// values distinct to it.
pub type Entry = (String, Reports, Vec<(String, String)>);

/// One entry of the walk.
fn named(command: &str, reports: Reports, values: Vec<(String, String)>) -> Entry {
    (command.to_owned(), reports, values)
}

/// The ten actions of the vocabulary, each with values no other command drives.
fn actions(world: &World) -> Vec<Entry> {
    let bounded = ("duration_s".to_owned(), "60".to_owned());
    let acting = |command: &str, reports: Reports, mut rest: Vec<(String, String)>| {
        rest.push(("actor".to_owned(), "operator".to_owned()));
        rest.push(("reason".to_owned(), reason_of(command)));
        named(command, reports, rest)
    };
    vec![
        acting("pause", Reports::Printing, vec![]),
        acting("resume", Reports::Paused, vec![]),
        acting("cancel", Reports::Printing, vec![]),
        acting(
            "set-feedrate-factor",
            Reports::Printing,
            vec![("factor".to_owned(), "1.11".to_owned()), bounded.clone()],
        ),
        acting(
            "set-flowrate-factor",
            Reports::Printing,
            vec![("factor".to_owned(), "0.97".to_owned()), bounded.clone()],
        ),
        acting(
            "set-tool-target-c",
            Reports::Printing,
            vec![
                ("tool".to_owned(), "0".to_owned()),
                ("target_c".to_owned(), "207".to_owned()),
                bounded.clone(),
            ],
        ),
        acting(
            "set-bed-target-c",
            Reports::Printing,
            vec![("target_c".to_owned(), "57".to_owned()), bounded.clone()],
        ),
        acting(
            "set-fan-percent",
            Reports::Printing,
            vec![("percent".to_owned(), "43".to_owned()), bounded],
        ),
        acting(
            "acknowledge-failure",
            Reports::Printing,
            vec![
                ("event_id".to_owned(), world.event_id.clone()),
                ("disposition".to_owned(), "watch".to_owned()),
            ],
        ),
        acting(
            "start-print",
            Reports::Operational,
            vec![
                ("file_name".to_owned(), world.printable_file()),
                (
                    "manifest".to_owned(),
                    manifest_of("start-print", 1.3, &world.printable_file()).to_string(),
                ),
            ],
        ),
    ]
}

/// Every invocation of one command: the whole cross-product.
pub fn invocations(driven: &Driven, world: &World) -> Vec<Invocation> {
    let mut found = vec![Invocation {
        arguments: vec![driven.command.name.clone()],
        environment: Vec::new(),
        machine_readable: false,
        explicit_config: false,
        options: BTreeSet::new(),
    }];
    for field in &driven.command.fields {
        let text = driven.text(&field.parameter.name);
        found = found
            .into_iter()
            .flat_map(|so_far| {
                let text = text.clone();
                field.forms.iter().map(move |form| {
                    let mut next = so_far.clone();
                    let given = match form.supply {
                        Supply::Value => text.clone(),
                        Supply::File => {
                            written(world, &driven.command.name, &field.parameter.name, &text)
                        }
                    };
                    next.arguments.push(form.option.clone());
                    next.arguments.push(given);
                    next.options.insert(form.option.clone());
                    next
                })
            })
            .collect();
    }
    found
        .into_iter()
        .flat_map(|so_far| [true, false].map(|machine| with_output(&so_far, machine)))
        .flat_map(|so_far| [true, false].map(|explicit| with_config(&so_far, world, explicit)))
        .collect()
}

/// One invocation, with machine-readable output asked for or left alone.
fn with_output(so_far: &Invocation, machine_readable: bool) -> Invocation {
    let mut next = so_far.clone();
    if machine_readable {
        next.arguments.push(JSON_OPTION.to_owned());
        next.options.insert(JSON_OPTION.to_owned());
        next.machine_readable = true;
    }
    next
}

/// One invocation, configured by a file it names or by the environment.
fn with_config(so_far: &Invocation, world: &World, explicit: bool) -> Invocation {
    let mut next = so_far.clone();
    if explicit {
        next.arguments.push(CONFIG_OPTION.to_owned());
        next.arguments
            .push(world.client_config().display().to_string());
        next.options.insert(CONFIG_OPTION.to_owned());
        next.explicit_config = true;
    } else {
        next.environment = world.environment(CREDENTIAL);
    }
    next
}

/// One value written into a file, for the form that carries a path.
fn written(world: &World, command: &str, field: &str, text: &str) -> String {
    let directory = world.root.path().join("values");
    std::fs::create_dir_all(&directory).expect("a directory for the values a walk writes");
    let path: PathBuf = directory.join(format!("{command}-{field}"));
    std::fs::write(&path, text).expect("a value is writable");
    path.display().to_string()
}

/// Every option the surface declares and every option this walk drives.
///
/// The walk cannot fall behind the surface: an option or a form added to a
/// command is either refused by the closed-surface tier or driven here.
pub fn covers(driven: &[Driven], world: &World) -> Vec<String> {
    let mut findings = Vec::new();
    let client: BTreeSet<String> = surface()
        .into_iter()
        .filter(printobserver::surface::Command::is_client)
        .map(|command| command.name)
        .collect();
    let walked: BTreeSet<String> = driven
        .iter()
        .map(|found| found.command.name.clone())
        .collect();
    findings.extend(
        client
            .difference(&walked)
            .map(|missing| format!("`{missing}` is a client command this walk does not run")),
    );
    for found in driven {
        let declared: BTreeSet<String> = found.command.options().into_iter().collect();
        let given: BTreeSet<String> = invocations(found, world)
            .into_iter()
            .flat_map(|invocation| invocation.options)
            .collect();
        findings.extend(declared.difference(&given).map(|missing| {
            format!(
                "`{}` accepts `{missing}` and this walk never drives it",
                found.command.name
            )
        }));
    }
    findings
}

/// The JSON one value of one kind is sent as.
pub fn as_sent(kind: ValueKind, text: &str) -> Value {
    match kind {
        ValueKind::Text => Value::String(text.to_owned()),
        ValueKind::Number => json!(text.parse::<f64>().expect("a number")),
        ValueKind::Integer => json!(text.parse::<i64>().expect("a whole number")),
        ValueKind::Boolean => json!(text.parse::<bool>().expect("true or false")),
        ValueKind::Structured => printobserver_types::serde_json::from_str(text)
            .unwrap_or_else(|_| Value::String(text.to_owned())),
    }
}
