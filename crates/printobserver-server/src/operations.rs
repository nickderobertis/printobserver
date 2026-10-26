//! The public operations this server serves, declared once.
//!
//! # One operation per public operation, and no more
//!
//! The list here is the whole API. It is exactly one operation per variant of
//! [`PrintAction`](printobserver_core::PrintAction) and of
//! [`SpoolCommand`](printobserver_core::SpoolCommand) the contracts declare,
//! plus thirteen operations beside them — the prints listing, status, context,
//! image materialization, history, the event log followed in the order it was
//! stored, a fresh look, the manifest's read and write, and the four reads of
//! the spool ledger: the spools, one spool, the printer's own history and a
//! filament check — and nothing else. The router is built by folding
//! over this array rather than by writing routes out, so the set served and the
//! set declared are one thing; and this crate's own tier derives the required
//! set from the contracts' own `PrintAction` rather than from a list of its
//! own, so growing this array and the routes together cannot satisfy it.
//!
//! The spool ledger's operations name no print: a spool is the printer's, not
//! one print's, so they sit beneath `/spools` and `/history` rather than
//! beneath a print, and a supervision turn's print binding constrains none of
//! them.
//!
//! Every operation sits beneath [`VERSION_PREFIX`], takes JSON where it takes a
//! body at all, and answers JSON.
//!
//! # Each operation declares what it takes and what it answers
//!
//! [`Operation::request`] is the request schema of one route — every value a
//! caller supplies, where it travels and whether it is required — and
//! [`Operation::response_schema`] is the schema of each answer that route can
//! produce. Neither is written out for the fifteen mutating operations: their
//! body is derived from the contracts' own
//! [`PrintAction`](printobserver_core::PrintAction) or
//! [`SpoolCommand`](printobserver_core::SpoolCommand) at run time, so a
//! variant that gains a field gains it here the moment the type does. The
//! command-line program builds its own parser from these declarations rather
//! than from a list of its own, which is what stops the two surfaces drifting.
//!
//! # The ingress is not one of them
//!
//! [`INGRESS_PATH`] is the endpoint `Obico`'s webhook notification plugin posts
//! to. It is the producer's own ingress rather than an operation a client
//! calls — it is authenticated by a shared secret rather than by an actor, it
//! carries `Obico`'s body rather than this system's, and it answers before its
//! handling completes. So it is declared separately and served outside the
//! versioned prefix, where no client can mistake one surface for the other.

use printobserver_types::serde_json::{Value, json};

// The vocabulary the mutating operations are folded from, re-exported beside
// them: a client of this surface that derives its own commands from these
// operations — the command-line program — reads which action each is and what
// that action carries from here, without naming the supervision domain itself.
pub use printobserver_core::{ActionKind, PrintAction, SpoolCommand};

/// The one versioned prefix every public operation is served beneath.
pub const VERSION_PREFIX: &str = "/v1";

/// The media type every operation takes a body in and answers in.
pub const MEDIA_TYPE: &str = "application/json";

/// Where `Obico`'s webhook notification plugin posts, which is not a public
/// operation. See this module's own comment.
pub const INGRESS_PATH: &str = "/obico/webhook";

/// The field a context answer carries the latest image's own path in.
pub const CONTEXT_IMAGE_PATH_FIELD: &str = "image_path";

/// The field an image answer carries that image's own path in.
pub const IMAGE_PATH_FIELD: &str = "path";

/// The field of a request body the action vocabulary's tag travels in, which a
/// caller never supplies: the operation it sends to is what names the action.
const ACTION_TAG: &str = "action";

/// The field the spool ledger's vocabulary carries its tag in, which a caller
/// never supplies either.
pub const COMMAND_TAG: &str = "command";

/// The method one operation is reached by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    /// A read.
    Get,
    /// A request that changes something.
    Post,
    /// A write that replaces what is there.
    Put,
}

