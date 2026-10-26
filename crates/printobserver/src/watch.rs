//! `printobserver watch`: what the supervisor records, as one stream.
//!
//! # One read, composed
//!
//! The stream is the server's `events` read, asked again from its own cursor
//! for as long as the watch runs. The filters by kind are the server's own;
//! what this program adds is the sampling of one kind by `received_at`, the
//! heartbeat while nothing passes, the retry from the cursor while the server
//! cannot be reached, and the one ending line every stream finishes with.
//!
//! # Every stream ends saying why
//!
//! The last line is always the ending: the watched print ended, the watch was
//! interrupted, the server was unreachable for longer than the watch retries
//! for, or the server refused the credential or the request. A consumer can
//! always tell a stream that ended or failed from one where nothing happened,
//! which is the whole of why this command exists rather than a loop of each
//! consumer's own.
//!
//! # A print's end is read from the answer, not from a filter
//!
//! Watching a print, the end is the answer's `print_ended`, which the server
//! reports whatever the filters say. So that every passing event stored before
//! the end is printed before the ending, and none after it, the read always
//! lets the `print_ended` event itself through, and this program decides
//! whether to print it by the filters the caller gave: the stream stops at that
//! event in the store's own order, printing it only when the caller's filters
//! pass it.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use printobserver_types::Timestamp;
use printobserver_types::serde_json::{self, Value, json};
use tokio::time::Instant;

use crate::config::{ClientConfig, load};
use crate::failure::Exit;
use crate::parse::query_escaped;
use crate::transport::send;

/// The field the sampling option carries.
pub const SAMPLE_FIELD: &str = "sample";

/// The field the heartbeat option carries.
pub const HEARTBEAT_FIELD: &str = "heartbeat_s";

/// The field the reconnection option carries.
pub const RECONNECT_FIELD: &str = "reconnect_s";

/// The field one kind to pass travels in, as this command spells it.
pub const KIND_FIELD: &str = "kind";

/// The field one kind never to pass travels in, as this command spells it.
pub const EXCLUDE_KIND_FIELD: &str = "exclude_kind";

/// How long the watch retries an unreachable server for when it is asked
/// nothing else, in seconds.
pub const DEFAULT_RECONNECT_S: u64 = 30;

/// How long one read waits on the server for an event to pass, in seconds.
///
/// Well inside the server's own bound; a heartbeat is this program's own
/// timer, so it does not wait on the read.
const LONG_POLL_S: u32 = 25;

/// How many events one read asks for: the read's own default, so a print that
/// recorded more than this is followed across several reads.
const READ_LIMIT: u32 = printobserver_server::DEFAULT_EVENTS_LIMIT;

/// How long the watch waits between two attempts on a server it cannot reach.
const RETRY_EVERY: Duration = Duration::from_millis(500);

/// The kind a print's end is recorded under.
const PRINT_ENDED: &str = "print_ended";

/// One watch, as the caller asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    /// The configuration file that was named, when one was.
    pub config: Option<PathBuf>,
    /// Whether machine-readable output was asked for.
    pub machine_readable: bool,
    /// The print to watch, when one was named.
    pub print_id: Option<String>,
    /// The event to start after, when one was named.
    pub after: Option<String>,
    /// Only these kinds; empty, every kind.
    pub kinds: Vec<String>,
    /// Never these kinds.
    pub exclude_kinds: Vec<String>,
    /// The kinds sampled, each with the least time between two that pass.
    pub samples: BTreeMap<String, i64>,
    /// How often to say the watch is still running with nothing to print, in
    /// seconds; zero, never.
    pub heartbeat_s: u64,
    /// How long to retry a server that cannot be reached, in seconds.
    pub reconnect_s: u64,
}

