//! Whether the failure detector is scoring the active print.
//!
//! This system never receives the detector's per-frame scores. Its health
//! check ([`crate::DetectorProbe`]) says whether the detector's service
//! answers, and its failure alerts arrive by webhook — and neither says that
//! the detector is looking at this print at all. What does is the detector's
//! own record of the printer's live prediction: the print it is watching, its
//! latest snapshot and its latest score. A [`ScoringProbe`] the server supplies
//! reads that record once per observation interval, at the start of every
//! round of periodic observations.
//!
//! # Scoring evidence
//!
//! The record carries no instant for its prediction, and a score of zero means
//! both "no score yet" and "scored, nothing flagged", so a single read proves
//! nothing. The evidence is a change: the detector is scoring the active print
//! when the print its record names is that print, and its snapshot or its
//! score moved between two of this server's reads within the last
//! [`EVIDENCE_INTERVALS`] observation intervals. A read naming another print,
//! or an earlier one, is evidence that it is not, and ends any evidence held.
//! A read that fails is recorded as a port failure against each observation
//! the round writes, and leaves the evidence standing until it is older than
//! those intervals.
//!
//! The status names it flagged once a failure alert of the detector's own has
//! arrived for the print — noted as the alert is received, and read back out
//! of the print's history at each read, so a server started again during the
//! print still says so. A history that cannot be read takes nothing from that
//! round's read, and is recorded as the read failing.
//!
//! # Which print the record names
//!
//! By the provider's own identifier when the open print carries one. Otherwise
//! by its file and its start, held tighter than an alert's: the start the record
//! names must lie within [`JOB_IDENTITY_TOLERANCE_S`] of the earliest start
//! this server has sighted for the print's job, on either side.
//!
//! The earliest sighting, not the start the print records now, because a
//! resume moves the recorded start later by the length of the pause — the
//! printer takes the pause back out of the job's running time — while the
//! provider keeps the job's own start. The earliest sighting was taken before
//! any pause this server watched, so it stays at the job's own start; an
//! earlier job of the same file began long before it, which an alert's rule
//! (an earlier start is no objection) could not tell apart. A print this
//! server first sighted only after a pause is matched by nothing until an
//! alert names its provider identifier: its status then reads no scoring
//! rather than another print's. Nor is a print matched with either start
//! missing: a file name alone names every job of that file.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use printobserver_printer_api::BoxFuture;
use printobserver_types::{EventKind, PrintId, Timestamp};
use printobserver_vision_api::ProviderPrint;

use crate::clock::seconds_between;
use crate::error::CoreError;
use crate::listing::JOB_IDENTITY_TOLERANCE_S;
use crate::records::PrintRecord;
use crate::store::HistoryQuery;
use crate::supervisor::Supervisor;

/// How many observation intervals scoring evidence stands for.
pub const EVIDENCE_INTERVALS: u32 = 2;

/// What reads the failure detector's own record of the printer's live
/// prediction for the status.
pub trait ScoringProbe: Send + Sync {
    /// Read the record once.
    fn read_scoring(&self) -> BoxFuture<'_, ScoringRead>;

    /// The kind the detector's own failure alerts are written down under, so
    /// a print's history says whether one has arrived for it.
    fn alert_kind(&self) -> EventKind;
}

/// What one read of the detector's record answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoringRead {
    /// No read was made, and why: the server cannot yet say which record to
    /// read.
    NotRead(String),
    /// The read failed, in the port's own words.
    Failed(String),
    /// What the record says.
    Read(ScoringReading),
}

/// The detector's record, as far as the status needs it: nothing else of the
/// answer is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringReading {
    /// The print the detector says it is watching, when it names one.
    pub print: Option<ProviderPrint>,
    /// A fingerprint of its latest snapshot and score: it changes when either
    /// does.
    pub evidence: u64,
}

/// Scoring found of one print, as the status answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scoring {
    /// When the evidence began.
    pub since: Timestamp,
    /// Whether a failure alert of the detector's own has arrived for it.
    pub flagged: bool,
}

/// What the reads of the detector's record have found, held between them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScoringState {
    /// The last read's fingerprint, and the open print it named.
    last: Option<(PrintId, u64)>,
    /// The evidence held: the print, when on the elapsed clock it last moved,
    /// and the instant the evidence began.
    evidence: Option<(PrintId, Duration, Timestamp)>,
    /// The prints a failure alert of the detector's own has arrived for.
    flagged: BTreeSet<PrintId>,
    /// Why the record is not being read, or what the last read failed with.
    unread: Option<String>,
    /// The earliest start of its job this server has sighted, by print.
    first_starts: BTreeMap<PrintId, Timestamp>,
}

impl ScoringState {
    /// The scoring the evidence shows of one print — or of whichever print it
    /// is of — at `now` on the elapsed clock.
    pub(crate) fn scoring(
        &self,
        print_id: Option<PrintId>,
        now: Duration,
        window: Duration,
    ) -> Option<Scoring> {
        let (of, moved, since) = self.evidence?;
        if print_id.is_some_and(|wanted| wanted != of) || now.saturating_sub(moved) > window {
            return None;
        }
        Some(Scoring {
            since,
            flagged: self.flagged.contains(&of),
        })
    }

