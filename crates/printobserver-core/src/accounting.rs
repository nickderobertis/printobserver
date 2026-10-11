//! The printer timeline, and what every print takes from the spools that fed it.
//!
//! # A print is opened, charged in segments, and settled once
//!
//! Every print this system opens after spool tracking was installed — by a
//! start through it, by a read adopting a job somebody started at the printer,
//! or by an alert about a print it had not seen — gets a `print_started`
//! naming the spool on each tool. A print without one was already open when
//! tracking was installed, is never charged, and its close-out says so with one
//! `print_untracked`.
//!
//! A tool's filament is charged in **segments**, each under its own
//! [`ChargeKey`]: segment 0 opens at the start, and a spool loaded or unloaded
//! mid-print, or a filament-change signal, charges the open segment up to the
//! position read then and opens the next. Which segment is open is read back
//! from the store — one past the highest the print has charged or held on that
//! tool — and the closing segment is charged the tool's total less every
//! length already charged or held for that print and tool. Both are read from
//! what the store holds, never from memory, so a supervisor built after a
//! restart charges exactly what is left.
//!
//! Close-out works the print's remaining charges out, then settles it through
//! [`FilamentStore::settle_print`](crate::store::FilamentStore::settle_print)
//! — its charges, pends, marks, `print_ended` and settlement in one write —
//! and only then ends the print record. A close-out that finds the print
//! settled only ends it, so whatever is replayed after a crash debits nothing
//! twice.
//!
//! # After an unconfirmed filament change, the remainder is held
//!
//! A filament-change signal charges the loaded spool up to the stop point and
//! marks it as one whose change nobody confirmed. From then until a spool is
//! loaded onto that tool or that spool is confirmed in place, what the tool
//! feeds is not charged to the spool still recorded there: the open segment is
//! **pended**. Whether a segment is post-signal is read from the marks alone —
//! the spool judged for the tool still carries that print's filament-change
//! mark — so a restart between a confirmation and the close-out changes
//! nothing.

use std::collections::{BTreeMap, BTreeSet};

use printobserver_printer_api::{FileFacts, JobSnapshot, PrinterState, ToolFilament};
use printobserver_types::{
    EventBody, EventKind, EventPayload, EventSource, FileName, PrintId, Timestamp,
};
use printobserver_vision_api::PrinterSignal;

use crate::error::CoreError;
use crate::filament::grams_for_length;
use crate::kinds::{
    ActionExecutedPayload, ActionRequestedPayload, ChargePendedPayload,
    FilamentChangeSignalledPayload, FilamentConsumedPayload, PrintEndedPayload,
    PrintStartedPayload, PrintUntrackedPayload, SpoolUnverifiedPayload, system_source,
};
use crate::records::{
    ActionId, ChargeKey, ChargeMethod, FilePosition, LoadedSpool, PendingCharge, PendingTaken,
    Preflight, PrintAction, PrintOutcome, PrintRecord, SpoolId, SpoolRecord, SpoolTrust,
    UnverifiedReason,
};
use crate::store::{
    Charge, ChargeRecord, EventDraft, HistoryQuery, MAX_HISTORY_LIMIT, Pend, SpoolChange,
    SpoolMutation,
};
use crate::supervisor::Supervisor;

/// Below this many millimetres a length is nothing, and no charge is written
/// for it: a zero-length charge would be a record of nothing having happened.
pub(crate) const NOTHING_MM: f64 = 1e-6;

/// Why a print that predates spool tracking carries no charge.
pub const UNTRACKED_REASON: &str =
    "the print was already open when spool tracking was installed, so it is not charged";

/// The note a charge taken from a pending charge carries.
const TAKEN_NOTE: &str = "fed after a filament change nobody confirmed, and taken by this spool";

/// The note a post-signal segment charged to the spool loaded after it carries.
pub(crate) const AFTER_CHANGE: &str =
    "fed after a filament change nobody confirmed, and charged to the spool loaded after it";

/// What one print has had charged and held so far, as the store holds it.
#[derive(Debug, Clone, Default)]
pub(crate) struct PrintLedger {
    /// Every charge recorded against the print.
    charges: Vec<ChargeRecord>,
    /// Every charge still held for the print.
    pending: Vec<PendingCharge>,
}

/// The segment a print key names, when it is one.
const fn segment_of(key: &ChargeKey) -> Option<u32> {
    match key {
        ChargeKey::Print { segment, .. } => Some(*segment),
        ChargeKey::Unobserved { .. } => None,
    }
}

impl PrintLedger {
    /// Every length charged or held for the print on one tool, in millimetres.
    pub(crate) fn already_mm(&self, tool: i64) -> f64 {
        let charged: f64 = self
            .charges
            .iter()
            .filter(|charge| charge.tool == tool)
            .map(|charge| charge.length_mm)
            .sum();
        let held: f64 = self
            .pending
            .iter()
            .filter(|pending| pending.tool == tool)
            .map(|pending| pending.length_mm)
            .sum();
        charged + held
    }