impl Method {
    /// The method as it is spelled on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

impl core::fmt::Display for Method {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Where one value of a request travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Located {
    /// In the request target, as a segment of the path.
    Path,
    /// In the request target, after the question mark.
    Query,
    /// In the JSON body.
    Body,
}

/// What kind of value one parameter takes.
///
/// Declared rather than left to a consumer to rediscover from the schema,
/// because a consumer that has to read a value off a command line has to know
/// what to read it as: `1.15` is a number and `1.15` is also a perfectly good
/// reason to give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueKind {
    /// Text, taken as it was given.
    Text,
    /// A number, whole or otherwise.
    Number,
    /// A whole number.
    Integer,
    /// True or false.
    Boolean,
    /// A document rather than a scalar.
    Structured,
}

impl ValueKind {
    /// Whether a value of this kind is a document rather than a scalar.
    #[must_use]
    pub const fn structured(self) -> bool {
        matches!(self, Self::Structured)
    }
}

/// One value declared beside an operation, at one place in its request.
///
/// Only the operations whose body is not one action of the vocabulary declare
/// their values this way; a mutating operation's are the fields the contracts'
/// own `PrintAction` declares for the variant it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Declared {
    /// What it is called, spelled as the wire spells it.
    pub name: &'static str,
    /// Whether a request without it is refused.
    pub required: bool,
    /// What kind of value it takes.
    pub kind: ValueKind,
    /// The contracts' own type it is a value of, when it is one of theirs.
    ///
    /// A consumer generating a typed client needs the type rather than the
    /// kind: [`ValueKind::Structured`] says a manifest is a document, and not
    /// that it is a `JobManifest`. A value this system mints no type for — a
    /// count, a reason — declares none.
    pub shape: Option<&'static str>,
    /// Whether a request may carry it more than once, each a value of its
    /// own: only a query value can be, and it is then a list of its kind.
    pub repeated: bool,
}

impl Declared {
    /// One value carried at most once, of a kind and of no named type.
    #[must_use]
    pub const fn once(name: &'static str, required: bool, kind: ValueKind) -> Self {
        Self {
            name,
            required,
            kind,
            shape: None,
            repeated: false,
        }
    }
}

/// One value a request to an operation carries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Parameter {
    /// What it is called, spelled as the wire spells it.
    pub name: String,
    /// Whether a request without it is refused.
    pub required: bool,
    /// Where it travels.
    pub located: Located,
    /// What kind of value it takes.
    pub kind: ValueKind,
    /// Whether a request may carry it more than once, each a value of its own.
    pub repeated: bool,
}

/// One answer a route can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Answer {
    /// The operation was carried out, or the read was answered.
    Success,
    /// The policy rejected the action, and the rejection is the answer.
    Rejected,
}

impl Answer {
    /// Its own name, for a report that walks the answers of a route.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Rejected => "rejected",
        }
    }
}

impl core::fmt::Display for Answer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What one operation does to the machine or the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// It reads, and changes nothing.
    Read,
    /// It changes something, so it carries a reason and passes the policy.
    Mutating(ActionKind),
    /// It asks the spool ledger for one command, so it carries a reason and
    /// passes the grants every action passes — and asks nothing of the
    /// machine. Its kind is one of the ledger's five and no print action.
    Command(SpoolCommandKind),
    /// It writes a record without asking anything of the machine.
    Write,
}

/// One command of the spool ledger's vocabulary, and nothing else.
///
/// A kind of its own rather than the whole [`ActionKind`], so that an
/// operation declared as a spool command cannot name a print action: the
/// grants rule on the [`ActionKind`] it answers, and the body is read from the
/// `SpoolCommand` variant that kind tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpoolCommandKind {
    /// Register a spool.
    RegisterSpool,
    /// Load a spool onto a tool.
    LoadSpool,
    /// Unload a spool from its tool.
    UnloadSpool,
    /// Correct a spool's figure.
    AdjustSpool,
    /// Retire a spool.
    RetireSpool,
}

impl SpoolCommandKind {
    /// Every command of the ledger, in the order the contracts declare them.
    pub const ALL: [Self; 5] = [
        Self::RegisterSpool,
        Self::LoadSpool,
        Self::UnloadSpool,
        Self::AdjustSpool,
        Self::RetireSpool,
    ];

    /// The action kind the envelope grants this command under.
    #[must_use]
    pub const fn action_kind(self) -> ActionKind {
        match self {
            Self::RegisterSpool => ActionKind::RegisterSpool,
            Self::LoadSpool => ActionKind::LoadSpool,
            Self::UnloadSpool => ActionKind::UnloadSpool,
            Self::AdjustSpool => ActionKind::AdjustSpool,
            Self::RetireSpool => ActionKind::RetireSpool,
        }
    }
}

/// One public operation: what it is called, how it is reached, and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operation {
    /// The operation's own name. For a mutating operation this is the spelling
    /// the contracts' own `PrintAction` tags that variant with.
    pub name: &'static str,
    /// The method it is reached by.
    pub method: Method,
    /// Its path beneath [`VERSION_PREFIX`].
    pub path: &'static str,
    /// What it does.
    pub effect: Effect,
    /// The media type it takes a body in, when it takes one.
    pub accepts: Option<&'static str>,
    /// The media type it answers in.
    pub answers: &'static str,
    /// The values it takes after the question mark.
    pub query: &'static [Declared],
    /// The values it takes in its body, for an operation whose body is not one
    /// action of the vocabulary. A mutating operation declares none here: its
    /// body is the `PrintAction` or `SpoolCommand` variant it names, read from
    /// the contracts themselves by [`Operation::request`].
    pub body: &'static [Declared],
    /// The field of its answer carrying an absolute path to a materialized
    /// image, when its answer carries one.
    ///
    /// Every image answer this server gives is a path on **this server's own
    /// filesystem** and no route of it answers image bytes, so a consumer
    /// reading this field is reading which of its values names a file it may
    /// not be able to open. Declared here rather than discovered, because what
    /// a consumer owes such a field is a failure of its own.
    pub image_path_field: Option<&'static str>,
}

