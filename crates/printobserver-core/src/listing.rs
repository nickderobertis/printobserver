//! Every print this system holds, and the one the printer is running now.
//!
//! A print is otherwise known here only once a provider reports on it, and an
//! identifier nothing lists is one nobody can act with. So reading the listing
//! also **adopts** the job the printer reports: when that job is in one of
//! [`ACTIVE_STATES`] and no open print records it, one is opened for it, and
//! the listing names it active. An alert about the same job later attaches its
//! provider's identifier to that print rather than opening a second, which is
//! [`Supervisor::handle_event`]'s own rule.
//!
//! # Which open print a running job is
//!
//! The printer names a job by its file and by nothing else, so a file name
//! alone cannot tell one job from a later one of the same file. What can is how
//! long each has been printing. `OctoPrint` reports a job's running time as the
//! time since it started less every pause it has **resumed** from: a pause
//! counts while it lasts, and comes back out at the resume. So the running time
//! a read finds a job *printing* at is the time it has spent printing, and that
//! only grows; the one a read finds it *paused* at is at least that.
//!
//! A print records the longest running time any read found its job printing at
//! ([`PrintRecord::job_print_time_s`]) and, from the latest read that found its
//! job printing or paused, when that read put the job's start
//! ([`PrintRecord::job_started_at`] — the read's instant less the running time
//! it reported, which a resume moves later, so a read putting it further than
//! the tolerance from the one recorded resets it).
//! An open print of the running job's file is that job unless the job reports
//! a running time more than [`JOB_IDENTITY_TOLERANCE_S`] short of the longest
//! the print recorded: a job cannot have printed for less than it had already
//! printed, so that one is a later job, and the print is closed with
//! [`REPLACED_REASON`] before the running job is given a print of its own. The
//! tolerance is printing time, and a pause, observed or not, shortens no job's
//! printing time, so no pause splits a print.
//!
//! Where either side reports no running time — a print recorded before these
//! were, one an alert or a start opened before any read saw its job, one only
//! ever seen paused, or a job whose printer reports none — the file name is
//! the whole of what is known, and it decides as it did before.
//!
//! What this cannot tell apart, and so keeps as one print: a later job that,
//! when it is first read, has printed for as long as the earlier job was last
//! seen printing, less the tolerance, or longer. An earlier job paused between
//! two reads and resumed reports exactly that, because the pause is taken back
//! out of its running time at the resume; nothing a read returns tells the two
//! apart.
//!
//! # A read that finds the job over closes its print
//!
//! A print ends when the printer is seen not to be running it, whoever was
//! looking. A supervision turn that reads a terminal state closes its print,
//! and so does every read this module makes and the status and context reads
//! make: a job no longer in one of [`ACTIVE_STATES`] closes the open prints it
//! could be, through the same close-out a turn takes. A printer that cannot be
//! read closes nothing, and neither does a state this vocabulary does not name,
//! because neither is a finding that the job is over.
//!
//! A close-out from a read never waits on a supervision turn: the read may be
//! the agent's own, made from inside that turn. So a print whose turn is
//! running is left for that turn, and the next read after it, to close, and a
//! turn that finds its print ended by the time it holds the print runs nothing.
//!
//! # What it asks of the printer
//!
//! One read of the job, which is a read rather than an action: nothing here
//! reaches an action method of the printer port, so adopting a job commands
//! nothing at the machine. Closing a print is the one exception, and it is the
//! close-out's rather than this module's: the close-out expires the print's
//! active interventions, and an expiry may put an adjusted value back through
//! the same decided path any action takes.
//!
//! # What an adopted print records
//!
//! The store records a print's state when it opens it and when it ends it, and
//! at no point between. An adopted print is opened exactly as an alert opens
//! one, so it records `printing` whether or not the machine is paused at that
//! moment: what the machine is doing now is the status read's answer, and never
//! the record's.

use printobserver_printer_api::{JobSnapshot, PrinterState};
use printobserver_types::{PrintId, Timestamp};

use crate::clock::seconds_between;
use crate::error::CoreError;
use crate::records::PrintRecord;
use crate::supervisor::Supervisor;

/// The job states a printer's job is adoptable in: a job it is running, and a
/// job it has paused part-way through.
///
/// Every other state is a machine with no job in progress — idle, cancelling
/// the one it had, in error, unreachable, or saying something this vocabulary
/// does not name — and a print opened for one of those would be a record of a
/// job that is not there.
pub const ACTIVE_STATES: [PrinterState; 2] = [PrinterState::Printing, PrinterState::Paused];

/// How far, in seconds of printing, a job's reported running time may fall
/// short of the longest a print recorded for its job and still be that job —
/// and how much later than a print's recorded start an alert may put its job's
/// start and still be that print's.
///
/// A running time is whole seconds read off the printer's clock at an instant
/// read off another, and `OctoPrint` reports a job it is still *pausing* — the
/// head finishing its move — as printing with the pause already counting, so
/// two reads of one job do not agree to the second. Two jobs of one file
/// cannot be closer together than the time it takes to print one and start
/// the next.
pub const JOB_IDENTITY_TOLERANCE_S: i64 = 120;

