//! The prints the printer's own tallies counted and this system never saw.
//!
//! `OctoPrint` counts every print of every file it holds, by outcome. A print
//! made while this system was down, or one it otherwise never watched, shows up
//! there and nowhere else, so [`Supervisor::account_unobserved_prints`] —
//! which the server calls as it starts — compares each file's tallies with the
//! ones last accounted, less the prints of that file this system itself settled
//! since then, counted print by print.
//!
//! Each print left over is written down as an `unaccounted_print` and keyed by
//! its ordinal in the file's tally: a successful one charges the spool on each
//! tool its analysed length, and any other marks that spool as one whose figure
//! is in doubt. A tally that went **down** — the file deleted and uploaded
//! again — advances that file's generation, so an ordinal reused after it is a
//! new key: its count is the new generation's growth from zero, accounted like
//! any other when no print of the file was settled in the window, and otherwise
//! answered by marking the loaded spools rather than charging on a guess.
//!
//! Everything one run finds is written by one
//! [`account_tallies`](crate::store::FilamentStore::account_tallies), so a run
//! interrupted before it commits leaves nothing and one repeated after it
//! commits finds nothing new. The first run, with no tallies held, stores the
//! baseline and flags nothing.

use std::collections::BTreeMap;

use printobserver_printer_api::FileFacts;
use printobserver_types::{EventRecord, Timestamp};

use crate::accounting::{Feed, Figures, NOTHING_MM, lengths_of};
use crate::error::CoreError;
use crate::kinds::{
    FileTallyRebaselinedPayload, RebaselineDisposition, UnaccountedPrintPayload, system_source,
};
use crate::records::{ChargeKey, ChargeMethod, PrintOutcome, SpoolRecord, UnverifiedReason};
use crate::store::{Charge, EventDraft, FileTally, SpoolMutation};
use crate::supervisor::Supervisor;

/// One print the tallies counted and nobody accounted for.
#[derive(Debug, Clone, Copy)]
struct Unseen {
    /// Its ordinal in the file's tally, within the generation.
    print_number: i64,
    /// How it ended, by the tally: completed, or not.
    outcome: PrintOutcome,
}

/// The prints of one file this system settled in a window, by outcome.
#[derive(Debug, Clone, Copy, Default)]
struct Settled {
    /// The ones it settled completed.
    successes: i64,
    /// The ones it settled any other way.
    failures: i64,
}

impl Settled {
    /// Every print it settled.
    const fn total(self) -> i64 {
        self.successes + self.failures
    }
}

/// The unseen prints between two tallies of one generation: the highest
/// ordinals of the growth, completed ones first. A tally whose counts overflow
/// when added is no tally, and has none.
fn unseen_between(from: (i64, i64), to: (i64, i64), settled: Settled) -> Vec<Unseen> {
    let grown = |now: i64, before: i64, seen: i64| {
        now.checked_sub(before)
            .and_then(|growth| growth.checked_sub(seen))
            .map(|unseen| unseen.max(0))
    };
    let (Some(successes), Some(failures), Some(total)) = (
        grown(to.0, from.0, settled.successes),
        grown(to.1, from.1, settled.failures),
        to.0.checked_add(to.1),
    ) else {
        return Vec::new();
    };
    let Some(first) = successes
        .checked_add(failures)
        .and_then(|unseen| total.checked_sub(unseen))
        .and_then(|before| before.checked_add(1))
    else {
        return Vec::new();
    };
    (0..successes.saturating_add(failures))
        .map(|offset| Unseen {
            print_number: first.saturating_add(offset),
            outcome: if offset < successes {
                PrintOutcome::Completed
            } else {
                PrintOutcome::Failed
            },
        })
        .collect()
}

/// The prints of one file this system settled at or after an instant, or ever
/// when there is none, by outcome.
fn settled_since(
    settlements: &[(String, Timestamp, PrintOutcome)],
    file_name: &str,
    since: Option<Timestamp>,
) -> Settled {
    let mut settled = Settled::default();
    for (_, _, outcome) in settlements
        .iter()
        .filter(|(file, at, _)| file == file_name && since.is_none_or(|since| *at >= since))
    {
        if *outcome == PrintOutcome::Completed {
            settled.successes += 1;
        } else {
            settled.failures += 1;
        }
    }
    settled
}