impl Watch {
    /// The watch the values of one invocation ask for.
    ///
    /// # Errors
    ///
    /// Returns what to tell the caller when a sampling is not `KIND=SECONDS`,
    /// or a number of seconds is not a whole number at or above its least.
    pub fn of(
        config: Option<PathBuf>,
        machine_readable: bool,
        values: &BTreeMap<String, Value>,
    ) -> Result<Self, String> {
        let text = |name: &str| values.get(name).and_then(Value::as_str).map(str::to_owned);
        let texts = |name: &str| -> Vec<String> {
            match values.get(name) {
                Some(Value::Array(each)) => each
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                _ => Vec::new(),
            }
        };
        let seconds = |name: &str, least: i64, otherwise: u64| -> Result<u64, String> {
            match values.get(name).and_then(Value::as_i64) {
                None => Ok(otherwise),
                Some(given) if given >= least => Ok(given.unsigned_abs()),
                Some(given) => Err(format!(
                    "`--{}` takes a whole number of seconds from {least}, and `{given}` is not one",
                    name.replace('_', "-")
                )),
            }
        };
        let mut samples = BTreeMap::new();
        for sampling in texts(SAMPLE_FIELD) {
            let (kind, every) = sampling
                .split_once('=')
                .filter(|(kind, _)| !kind.is_empty())
                .ok_or_else(|| {
                    format!("`--sample` takes `KIND=SECONDS`, and `{sampling}` is not that")
                })?;
            let every: i64 = every
                .parse()
                .ok()
                .filter(|every: &i64| *every > 0)
                .ok_or_else(|| {
                    format!(
                        "`--sample` takes a whole number of seconds above zero after `=`, and \
                         `{sampling}` does not carry one"
                    )
                })?;
            samples.insert(kind.to_owned(), every);
        }
        Ok(Self {
            config,
            machine_readable,
            print_id: text("print_id"),
            after: text("after"),
            kinds: texts(KIND_FIELD),
            exclude_kinds: texts(EXCLUDE_KIND_FIELD),
            samples,
            heartbeat_s: seconds(HEARTBEAT_FIELD, 0, 0)?,
            reconnect_s: seconds(RECONNECT_FIELD, 0, DEFAULT_RECONNECT_S)?,
        })
    }

    /// Whether the caller's own filters pass one kind.
    fn passes(&self, kind: &str) -> bool {
        (self.kinds.is_empty() || self.kinds.iter().any(|one| one == kind))
            && !self.exclude_kinds.iter().any(|one| one == kind)
    }

    /// The request target of one read from a cursor.
    ///
    /// Watching a print, the read always lets the print's own end through, so
    /// the stream can stop at it; this program then prints it by the caller's
    /// filters alone.
    fn target(&self, cursor: Option<&str>, wait_s: u32) -> String {
        let mut asked: Vec<(&str, String)> = Vec::new();
        if let Some(after) = cursor {
            asked.push(("after", after.to_owned()));
        }
        let mut kinds = self.kinds.clone();
        let mut excluded = self.exclude_kinds.clone();
        if let Some(print_id) = &self.print_id {
            asked.push(("print_id", print_id.clone()));
            if !kinds.is_empty() && !kinds.iter().any(|kind| kind == PRINT_ENDED) {
                kinds.push(PRINT_ENDED.to_owned());
            }
            excluded.retain(|kind| kind != PRINT_ENDED);
        }
        asked.extend(kinds.into_iter().map(|kind| ("kinds", kind)));
        asked.extend(excluded.into_iter().map(|kind| ("exclude_kinds", kind)));
        asked.push(("wait_s", wait_s.to_string()));
        asked.push(("limit", READ_LIMIT.to_string()));
        let query: Vec<String> = asked
            .into_iter()
            .map(|(name, value)| format!("{name}={}", query_escaped(&value)))
            .collect();
        format!("/v1/events?{}", query.join("&"))
    }
}

/// Why a watch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The watched print ended.
    PrintEnded,
    /// A signal asked the watch to stop.
    Interrupted,
    /// The server could not be reached for longer than the watch retries for.
    Unreachable,
    /// The server refused the credential or the request.
    Refused,
}

impl Ending {
    /// The reason, as the ending line spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrintEnded => "print_ended",
            Self::Interrupted => "interrupted",
            Self::Unreachable => "unreachable",
            Self::Refused => "refused",
        }
    }

    /// What the caller's shell sees.
    #[must_use]
    pub const fn exit(self) -> Exit {
        match self {
            Self::PrintEnded | Self::Interrupted => Exit::Success,
            Self::Unreachable => Exit::Unreachable,
            Self::Refused => Exit::Refused,
        }
    }
}

/// What passes the sampling of one kind: the first, and then one at least the
/// sampling's interval after the last that passed, by `received_at`.
#[derive(Debug, Default)]
struct Sampler {
    /// When the last event of each sampled kind that passed was received.
    last: BTreeMap<String, Timestamp>,
}

impl Sampler {
    /// Whether one event passes, noting it when it does.
    fn passes(&mut self, watch: &Watch, kind: &str, received_at: Option<Timestamp>) -> bool {
        let Some(every) = watch.samples.get(kind) else {
            return true;
        };
        // An instant this program cannot read is not one it can sample by, so
        // the event is passed rather than silently dropped.
        let Some(received_at) = received_at else {
            return true;
        };
        let due = self.last.get(kind).is_none_or(|last| {
            last.plus_seconds(*every)
                .is_ok_and(|next| received_at >= next)
        });
        if due {
            self.last.insert(kind.to_owned(), received_at);
        }
        due
    }
}