impl Operation {
    /// Its whole path, including the versioned prefix.
    #[must_use]
    pub fn full_path(&self) -> String {
        format!("{VERSION_PREFIX}{}", self.path)
    }

    /// Which print action this operation asks for, when it asks for one.
    #[must_use]
    pub const fn action_kind(&self) -> Option<ActionKind> {
        match self.effect {
            Effect::Mutating(kind) => Some(kind),
            _ => None,
        }
    }

    /// Which spool command this operation asks for, when it asks for one.
    #[must_use]
    pub const fn command_kind(&self) -> Option<ActionKind> {
        match self.effect {
            Effect::Command(kind) => Some(kind.action_kind()),
            _ => None,
        }
    }

    /// Which action of either vocabulary this operation asks for, when it asks
    /// for one: the kind the envelope grants it under.
    #[must_use]
    pub const fn granted_as(&self) -> Option<ActionKind> {
        match self.effect {
            Effect::Mutating(kind) => Some(kind),
            Effect::Command(kind) => Some(kind.action_kind()),
            Effect::Read | Effect::Write => None,
        }
    }

    /// Whether this operation changes something.
    ///
    /// Asking the machine for an action and replacing the manifest a print runs
    /// under are both changes, and both carry a reason. What separates them is
    /// whether the policy has anything to rule on — not whether the caller has
    /// to say why.
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        matches!(
            self.effect,
            Effect::Mutating(_) | Effect::Command(_) | Effect::Write
        )
    }

    /// Every value a request to this operation carries, and where it carries it.
    ///
    /// The body of a mutating operation is not written out anywhere: it is the
    /// fields the contracts' own `PrintAction` or `SpoolCommand` declares for
    /// the variant this operation names, read from that type's own schema,
    /// less the tag and any value the path already carries. A variant that
    /// gains a field gains it here without this file changing, which is what
    /// stops a consumer building a request surface from drifting from the
    /// vocabulary.
    ///
    /// # Panics
    ///
    /// Panics when the contracts declare no variant this operation names, which
    /// is a declared list and a closed type that have come apart.
    #[must_use]
    pub fn request(&self) -> Vec<Parameter> {
        let mut found = path_parameters(self.path);
        found.extend(declared(self.query, Located::Query));
        match Vocabulary::of(self.effect) {
            Some((vocabulary, kind)) => {
                let routed: Vec<String> = found.iter().map(|path| path.name.clone()).collect();
                found.extend(
                    action_parameters(vocabulary, kind)
                        .into_iter()
                        .filter(|field| !routed.contains(&field.name)),
                );
            }
            None => found.extend(declared(self.body, Located::Body)),
        }
        found
    }

    /// Every value a request to this operation carries, with its own shape.
    ///
    /// [`Operation::request`] answers what a command line needs — a name, a
    /// place and a kind. This answers what a **typed** consumer needs beside
    /// that: the shape of each value, as a schema referring to the contracts'
    /// own types by name where the value is one of theirs. `structured` says a
    /// manifest is a document; `{"$ref": "#/$defs/JobManifest"}` says which
    /// document, which is the difference between a generated client that types
    /// it and one that takes anything.
    ///
    /// The order is [`Operation::request`]'s own, so the two can be read side
    /// by side.
    ///
    /// # Panics
    ///
    /// Panics when the contracts declare no variant this operation names, which
    /// is a declared list and a closed type that have come apart.
    #[must_use]
    pub fn request_shapes(&self) -> Vec<(Parameter, Value)> {
        let action = match Vocabulary::of(self.effect) {
            Some((vocabulary, kind)) => action_shapes(vocabulary, kind),
            None => Vec::new(),
        };
        self.request()
            .into_iter()
            .map(|parameter| {
                let shape = match parameter.located {
                    // What makes a path segment an identifier is the type that
                    // minted it, and a route mints nothing: it is a string.
                    Located::Path => json!({ "type": "string" }),
                    Located::Query => declared_shape(self.query, &parameter.name),
                    Located::Body if action.is_empty() => {
                        declared_shape(self.body, &parameter.name)
                    }
                    Located::Body => action
                        .iter()
                        .find(|(name, _)| *name == parameter.name)
                        .map_or_else(
                            || {
                                panic!(
                                    "`{}` takes `{}` in its body, and the action it names \
                                     declares no such field",
                                    self.name, parameter.name
                                )
                            },
                            |(_, shape)| shape.clone(),
                        ),
                };
                (parameter, shape)
            })
            .collect()
    }

    /// Every answer this operation can produce.
    ///
    /// A read answers what it read. An operation asking for an action answers
    /// that as well as the policy's own rejection, which is an answer rather
    /// than a transport failure: it carries the reason, the value asked for and
    /// the range allowed.
    #[must_use]
    pub const fn answers_with(&self) -> &'static [Answer] {
        match self.effect {
            Effect::Mutating(_) | Effect::Command(_) => &[Answer::Success, Answer::Rejected],
            Effect::Read | Effect::Write => &[Answer::Success],
        }
    }

    /// The schema of one answer this operation produces.
    ///
    /// A rejection is the same shape as an acceptance and deliberately so: the
    /// answer to a rejected action is the action record carrying the policy's
    /// own decision, rather than a second document a caller has to know about.
    ///
    /// # Panics
    ///
    /// Panics when this operation is not one this server serves an answer
    /// shape for, which is a declared list and a router that have come apart.
    #[must_use]
    pub fn response_schema(&self, answer: Answer) -> Value {
        use printobserver_types::schemars::schema_for;
        debug_assert!(
            self.answers_with().contains(&answer),
            "`{}` produces no {answer} answer",
            self.name
        );
        let schema = match (self.name, self.effect) {
            (_, Effect::Mutating(_)) => schema_for!(crate::wire::ActionAnswer),
            (_, Effect::Command(_)) => schema_for!(crate::wire::SpoolCommandAnswer),
            ("spools", _) => schema_for!(crate::wire::SpoolsAnswer),
            ("spool", _) => schema_for!(crate::wire::SpoolAnswer),
            ("printer_history", _) => schema_for!(crate::wire::TimelineAnswer),
            ("preflight", _) => schema_for!(crate::wire::PreflightAnswer),
            ("prints", _) => schema_for!(crate::wire::PrintsAnswer),
            ("status", _) => schema_for!(crate::wire::StatusAnswer),
            ("context", _) => schema_for!(crate::wire::ContextAnswer),
            ("image", _) => schema_for!(crate::wire::ImageAnswer),
            ("history", _) => schema_for!(crate::wire::HistoryAnswer),
            ("events", _) => schema_for!(crate::wire::EventsAnswer),
            ("look", _) => schema_for!(crate::wire::LookAnswer),
            ("manifest_get" | "manifest_set", _) => schema_for!(crate::wire::ManifestAnswer),
            (name, _) => panic!("the operation `{name}` has no declared answer shape"),
        };
        schema.to_value()
    }
}