    /// Why the record is not being read, or what the last read failed with.
    pub(crate) fn unread(&self) -> Option<&str> {
        self.unread.as_deref()
    }
}

/// Whether the print a record names is one open print, whose job this server
/// first sighted starting at `first_start`.
fn names(print: &PrintRecord, first_start: Option<Timestamp>, named: &ProviderPrint) -> bool {
    match print.provider_print_id {
        Some(id) => id == named.id,
        None => {
            named.file_name.is_some()
                && print.file_name == named.file_name
                && match (first_start, named.started_at) {
                    (Some(ours), Some(theirs)) => {
                        seconds_between(ours, theirs).abs() <= JOB_IDENTITY_TOLERANCE_S
                    }
                    // A file name alone names any job of that file.
                    _ => false,
                }
        }
    }
}

impl Supervisor {
    /// Note one start a read put a print's job at, keeping the earliest.
    pub(crate) fn note_sighted_start(&self, print_id: PrintId, started_at: Timestamp) {
        self.watch_state()
            .scoring
            .first_starts
            .entry(print_id)
            .and_modify(|held| *held = (*held).min(started_at))
            .or_insert(started_at);
    }

    /// Note that a failure alert of the detector's own arrived for one print.
    pub(crate) fn note_flagged(&self, print_id: PrintId) {
        self.watch_state().scoring.flagged.insert(print_id);
    }

    /// Read the detector's record once, for the prints a round observes, and
    /// answer what failed, for the round to record against what it writes.
    ///
    /// No print active, nothing held: evidence is only ever of an active
    /// print.
    pub(crate) async fn read_scoring(&self, active: &[PrintRecord]) -> Option<String> {
        let probe = self.scoring_probe()?;
        if active.is_empty() {
            let mut held = self.watch_state();
            held.scoring.last = None;
            held.scoring.evidence = None;
            return None;
        }
        let read = probe.read_scoring().await;
        let reading = match read {
            ScoringRead::NotRead(why) => {
                self.watch_state().scoring.unread = Some(why);
                return None;
            }
            ScoringRead::Failed(detail) => {
                self.watch_state().scoring.unread = Some(format!(
                    "the last read of the detector's record failed: {detail}"
                ));
                return Some(detail);
            }
            ScoringRead::Read(reading) => reading,
        };
        for print in active {
            if let Some(started_at) = print.job_started_at {
                self.note_sighted_start(print.id, started_at);
            }
        }
        let first_starts = {
            let mut held = self.watch_state();
            held.scoring
                .first_starts
                .retain(|print_id, _| active.iter().any(|print| print.id == *print_id));
            held.scoring.first_starts.clone()
        };
        let named = reading.print.as_ref().and_then(|named| {
            active
                .iter()
                .find(|print| names(print, first_starts.get(&print.id).copied(), named))
                .map(|print| print.id)
        });
        let flagged = match named {
            Some(print_id) => match self.flagged_in_history(print_id, probe.alert_kind()).await {
                Ok(flagged) => flagged,
                Err(error) => {
                    // Whether the detector flagged the print cannot be said, so
                    // nothing read this round is taken: a status saying nothing
                    // was flagged would be a claim nobody could check.
                    let detail = format!(
                        "the print's history could not be read to say whether the detector \
                         flagged it: {error}"
                    );
                    self.watch_state().scoring.unread = Some(detail.clone());
                    return Some(detail);
                }
            },
            None => false,
        };
        let now = self.clock().elapsed();
        let wall = self.clock().now();
        let window = self
            .config()
            .observation_interval
            .saturating_mul(EVIDENCE_INTERVALS);
        let mut held = self.watch_state();
        let state = &mut held.scoring;
        state.unread = None;
        let Some(print_id) = named else {
            // The detector is watching another print, or none: whatever was
            // held of the active one is over.
            state.last = None;
            state.evidence = None;
            return None;
        };
        if flagged {
            state.flagged.insert(print_id);
        }
        let moved = state
            .last
            .is_some_and(|(last, evidence)| last == print_id && evidence != reading.evidence);
        state.last = Some((print_id, reading.evidence));
        if moved {
            // A stretch of evidence goes on only while it stands; one that
            // expired before this read is over, and this read begins another.
            let since = match state.evidence {
                Some((of, last_moved, since))
                    if of == print_id && now.saturating_sub(last_moved) <= window =>
                {
                    since
                }
                _ => wall,
            };
            state.evidence = Some((print_id, now, since));
        } else if state.evidence.is_some_and(|(of, _, _)| of != print_id) {
            state.evidence = None;
        }
        None
    }

    /// Whether a print's history holds a failure alert of the detector's own.
    async fn flagged_in_history(
        &self,
        print_id: PrintId,
        kind: EventKind,
    ) -> Result<bool, CoreError> {
        Ok(!self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![kind],
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?
            .is_empty())
    }
}
