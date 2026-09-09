//! Reading one invocation, against a surface nothing here writes down.
//!
//! Every option this accepts comes from [`surface`](crate::surface::surface),
//! which comes from what the server declares. There is no table here mapping
//! options to fields and no branch per command: an option is accepted when the
//! command it was given to has a field with a form of that name, and refused
//! otherwise. So a command cannot accept an option the server's declaration
//! does not name, and cannot fail to accept one it does.
//!
//! # A value the caller must supply is one the caller supplies
//!
//! A required field with no form given is refused naming it, before anything is
//! sent. Nothing here substitutes a value of its own for one the caller did not
//! give: an adjustment that always sent one fixed number would pass every check
//! about what it may ask for and would still be the wrong request.

use std::collections::BTreeMap;
use std::path::PathBuf;

use printobserver_server::{Located, ValueKind};
use printobserver_types::serde_json;
use printobserver_types::serde_json::{Map, Value, json};

use crate::surface::{
    CONFIG_OPTION, Command, Field, HELP_OPTION, JSON_OPTION, MAX_DURATION_SECONDS,
    MIN_DURATION_SECONDS, SERVE_COMMAND, Supply, VERSION_OPTION, command, is_duration, usage,
};

/// One request to a running supervisor, as the caller asked for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The command that was named.
    pub command: Command,
    /// The configuration file that was named, when one was.
    pub config: Option<PathBuf>,
    /// Whether machine-readable output was asked for.
    pub machine_readable: bool,
    /// Every value the caller supplied, by the name the request carries it as.
    pub values: BTreeMap<String, Value>,
}

impl Call {
    /// The request target this call is made to.
    ///
    /// # Panics
    ///
    /// Panics when the command names no operation, which a call is never built
    /// for.
    #[must_use]
    pub fn target(&self) -> String {
        let operation = self
            .command
            .operation
            .as_ref()
            .expect("a call names an operation");
        let mut target = operation.full_path();
        let mut asked = Vec::new();
        for field in &self.command.fields {
            let Some(value) = self.values.get(&field.parameter.name) else {
                continue;
            };
            let plain = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            match field.parameter.located {
                Located::Path => {
                    target = target.replace(&format!("{{{}}}", field.parameter.name), &plain);
                }
                Located::Query => asked.push(format!("{}={plain}", field.parameter.name)),
                Located::Body => {}
            }
        }
        if !asked.is_empty() {
            target.push('?');
            target.push_str(&asked.join("&"));
        }
        target
    }

    /// The body this call sends, for a call that sends one.
    #[must_use]
    pub fn body(&self) -> Option<String> {
        let operation = self.command.operation.as_ref()?;
        operation.accepts?;
        let carried: Map<String, Value> = self
            .command
            .fields
            .iter()
            .filter(|field| field.parameter.located == Located::Body)
            .filter_map(|field| {
                self.values
                    .get(&field.parameter.name)
                    .map(|value| (field.parameter.name.clone(), value.clone()))
            })
            .collect();
        Some(Value::Object(carried).to_string())
    }
}

/// What the arguments name.
#[derive(Debug, Clone, PartialEq)]
pub enum Invocation {
    /// Print the command surface and exit successfully.
    Usage,
    /// Print this program's own version and exit successfully.
    Version,
    /// Run the supervisor over one configuration file.
    Serve {
        /// The configuration file to run under.
        config: PathBuf,
    },
    /// Make one request to a running supervisor.
    Call(Box<Call>),
    /// The arguments do not name anything this program does.
    Refused {
        /// What to tell the caller.
        detail: String,
    },
}

/// One refusal, with the command surface beneath it.
fn refused(detail: impl core::fmt::Display) -> Invocation {
    Invocation::Refused {
        detail: format!("{detail}\n\n{}", usage()),
    }
}

/// Read one invocation out of the arguments.
///
/// # Panics
///
/// Panics when the surface holds a field with no form, which
/// [`Field`](crate::surface::Field) cannot be built without.
#[must_use]
pub fn parse(arguments: &[String]) -> Invocation {
    let Some(named) = arguments.first() else {
        return Invocation::Usage;
    };
    match named.as_str() {
        HELP_OPTION | "-h" | "help" => return Invocation::Usage,
        VERSION_OPTION => return Invocation::Version,
        _ => {}
    }
    let Some(command) = command(named) else {
        return refused(format!("`{named}` is not a command of this program."));
    };
    let mut config: Option<PathBuf> = None;
    let mut machine_readable = false;
    let mut values: BTreeMap<String, Value> = BTreeMap::new();
    let mut given: BTreeMap<String, String> = BTreeMap::new();

    let mut rest = arguments[1..].iter();
    while let Some(argument) = rest.next() {
        let option = argument.as_str();
        match option {
            HELP_OPTION => return Invocation::Usage,
            VERSION_OPTION => return Invocation::Version,
            JSON_OPTION => {
                machine_readable = true;
                continue;
            }
            CONFIG_OPTION => {
                let Some(path) = rest.next() else {
                    return refused(format!("`{CONFIG_OPTION}` takes the path of a file."));
                };
                if config.is_some() {
                    return refused(format!(
                        "`{CONFIG_OPTION}` was given twice, and this program reads one \
                         configuration file."
                    ));
                }
                config = Some(PathBuf::from(path));
                continue;
            }
            _ => {}
        }
        let Some((field, supply)) = command.field_for(option) else {
            return refused(format!(
                "`{option}` is not an option of `{}`.",
                command.name
            ));
        };
        let Some(given_value) = rest.next() else {
            return refused(format!("`{option}` takes a value."));
        };
        if let Some(already) = given.get(&field.parameter.name) {
            return refused(format!(
                "`{}` was given twice, as `{already}` and as `{option}`.",
                field.parameter.name
            ));
        }
        given.insert(field.parameter.name.clone(), option.to_owned());
        match read_value(field, supply, given_value) {
            Ok(value) => {
                values.insert(field.parameter.name.clone(), value);
            }
            Err(detail) => return refused(detail),
        }
    }

    if command.name == SERVE_COMMAND {
        return match config {
            Some(config) => Invocation::Serve { config },
            None => refused(format!(
                "`{SERVE_COMMAND}` needs the one configuration file it runs under, named \
                 with `{CONFIG_OPTION}`."
            )),
        };
    }
    for field in &command.fields {
        if field.required() && !values.contains_key(&field.parameter.name) {
            return refused(format!(
                "`{}` needs `{}`, and this invocation carries none.",
                command.name, field.forms[0].option
            ));
        }
    }
    Invocation::Call(Box::new(Call {
        command,
        config,
        machine_readable,
        values,
    }))
}