/// The values declared beside an operation, at one place in the request.
fn declared(entries: &'static [Declared], located: Located) -> impl Iterator<Item = Parameter> {
    entries.iter().map(move |entry| Parameter {
        name: entry.name.to_owned(),
        required: entry.required,
        located,
        kind: entry.kind,
        repeated: entry.repeated,
    })
}

/// The shape one value declared beside an operation carries.
///
/// The contracts' own type where the declaration names one, and the kind's own
/// scalar shape where it names none — a reason and a count are values this
/// system mints no type for.
fn declared_shape(entries: &'static [Declared], name: &str) -> Value {
    let Some(entry) = entries.iter().find(|entry| entry.name == name) else {
        return json!({ "type": "string" });
    };
    let one = if let Some(shape) = entry.shape {
        json!({ "$ref": format!("#/$defs/{shape}") })
    } else {
        match entry.kind {
            ValueKind::Text => json!({ "type": "string" }),
            ValueKind::Number => json!({ "type": "number" }),
            ValueKind::Integer => json!({ "type": "integer" }),
            ValueKind::Boolean => json!({ "type": "boolean" }),
            ValueKind::Structured => json!({ "type": "object" }),
        }
    };
    // A value a request may carry more than once is a list of its kind, which
    // is what a typed consumer sends it as.
    if entry.repeated {
        json!({ "type": "array", "items": one })
    } else {
        one
    }
}

/// Which of the contracts' two closed vocabularies one mutating operation's
/// body is a variant of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vocabulary {
    /// The print actions, tagged by `action`.
    Actions,
    /// The spool ledger's commands, tagged by `command`.
    Commands,
}

impl Vocabulary {
    /// The vocabulary one effect's body is read from, and the variant it names.
    const fn of(effect: Effect) -> Option<(Self, ActionKind)> {
        match effect {
            Effect::Mutating(kind) => Some((Self::Actions, kind)),
            Effect::Command(kind) => Some((Self::Commands, kind.action_kind())),
            Effect::Read | Effect::Write => None,
        }
    }