/// Where the lines go, in the rendering asked for.
struct Lines {
    /// Whether each line is one JSON object.
    machine_readable: bool,
}

impl Lines {
    /// Write one line and flush it, so a consumer reading a pipe sees it now.
    fn write(text: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    }

    /// One event that passed.
    fn event(&self, event: &Value) {
        if self.machine_readable {
            Self::write(&event.to_string());
            return;
        }
        let field = |name: &str| event.get(name).and_then(Value::as_str).unwrap_or("?");
        let scope = event
            .get("print_id")
            .and_then(Value::as_str)
            .map_or_else(|| "printer".to_owned(), |print| format!("print {print}"));
        Self::write(&format!(
            "{}  {}  {scope}  event {}",
            field("received_at"),
            field("kind"),
            field("id")
        ));
    }

    /// The watch is running, and nothing passed since the last line.
    fn heartbeat(&self, cursor: Option<&str>) {
        let at = Timestamp::now().to_string();
        if self.machine_readable {
            Self::write(&json!({ "watch_heartbeat": { "at": at, "cursor": cursor } }).to_string());
        } else {
            Self::write(&format!(
                "{at}  still watching; nothing new since the last line"
            ));
        }
    }

    /// The last line.
    fn ending(&self, ending: Ending, detail: &str, cursor: Option<&str>) {
        if self.machine_readable {
            Self::write(
                &json!({
                    "watch_ended": {
                        "reason": ending.as_str(),
                        "detail": detail,
                        "cursor": cursor,
                    }
                })
                .to_string(),
            );
            return;
        }
        let why = match ending {
            Ending::PrintEnded => "the print ended",
            Ending::Interrupted => "the watch was interrupted",
            Ending::Unreachable => "the supervisor could not be reached",
            Ending::Refused => "the supervisor refused it",
        };
        Self::write(&format!("watch ended: {why}. {detail}"));
    }
}

/// What one read came back with.
enum Read {
    /// The server answered the read.
    Answered(Value),
    /// Nothing answered.
    Unreachable(String),
    /// The server refused the credential or the request.
    Refused(String),
}

/// Ask the server one read, on the thread this is called on.
fn read(config: &ClientConfig, target: &str) -> Read {
    let answered = match send(config, "GET", target, None) {
        Ok(answered) => answered,
        Err(unreachable) => return Read::Unreachable(unreachable.to_string()),
    };
    let document: Option<Value> = serde_json::from_str(&answered.body).ok();
    match (answered.status, document) {
        (200, Some(document)) => Read::Answered(document),
        (200, None) => Read::Refused(format!(
            "the supervisor at {} answered something this program cannot read",
            config.address()
        )),
        (401, _) => Read::Refused(format!(
            "the supervisor at {} refused the credential this program presented. Run \
             `printobserver credential issue`, or configure the credential whose verifier the \
             supervisor holds",
            config.address()
        )),
        (status, document) => Read::Refused(format!(
            "the supervisor answered {status}: {}",
            document
                .as_ref()
                .and_then(|said| said.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("it said nothing this program can read")
        )),
    }
}

/// Wait for the first of the signals that ask a watch to stop, and name it.
async fn stopped() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut interrupt), Ok(mut terminate)) = (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
        ) else {
            return core::future::pending().await;
        };
        tokio::select! {
            _ = interrupt.recv() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
        let (Ok(mut interrupt), Ok(mut broken), Ok(mut closed)) =
            (ctrl_c(), ctrl_break(), ctrl_close())
        else {
            return core::future::pending().await;
        };
        tokio::select! {
            _ = interrupt.recv() => "Ctrl+C",
            _ = broken.recv() => "Ctrl+Break",
            _ = closed.recv() => "the console closing",
        }
    }
}

/// Run one watch until it ends, and answer what it exits with.
///
/// A configuration this program cannot read ends it before any stream starts,
/// as every command that reaches a server is ended, and prints no line.
#[must_use]
pub fn run(watch: &Watch) -> Exit {
    let config = match load(watch.config.as_deref()) {
        Ok(config) => config,
        Err(unconfigured) => {
            eprintln!("printobserver: {unconfigured}");
            return Exit::Unconfigured;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("printobserver could not start a runtime: {error}");
            return Exit::Refused;
        }
    };
    let ending = runtime.block_on(follow(watch, config));
    // A read still in flight is abandoned rather than waited out: the ending
    // line is already printed.
    runtime.shutdown_background();
    ending.exit()
}