/// Why a print is ended when a later job of its file is found running.
pub const REPLACED_REASON: &str = "a later job of the same file replaced it";

/// The printer's job, as one read observed it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedJob {
    /// What the printer reported.
    pub job: JobSnapshot,
    /// The instant the report was read at.
    pub observed_at: Timestamp,
}

impl ObservedJob {
    /// Whether the printer is running this job, or has paused it part-way.
    #[must_use]
    pub fn is_active(&self) -> bool {
        ACTIVE_STATES.contains(&self.job.state)
    }

    /// Whether the printer reports this job over.
    #[must_use]
    pub const fn has_ended(&self) -> bool {
        is_over(&self.job.state)
    }

    /// When the job began, less every pause it has resumed from: the instant
    /// it was observed, less the running time the printer reported for it
    /// then. Unknown when it reported none.
    #[must_use]
    pub fn started_at(&self) -> Option<Timestamp> {
        let running = self.job.print_time_s?;
        self.observed_at.plus_seconds(-running).ok()
    }

    /// How long the job has spent printing, when this read found it printing
    /// and reporting a running time; a paused job's running time counts the
    /// pause, so it is not one.
    #[must_use]
    pub fn printing_time_s(&self) -> Option<i64> {
        (self.job.state == PrinterState::Printing)
            .then_some(self.job.print_time_s)
            .flatten()
    }
}

/// Whether a running job of a print's file may be the job that print records.
///
/// Not when the job reports a running time more than
/// [`JOB_IDENTITY_TOLERANCE_S`] short of the longest the print recorded for its
/// job; otherwise, and whenever either side reports none, it may — the file
/// name the caller already compared is then the whole of what is known.
#[must_use]
pub fn may_be_same_job(print: &PrintRecord, observed: &ObservedJob) -> bool {
    match (print.job_print_time_s, observed.job.print_time_s) {
        (Some(longest), Some(running)) => {
            running.saturating_add(JOB_IDENTITY_TOLERANCE_S) >= longest
        }
        _ => true,
    }
}

/// Whether a job a provider says began at `started_at` may be the job a print
/// of its file records.
///
/// Not when it began more than [`JOB_IDENTITY_TOLERANCE_S`] after the start
/// the print records. Earlier is no objection: a resume moves the recorded
/// start later than the job's own, and a provider records the job's own.
#[must_use]
pub fn may_have_started_it(print: &PrintRecord, started_at: Option<Timestamp>) -> bool {
    match (print.job_started_at, started_at) {
        (Some(recorded), Some(started)) => {
            seconds_between(recorded, started) <= JOB_IDENTITY_TOLERANCE_S
        }
        _ => true,
    }
}

/// Whether a job in this state is over: a state the printer's vocabulary
/// names, and not one of [`ACTIVE_STATES`].
#[must_use]
pub const fn is_over(state: &PrinterState) -> bool {
    !matches!(
        state,
        PrinterState::Printing | PrinterState::Paused | PrinterState::Unknown(_)
    )
}

/// The reason a print ends with when the printer was seen in `state`.
pub(crate) fn reached_reason(state: &PrinterState) -> String {
    format!("the print reached {state:?}")
}

/// Every print, and the one the printer is running now.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintListing {
    /// Every print, ended or not, most recently opened first.
    pub prints: Vec<PrintRecord>,
    /// The print the printer's job belongs to, present exactly when the printer
    /// reports a job in one of [`ACTIVE_STATES`] with a file name.
    pub active: Option<PrintId>,
}

/// What one observation of the printer's job settled among the open prints.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settled {
    /// The open print the running job is, when one is.
    pub running: Option<PrintRecord>,
    /// Every print this observation closed.
    pub closed: Vec<PrintId>,
}

impl Supervisor {
    /// Every print, and the one the printer's job belongs to — **opening** one
    /// for that job when no open print records it, and **closing** the open
    /// prints the printer's job shows are over.
    ///
    /// Named for the half a caller sees, because it is not only a read: the
    /// listing is what a caller asked for, and the print it may open is what
    /// makes that listing name the job at all. A printer that cannot be read is
    /// a listing with nothing active, and nothing closed, rather than a
    /// failure: the stored prints are still the answer to which prints there
    /// are.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the prints could not be read, or a
    /// print could not be opened or closed.
    pub async fn list_and_adopt_prints(&self) -> Result<PrintListing, CoreError> {
        let _resolving = self.resolving().await;
        let Some(observed) = self.observe_job().await else {
            return Ok(PrintListing {
                prints: self.stores().prints.prints().await?,
                active: None,
            });
        };
        let settled = self.settle_open_prints(&observed).await?;
        let active = match (settled.running, &observed.job.file_name) {
            (Some(running), _) => Some(running.id),
            (None, Some(file_name)) if observed.is_active() => {
                Some(self.open_running_print(&observed, file_name).await?.id)
            }
            (None, _) => None,
        };
        Ok(PrintListing {
            prints: self.stores().prints.prints().await?,
            active,
        })
    }