    /// The field the vocabulary's tag travels in.
    const fn tag(self) -> &'static str {
        match self {
            Self::Actions => ACTION_TAG,
            Self::Commands => COMMAND_TAG,
        }
    }

    /// The vocabulary's own schema, as the contracts declare it.
    fn schema(self) -> Value {
        use printobserver_types::schemars::schema_for;
        match self {
            Self::Actions => schema_for!(PrintAction).to_value(),
            Self::Commands => schema_for!(SpoolCommand).to_value(),
        }
    }
}

/// The shape of each field one action of the vocabulary takes.
///
/// Read out of the contracts' own `PrintAction` or `SpoolCommand` beside
/// [`action_parameters`], which walks the same variant for the same fields —
/// so a field that gains a shape here gains a parameter there, and neither can
/// carry one the other does not.
///
/// # Panics
///
/// Panics when the contracts declare no variant tagged with this action.
fn action_shapes(vocabulary: Vocabulary, kind: ActionKind) -> Vec<(String, Value)> {
    let schema = vocabulary.schema();
    let variant = action_variant(&schema, vocabulary, kind);
    variant
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("the action `{kind:?}` declares no properties"))
        .iter()
        .filter(|(name, _)| name.as_str() != vocabulary.tag())
        .map(|(name, field)| (name.clone(), field.clone()))
        .collect()
}

/// The variant of the contracts' own action vocabulary one action tags.
///
/// # Panics
///
/// Panics when the contracts declare no variant tagged with this action, which
/// is this server's declared list and that closed type having come apart.
fn action_variant(schema: &Value, vocabulary: Vocabulary, kind: ActionKind) -> Value {
    let tag = printobserver_types::serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("`{kind:?}` renders as the tag it is spelled by"));
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants.iter().find(|variant| {
                variant.pointer(&format!("/properties/{}/const", vocabulary.tag()))
                    == Some(&json!(tag.clone()))
            })
        })
        .cloned()
        .unwrap_or_else(|| panic!("the contracts declare no action tagged `{tag}`"))
}

/// The values one path template takes, in the order it takes them.
fn path_parameters(path: &str) -> Vec<Parameter> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| Parameter {
            name: name.to_owned(),
            // A route with a hole in its path is not a route: nothing reaches
            // the handler without every segment of it.
            required: true,
            located: Located::Path,
            // A path segment is text: what makes one an identifier is the type
            // that minted it rather than anything a route can say.
            kind: ValueKind::Text,
            repeated: false,
        })
        .collect()
}

/// The body one action of the vocabulary takes, out of the contracts' own type.
///
/// # Panics
///
/// Panics when the contracts declare no variant tagged with this action, which
/// is this server's declared list and that closed type having come apart.
fn action_parameters(vocabulary: Vocabulary, kind: ActionKind) -> Vec<Parameter> {
    let schema = vocabulary.schema();
    let variant = action_variant(&schema, vocabulary, kind);
    let required: Vec<&str> = variant
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let properties = variant
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("the action `{kind:?}` declares no properties"));
    properties
        .iter()
        .filter(|(name, _)| name.as_str() != vocabulary.tag())
        .map(|(name, field)| Parameter {
            name: name.clone(),
            required: required.contains(&name.as_str()),
            located: Located::Body,
            kind: kind_of(&resolve(&schema, field)),
            repeated: false,
        })
        .collect()
}

/// One field's own schema, following the one reference a contract field takes.
fn resolve(schema: &Value, field: &Value) -> Value {
    let Some(reference) = field.get("$ref").and_then(Value::as_str) else {
        return field.clone();
    };
    reference
        .strip_prefix("#/")
        .and_then(|rest| schema.pointer(&format!("/{rest}")))
        .cloned()
        .unwrap_or_else(|| field.clone())
}

/// What kind of value one shape describes.
///
/// Read off the shape rather than off the field's name: a closed set of string
/// constants is text however many of them there are, and an object is a
/// document however few fields it has. A shape that says nothing this can
/// decide is a document, which is the reading that still lets a caller supply
/// it — as itself, or out of a file.
fn kind_of(shape: &Value) -> ValueKind {
    if let Some(declared) = shape.get("type") {
        let named = |name: &str| match declared {
            Value::String(one) => one == name,
            Value::Array(many) => many.iter().any(|one| one == name),
            _ => false,
        };
        if named("object") || named("array") {
            return ValueKind::Structured;
        }
        if named("string") {
            return ValueKind::Text;
        }
        if named("integer") {
            return ValueKind::Integer;
        }
        if named("number") {
            return ValueKind::Number;
        }
        if named("boolean") {
            return ValueKind::Boolean;
        }
    }
    match shape.get("oneOf").or_else(|| shape.get("anyOf")) {
        Some(Value::Array(branches)) if !branches.is_empty() => {
            let kinds: Vec<ValueKind> = branches
                .iter()
                .map(|branch| match branch.get("const") {
                    Some(Value::String(_)) => ValueKind::Text,
                    _ => kind_of(branch),
                })
                .collect();
            let first = kinds[0];
            if kinds.iter().all(|kind| *kind == first) {
                first
            } else {
                ValueKind::Structured
            }
        }
        _ => ValueKind::Structured,
    }
}