    /// The segment open on one tool: one past the highest charged or held.
    pub(crate) fn open_segment(&self, tool: i64) -> u32 {
        let charged = self
            .charges
            .iter()
            .filter(|charge| charge.tool == tool)
            .filter_map(|charge| segment_of(&charge.key));
        let held = self
            .pending
            .iter()
            .filter(|pending| pending.tool == tool)
            .filter_map(|pending| segment_of(&pending.charge_key));
        charged.chain(held).max().map_or(0, |highest| highest + 1)
    }

    /// The spool the latest charged segment on one tool was charged to.
    pub(crate) fn last_spool(&self, tool: i64) -> Option<SpoolId> {
        self.charges
            .iter()
            .filter(|charge| charge.tool == tool)
            .max_by_key(|charge| segment_of(&charge.key))
            .and_then(|charge| charge.spool_id)
    }

    /// Every tool the print has charged or held anything on.
    fn tools(&self) -> BTreeSet<i64> {
        self.charges
            .iter()
            .map(|charge| charge.tool)
            .chain(self.pending.iter().map(|pending| pending.tool))
            .collect()
    }
}

/// The figure each spool stands at as one write's charges are worked out, so
/// that two charges against one spool in one write chain rather than both
/// starting from the figure read.
#[derive(Debug, Default)]
pub(crate) struct Figures {
    /// Each spool's figure so far, once it has been charged in this write.
    remaining: BTreeMap<SpoolId, f64>,
}

impl Figures {
    /// One spool's figure before and after a debit of so many grams.
    fn debit(&mut self, spool: &SpoolRecord, grams: f64) -> (f64, f64) {
        let before = *self.remaining.get(&spool.id).unwrap_or(&spool.remaining_g);
        let after = before - grams;
        self.remaining.insert(spool.id, after);
        (before, after)
    }

    /// One spool's figure once every debit so far has been taken.
    pub(crate) fn now(&self, spool: &SpoolRecord) -> f64 {
        *self.remaining.get(&spool.id).unwrap_or(&spool.remaining_g)
    }
}

/// One stretch of filament a tool fed, before it is charged or held.
#[derive(Debug, Clone)]
pub(crate) struct Feed {
    /// The durable identity of the debit, which names the tool that fed it
    /// and, for a print this system watched, the print.
    pub(crate) key: ChargeKey,
    /// How the length was worked out.
    pub(crate) method: ChargeMethod,
    /// The length fed, in millimetres.
    pub(crate) length_mm: f64,
    /// The file's analysed length for the tool, when it had one.
    pub(crate) analysed_length_mm: Option<f64>,
    /// How far into the file the stretch runs, when that was read.
    pub(crate) position: Option<FilePosition>,
    /// Anything a reader needs to make sense of the figure.
    pub(crate) note: Option<String>,
}

impl Feed {
    /// The tool that fed it, as its key names it.
    const fn tool(&self) -> i64 {
        match &self.key {
            ChargeKey::Print { tool, .. } | ChargeKey::Unobserved { tool, .. } => *tool,
        }
    }

    /// The print it was fed in, when its key names one.
    const fn print_id(&self) -> Option<PrintId> {
        match &self.key {
            ChargeKey::Print { print_id, .. } => Some(*print_id),
            ChargeKey::Unobserved { .. } => None,
        }
    }
}

/// Where a print is, and how that is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Whereabouts {
    /// Read from the job just now.
    ReadNow(FilePosition),
    /// The last position recorded, the job reporting none now.
    Recorded(FilePosition),
    /// No position has been read for it at all.
    Unknown,
}

impl Whereabouts {
    /// The position, however it is known.
    const fn position(&self) -> Option<&FilePosition> {
        match self {
            Self::ReadNow(position) | Self::Recorded(position) => Some(position),
            Self::Unknown => None,
        }
    }
}

/// The filament each tool fed up to a position, and how that was worked out.
#[derive(Debug, Clone)]
pub(crate) struct Extrusion {
    /// The length per tool, in millimetres.
    pub(crate) lengths: BTreeMap<i64, f64>,
    /// How the lengths were worked out.
    pub(crate) method: ChargeMethod,
    /// Why, when the lengths are a fallback.
    pub(crate) note: Option<String>,
}

/// What the printer says of a print's file: its facts and its analysis.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileAnalysis {
    /// The file's facts, when they could be read.
    pub(crate) facts: Option<FileFacts>,
    /// The analysed length per tool, when the file has an analysis.
    pub(crate) lengths: Option<BTreeMap<i64, f64>>,
}

impl FileAnalysis {
    /// The analysed length for one tool, when the file has an analysis.
    pub(crate) fn length(&self, tool: i64) -> Option<f64> {
        self.lengths
            .as_ref()
            .map(|lengths| lengths.get(&tool).copied().unwrap_or(0.0))
    }

    /// The file's size in bytes, when it is known.
    fn size_bytes(&self) -> Option<i64> {
        self.facts.as_ref().and_then(|facts| facts.size_bytes)
    }
}