/// One stream being followed: where it has read to and what it has passed.
struct Stream<'watch> {
    /// What the caller asked for.
    watch: &'watch Watch,
    /// Where the lines go.
    lines: Lines,
    /// The last event examined, which the next read starts after.
    cursor: Option<String>,
    /// What the sampling has passed.
    sampler: Sampler,
    /// The watched print's end, once an answer has told it.
    ended: Option<Value>,
    /// When the last line was printed, which the heartbeat counts from.
    last_line: Instant,
}

impl Stream<'_> {
    /// End the stream with its last line.
    fn end(&self, ending: Ending, detail: &str) -> Ending {
        self.lines.ending(ending, detail, self.cursor.as_deref());
        ending
    }

    /// Wait out one read, saying the watch is still running every heartbeat,
    /// and ending it on a signal.
    async fn ask(
        &mut self,
        config: &ClientConfig,
        stop: &mut core::pin::Pin<&mut impl Future<Output = &'static str>>,
    ) -> Result<Read, Ending> {
        // Once the print has ended, read on at once to its end event rather
        // than waiting for events that will not come.
        let wait_s = if self.ended.is_some() { 0 } else { LONG_POLL_S };
        let target = self.watch.target(self.cursor.as_deref(), wait_s);
        let asking = config.clone();
        let mut pending = tokio::task::spawn_blocking(move || read(&asking, &target));
        let heartbeat =
            (self.watch.heartbeat_s > 0).then(|| Duration::from_secs(self.watch.heartbeat_s));
        loop {
            let beat = heartbeat.map(|every| self.last_line + every);
            tokio::select! {
                done = &mut pending => {
                    return done.map_err(|error| {
                        self.end(
                            Ending::Refused,
                            &format!("the read could not be completed: {error}"),
                        )
                    });
                }
                () = tokio::time::sleep_until(beat.unwrap_or_else(Instant::now)),
                    if beat.is_some() => {
                    self.lines.heartbeat(self.cursor.as_deref());
                    self.last_line = Instant::now();
                }
                signal = stop.as_mut() => {
                    return Err(self.end(Ending::Interrupted, &format!("stopped by {signal}")));
                }
            }
        }
    }

    /// Print what one answer carries that passes, and end the stream when the
    /// watched print's end is reached.
    fn take(&mut self, answer: &Value) -> Option<Ending> {
        if let Some(ended) = answer.get("print_ended").filter(|ended| !ended.is_null()) {
            self.ended = Some(ended.clone());
        }
        let events = answer
            .get("events")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for event in events {
            let kind = event
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let received_at = event
                .get("received_at")
                .and_then(Value::as_str)
                .and_then(|at| at.parse().ok());
            if self.watch.passes(kind) && self.sampler.passes(self.watch, kind, received_at) {
                self.lines.event(event);
                self.last_line = Instant::now();
            }
            if let Some(id) = event.get("id").and_then(Value::as_str) {
                self.cursor = Some(id.to_owned());
            }
            let is_the_end = self.watch.print_id.is_some()
                && kind == PRINT_ENDED
                && event.get("print_id").and_then(Value::as_str) == self.watch.print_id.as_deref();
            if is_the_end {
                let ended = self.ended.clone().unwrap_or_else(|| {
                    json!({ "event_id": event.get("id"), "received_at": event.get("received_at") })
                });
                return Some(self.end(Ending::PrintEnded, &ended_detail(self.watch, &ended)));
            }
        }
        if let Some(read_to) = answer.get("cursor").and_then(Value::as_str) {
            self.cursor = Some(read_to.to_owned());
        }
        // An end already behind the cursor — a watch asked to start after it —
        // is one the stream has nothing left to print before.
        match &self.ended {
            Some(ended) if events.is_empty() => {
                Some(self.end(Ending::PrintEnded, &ended_detail(self.watch, ended)))
            }
            _ => None,
        }
    }
}