/// What one run of the accounting writes.
#[derive(Debug, Default)]
struct Accounting {
    /// The advanced tallies.
    tallies: Vec<FileTally>,
    /// The charges of unseen prints that completed.
    charges: Vec<Charge>,
    /// The marks on spools nothing could be charged to.
    marks: Vec<SpoolMutation>,
    /// The `file_tally_rebaselined` and `unaccounted_print` events.
    events: Vec<EventDraft>,
    /// Each spool's figure as the charges are worked out.
    figures: Figures,
}

impl Supervisor {
    /// Account the prints the printer's tallies counted that this system never
    /// saw, and answer what that recorded.
    ///
    /// # Errors
    ///
    /// Returns the printer's own error when its tallies could not be read, and
    /// the store's own when the accounting could not be written — in which case
    /// nothing of this run was.
    // llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    pub async fn account_unobserved_prints(&self) -> Result<Vec<EventRecord>, CoreError> {
        let files = self
            .read_printed_files()
            .await
            .map_err(CoreError::Printer)?;
        let _writing = self.spool_writes().await;
        let held = self.stores().filament.file_tallies().await?;
        let now = self.clock().now();
        if held.is_empty() {
            let baseline = files
                .iter()
                .filter(|file| file.successes >= 0 && file.failures >= 0)
                .map(|file| FileTally {
                    file_name: file.file_name.clone(),
                    origin: file.origin.clone(),
                    generation: 0,
                    successes: file.successes,
                    failures: file.failures,
                    observed_at: now,
                })
                .collect();
            return Ok(self
                .stores()
                .filament
                .account_tallies(baseline, Vec::new(), Vec::new(), Vec::new())
                .await?);
        }
        let loaded = self.stores().filament.loaded_spools().await?;
        let settlements = self.settlements_of(&files).await?;
        let mut accounting = Accounting::default();
        // A tally is read at this boundary: a count below zero is not one the
        // printer can have, and a file reporting one is left as last held
        // rather than accounted on it.
        for file in files
            .iter()
            .filter(|file| file.successes >= 0 && file.failures >= 0)
        {
            let previous = held
                .iter()
                .find(|tally| tally.file_name == file.file_name && tally.origin == file.origin);
            self.account_file(file, previous, &settlements, &loaded, now, &mut accounting)?;
        }
        Ok(self
            .stores()
            .filament
            .account_tallies(
                accounting.tallies,
                accounting.charges,
                accounting.marks,
                accounting.events,
            )
            .await?)
    }

    /// Every settlement this system made of a print of one of these files:
    /// the file, when, and with which outcome.
    async fn settlements_of(
        &self,
        files: &[FileFacts],
    ) -> Result<Vec<(String, Timestamp, PrintOutcome)>, CoreError> {
        let mut settlements = Vec::new();
        for print in self.stores().prints.prints().await? {
            let Some(file_name) = print
                .file_name
                .filter(|name| files.iter().any(|file| &file.file_name == name))
            else {
                continue;
            };
            if let Some((at, outcome)) = self.stores().filament.print_settlement(print.id).await? {
                settlements.push((file_name, at, outcome));
            }
        }
        Ok(settlements)
    }

    /// Account one file's tally against the one last held for it.
    fn account_file(
        &self,
        file: &FileFacts,
        previous: Option<&FileTally>,
        settlements: &[(String, Timestamp, PrintOutcome)],
        loaded: &[SpoolRecord],
        now: Timestamp,
        accounting: &mut Accounting,
    ) -> Result<(), CoreError> {
        let since = previous.map(|tally| tally.observed_at);
        let settled = settled_since(settlements, &file.file_name, since);
        let (generation, unseen) = match previous {
            Some(held) if file.successes < held.successes || file.failures < held.failures => {
                let generation = held.generation + 1;
                let accounted = settled.total() == 0;
                accounting.events.push(self.draft_of(
                    None,
                    system_source(),
                    &FileTallyRebaselinedPayload {
                        file_name: file.file_name.clone(),
                        origin: file.origin.clone(),
                        old_generation: held.generation,
                        new_generation: generation,
                        old_successes: held.successes,
                        old_failures: held.failures,
                        new_successes: file.successes,
                        new_failures: file.failures,
                        disposition: if accounted {
                            RebaselineDisposition::Accounted
                        } else {
                            RebaselineDisposition::Unverified
                        },
                    },
                )?);
                if accounted {
                    let unseen =
                        unseen_between((0, 0), (file.successes, file.failures), Settled::default());
                    (generation, unseen)
                } else {
                    for spool in spools_for(file, loaded) {
                        accounting.marks.push(self.mark_of(
                            spool.id,
                            UnverifiedReason::TallyReset {
                                file_name: file.file_name.clone(),
                                origin: file.origin.clone(),
                            },
                            None,
                        )?);
                    }
                    (generation, Vec::new())
                }
            }
            Some(held) => (
                held.generation,
                unseen_between(
                    (held.successes, held.failures),
                    (file.successes, file.failures),
                    settled,
                ),
            ),
            None => (
                0,
                unseen_between((0, 0), (file.successes, file.failures), settled),
            ),
        };
        for print in unseen {
            self.account_unseen(file, generation, print, loaded, accounting)?;
        }
        accounting.tallies.push(FileTally {
            file_name: file.file_name.clone(),
            origin: file.origin.clone(),
            generation,
            successes: file.successes,
            failures: file.failures,
            observed_at: now,
        });
        Ok(())
    }

    /// Account one unseen print: charge a completed one its analysed length,
    /// mark the spools of any other.
    fn account_unseen(
        &self,
        file: &FileFacts,
        generation: u32,
        print: Unseen,
        loaded: &[SpoolRecord],
        accounting: &mut Accounting,
    ) -> Result<(), CoreError> {
        let printed_at = file.last_print.as_ref().map(|last| last.at);
        let analysed: Option<BTreeMap<i64, f64>> = file.filament.as_deref().map(lengths_of);
        let mut charged = false;
        match (&analysed, print.outcome) {
            (Some(lengths), PrintOutcome::Completed) => {
                for (tool, length) in lengths {
                    if *length <= NOTHING_MM {
                        continue;
                    }
                    let spool = loaded
                        .iter()
                        .find(|spool| spool.loaded_on_tool == Some(*tool));
                    charged |= spool.is_some();
                    accounting.charges.push(self.charge_of(
                        Feed {
                            key: ChargeKey::Unobserved {
                                origin: file.origin.clone(),
                                file_name: file.file_name.clone(),
                                generation,
                                print_number: print.print_number,
                                tool: *tool,
                            },
                            method: ChargeMethod::UnobservedEstimate,
                            length_mm: *length,
                            analysed_length_mm: Some(*length),
                            position: None,
                            note: None,
                        },
                        spool,
                        &mut accounting.figures,
                        system_source(),
                    )?);
                }
            }
            _ => {
                for spool in spools_for(file, loaded) {
                    accounting.marks.push(self.mark_of(
                        spool.id,
                        UnverifiedReason::UnaccountedPrint {
                            file_name: file.file_name.clone(),
                            printed_at: printed_at.unwrap_or_else(|| self.clock().now()),
                        },
                        None,
                    )?);
                }
            }
        }
        accounting.events.push(self.draft_of(
            None,
            system_source(),
            &UnaccountedPrintPayload {
                file_name: file.file_name.clone(),
                printed_at,
                success: print.outcome == PrintOutcome::Completed,
                charged,
            },
        )?);
        Ok(())
    }
}

/// The loaded spools a file's prints would have fed: those on the tools its
/// analysis names, or every loaded spool when it has none.
fn spools_for<'a>(file: &FileFacts, loaded: &'a [SpoolRecord]) -> Vec<&'a SpoolRecord> {
    match file.filament.as_deref().map(lengths_of) {
        Some(lengths) => loaded
            .iter()
            .filter(|spool| {
                spool
                    .loaded_on_tool
                    .is_some_and(|tool| lengths.get(&tool).is_some_and(|l| *l > NOTHING_MM))
            })
            .collect(),
        None => loaded.iter().collect(),
    }
}
