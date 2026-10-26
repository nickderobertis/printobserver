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

use core::num::NonZeroU32;

use printobserver_sdk::PrintOutcome;
use printobserver_types::serde::Deserialize;
use printobserver_types::serde_json::{self, Value, json};
use printobserver_types::{EventId, EventKind, EventRecord, PrintId, Timestamp};
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
    pub print_id: Option<PrintId>,
    /// The event to start after, when one was named.
    pub after: Option<EventId>,
    /// Only these kinds; empty, every kind. Carried as given: the server rules
    /// on a kind's spelling, and refuses the read naming one it will not take.
    pub kinds: Vec<String>,
    /// Never these kinds, carried as given for the same reason.
    pub exclude_kinds: Vec<String>,
    /// The kinds sampled, each with the least number of seconds between two
    /// that pass.
    pub samples: BTreeMap<EventKind, NonZeroU32>,
    /// How often to say the watch is still running with nothing to print, in
    /// seconds; zero, never. Never negative, which only [`Watch::of`] can
    /// hold it to, so it is read through [`Watch::heartbeat_s`].
    heartbeat_s: u64,
    /// How long to retry a server that cannot be reached, in seconds.
    reconnect_s: u64,
}

impl Watch {
    /// The watch the values of one invocation ask for.
    ///
    /// # Errors
    ///
    /// Returns what to tell the caller when a print or an event is not an
    /// identifier this system mints, a sampling is not `KIND=SECONDS` naming a
    /// kind and a whole number of seconds above zero, or a number of seconds is
    /// not a whole number at or above its least.
    pub fn of(
        config: Option<PathBuf>,
        machine_readable: bool,
        values: &BTreeMap<String, Value>,
    ) -> Result<Self, String> {
        let text = |name: &str| values.get(name).and_then(Value::as_str);
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
            let (kind, every) = sampling.split_once('=').ok_or_else(|| {
                format!("`--sample` takes `KIND=SECONDS`, and `{sampling}` is not that")
            })?;
            let kind = EventKind::new(kind)
                .map_err(|refused| format!("`--sample` names no kind: {refused}"))?;
            let every: NonZeroU32 = every.parse().map_err(|_| {
                format!(
                    "`--sample` takes a whole number of seconds above zero after `=`, and \
                         `{sampling}` does not carry one"
                )
            })?;
            samples.insert(kind, every);
        }
        Ok(Self {
            config,
            machine_readable,
            print_id: identifier(text("print_id"), "--print-id")?,
            after: identifier(text("after"), "--after")?,
            kinds: texts(KIND_FIELD),
            exclude_kinds: texts(EXCLUDE_KIND_FIELD),
            samples,
            heartbeat_s: seconds(HEARTBEAT_FIELD, 0, 0)?,
            reconnect_s: seconds(RECONNECT_FIELD, 0, DEFAULT_RECONNECT_S)?,
        })
    }

    /// How often a quiet watch says it is still running, in seconds; zero,
    /// never.
    #[must_use]
    pub const fn heartbeat_s(&self) -> u64 {
        self.heartbeat_s
    }

    /// How long the watch retries a server that cannot be reached, in seconds.
    #[must_use]
    pub const fn reconnect_s(&self) -> u64 {
        self.reconnect_s
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
    fn target(&self, cursor: Option<EventId>, wait_s: u32) -> String {
        let mut asked: Vec<(&str, String)> = Vec::new();
        if let Some(after) = cursor {
            asked.push(("after", after.to_string()));
        }
        let mut kinds = self.kinds.clone();
        let mut excluded = self.exclude_kinds.clone();
        if let Some(print_id) = self.print_id {
            asked.push(("print_id", print_id.to_string()));
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

/// One identifier an option gave, read as the type that mints it.
fn identifier<T>(given: Option<&str>, option: &str) -> Result<Option<T>, String>
where
    T: core::str::FromStr,
    T::Err: core::fmt::Display,
{
    given
        .map(|text| {
            text.parse()
                .map_err(|refused| format!("`{option}` is `{text}`: {refused}"))
        })
        .transpose()
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
    last: BTreeMap<EventKind, Timestamp>,
}

impl Sampler {
    /// Whether one event passes, noting it when it does.
    fn passes(&mut self, watch: &Watch, kind: &EventKind, received_at: Timestamp) -> bool {
        let Some(every) = watch.samples.get(kind) else {
            return true;
        };
        let due = self.last.get(kind).is_none_or(|last| {
            last.plus_seconds(i64::from(every.get()))
                .is_ok_and(|next| received_at >= next)
        });
        if due {
            self.last.insert(kind.clone(), received_at);
        }
        due
    }
}

/// One print's end, as an events answer tells it.
#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct Ended {
    /// The `print_ended` event.
    event_id: EventId,
    /// When it was received.
    received_at: Timestamp,
    /// How the print ended.
    outcome: PrintOutcome,
}

/// One events answer, as the server sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct Answer {
    /// The events, each printed as it came.
    events: Vec<Value>,
    /// The last event the read examined.
    cursor: Option<EventId>,
    /// The watched print's end, once it has ended.
    #[serde(default)]
    print_ended: Option<Ended>,
}

/// One events answer, read: each event as the record it is, beside the text
/// the server sent for it, which is what is printed.
#[derive(Debug, Clone)]
struct Page {
    /// The events, in the order stored.
    events: Vec<(EventRecord, Value)>,
    /// The last event the read examined.
    cursor: Option<EventId>,
    /// The watched print's end, once it has ended.
    print_ended: Option<Ended>,
}

impl Page {
    /// One answer's body, read, or why it cannot be.
    fn read(body: &str) -> Result<Self, serde_json::Error> {
        let answer: Answer = serde_json::from_str(body)?;
        let events = answer
            .events
            .into_iter()
            .map(|event| Ok((EventRecord::deserialize(&event)?, event)))
            .collect::<Result<_, serde_json::Error>>()?;
        Ok(Self {
            events,
            cursor: answer.cursor,
            print_ended: answer.print_ended,
        })
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
    fn event(&self, head: &EventRecord, event: &Value) {
        if self.machine_readable {
            Self::write(&event.to_string());
            return;
        }
        let scope = head
            .print_id
            .map_or_else(|| "printer".to_owned(), |print| format!("print {print}"));
        Self::write(&format!(
            "{}  {}  {scope}  event {}",
            head.received_at,
            head.kind(),
            head.id
        ));
    }

    /// The watch is running, and nothing passed since the last line.
    fn heartbeat(&self, cursor: Option<EventId>) {
        let at = Timestamp::now().to_string();
        if self.machine_readable {
            Self::write(
                &json!({
                    "watch_heartbeat": { "at": at, "cursor": cursor.map(|id| id.to_string()) }
                })
                .to_string(),
            );
        } else {
            Self::write(&format!(
                "{at}  still watching; nothing new since the last line"
            ));
        }
    }

    /// The last line.
    fn ending(&self, ending: Ending, detail: &str, cursor: Option<EventId>) {
        if self.machine_readable {
            Self::write(
                &json!({
                    "watch_ended": {
                        "reason": ending.as_str(),
                        "detail": detail,
                        "cursor": cursor.map(|id| id.to_string()),
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
        let said = format!("watch ended: {why}. {detail}");
        // In words, a stream that failed says so where a failure is said; one
        // that ended as asked says so where its lines went.
        if matches!(ending, Ending::Unreachable | Ending::Refused) {
            eprintln!("{said}");
        } else {
            Self::write(&said);
        }
    }
}

/// What one read came back with.
enum Read {
    /// The server answered the read.
    Answered(Page),
    /// Nothing answered.
    Unreachable(String),
    /// The server refused the credential or the request, or answered
    /// something this program cannot read.
    Refused(String),
}

/// Ask the server one read, on the thread this is called on.
fn read(config: &ClientConfig, target: &str) -> Read {
    let answered = match send(config, "GET", target, None) {
        Ok(answered) => answered,
        Err(unreachable) => return Read::Unreachable(unreachable.to_string()),
    };
    if answered.status == 200 {
        return match Page::read(&answered.body) {
            Ok(page) => Read::Answered(page),
            Err(error) => Read::Refused(format!(
                "the supervisor at {} answered an events read with something this program                  cannot read: {error}",
                config.address()
            )),
        };
    }
    if answered.status == 401 {
        return Read::Refused(format!(
            "the supervisor at {} refused the credential this program presented. Run \
             `printobserver credential issue`, or configure the credential whose verifier the \
             supervisor holds",
            config.address()
        ));
    }
    let said: Option<Value> = serde_json::from_str(&answered.body).ok();
    Read::Refused(format!(
        "the supervisor answered {}: {}",
        answered.status,
        said.as_ref()
            .and_then(|said| said.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("it said nothing this program can read")
    ))
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
    // The interruption journey drives the two Unix signals end to end. The
    // console's three controls stand in for them here, and what follows a
    // control is the one path that journey proves: the same `stopped` future,
    // the same ending line, the same exit. No runner this repository uses can
    // deliver a console control to a child it started, so the platform's own
    // listeners are the one part of this that is not driven.
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
    cursor: Option<EventId>,
    /// What the sampling has passed.
    sampler: Sampler,
    /// The watched print's end, once an answer has told it.
    ended: Option<Ended>,
    /// When the last line was printed, which the heartbeat counts from.
    last_line: Instant,
}

impl Stream<'_> {
    /// End the stream with its last line.
    fn end(&self, ending: Ending, detail: &str) -> Ending {
        self.lines.ending(ending, detail, self.cursor);
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
        let target = self.watch.target(self.cursor, wait_s);
        let asking = config.clone();
        let mut pending = tokio::task::spawn_blocking(move || read(&asking, &target));
        let heartbeat =
            (self.watch.heartbeat_s > 0).then(|| Duration::from_secs(self.watch.heartbeat_s));
        loop {
            // A heartbeat further off than the timer can name is one that
            // never comes, so the read is waited on alone.
            let beat = heartbeat.and_then(|every| self.last_line.checked_add(every));
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
                    self.lines.heartbeat(self.cursor);
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
    fn take(&mut self, page: Page) -> Option<Ending> {
        if page.print_ended.is_some() {
            self.ended = page.print_ended;
        }
        let drained = page.events.is_empty();
        for (head, event) in &page.events {
            if self.watch.passes(head.kind().as_str())
                && self
                    .sampler
                    .passes(self.watch, head.kind(), head.received_at)
            {
                self.lines.event(head, event);
                self.last_line = Instant::now();
            }
            self.cursor = Some(head.id);
            let is_the_end = self.watch.print_id.is_some()
                && head.kind().as_str() == PRINT_ENDED
                && head.print_id == self.watch.print_id;
            if is_the_end {
                let detail = self.ended.as_ref().map_or_else(
                    || format!("its end is event {}", head.id),
                    |ended| ended_detail(self.watch, ended),
                );
                return Some(self.end(Ending::PrintEnded, &detail));
            }
        }
        if page.cursor.is_some() {
            self.cursor = page.cursor;
        }
        // An end already behind the cursor — a watch asked to start after it —
        // is one the stream has nothing left to print before.
        match &self.ended {
            Some(ended) if drained => {
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
        cursor: watch.after,
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
            Ok(Read::Answered(page)) => {
                unreachable_since = None;
                if let Some(ending) = stream.take(page) {
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
fn ended_detail(watch: &Watch, ended: &Ended) -> String {
    format!(
        "print {} ended {} at {} (event {})",
        watch
            .print_id
            .map_or_else(|| "?".to_owned(), |print| print.to_string()),
        match ended.outcome {
            PrintOutcome::Completed => "completed",
            PrintOutcome::Cancelled => "cancelled",
            PrintOutcome::Failed => "failed",
        },
        ended.received_at,
        ended.event_id
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use printobserver_types::serde_json::{Value, json};
    use printobserver_types::{EventId, EventKind, Timestamp};

    use super::{Sampler, Watch};

    /// A print identifier, as this system mints one.
    const PRINT: &str = "01a12bd0-e46a-7a32-8d69-82a6f9ca508f";

    /// An event identifier, as this system mints one.
    const AFTER: &str = "01a12bd0-e46a-7a32-8d69-82a6f9ca5090";

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
        let observation = EventKind::new("periodic_observation").expect("a kind");
        assert_eq!(
            read.samples.get(&observation).map(|every| every.get()),
            Some(300)
        );
        for refused in [
            "periodic_observation",
            "=300",
            "Not-A-Kind=300",
            "periodic_observation=0",
            "a=b",
        ] {
            let said = watch(&[("sample", json!([refused]))]).expect_err(refused);
            assert!(said.contains("--sample"), "{refused}: {said}");
        }
        for (field, flag) in [
            ("reconnect_s", "--reconnect-s"),
            ("heartbeat_s", "--heartbeat-s"),
        ] {
            let said = watch(&[(field, json!(-1))]).expect_err("a negative number of seconds");
            assert!(said.contains(flag), "{said}");
        }
        let said = watch(&[("print_id", json!("not-a-print"))]).expect_err("a malformed print");
        assert!(said.contains("--print-id"), "{said}");
    }

    /// Every number of seconds from zero up is taken, however long, because
    /// the watch states no upper bound of its own.
    #[test]
    fn a_heartbeat_and_a_retry_take_any_whole_number_of_seconds() {
        for seconds in [86_401, i64::MAX] {
            let read = watch(&[
                ("heartbeat_s", json!(seconds)),
                ("reconnect_s", json!(seconds)),
            ])
            .expect("a long heartbeat and retry are taken");
            assert_eq!(read.heartbeat_s(), seconds.unsigned_abs());
            assert_eq!(read.reconnect_s(), seconds.unsigned_abs());
        }
    }

    /// Watching a print, the read lets the print's end through whatever the
    /// caller's filters say, and the caller's filters decide what is printed.
    #[test]
    fn watching_a_print_reads_its_end_whatever_the_filters_say() {
        let included = watch(&[
            ("print_id", json!(PRINT)),
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
            ("print_id", json!(PRINT)),
            ("exclude_kind", json!(["print_ended", "camera_look"])),
        ])
        .expect("a watch is read");
        let after: EventId = AFTER.parse().expect("an event identifier");
        let target = excluded.target(Some(after), 25);
        assert!(target.contains("exclude_kinds=camera_look"), "{target}");
        assert!(!target.contains("exclude_kinds=print_ended"), "{target}");
        assert!(
            target.starts_with(&format!("/v1/events?after={AFTER}&print_id={PRINT}")),
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
        let at = |text: &str| -> Timestamp { text.parse().expect("an instant") };
        let kind = &EventKind::new("periodic_observation").expect("a kind");
        assert!(passing.passes(&sampled, kind, at("2026-10-25T10:00:00Z")));
        assert!(!passing.passes(&sampled, kind, at("2026-10-25T10:04:59Z")));
        assert!(passing.passes(&sampled, kind, at("2026-10-25T10:05:00Z")));
        assert!(!passing.passes(&sampled, kind, at("2026-10-25T10:09:00Z")));
        assert!(passing.passes(&sampled, kind, at("2026-10-25T11:00:00Z")));
        assert!(passing.passes(
            &sampled,
            &EventKind::new("camera_look").expect("a kind"),
            at("2026-10-25T11:00:01Z")
        ));
    }
}
