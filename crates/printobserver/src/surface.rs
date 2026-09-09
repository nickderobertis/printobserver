//! Every command this program has, built from what the server declares.
//!
//! # Nothing here is a list of commands
//!
//! [`surface`] folds over
//! [`OPERATIONS`](printobserver_server::OPERATIONS) — one client command per
//! public operation the server serves — and puts the one command that runs the
//! server beside them. Each client command's options are built from that
//! operation's own [`request`](printobserver_server::Operation::request), whose
//! body for an action is the fields the contracts' `PrintAction` declares for
//! that variant. So the set of commands, the set of options each takes, and
//! which of them a caller must supply are all facts about crates outside this
//! one, and growing a declaration here cannot grow the surface.
//!
//! # Four global options, and no fifth
//!
//! [`GLOBAL_OPTIONS`] is the whole of what every command takes beside its own
//! values: the machine-readable-output flag, the configuration file, and the
//! two the caller reads this program with. None of them commands anything, and
//! none of them can be composed into an action — which is why the
//! configuration file's path is one of them while the address and the
//! credential it carries are not.

use printobserver_server::{Located, OPERATIONS, Operation, Parameter};

/// The flag that asks for machine-readable output.
pub const JSON_OPTION: &str = "--json";

/// The option naming the configuration file to read.
pub const CONFIG_OPTION: &str = "--config";

/// The flag that prints the command surface.
pub const HELP_OPTION: &str = "--help";

/// The flag that prints this program's own version.
pub const VERSION_OPTION: &str = "--version";

/// Every option every command takes beside its own values, and there is no
/// other.
pub const GLOBAL_OPTIONS: [&str; 4] = [JSON_OPTION, CONFIG_OPTION, HELP_OPTION, VERSION_OPTION];

/// The one command that is not a request to a running server.
pub const SERVE_COMMAND: &str = "server";

/// The field a bounded intervention's duration travels in, as the contracts
/// spell it.
pub const DURATION_FIELD: &str = "duration_s";

/// The shortest bounded intervention this program will ask for, in seconds.
///
/// One second rather than none: a duration of zero is an adjustment that
/// expires at the instant it is applied, which is an adjustment nobody asked
/// for spelled as one they did.
pub const MIN_DURATION_SECONDS: i64 = 1;

/// The longest bounded intervention this program will ask for, in seconds.
///
/// A day. A bounded intervention is a change somebody is coming back to; one
/// standing longer than the longest print is not bounded in any sense a
/// supervisor can act on, and is refused where it is asked for rather than
/// scheduled and forgotten.
pub const MAX_DURATION_SECONDS: i64 = 86_400;

/// The suffix an option takes when it carries a path to the value rather than
/// the value.
pub const FILE_SUFFIX: &str = "-file";

/// How a caller supplies one value on a command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Supply {
    /// The value itself.
    Value,
    /// The path of a file carrying the value.
    File,
}

/// One way one value may be supplied.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form {
    /// The option that carries it.
    pub option: String,
    /// What the option's own value is.
    pub supply: Supply,
}

/// One value a command takes, and every form it takes it in.
///
/// A scalar takes one form, because there is nothing to choose between. A
/// structured value takes two — as itself, or as the path of a file carrying
/// it — because a manifest on a command line is a document and a document a
/// caller already has is a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// What the server calls it, and what the request carries it as.
    pub parameter: Parameter,
    /// Every form it may be supplied in, in the order they are declared.
    pub forms: Vec<Form>,
}

impl Field {
    /// One field, in the forms a value of its kind takes.
    fn of(parameter: Parameter) -> Self {
        let spelled = option_for(&parameter.name);
        let mut forms = vec![Form {
            option: spelled.clone(),
            supply: Supply::Value,
        }];
        if parameter.kind.structured() {
            forms.push(Form {
                option: format!("{spelled}{FILE_SUFFIX}"),
                supply: Supply::File,
            });
        }
        Self { parameter, forms }
    }

    /// Whether a command without this field is refused.
    #[must_use]
    pub const fn required(&self) -> bool {
        self.parameter.required
    }
}

/// One command of this program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// What a caller types.
    pub name: String,
    /// The operation it calls, for a command that calls one.
    pub operation: Option<Operation>,
    /// The values it takes.
    pub fields: Vec<Field>,
}

impl Command {
    /// Whether this command is a request to an already-running server.
    #[must_use]
    pub const fn is_client(&self) -> bool {
        self.operation.is_some()
    }

    /// The field one option carries, and the form it carries it in.
    #[must_use]
    pub fn field_for(&self, option: &str) -> Option<(&Field, Supply)> {
        self.fields.iter().find_map(|field| {
            field
                .forms
                .iter()
                .find(|form| form.option == option)
                .map(|form| (field, form.supply))
        })
    }

    /// Every option this command accepts of its own, in the order declared.
    #[must_use]
    pub fn options(&self) -> Vec<String> {
        self.fields
            .iter()
            .flat_map(|field| field.forms.iter().map(|form| form.option.clone()))
            .collect()
    }

    /// One line of the command surface.
    fn usage_line(&self) -> String {
        let taken: Vec<String> = self
            .fields
            .iter()
            .map(|field| {
                let option = &field.forms[0].option;
                let value = if field.parameter.kind.structured() {
                    "<json>"
                } else {
                    "<value>"
                };
                if field.required() {
                    format!("{option} {value}")
                } else {
                    format!("[{option} {value}]")
                }
            })
            .collect();
        format!("  printobserver {} {}", self.name, taken.join(" "))
            .trim_end()
            .to_owned()
    }
}

/// How one value's name is spelled as an option.
#[must_use]
pub fn option_for(name: &str) -> String {
    format!("--{}", name.replace('_', "-"))
}

/// How one operation's name is spelled as a command.
#[must_use]
pub fn command_for(operation: &str) -> String {
    operation.replace('_', "-")
}

/// Every command this program has, and there is no other.
///
/// One per operation the server declares, plus the one that runs the server.
/// That command takes no value of its own: the configuration file it runs
/// under is the global option every command takes.
#[must_use]
pub fn surface() -> Vec<Command> {
    let mut found = vec![Command {
        name: SERVE_COMMAND.to_owned(),
        operation: None,
        fields: Vec::new(),
    }];
    found.extend(OPERATIONS.iter().map(|operation| Command {
        name: command_for(operation.name),
        operation: Some(*operation),
        fields: operation.request().into_iter().map(Field::of).collect(),
    }));
    found
}

/// The command of one name, when this program has one.
#[must_use]
pub fn command(name: &str) -> Option<Command> {
    surface().into_iter().find(|found| found.name == name)
}

/// Whether one parameter is a bounded intervention's duration.
#[must_use]
pub fn is_duration(parameter: &Parameter) -> bool {
    parameter.name == DURATION_FIELD && parameter.located == Located::Body
}

/// The whole command surface, which is also what `--help` prints.
#[must_use]
pub fn usage() -> String {
    let lines: Vec<String> = surface().iter().map(Command::usage_line).collect();
    format!(
        "printobserver — a supervision layer between a 3D printer and an agent.\n\n\
         Usage:\n{}\n\nEvery command also takes {JSON_OPTION} (machine-readable output), \
         {CONFIG_OPTION} <path>,\n{HELP_OPTION} and {VERSION_OPTION}. Where the server is and \
         what authenticates to it\nare read from that file and from the environment, never \
         from a command line.\n",
        lines.join("\n")
    )
}

/// This program's own version, as the manifest declares it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What `--version` prints.
#[must_use]
pub fn version() -> String {
    format!("printobserver {VERSION}\n")
}