/// One mutating operation, at the path its action is asked for under.
const fn action(name: &'static str, path: &'static str, kind: ActionKind) -> Operation {
    Operation {
        name,
        method: Method::Post,
        path,
        effect: Effect::Mutating(kind),
        accepts: Some(MEDIA_TYPE),
        answers: MEDIA_TYPE,
        query: &[],
        body: &[],
        image_path_field: None,
    }
}

/// One spool command, at the path its command is asked for under.
const fn command(name: &'static str, path: &'static str, kind: SpoolCommandKind) -> Operation {
    Operation {
        effect: Effect::Command(kind),
        ..action(name, path, kind.action_kind())
    }
}

/// One read, which takes no body.
const fn read(name: &'static str, path: &'static str) -> Operation {
    Operation {
        name,
        method: Method::Get,
        path,
        effect: Effect::Read,
        accepts: None,
        answers: MEDIA_TYPE,
        query: &[],
        body: &[],
        image_path_field: None,
    }
}

/// How one operation's name is spelled as a command of the `printobserver`
/// program: each underscore a hyphen.
///
/// The one spelling both the command-line program's commands and the
/// supervising agent's allowed commands are made from, so the command the agent
/// is allowed to run is the command the program has.
#[must_use]
pub fn command_for(operation: &str) -> String {
    operation.replace('_', "-")
}

/// Every public operation this server serves, and there is no other.
pub const OPERATIONS: [Operation; 28] = [
    // Takes nothing: it is how a caller finds the identifier every other
    // print-specific operation takes.
    // llmlint: ignore[names_match_behavior] suppressions.toml has the reason.
    read("prints", "/prints"),
    read("status", "/prints/{print_id}/status"),
    Operation {
        image_path_field: Some(CONTEXT_IMAGE_PATH_FIELD),
        ..read("context", "/prints/{print_id}/context")
    },
    Operation {
        image_path_field: Some(IMAGE_PATH_FIELD),
        ..read("image", "/images/{image_id}")
    },
    Operation {
        query: &[Declared::once("limit", false, ValueKind::Integer)],
        ..read("history", "/prints/{print_id}/history")
    },
    // The event log in the order it was stored, after a cursor: one print's
    // events and the printer's, or every event, filtered by kind, waiting up
    // to `wait_s` seconds for one to pass. A read, so it takes no actor.
    Operation {
        query: &[
            Declared::once("after", false, ValueKind::Text),
            Declared::once("print_id", false, ValueKind::Text),
            Declared {
                repeated: true,
                ..Declared::once("kinds", false, ValueKind::Text)
            },
            Declared {
                repeated: true,
                ..Declared::once("exclude_kinds", false, ValueKind::Text)
            },
            Declared::once("wait_s", false, ValueKind::Integer),
            Declared::once("limit", false, ValueKind::Integer),
        ],
        ..read("events", "/events")
    },
    read("manifest_get", "/prints/{print_id}/manifest"),
    // A fresh look: waits up to `wait_s` seconds for something to arrive for
    // the print, returning the moment something does, then takes a frame and
    // reads the printer as it is now. A read, so it takes no actor.
    Operation {
        query: &[Declared::once("wait_s", false, ValueKind::Integer)],
        image_path_field: Some(CONTEXT_IMAGE_PATH_FIELD),
        ..read("look", "/prints/{print_id}/look")
    },
    Operation {
        name: "manifest_set",
        method: Method::Put,
        path: "/prints/{print_id}/manifest",
        effect: Effect::Write,
        accepts: Some(MEDIA_TYPE),
        answers: MEDIA_TYPE,
        query: &[],
        body: &[
            Declared::once("reason", true, ValueKind::Text),
            Declared {
                shape: Some("JobManifest"),
                ..Declared::once("manifest", true, ValueKind::Structured)
            },
        ],
        image_path_field: None,
    },
    // Every spool, the retired ones only when asked for, each beside one line
    // saying what it holds — and the charges held for a tool after a filament
    // change nobody has confirmed.
    Operation {
        query: &[Declared::once("include_retired", false, ValueKind::Boolean)],
        ..read("spools", "/spools")
    },
    read("spool", "/spools/{spool_id}"),
    // The printer's own history: every print, charge and spool event, newest
    // first, a page at a time. Named for the printer rather than for a print,
    // because `history` is one print's.
    Operation {
        query: &[
            Declared {
                repeated: true,
                ..Declared::once("kinds", false, ValueKind::Text)
            },
            Declared::once("since", false, ValueKind::Text),
            Declared::once("until", false, ValueKind::Text),
            Declared::once("spool_id", false, ValueKind::Text),
            Declared::once("before", false, ValueKind::Text),
            Declared::once("limit", false, ValueKind::Integer),
        ],
        ..read("printer_history", "/history")
    },
    // Whether the spools loaded hold enough for one file, read now: it records
    // nothing and starts nothing.
    Operation {
        query: &[Declared::once("file_name", true, ValueKind::Text)],
        ..read("preflight", "/preflight")
    },
    action(
        "pause",
        "/prints/{print_id}/actions/pause",
        ActionKind::Pause,
    ),
    action(
        "resume",
        "/prints/{print_id}/actions/resume",
        ActionKind::Resume,
    ),
    action(
        "cancel",
        "/prints/{print_id}/actions/cancel",
        ActionKind::Cancel,
    ),
    action(
        "start_print",
        "/prints/{print_id}/actions/start_print",
        ActionKind::StartPrint,
    ),
    action(
        "set_feedrate_factor",
        "/prints/{print_id}/actions/set_feedrate_factor",
        ActionKind::SetFeedrateFactor,
    ),
    action(
        "set_flowrate_factor",
        "/prints/{print_id}/actions/set_flowrate_factor",
        ActionKind::SetFlowrateFactor,
    ),
    action(
        "set_tool_target_c",
        "/prints/{print_id}/actions/set_tool_target_c",
        ActionKind::SetToolTargetC,
    ),
    action(
        "set_bed_target_c",
        "/prints/{print_id}/actions/set_bed_target_c",
        ActionKind::SetBedTargetC,
    ),
    action(
        "set_fan_percent",
        "/prints/{print_id}/actions/set_fan_percent",
        ActionKind::SetFanPercent,
    ),
    action(
        "acknowledge_failure",
        "/prints/{print_id}/actions/acknowledge_failure",
        ActionKind::AcknowledgeFailure,
    ),
    command(
        "register_spool",
        "/spools/actions/register_spool",
        SpoolCommandKind::RegisterSpool,
    ),
    command(
        "load_spool",
        "/spools/{spool_id}/actions/load_spool",
        SpoolCommandKind::LoadSpool,
    ),
    command(
        "unload_spool",
        "/spools/{spool_id}/actions/unload_spool",
        SpoolCommandKind::UnloadSpool,
    ),
    command(
        "adjust_spool",
        "/spools/{spool_id}/actions/adjust_spool",
        SpoolCommandKind::AdjustSpool,
    ),
    command(
        "retire_spool",
        "/spools/{spool_id}/actions/retire_spool",
        SpoolCommandKind::RetireSpool,
    ),
];