/// The lengths per tool one list of tool filament adds up to.
///
/// A printer's answer is read at this boundary: a tool numbered below zero, or
/// a length that is negative or not a number, is no filament any tool fed, and
/// is left out rather than charged.
pub(crate) fn lengths_of(filament: &[ToolFilament]) -> BTreeMap<i64, f64> {
    let mut lengths = BTreeMap::new();
    for each in filament
        .iter()
        .filter(|each| each.tool >= 0 && each.length_mm.is_finite() && each.length_mm >= 0.0)
    {
        *lengths.entry(each.tool).or_insert(0.0) += each.length_mm;
    }
    // Lengths that are each finite can still sum past what a figure holds;
    // such a tool's total is no length either.
    lengths.retain(|_, total| total.is_finite());
    lengths
}

/// What a tool fed of a print's open segment, up to a position, when it fed
/// anything since what the print has already charged or held on it.
pub(crate) fn feed_to(
    print_id: PrintId,
    tool: i64,
    ledger: &PrintLedger,
    extrusion: &Extrusion,
    position: &FilePosition,
    analysis: &FileAnalysis,
) -> Option<Feed> {
    let length = extrusion.lengths.get(&tool).copied().unwrap_or(0.0) - ledger.already_mm(tool);
    (length > NOTHING_MM).then(|| Feed {
        key: ChargeKey::Print {
            print_id,
            tool,
            segment: ledger.open_segment(tool),
        },
        method: extrusion.method,
        length_mm: length,
        analysed_length_mm: analysis.length(tool),
        position: Some(position.clone()),
        note: extrusion.note.clone(),
    })
}

/// A print's file, as a name a file API takes, when it has one.
fn file_of(print: &PrintRecord) -> Option<FileName> {
    print
        .file_name
        .as_deref()
        .and_then(|name| FileName::new(name).ok())
}

/// Whether a spool holds one print's filament-change mark, of either kind.
pub(crate) fn holds_change_mark(spool: &SpoolRecord, print_id: PrintId) -> bool {
    marks_of(spool).iter().any(|reason| {
        matches!(
            reason,
            UnverifiedReason::FilamentChangeUnconfirmed { print_id: held }
            | UnverifiedReason::FilamentChangeUnconfirmedPosition { print_id: held }
            if *held == print_id
        )
    })
}

/// Whether a spool holds any filament-change mark at all.
pub(crate) fn holds_any_change_mark(spool: &SpoolRecord) -> bool {
    marks_of(spool).iter().any(|reason| {
        matches!(
            reason,
            UnverifiedReason::FilamentChangeUnconfirmed { .. }
                | UnverifiedReason::FilamentChangeUnconfirmedPosition { .. }
        )
    })
}

/// Every reason a spool's figure is in doubt, oldest first.
pub(crate) fn marks_of(spool: &SpoolRecord) -> Vec<UnverifiedReason> {
    match &spool.trust {
        SpoolTrust::Verified => Vec::new(),
        SpoolTrust::Unverified { marks } => marks.iter().map(|mark| mark.reason.clone()).collect(),
    }
}

/// The spool on one tool: the unretired one when there is one, otherwise a
/// spool retired while it was loaded there — whose charges for what it fed are
/// still its own.
pub(crate) fn spool_on_tool(spools: &[SpoolRecord], tool: i64) -> Option<&SpoolRecord> {
    let on_tool = || {
        spools
            .iter()
            .filter(move |spool| spool.loaded_on_tool == Some(tool))
    };
    on_tool()
        .find(|spool| spool.retired_at.is_none())
        .or_else(|| on_tool().max_by_key(|spool| spool.updated_at))
}

/// The open position a job read reports for a print, when it reports one.
pub(crate) fn position_from(
    job: &JobSnapshot,
    print: &PrintRecord,
    observed_at: Timestamp,
) -> Option<FilePosition> {
    let active = matches!(job.state, PrinterState::Printing | PrinterState::Paused);
    let same_file = print.file_name.is_none() || job.file_name == print.file_name;
    if !active || !same_file {
        return None;
    }
    // A position before the file's start is no position, and a size of
    // nothing is no size: neither is kept.
    job.file_position_bytes
        .filter(|bytes| *bytes >= 0)
        .map(|bytes| FilePosition {
            bytes,
            size_bytes: job.size_bytes.filter(|size| *size > 0),
            observed_at,
        })
}

/// What a tracked print's close-out writes: its charges, its pends and its
/// marks, in that order.
type ClosingWrites = (Vec<Charge>, Vec<Pend>, Vec<SpoolMutation>);

/// What a tracked print's close-out read.
#[derive(Debug, Clone)]
struct Closing {
    /// How the print ended.
    outcome: PrintOutcome,
    /// Whether anything was watching as it ended: unfinished and unwatched,
    /// what it fed after the last position recorded is unknown.
    watch: Watch,
    /// What each tool fed by the outcome's rule, when that could be worked out.
    totals: Option<Extrusion>,
    /// What the printer says of the print's file.
    analysis: FileAnalysis,
    /// The last position recorded.
    position: Option<FilePosition>,
}