    /// Close every open print the printer's job shows is over, and record the
    /// running job's start on the open print it is.
    ///
    /// What a supervisor does with the printer as it starts, before it adopts
    /// what is left open: a print that ended while nothing was watching is not
    /// one to go on watching. A printer that cannot be read settles nothing.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the open prints could not be read or
    /// a print could not be closed.
    pub async fn settle_with_printer(&self) -> Result<Settled, CoreError> {
        let _resolving = self.resolving().await;
        match self.observe_job().await {
            Some(observed) => self.settle_open_prints(&observed).await,
            None => Ok(Settled::default()),
        }
    }

    /// Read the printer's job, and the instant it was read at; nothing when it
    /// cannot be read.
    pub(crate) async fn observe_job(&self) -> Option<ObservedJob> {
        let job = self.read_job().await.ok()?;
        Some(ObservedJob {
            job,
            observed_at: self.clock().now(),
        })
    }

    /// Settle the open prints against one observation of the printer's job.
    ///
    /// A job that is over closes every open print, since one printer runs one
    /// job and it is running none. A running job is the most recently opened
    /// open print of its file that may be the same job; every open print of
    /// its file that cannot be is closed as replaced. Prints of other files are
    /// left as they are.
    pub(crate) async fn settle_open_prints(
        &self,
        observed: &ObservedJob,
    ) -> Result<Settled, CoreError> {
        let mut settled = Settled::default();
        let open = self.stores().prints.open_prints().await?;
        if observed.has_ended() {
            let reason = reached_reason(&observed.job.state);
            for print in open {
                if self
                    .close_out_from_read(print.id, &observed.job.state, &reason)
                    .await?
                {
                    settled.closed.push(print.id);
                }
            }
            return Ok(settled);
        }
        let Some(file_name) = observed
            .job
            .file_name
            .as_ref()
            .filter(|_| observed.is_active())
        else {
            return Ok(settled);
        };
        // Most recently opened first, so the first that may be the job is the
        // most recent of them.
        for print in open
            .into_iter()
            .filter(|print| print.file_name.as_ref() == Some(file_name))
        {
            if !may_be_same_job(&print, observed) {
                // The earlier job ended before this one began, and a printer
                // begins a job only from idle.
                if self
                    .close_out_from_read(print.id, &PrinterState::Operational, REPLACED_REASON)
                    .await?
                {
                    settled.closed.push(print.id);
                }
            } else if settled.running.is_none() {
                settled.running = Some(self.record_sighting(print, observed).await?);
            }
        }
        Ok(settled)
    }

    /// Record what one read saw of the job a print was matched to.
    ///
    /// The start a print records is moved only when a read puts the job's
    /// start more than [`JOB_IDENTITY_TOLERANCE_S`] from it, as a resume after
    /// a longer pause does: a whole-second running time read at an instant off
    /// another clock puts one job's start a second or so apart from one read to
    /// the next, and a record rewritten at every read for that would be a
    /// record of the reads rather than of the job. Nothing is written when the
    /// read saw nothing the print does not already carry.
    async fn record_sighting(
        &self,
        print: PrintRecord,
        observed: &ObservedJob,
    ) -> Result<PrintRecord, CoreError> {
        let Some(seen) = observed.started_at() else {
            return Ok(print);
        };
        let printing = observed.printing_time_s();
        let started_at = match print.job_started_at {
            Some(held) if seconds_between(held, seen).abs() <= JOB_IDENTITY_TOLERANCE_S => held,
            _ => seen,
        };
        if print.job_started_at == Some(started_at) && printing <= print.job_print_time_s {
            return Ok(print);
        }
        Ok(self
            .stores()
            .prints
            .record_job_sighting(print.id, started_at, printing)
            .await?)
    }

    /// Open a print for the job the printer is running, recording what the
    /// read saw of it.
    async fn open_running_print(
        &self,
        observed: &ObservedJob,
        file_name: &str,
    ) -> Result<PrintRecord, CoreError> {
        let opened = self
            .stores()
            .prints
            .open_print(None, Some(file_name.to_owned()))
            .await?;
        self.record_sighting(opened, observed).await
    }

    /// Close one print a read found over, unless a supervision turn holds it.
    ///
    /// Answers whether this closed it: a print a turn holds is that turn's, and
    /// one already ended has nothing left to close.
    pub(crate) async fn close_out_from_read(
        &self,
        print_id: PrintId,
        state: &PrinterState,
        reason: &str,
    ) -> Result<bool, CoreError> {
        let Some(_turn) = self.turns().try_acquire(print_id) else {
            return Ok(false);
        };
        self.close_out_print(print_id, state, reason).await
    }
}