/// The thirteen operations this server serves beside the two vocabularies.
///
/// Twelve of them read and one writes a manifest, which asks nothing of the
/// machine; what they have in common is that none of them is an action or a
/// spool command the contracts declare. A look writes its own record into the
/// print's history, as every read of the camera is kept, and asks nothing of
/// the machine either; a filament check records nothing at all.
pub const BESIDE_THE_ACTIONS: [&str; 13] = [
    "prints",
    "status",
    "context",
    "image",
    "history",
    "events",
    "manifest_get",
    "look",
    "manifest_set",
    "spools",
    "spool",
    "printer_history",
    "preflight",
];

/// The operation of one name, when this server serves one.
#[must_use]
pub fn operation(name: &str) -> Option<&'static Operation> {
    OPERATIONS.iter().find(|declared| declared.name == name)
}

#[cfg(test)]
mod tests {
    use printobserver_core::ActionKind;

    use super::{Effect, MEDIA_TYPE, Method, OPERATIONS, VERSION_PREFIX, operation};

    /// Every method is spelled the way the wire spells it.
    #[test]
    fn every_method_is_spelled_as_the_wire_spells_it() {
        for (method, spelling) in [
            (Method::Get, "GET"),
            (Method::Post, "POST"),
            (Method::Put, "PUT"),
        ] {
            assert_eq!(method.as_str(), spelling);
            assert_eq!(method.to_string(), spelling);
        }
    }

    /// An operation's whole path carries the one versioned prefix.
    #[test]
    fn every_operation_sits_beneath_the_one_versioned_prefix() {
        for declared in OPERATIONS {
            let whole = declared.full_path();
            assert!(
                whole.starts_with(VERSION_PREFIX),
                "`{}` is served at {whole}",
                declared.name
            );
            assert_eq!(declared.answers, MEDIA_TYPE);
        }
    }