/// How a print ended, from everything its close-out read.
#[derive(Debug, Clone, Copy)]
struct Ending<'a> {
    /// The state the printer was in as it ended.
    state: &'a PrinterState,
    /// The last job read, when one was made.
    job: Option<&'a JobSnapshot>,
    /// The print's file, when it is known as a name a file API takes.
    file_name: Option<&'a FileName>,
    /// The last position recorded.
    position: Option<&'a FilePosition>,
    /// The file's size, when it is known.
    size_bytes: Option<i64>,
    /// What the file's tally says of a print since this one was opened.
    tally: Tally,
    /// Who cancelled it, as far as this system knows.
    cancelled_by: Canceller,
}

/// Whether a print was watched to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watch {
    /// This supervisor saw it running, or it completed: what it fed is known.
    Watched,
    /// It ended unfinished while nothing was watching.
    Unwatched,
}

/// What a file's tally says of a print since one was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tally {
    /// A successful print of the file was recorded since.
    CompletedSince,
    /// Nothing the tally says shows this print completed.
    NoCompletion,
}

/// Who cancelled a print, as far as this system knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Canceller {
    /// This system's own cancel was executed for it.
    Here,
    /// No cancel of this system's was executed for it.
    NotHere,
}

impl Ending<'_> {
    /// The outcome these say the print ended with.
    fn outcome(&self) -> PrintOutcome {
        let reached_the_end = self.position.is_some_and(|position| {
            let size = position.size_bytes.or(self.size_bytes);
            size.is_some_and(|size| size > 0 && position.bytes >= size)
        });
        let reported_complete = self.job.is_some_and(|job| {
            self.file_name
                .is_none_or(|file| job.file_name.as_deref() == Some(file.as_str()))
                && job
                    .completion
                    .is_some_and(|completion| completion.value() >= 1.0)
        });
        if reached_the_end || reported_complete || self.tally == Tally::CompletedSince {
            return PrintOutcome::Completed;
        }
        if self.cancelled_by == Canceller::Here || *self.state == PrinterState::Cancelling {
            return PrintOutcome::Cancelled;
        }
        match self.state {
            PrinterState::Error | PrinterState::Offline => PrintOutcome::Failed,
            _ => PrintOutcome::Cancelled,
        }
    }
}

impl Supervisor {
    /// One event draft this system writes now, about a print or about none.
    pub(crate) fn draft_of<P: EventPayload>(
        &self,
        print_id: Option<PrintId>,
        source: EventSource,
        payload: &P,
    ) -> Result<EventDraft, CoreError> {
        Ok(EventDraft {
            print_id,
            source,
            received_at: self.clock().now(),
            body: EventBody::of(payload)?,
            raw: None,
        })
    }

    /// The charge one feed makes against a spool, or records uncharged with no
    /// spool, carrying its `filament_consumed` event.
    pub(crate) fn charge_of(
        &self,
        feed: Feed,
        spool: Option<&SpoolRecord>,
        figures: &mut Figures,
        source: EventSource,
    ) -> Result<Charge, CoreError> {
        let grams = spool
            .map(|spool| grams_for_length(feed.length_mm, spool.diameter_mm, spool.density_g_cm3));
        let figure = match (spool, grams) {
            (Some(spool), Some(grams)) => Some(figures.debit(spool, grams)),
            _ => None,
        };
        let payload = FilamentConsumedPayload {
            charge_key: feed.key.clone(),
            tool: feed.tool(),
            spool_id: spool.map(|spool| spool.id),
            method: feed.method,
            length_mm: feed.length_mm,
            grams,
            diameter_mm: spool.map(|spool| spool.diameter_mm),
            density_g_cm3: spool.map(|spool| spool.density_g_cm3),
            analysed_length_mm: feed.analysed_length_mm,
            position: feed.position.clone(),
            remaining_before_g: figure.map(|(before, _)| before),
            remaining_after_g: figure.map(|(_, after)| after),
            note: feed.note.clone(),
        };
        let event = self.draft_of(feed.print_id(), source, &payload)?;
        Ok(Charge {
            tool: feed.tool(),
            print_id: feed.print_id(),
            key: feed.key,
            spool_id: spool.map(|spool| spool.id),
            length_mm: feed.length_mm,
            grams,
            at: event.received_at,
            event,
        })
    }

    /// The pend one feed makes, held for the next spool loaded onto its tool,
    /// carrying its `charge_pended` event.
    pub(crate) fn pend_of(&self, feed: Feed, print_id: PrintId) -> Result<Pend, CoreError> {
        let pending = PendingCharge {
            tool: feed.tool(),
            charge_key: feed.key,
            print_id,
            length_mm: feed.length_mm,
            method: feed.method,
            analysed_length_mm: feed.analysed_length_mm,
            position: feed.position,
            note: feed.note,
            pended_at: self.clock().now(),
        };
        let event = self.draft_of(
            Some(print_id),
            system_source(),
            &ChargePendedPayload {
                pending: pending.clone(),
            },
        )?;
        Ok(Pend { pending, event })
    }

    /// The mutation that marks one spool's figure as in doubt, with its
    /// `spool_unverified` event.
    pub(crate) fn mark_of(
        &self,
        spool_id: SpoolId,
        reason: UnverifiedReason,
        print_id: Option<PrintId>,
    ) -> Result<SpoolMutation, CoreError> {
        let event = self.draft_of(
            print_id,
            system_source(),
            &SpoolUnverifiedPayload {
                spool_id,
                reason: reason.clone(),
            },
        )?;
        Ok(SpoolMutation {
            spool_id,
            change: SpoolChange::MarkUnverified { reason },
            at: event.received_at,
            event,
        })
    }