/// Read one value in the form it was supplied.
fn read_value(field: &Field, supply: Supply, given: &str) -> Result<Value, String> {
    let name = &field.parameter.name;
    let text = match supply {
        Supply::Value => given.to_owned(),
        Supply::File => std::fs::read_to_string(given).map_err(|error| {
            format!("`{name}` names the file {given}, which could not be read: {error}")
        })?,
    };
    let value =
        match field.parameter.kind {
            ValueKind::Text => Value::String(text),
            ValueKind::Number => {
                let read: f64 = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("`{name}` takes a number, and `{text}` is not one"))?;
                json!(read)
            }
            ValueKind::Integer => {
                let read: i64 = text.trim().parse().map_err(|_| {
                    format!("`{name}` takes a whole number, and `{text}` is not one")
                })?;
                json!(read)
            }
            ValueKind::Boolean => {
                let read: bool = text.trim().parse().map_err(|_| {
                    format!("`{name}` takes true or false, and `{text}` is not one")
                })?;
                json!(read)
            }
            // A document, given as itself or read out of a file. Text that is not a
            // document is taken as the string it is, so a value of a closed set
            // spelled without quotes is the value a caller meant.
            ValueKind::Structured => serde_json::from_str::<Value>(text.trim())
                .unwrap_or_else(|_| Value::String(text.trim().to_owned())),
        };
    check(field, &value)?;
    Ok(value)
}

/// What a value has to be beside being of its own kind.
fn check(field: &Field, value: &Value) -> Result<(), String> {
    if field.parameter.located == Located::Path {
        let Value::String(text) = value else {
            return Ok(());
        };
        // A path value reaches a request target. One carrying anything outside
        // this set would be a second request rather than an identifier.
        if text.is_empty()
            || !text
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-._~".contains(character))
        {
            return Err(format!(
                "`{}` is `{text}`, which is not an identifier this system mints",
                field.parameter.name
            ));
        }
    }
    if is_duration(&field.parameter) {
        let seconds = value.as_i64().unwrap_or_default();
        if !(MIN_DURATION_SECONDS..=MAX_DURATION_SECONDS).contains(&seconds) {
            return Err(format!(
                "a bounded change stands for between {MIN_DURATION_SECONDS} and \
                 {MAX_DURATION_SECONDS} seconds, and `{seconds}` is outside that"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use printobserver_server::{Located, Parameter, ValueKind};
    use printobserver_types::serde_json::json;

    use super::read_value;
    use crate::surface::{Field, Form, Supply};

    /// One field of one kind, in the form that carries the value itself.
    fn field(kind: ValueKind) -> Field {
        Field {
            parameter: Parameter {
                name: "asked".to_owned(),
                required: true,
                located: Located::Body,
                kind,
            },
            forms: vec![Form {
                option: "--asked".to_owned(),
                supply: Supply::Value,
            }],
        }
    }

    /// A value is read as the kind its own parameter declares.
    ///
    /// The walk over the whole set rather than one of them: `1.15` is a number
    /// and `1.15` is also a perfectly good reason to give, so what a value is
    /// read as is a fact about the field rather than about the text.
    #[test]
    fn a_value_is_read_as_the_kind_its_parameter_declares() {
        for (kind, given, expected) in [
            (ValueKind::Text, "1.15", json!("1.15")),
            (ValueKind::Number, "1.15", json!(1.15)),
            (ValueKind::Integer, "60", json!(60)),
            (ValueKind::Boolean, "true", json!(true)),
            (ValueKind::Structured, "{\"a\":1}", json!({"a": 1})),
            // Text that is not a document is the value a caller meant, which is
            // what lets a closed set be spelled without quotes.
            (ValueKind::Structured, "operator", json!("operator")),
        ] {
            let read = read_value(&field(kind), Supply::Value, given)
                .unwrap_or_else(|refusal| panic!("`{given}` as {kind:?}: {refusal}"));

            assert_eq!(read, expected, "`{given}` was read as {read} for {kind:?}");
        }
    }

    /// A value that is not of its own kind is refused, saying what it takes.
    #[test]
    fn a_value_that_is_not_of_its_own_kind_is_refused() {
        for (kind, given, said) in [
            (ValueKind::Number, "quickly", "takes a number"),
            (ValueKind::Integer, "1.5", "takes a whole number"),
            (ValueKind::Boolean, "perhaps", "takes true or false"),
        ] {
            let refusal = read_value(&field(kind), Supply::Value, given)
                .expect_err("this value is not of that kind");

            assert!(
                refusal.contains(said),
                "{kind:?} was refused with {refusal}"
            );
        }
    }
}