    /// A mutating operation names the action it asks for, and a read names none.
    #[test]
    fn a_mutating_operation_names_its_action_and_a_read_names_none() {
        for declared in OPERATIONS {
            match declared.effect {
                Effect::Mutating(kind) => {
                    assert_eq!(declared.action_kind(), Some(kind));
                    assert_eq!(declared.command_kind(), None);
                    assert_eq!(declared.granted_as(), Some(kind));
                    assert!(declared.is_mutating());
                }
                Effect::Command(kind) => {
                    assert_eq!(declared.action_kind(), None);
                    assert_eq!(declared.command_kind(), Some(kind.action_kind()));
                    assert_eq!(declared.granted_as(), Some(kind.action_kind()));
                    assert!(declared.is_mutating());
                }
                Effect::Write => {
                    assert_eq!(declared.action_kind(), None);
                    assert!(
                        declared.is_mutating(),
                        "`{}` writes a record and is not held to the reason every \
                         change carries",
                        declared.name
                    );
                }
                Effect::Read => {
                    assert_eq!(declared.action_kind(), None);
                    assert!(!declared.is_mutating());
                }
            }
        }
    }

    /// A mutating operation takes JSON and answers JSON; a read takes no body.
    ///
    /// The two constructors are the only way an entry of the declared list is
    /// made, so this is the shape every entry has by construction.
    #[test]
    fn a_mutating_operation_takes_a_body_and_a_read_takes_none() {
        let mutating = super::action(
            "pause",
            "/prints/{print_id}/actions/pause",
            ActionKind::Pause,
        );
        assert_eq!(mutating.method, Method::Post);
        assert_eq!(mutating.accepts, Some(MEDIA_TYPE));
        assert_eq!(mutating.answers, MEDIA_TYPE);
        assert_eq!(mutating.action_kind(), Some(ActionKind::Pause));

        let reading = super::read("status", "/prints/{print_id}/status");
        assert_eq!(reading.method, Method::Get);
        assert_eq!(reading.accepts, None);
        assert_eq!(reading.answers, MEDIA_TYPE);
        assert_eq!(reading.effect, Effect::Read);
    }

    /// A spool command's body is its command's own fields, less the tag and
    /// the spool its path already names.
    #[test]
    fn a_spool_commands_body_is_its_fields_less_the_tag_and_the_routed_spool() {
        let load = operation("load_spool").expect("the load is served");
        let names: Vec<(String, super::Located)> = load
            .request()
            .into_iter()
            .map(|parameter| (parameter.name, parameter.located))
            .collect();
        assert_eq!(
            names,
            vec![
                ("spool_id".to_owned(), super::Located::Path),
                ("actor".to_owned(), super::Located::Body),
                ("reason".to_owned(), super::Located::Body),
                ("tool".to_owned(), super::Located::Body),
            ]
        );
        let register = operation("register_spool").expect("the registration is served");
        assert!(
            register
                .request()
                .iter()
                .all(|parameter| parameter.name != super::COMMAND_TAG
                    && parameter.located == super::Located::Body)
        );
    }

    /// The ledger's own kind names exactly the commands the contracts' own
    /// `SpoolCommand` declares, each by the tag it is spelled with, and every
    /// spool command operation is declared under one of them.
    #[test]
    fn the_ledgers_kinds_are_the_commands_the_contracts_declare() {
        let schema = super::Vocabulary::Commands.schema();
        let declared: Vec<String> = schema["oneOf"]
            .as_array()
            .expect("the spool vocabulary is a union")
            .iter()
            .filter_map(|variant| variant.pointer("/properties/command/const"))
            .filter_map(|tag| tag.as_str().map(str::to_owned))
            .collect();
        let kinds: Vec<String> = super::SpoolCommandKind::ALL
            .iter()
            .map(|kind| {
                printobserver_types::serde_json::to_value(kind.action_kind())
                    .ok()
                    .and_then(|tag| tag.as_str().map(str::to_owned))
                    .expect("an action kind renders as its tag")
            })
            .collect();
        assert_eq!(kinds, declared);
        let served: Vec<super::SpoolCommandKind> = OPERATIONS
            .iter()
            .filter_map(|operation| match operation.effect {
                Effect::Command(kind) => Some(kind),
                _ => None,
            })
            .collect();
        assert_eq!(served, super::SpoolCommandKind::ALL.to_vec());
    }

    /// A value a request may carry more than once is described as a list.
    #[test]
    fn a_repeated_value_is_described_as_a_list_of_its_kind() {
        let history = operation("printer_history").expect("the printer history is served");
        let kinds = history
            .request_shapes()
            .into_iter()
            .find(|(parameter, _)| parameter.name == "kinds")
            .expect("the history takes kinds");
        assert!(kinds.0.repeated);
        assert_eq!(
            kinds.1,
            printobserver_types::serde_json::json!({"type": "array", "items": {"type": "string"}})
        );
    }

    /// An operation this server does not serve is answered as none.
    #[test]
    fn an_operation_this_server_does_not_serve_is_answered_as_none() {
        assert_eq!(operation("pause").map(|found| found.name), Some("pause"));
        assert!(operation("reboot").is_none());
    }
}