    /// Every pending charge a spool takes, each charged to it under its own
    /// key and named for the command's own event.
    pub(crate) fn take_pending(
        &self,
        pending: &[PendingCharge],
        spool: &SpoolRecord,
        figures: &mut Figures,
        source: &EventSource,
    ) -> Result<(Vec<Charge>, Vec<PendingTaken>), CoreError> {
        let mut charges = Vec::new();
        let mut taken = Vec::new();
        for held in pending {
            let note = match &held.note {
                Some(note) => format!("{TAKEN_NOTE}; {note}"),
                None => TAKEN_NOTE.to_owned(),
            };
            charges.push(self.charge_of(
                Feed {
                    key: held.charge_key.clone(),
                    method: held.method,
                    length_mm: held.length_mm,
                    analysed_length_mm: held.analysed_length_mm,
                    position: held.position.clone(),
                    note: Some(note),
                },
                Some(spool),
                figures,
                source.clone(),
            )?);
            taken.push(PendingTaken {
                charge_key: held.charge_key.clone(),
                print_id: held.print_id,
            });
        }
        Ok((charges, taken))
    }

    /// What one print has had charged and held so far.
    pub(crate) async fn print_ledger(&self, print_id: PrintId) -> Result<PrintLedger, CoreError> {
        let charges = self.stores().filament.charges(print_id).await?;
        let pending = self
            .stores()
            .filament
            .pending_charges(None)
            .await?
            .into_iter()
            .filter(|held| held.print_id == print_id)
            .collect();
        Ok(PrintLedger { charges, pending })
    }

    /// The `print_started` a print was opened with, when it has one.
    pub(crate) async fn print_started_of(
        &self,
        print_id: PrintId,
    ) -> Result<Option<PrintStartedPayload>, CoreError> {
        let found = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![PrintStartedPayload::kind()],
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?;
        Ok(found
            .first()
            .and_then(printobserver_types::EventRecord::payload_as::<PrintStartedPayload>)
            .transpose()?)
    }

