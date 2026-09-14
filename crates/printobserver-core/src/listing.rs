//! Every print this system holds, and the one the printer is running now.
//!
//! A print is otherwise known here only once a provider reports on it, and an
//! identifier nothing lists is one nobody can act with. So reading the listing
//! also **adopts** the job the printer reports: when that job is in one of
//! [`ACTIVE_STATES`] and no open print carries its file name, one is opened for
//! it, and the listing names it active. An alert about the same job later
//! attaches its provider's identifier to that print rather than opening a
//! second, which is [`Supervisor::handle_event`]'s own rule.
//!
//! # What it asks of the printer
//!
//! One read of the job, which is a read rather than an action: nothing here
//! reaches an action method of the printer port, so adopting a job commands
//! nothing at the machine.
//!
//! # What an adopted print records
//!
//! The store records a print's state when it opens it and when it ends it, and
//! at no point between. An adopted print is opened exactly as an alert opens
//! one, so it records `printing` whether or not the machine is paused at that
//! moment: what the machine is doing now is the status read's answer, and never
//! the record's.

use printobserver_printer_api::PrinterState;
use printobserver_types::PrintId;

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

/// Every print this system holds, and the one the printer is running now.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintListing {
    /// Every print, ended or not, most recently opened first.
    pub prints: Vec<PrintRecord>,
    /// The print the printer's job belongs to, present exactly when the printer
    /// reports a job in one of [`ACTIVE_STATES`] with a file name.
    pub active: Option<PrintId>,
}

impl Supervisor {
    /// Every print, and the one the printer's job belongs to — **opening** one
    /// for that job when no open print carries its file name.
    ///
    /// Named for both halves, because it is not only a read: the listing is
    /// what a caller asked for, and the print it may open is what makes that
    /// listing name the job at all. A printer that cannot be read is a listing
    /// with nothing active rather than a failure: the stored prints are still
    /// the answer to which prints there are.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the prints could not be read, or a
    /// print for the job could not be opened.
    pub async fn list_and_adopt_prints(&self) -> Result<PrintListing, CoreError> {
        let _resolving = self.resolving().await;
        let running = self
            .read_job()
            .await
            .ok()
            .filter(|job| ACTIVE_STATES.contains(&job.state))
            .and_then(|job| job.file_name);
        let prints = self.stores().prints.prints().await?;
        let Some(file_name) = running else {
            return Ok(PrintListing {
                prints,
                active: None,
            });
        };
        // Most recently opened first, so the first open print carrying the
        // name is the most recent of them.
        if let Some(found) = prints
            .iter()
            .find(|print| print.ended_at.is_none() && print.file_name.as_ref() == Some(&file_name))
        {
            let active = Some(found.id);
            return Ok(PrintListing { prints, active });
        }
        let opened = self
            .stores()
            .prints
            .open_print(None, Some(file_name))
            .await?;
        Ok(PrintListing {
            prints: self.stores().prints.prints().await?,
            active: Some(opened.id),
        })
    }
}