/// Follow the stream until it ends.
async fn follow(watch: &Watch, config: ClientConfig) -> Ending {
    let mut stream = Stream {
        watch,
        lines: Lines {
            machine_readable: watch.machine_readable,
        },
        cursor: watch.after.clone(),
        sampler: Sampler::default(),
        ended: None,
        last_line: Instant::now(),
    };
    let mut unreachable_since: Option<Instant> = None;
    let mut stop = std::pin::pin!(stopped());
    loop {
        let detail = match stream.ask(&config, &mut stop).await {
            Err(ending) => return ending,
            Ok(Read::Refused(detail)) => return stream.end(Ending::Refused, &detail),
            Ok(Read::Answered(answer)) => {
                unreachable_since = None;
                if let Some(ending) = stream.take(&answer) {
                    return ending;
                }
                continue;
            }
            Ok(Read::Unreachable(detail)) => detail,
        };
        let since = *unreachable_since.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_secs(watch.reconnect_s) {
            return stream.end(
                Ending::Unreachable,
                &format!(
                    "{detail}. It has not answered for longer than {} s",
                    watch.reconnect_s
                ),
            );
        }
        tokio::select! {
            () = tokio::time::sleep(RETRY_EVERY) => {}
            signal = stop.as_mut() => {
                return stream.end(Ending::Interrupted, &format!("stopped by {signal}"));
            }
        }
    }
}

/// What the ending of a watch over a print that ended says.
fn ended_detail(watch: &Watch, ended: &Value) -> String {
    let field = |name: &str| ended.get(name).and_then(Value::as_str).unwrap_or("?");
    format!(
        "print {} ended {} at {} (event {})",
        watch.print_id.as_deref().unwrap_or("?"),
        field("outcome"),
        field("received_at"),
        field("event_id")
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use printobserver_types::serde_json::{Value, json};

    use super::{Sampler, Watch};

    /// A watch asked these values.
    fn watch(values: &[(&str, Value)]) -> Result<Watch, String> {
        let values: BTreeMap<String, Value> = values
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        Watch::of(None, true, &values)
    }

    /// A sampling is `KIND=SECONDS`, and anything else is refused saying so.
    #[test]
    fn a_sampling_is_a_kind_and_a_whole_number_of_seconds() {
        let read =
            watch(&[("sample", json!(["periodic_observation=300"]))]).expect("a sampling is read");
        assert_eq!(read.samples.get("periodic_observation"), Some(&300));
        for refused in [
            "periodic_observation",
            "=300",
            "periodic_observation=0",
            "a=b",
        ] {
            let said = watch(&[("sample", json!([refused]))]).expect_err(refused);
            assert!(said.contains("--sample"), "{refused}: {said}");
        }
        let said = watch(&[("reconnect_s", json!(-1))]).expect_err("a negative reconnection");
        assert!(said.contains("--reconnect-s"), "{said}");
    }

    /// Watching a print, the read lets the print's end through whatever the
    /// caller's filters say, and the caller's filters decide what is printed.
    #[test]
    fn watching_a_print_reads_its_end_whatever_the_filters_say() {
        let included = watch(&[
            ("print_id", json!("p")),
            ("kind", json!(["periodic_observation"])),
        ])
        .expect("a watch is read");
        let target = included.target(None, 0);
        assert!(
            target.contains("kinds=periodic_observation&kinds=print_ended"),
            "{target}"
        );
        assert!(!included.passes("print_ended"));

        let excluded = watch(&[
            ("print_id", json!("p")),
            ("exclude_kind", json!(["print_ended", "camera_look"])),
        ])
        .expect("a watch is read");
        let target = excluded.target(Some("e"), 25);
        assert!(target.contains("exclude_kinds=camera_look"), "{target}");
        assert!(!target.contains("exclude_kinds=print_ended"), "{target}");
        assert!(
            target.starts_with("/v1/events?after=e&print_id=p"),
            "{target}"
        );
        assert!(!excluded.passes("print_ended"));
    }

    /// The first sampled event passes, one inside the interval since the last
    /// that passed does not, and one at the interval does.
    #[test]
    fn a_sampled_kind_passes_its_first_and_then_one_per_interval() {
        let sampled =
            watch(&[("sample", json!(["periodic_observation=300"]))]).expect("a watch is read");
        let mut passing = Sampler::default();
        let at = |text: &str| Some(text.parse().expect("an instant"));
        let kind = "periodic_observation";
        assert!(passing.passes(&sampled, kind, at("2026-10-25T10:00:00Z")));
        assert!(!passing.passes(&sampled, kind, at("2026-10-25T10:04:59Z")));
        assert!(passing.passes(&sampled, kind, at("2026-10-25T10:05:00Z")));
        assert!(!passing.passes(&sampled, kind, at("2026-10-25T10:09:00Z")));
        assert!(passing.passes(&sampled, kind, at("2026-10-25T11:00:00Z")));
        assert!(passing.passes(&sampled, "camera_look", at("2026-10-25T11:00:01Z")));
    }
}