    /// Write a print's `print_started`, naming the spool on each tool now.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub(crate) async fn record_print_started(
        &self,
        print_id: PrintId,
        file_name: Option<String>,
        started_by: Option<ActionId>,
        preflight: Option<Preflight>,
    ) -> Result<(), CoreError> {
        let loaded = self
            .stores()
            .filament
            .loaded_spools()
            .await?
            .into_iter()
            .filter_map(|spool| {
                spool.loaded_on_tool.map(|tool| LoadedSpool {
                    tool,
                    spool_id: spool.id,
                })
            })
            .collect();
        let draft = self.draft_of(
            Some(print_id),
            system_source(),
            &PrintStartedPayload {
                file_name,
                started_by,
                loaded,
                preflight,
            },
        )?;
        self.stores().events.append_event(draft).await?;
        Ok(())
    }

    /// Record how far into its file a print had got, from one job read.
    pub(crate) async fn record_position_from(
        &self,
        print: &PrintRecord,
        job: &JobSnapshot,
        observed_at: Timestamp,
    ) -> Result<Option<FilePosition>, CoreError> {
        let Some(position) = position_from(job, print, observed_at) else {
            return Ok(None);
        };
        self.stores()
            .filament
            .record_file_position(print.id, position.clone())
            .await?;
        Ok(Some(position))
    }

    /// What the printer says of one file: its facts, and the analysed length
    /// per tool from the job read when it carries one, otherwise from the
    /// file's own facts.
    pub(crate) async fn analysis_of(
        &self,
        file_name: &FileName,
        job: Option<&JobSnapshot>,
    ) -> FileAnalysis {
        let facts = self.read_file_facts(file_name.clone()).await.ok();
        let from_job = job
            .filter(|job| {
                job.file_name.as_deref() == Some(file_name.as_str()) && !job.filament.is_empty()
            })
            .map(|job| lengths_of(&job.filament));
        let lengths = from_job.or_else(|| {
            facts
                .as_ref()
                .and_then(|facts| facts.filament.as_deref().map(lengths_of))
        });
        FileAnalysis { facts, lengths }
    }

    /// The filament each tool fed up to a position: summed from the file's own
    /// moves, or — when the file could not be read — the analysed length
    /// scaled by how far into the file the position is, with a note saying
    /// why. Nothing when neither can be worked out.
    pub(crate) async fn extrusion_to(
        &self,
        file_name: &FileName,
        position: &FilePosition,
        analysis: &FileAnalysis,
    ) -> Option<Extrusion> {
        let refusal = match self
            .read_extruded_to(file_name.clone(), position.bytes)
            .await
        {
            Ok(filament) => {
                return Some(Extrusion {
                    lengths: lengths_of(&filament),
                    method: ChargeMethod::GcodeExtrusion,
                    note: None,
                });
            }
            Err(error) => error.to_string(),
        };
        let analysed = analysis.lengths.as_ref()?;
        let size = position
            .size_bytes
            .or_else(|| analysis.size_bytes())
            .filter(|size| *size > 0)?;
        #[expect(clippy::cast_precision_loss)]
        let fraction = (position.bytes as f64 / size as f64).clamp(0.0, 1.0);
        Some(Extrusion {
            lengths: analysed
                .iter()
                .map(|(tool, length)| (*tool, length * fraction))
                .collect(),
            method: ChargeMethod::FileFraction,
            note: Some(format!(
                "the file could not be read ({refusal}), so the analysed length was scaled by \
                 how far into the file the print got"
            )),
        })
    }

    /// Whether this system's own cancel was executed for a print.
    async fn cancelled_here(&self, print_id: PrintId) -> Result<bool, CoreError> {
        let events = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![
                    ActionRequestedPayload::kind(),
                    ActionExecutedPayload::kind(),
                ],
                since: None,
                until: None,
                limit: Some(MAX_HISTORY_LIMIT),
            })
            .await?;
        let executed: BTreeSet<ActionId> = events
            .iter()
            .filter_map(printobserver_types::EventRecord::payload_as::<ActionExecutedPayload>)
            .filter_map(Result::ok)
            .map(|payload| payload.action_id)
            .collect();
        Ok(events
            .iter()
            .filter_map(printobserver_types::EventRecord::payload_as::<ActionRequestedPayload>)
            .filter_map(Result::ok)
            .any(|payload| {
                matches!(payload.action, PrintAction::Cancel { .. })
                    && executed.contains(&payload.action_id)
            }))
    }

    /// Settle one print's filament as it is closed out, unless it is settled
    /// already.
    ///
    /// A print with no `print_started` predates spool tracking: it is settled
    /// with no charge and one `print_untracked` — when a job was ever seen for
    /// it at all; a print a refused start opened and nothing ever ran is not
    /// a print of the timeline. Any other print is charged by the rule of its
    /// outcome, as this module states.
    // llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    pub(crate) async fn settle_filament(
        &self,
        print: &PrintRecord,
        state: &PrinterState,
        reason: &str,
        closing_job: Option<&JobSnapshot>,
    ) -> Result<(), CoreError> {
        if self
            .stores()
            .filament
            .print_settlement(print.id)
            .await?
            .is_some()
        {
            return Ok(());
        }
        let Some(started) = self.print_started_of(print.id).await? else {
            return self
                .settle_untracked(print, state, reason, closing_job)
                .await;
        };
        let closing = self.closing(print, &started, state, closing_job).await?;
        let ended = self.draft_of(
            Some(print.id),
            system_source(),
            &PrintEndedPayload {
                outcome: closing.outcome,
                state: state.clone(),
                reason: reason.to_owned(),
                last_position: closing.position.clone(),
            },
        )?;
        let _writing = self.spool_writes().await;
        let spools = self.stores().filament.spools(true).await?;
        let ledger = self.print_ledger(print.id).await?;
        let mut used: BTreeSet<i64> = ledger.tools();
        used.extend(started.loaded.iter().map(|loaded| loaded.tool));
        let (charges, pends, marks) =
            self.closing_writes(print.id, used, &closing, &spools, &ledger)?;
        self.stores()
            .filament
            .settle_print(print.id, closing.outcome, ended, charges, pends, marks)
            .await?;
        Ok(())
    }

    /// Everything a tracked print's close-out reads before it works out what
    /// to charge: how it ended, and what each tool fed by that outcome's rule.
    async fn closing(
        &self,
        print: &PrintRecord,
        started: &PrintStartedPayload,
        state: &PrinterState,
        closing_job: Option<&JobSnapshot>,
    ) -> Result<Closing, CoreError> {
        let read = if closing_job.is_none() {
            self.read_job().await.ok()
        } else {
            None
        };
        let job = closing_job.or(read.as_ref());
        let file_name = print
            .file_name
            .as_deref()
            .or(started.file_name.as_deref())
            .and_then(|name| FileName::new(name).ok());
        let analysis = match &file_name {
            Some(file) => self.analysis_of(file, job).await,
            None => FileAnalysis::default(),
        };
        let position = self.stores().filament.file_position(print.id).await?;
        // A print completes after it was opened, whoever opened it; the job's
        // own start, as a read puts it, can lie before that, and a success
        // recorded between the two is an earlier print's.
        let began = print.opened_at;
        let tally = if analysis
            .facts
            .as_ref()
            .and_then(|facts| facts.last_print.as_ref())
            .is_some_and(|last| last.success && last.at >= began)
        {
            Tally::CompletedSince
        } else {
            Tally::NoCompletion
        };
        let cancelled_by = if self.cancelled_here(print.id).await? {
            Canceller::Here
        } else {
            Canceller::NotHere
        };
        let outcome = Ending {
            state,
            job,
            file_name: file_name.as_ref(),
            position: position.as_ref(),
            size_bytes: analysis.size_bytes(),
            tally,
            cancelled_by,
        }
        .outcome();
        let totals = match (outcome, &file_name, &position) {
            (PrintOutcome::Completed, _, _) => analysis.lengths.clone().map(|lengths| Extrusion {
                lengths,
                method: ChargeMethod::SlicerEstimate,
                note: None,
            }),
            (_, Some(file), Some(position)) => self.extrusion_to(file, position, &analysis).await,
            _ => None,
        };
        Ok(Closing {
            outcome,
            watch: if self.was_seen_running(print.id) || outcome == PrintOutcome::Completed {
                Watch::Watched
            } else {
                Watch::Unwatched
            },
            totals,
            analysis,
            position,
        })
    }

    /// What a tracked print's close-out writes: each tool's remainder charged
    /// to the spool on it, or held when the tool's last change is unconfirmed,
    /// and every spool whose figure the print leaves in doubt marked.
    fn closing_writes(
        &self,
        print_id: PrintId,
        mut used: BTreeSet<i64>,
        closing: &Closing,
        spools: &[SpoolRecord],
        ledger: &PrintLedger,
    ) -> Result<ClosingWrites, CoreError> {
        let mut figures = Figures::default();
        let mut charges = Vec::new();
        let mut pends = Vec::new();
        let mut marks = Vec::new();
        let uncharged = UnverifiedReason::UnchargedPrint { print_id };
        let Some(totals) = &closing.totals else {
            for tool in used {
                if let Some(spool) = spool_on_tool(spools, tool) {
                    marks.push(self.mark_of(spool.id, uncharged.clone(), Some(print_id))?);
                }
            }
            return Ok((charges, pends, marks));
        };
        used.extend(totals.lengths.keys().copied());
        for tool in used {
            let on_tool = spool_on_tool(spools, tool);
            if closing.watch == Watch::Unwatched
                && let Some(spool) = on_tool
            {
                marks.push(self.mark_of(spool.id, uncharged.clone(), Some(print_id))?);
            }
            let total = totals.lengths.get(&tool).copied().unwrap_or(0.0);
            let remainder = total - ledger.already_mm(tool);
            if remainder <= NOTHING_MM {
                continue;
            }
            let feed = Feed {
                key: ChargeKey::Print {
                    print_id,
                    tool,
                    segment: ledger.open_segment(tool),
                },
                method: totals.method,
                length_mm: remainder,
                analysed_length_mm: closing.analysis.length(tool),
                position: closing.position.clone(),
                note: totals.note.clone(),
            };
            let judged = on_tool.or_else(|| {
                ledger
                    .last_spool(tool)
                    .and_then(|id| spools.iter().find(|spool| spool.id == id))
            });
            if judged.is_some_and(|spool| holds_change_mark(spool, print_id)) {
                pends.push(self.pend_of(feed, print_id)?);
            } else {
                charges.push(self.charge_of(feed, on_tool, &mut figures, system_source())?);
            }
        }
        Ok((charges, pends, marks))
    }

    /// Settle a print with no `print_started`: no charge, one `print_untracked`
    /// and one `print_ended`, when a job was ever seen for it.
    async fn settle_untracked(
        &self,
        print: &PrintRecord,
        state: &PrinterState,
        reason: &str,
        closing_job: Option<&JobSnapshot>,
    ) -> Result<(), CoreError> {
        let ran = print.provider_print_id.is_some()
            || print.job_started_at.is_some()
            || print.job_print_time_s.is_some();
        if !ran {
            return Ok(());
        }
        let named = print
            .file_name
            .as_deref()
            .and_then(|name| FileName::new(name).ok());
        let outcome = Ending {
            state,
            job: closing_job,
            file_name: named.as_ref(),
            position: None,
            size_bytes: None,
            tally: Tally::NoCompletion,
            cancelled_by: Canceller::NotHere,
        }
        .outcome();
        // The `print_untracked` goes first, once: a close-out replayed after
        // it — the settlement refused, or committed with its answer lost —
        // finds it written and writes no second, and one replayed after the
        // settlement finds that and writes nothing at all.
        let written = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id: print.id,
                kinds: vec![PrintUntrackedPayload::kind()],
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?;
        if written.is_empty() {
            let draft = self.draft_of(
                Some(print.id),
                system_source(),
                &PrintUntrackedPayload {
                    reason: UNTRACKED_REASON.to_owned(),
                },
            )?;
            self.stores().events.append_event(draft).await?;
        }
        let ended = self.draft_of(
            Some(print.id),
            system_source(),
            &PrintEndedPayload {
                outcome,
                state: state.clone(),
                reason: reason.to_owned(),
                last_position: None,
            },
        )?;
        self.stores()
            .filament
            .settle_print(print.id, outcome, ended, Vec::new(), Vec::new(), Vec::new())
            .await?;
        Ok(())
    }

    /// The print a spool command made now happens during, when one is: the
    /// most recently opened open print that is tracked and not yet settled.
    pub(crate) async fn print_during(&self) -> Result<Option<PrintRecord>, CoreError> {
        for print in self.stores().prints.open_prints().await? {
            if self
                .stores()
                .filament
                .print_settlement(print.id)
                .await?
                .is_none()
                && self.print_started_of(print.id).await?.is_some()
            {
                return Ok(Some(print));
            }
        }
        Ok(None)
    }

    /// Where a print is now: read afresh from the job when it reports one,
    /// recorded as it is read, and otherwise the last position recorded —
    /// with the job the read found, when it found one.
    pub(crate) async fn position_now(
        &self,
        print: &PrintRecord,
    ) -> Result<(Whereabouts, Option<JobSnapshot>), CoreError> {
        let job = self.read_job().await.ok();
        let observed_at = self.clock().now();
        if let Some(job) = &job
            && let Some(position) = self.record_position_from(print, job, observed_at).await?
        {
            self.note_seen_running(print.id);
            return Ok((Whereabouts::ReadNow(position), Some(job.clone())));
        }
        let whereabouts = match self.stores().filament.file_position(print.id).await? {
            Some(recorded) => Whereabouts::Recorded(recorded),
            None => Whereabouts::Unknown,
        };
        Ok((whereabouts, job))
    }

    /// The filament each tool fed up to where a print is now, and that
    /// position — what a spool command made during it charges against.
    pub(crate) async fn extrusion_now(
        &self,
        print: &PrintRecord,
    ) -> Result<Option<(FilePosition, Extrusion, FileAnalysis)>, CoreError> {
        let Some(file_name) = file_of(print) else {
            return Ok(None);
        };
        let (whereabouts, job) = self.position_now(print).await?;
        let Some(position) = whereabouts.position().cloned() else {
            return Ok(None);
        };
        let analysis = self.analysis_of(&file_name, job.as_ref()).await;
        Ok(self
            .extrusion_to(&file_name, &position, &analysis)
            .await
            .map(|extrusion| (position, extrusion, analysis)))
    }

    /// Handle what the printer itself signalled, as an alert relayed it.
    ///
    /// A filament change reads the job afresh — the printer is paused at the
    /// change, so that is where the spool stopped feeding — charges the spool
    /// on each tool up to it, writes `filament_change_signalled`, and marks
    /// the spool as one whose change nobody has confirmed. When no fresh
    /// position can be read it charges up to the last position recorded and
    /// says so, with the mark that says its figure may be overstated. A spool
    /// already holding the print's mark — a second signal before any load —
    /// has its open segment pended instead, and is marked again.
    // llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    pub(crate) async fn handle_signal(
        &self,
        print: &PrintRecord,
        signal: PrinterSignal,
        source_kind: &EventKind,
    ) -> Result<(), CoreError> {
        let PrinterSignal::FilamentChange = signal;
        if print.ended_at.is_some()
            || self
                .stores()
                .filament
                .print_settlement(print.id)
                .await?
                .is_some()
            || self.print_started_of(print.id).await?.is_none()
        {
            return Ok(());
        }
        let (whereabouts, job) = self.position_now(print).await?;
        let read_now = matches!(whereabouts, Whereabouts::ReadNow(_));
        let position = whereabouts.position().cloned();
        let file_name = file_of(print);
        let analysis = match &file_name {
            Some(file) => self.analysis_of(file, job.as_ref()).await,
            None => FileAnalysis::default(),
        };
        let extrusion = match (&file_name, &position) {
            (Some(file), Some(position)) => self.extrusion_to(file, position, &analysis).await,
            _ => None,
        };
        let signalled = |tool: Option<i64>, spool_id: Option<SpoolId>| {
            self.draft_of(
                Some(print.id),
                system_source(),
                &FilamentChangeSignalledPayload {
                    tool,
                    spool_id,
                    source_kind: source_kind.clone(),
                    position: position.clone(),
                    position_read_at_signal: read_now,
                },
            )
        };
        let reason = if read_now {
            UnverifiedReason::FilamentChangeUnconfirmed { print_id: print.id }
        } else {
            UnverifiedReason::FilamentChangeUnconfirmedPosition { print_id: print.id }
        };

        let _writing = self.spool_writes().await;
        let loaded = self.stores().filament.loaded_spools().await?;
        if loaded.is_empty() {
            self.stores()
                .events
                .append_event(signalled(None, None)?)
                .await?;
            return Ok(());
        }
        let ledger = self.print_ledger(print.id).await?;
        let mut figures = Figures::default();
        let mut charges = Vec::new();
        let mut pends = Vec::new();
        let mut marks = Vec::new();
        for spool in &loaded {
            let Some(tool) = spool.loaded_on_tool else {
                continue;
            };
            self.stores()
                .events
                .append_event(signalled(Some(tool), Some(spool.id))?)
                .await?;
            let feed = match (&extrusion, &position) {
                (Some(extrusion), Some(position)) => {
                    feed_to(print.id, tool, &ledger, extrusion, position, &analysis)
                }
                _ => None,
            };
            match feed {
                Some(feed) if holds_change_mark(spool, print.id) => {
                    pends.push(self.pend_of(feed, print.id)?);
                }
                Some(feed) => {
                    charges.push(self.charge_of(
                        feed,
                        Some(spool),
                        &mut figures,
                        system_source(),
                    )?);
                }
                None => {}
            }
            marks.push(self.mark_of(spool.id, reason.clone(), Some(print.id))?);
        }
        self.stores()
            .filament
            .apply_spool_changes(marks, charges, pends)
            .await?;
        Ok(())
    }
}
