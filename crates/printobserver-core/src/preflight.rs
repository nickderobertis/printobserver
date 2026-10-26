//! Whether the spools loaded hold enough for a print, asked before it starts.
//!
//! [`Supervisor::preflight`] is the read: for every tool the file uses it names
//! the spool loaded there, every mark that spool holds, what the print needs
//! from it by `OctoPrint`'s analysis and the spool's own diameter and density,
//! the buffer asked beyond that, and a verdict. It answers at once, records
//! nothing, and never waits.
//!
//! A start is where the check bites. After the decision accepts a start, and
//! before the printer is asked, the check is made and a refusal turns the
//! decision into one of the filament rejections — written down like every
//! rejection. When a tool holds a tracked spool and the file has no analysis
//! yet, the start first waits for `OctoPrint`'s, re-reading the file until it
//! appears or [`SpoolPolicy::analysis_wait_s`](crate::SpoolPolicy) elapses on
//! the injected clock: this system computes no whole-file estimate of its own.
//! That wait is taken before the start holds the resolution of prints, so a
//! start waiting on an analysis holds up no alert and no read.

use std::time::Duration;

use printobserver_printer_api::FileFacts;
use printobserver_types::FileName;

use crate::accounting::{NOTHING_MM, lengths_of};
use crate::clock::seconds_between;
use crate::config::PreflightMode;
use crate::delay::Delay;
use crate::error::CoreError;
use crate::filament::{buffer_g, grams_for_length};
use crate::records::{
    ActionKind, Actor, ActorClass, DoubtedSpool, Preflight, PreflightNeed, PreflightObjection,
    PreflightSpool, PreflightTool, PreflightToolResult, PreflightVerdict, RejectionReason,
    ShortSpool, SpoolRecord, SpoolTrust, UnanalysedSpool, UnverifiedMarks,
};
use crate::supervisor::Supervisor;

/// Whether a start asked to go ahead despite the filament check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Override {
    /// It asked; only an operator's asking counts.
    Asked,
    /// It did not.
    NotAsked,
}

impl From<bool> for Override {
    fn from(asked: bool) -> Self {
        if asked { Self::Asked } else { Self::NotAsked }
    }
}

/// How often a start waiting for an analysis reads the file again.
pub const ANALYSIS_POLL: Duration = Duration::from_millis(250);

/// How bad one verdict is, worst first, as a whole print's verdict is the
/// worst of its tools'.
const fn severity(verdict: PreflightVerdict) -> u8 {
    match verdict {
        PreflightVerdict::Insufficient => 0,
        PreflightVerdict::UnverifiedSpool => 1,
        PreflightVerdict::NoAnalysis => 2,
        PreflightVerdict::Warned => 3,
        PreflightVerdict::Overridden => 4,
        PreflightVerdict::Unchecked => 5,
        PreflightVerdict::Passed => 6,
    }
}

/// The worst of the tools' verdicts; unchecked when there are none.
fn worst(tools: &[PreflightTool]) -> PreflightVerdict {
    tools
        .iter()
        .map(|tool| tool.result.verdict())
        .min_by_key(|verdict| severity(*verdict))
        .unwrap_or(PreflightVerdict::Unchecked)
}

/// The marks a spool holds, oldest first, when it holds any.
fn marks(spool: &SpoolRecord) -> Option<UnverifiedMarks> {
    match &spool.trust {
        SpoolTrust::Verified => None,
        SpoolTrust::Unverified { marks } => Some(marks.clone()),
    }
}

/// A spool, as a check names it.
fn named(spool: &SpoolRecord) -> PreflightSpool {
    PreflightSpool {
        id: spool.id,
        name: spool.name.clone(),
    }
}

/// What one tool's spool, judged against what the print needs from it, is
/// found to be: short, in doubt, or enough.
fn judged(spool: &SpoolRecord, need: PreflightNeed) -> PreflightToolResult {
    let marks = marks(spool);
    if spool.remaining_g < need.required_g + need.buffer_g {
        return PreflightToolResult::Insufficient(ShortSpool {
            spool: named(spool),
            remaining_g: spool.remaining_g,
            need,
            marks: marks.map(UnverifiedMarks::into_vec).unwrap_or_default(),
        });
    }
    match marks {
        None => PreflightToolResult::Passed {
            spool: named(spool),
            remaining_g: spool.remaining_g,
            need,
        },
        Some(marks) => PreflightToolResult::UnverifiedSpool(DoubtedSpool {
            spool: named(spool),
            remaining_g: spool.remaining_g,
            need: Some(need),
            marks,
        }),
    }
}

/// What one tool's spool is found to be when the file has no analysis: in
/// doubt when it holds a mark, otherwise unanalysed.
fn unanalysed(spool: &SpoolRecord) -> PreflightToolResult {
    match marks(spool) {
        None => PreflightToolResult::NoAnalysis(UnanalysedSpool {
            spool: named(spool),
            remaining_g: spool.remaining_g,
        }),
        Some(marks) => PreflightToolResult::UnverifiedSpool(DoubtedSpool {
            spool: named(spool),
            remaining_g: spool.remaining_g,
            need: None,
            marks,
        }),
    }
}

/// The refusal one check makes, when it makes one: the first short tool, else
/// the first unverified one, else a file still unanalysed.
fn refusal(preflight: &Preflight, waited_s: u64) -> Option<RejectionReason> {
    let short = preflight.tools.iter().find_map(|tool| match &tool.result {
        PreflightToolResult::Insufficient(short) => Some(RejectionReason::InsufficientFilament {
            tool: tool.tool,
            spool_id: short.spool.id,
            spool_name: short.spool.name.clone(),
            required_g: short.need.required_g,
            buffer_g: short.need.buffer_g,
            remaining_g: short.remaining_g,
            unverified: short.marks.first().map(|mark| mark.reason.clone()),
        }),
        _ => None,
    });
    let unverified = || {
        preflight.tools.iter().find_map(|tool| match &tool.result {
            PreflightToolResult::UnverifiedSpool(doubted) => {
                Some(RejectionReason::UnverifiedSpool {
                    tool: tool.tool,
                    spool_id: doubted.spool.id,
                    spool_name: doubted.spool.name.clone(),
                    reason: doubted.marks.oldest().reason.clone(),
                })
            }
            _ => None,
        })
    };
    let unanalysed = || {
        preflight
            .tools
            .iter()
            .any(|tool| matches!(tool.result, PreflightToolResult::NoAnalysis(_)))
            .then(|| RejectionReason::AnalysisUnavailable {
                file_name: preflight.file_name.clone(),
                waited_s,
            })
    };
    short.or_else(unverified).or_else(unanalysed)
}

/// How a start that went ahead despite an objection went ahead.
#[derive(Debug, Clone, Copy)]
enum LetThrough {
    /// An operator overrode the check.
    Overridden,
    /// The check is set to warn.
    Warned,
}

/// The same check, with every objecting tool's result replaced by one that
/// let the start go ahead, carrying what it objected to.
fn let_through(mut preflight: Preflight, how: LetThrough) -> Preflight {
    for tool in &mut preflight.tools {
        let objection = match tool.result.clone() {
            PreflightToolResult::Insufficient(short) => PreflightObjection::Insufficient(short),
            PreflightToolResult::UnverifiedSpool(doubted) => {
                PreflightObjection::UnverifiedSpool(doubted)
            }
            PreflightToolResult::NoAnalysis(unanalysed) => {
                PreflightObjection::NoAnalysis(unanalysed)
            }
            _ => continue,
        };
        tool.result = match how {
            LetThrough::Overridden => PreflightToolResult::Overridden { objection },
            LetThrough::Warned => PreflightToolResult::Warned { objection },
        };
    }
    preflight.verdict = worst(&preflight.tools);
    preflight
}

impl Supervisor {
    /// Whether the spools loaded hold enough for one file, read now.
    ///
    /// A read: it records nothing and never waits. A file whose facts cannot
    /// be read is a file with no analysis.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the loaded spools could not be read.
    pub async fn preflight(&self, file_name: &FileName) -> Result<Preflight, CoreError> {
        let facts = self.read_file_facts(file_name.clone()).await.ok();
        let loaded = self.stores().filament.loaded_spools().await?;
        Ok(self.judge(file_name, facts.as_ref(), &loaded))
    }

    /// The check of one file's facts against the spools loaded.
    fn judge(
        &self,
        file_name: &FileName,
        facts: Option<&FileFacts>,
        loaded: &[SpoolRecord],
    ) -> Preflight {
        let policy = &self.config().spools;
        let on_tool = |tool: i64| {
            loaded
                .iter()
                .find(|spool| spool.loaded_tool() == Some(tool))
        };
        let tools: Vec<PreflightTool> = match facts.and_then(|facts| facts.filament.as_deref()) {
            Some(filament) => lengths_of(filament)
                .into_iter()
                .filter(|(_, length)| *length > NOTHING_MM)
                .map(|(tool, length)| PreflightTool {
                    tool,
                    result: on_tool(tool).map_or(PreflightToolResult::Unchecked, |spool| {
                        let required_g =
                            grams_for_length(length, spool.diameter_mm, spool.density_g_cm3);
                        judged(
                            spool,
                            PreflightNeed {
                                required_g,
                                buffer_g: buffer_g(required_g, policy),
                            },
                        )
                    }),
                })
                .collect(),
            None => loaded
                .iter()
                .filter_map(|spool| {
                    Some(PreflightTool {
                        tool: spool.loaded_tool()?,
                        result: unanalysed(spool),
                    })
                })
                .collect(),
        };
        Preflight {
            file_name: file_name.as_str().to_owned(),
            verdict: worst(&tools),
            tools,
            checked_at: self.clock().now(),
        }
    }

    /// Wait for `OctoPrint`'s analysis of a file a start names, when the
    /// check will need one: a tool holds a tracked spool, the check is on, the
    /// actor may start a print at all, and the file has none yet.
    ///
    /// Returns once the analysis appears or the configured wait has elapsed on
    /// the injected clock, re-reading the file every [`ANALYSIS_POLL`].
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the loaded spools could not be read.
    pub(crate) async fn await_analysis(
        &self,
        file_name: &FileName,
        actor: &Actor,
    ) -> Result<(), CoreError> {
        let policy = &self.config().spools;
        let granted = self
            .config()
            .envelope
            .actions
            .get(&actor.class())
            .is_some_and(|kinds| kinds.contains(&ActionKind::StartPrint));
        if policy.preflight == PreflightMode::Off || policy.analysis_wait_s == 0 || !granted {
            return Ok(());
        }
        if self.stores().filament.loaded_spools().await?.is_empty() {
            return Ok(());
        }
        let wait = i64::try_from(policy.analysis_wait_s).unwrap_or(i64::MAX);
        let started = self.clock().now();
        loop {
            let analysed = self
                .read_file_facts(file_name.clone())
                .await
                .is_ok_and(|facts| facts.filament.is_some());
            if analysed || seconds_between(started, self.clock().now()) >= wait {
                return Ok(());
            }
            // A wait the system will not take a thread for ends the wait: the
            // start is judged on what the file says now.
            if Delay::new(ANALYSIS_POLL).await.is_err() {
                return Ok(());
            }
        }
    }

    /// The check a start the decision accepted is held to, and the refusal it
    /// turns the decision into when it makes one.
    ///
    /// Mode `off` checks nothing and records `unchecked`. Otherwise a short,
    /// unverified or still-unanalysed tool refuses the start in mode `refuse`
    /// — unless an operator overrode the check, which proceeds as
    /// `overridden` — and proceeds as `warned` in mode `warn`.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the loaded spools could not be read.
    pub(crate) async fn check_start(
        &self,
        file_name: &FileName,
        actor: &Actor,
        asked: Override,
    ) -> Result<(Preflight, Option<RejectionReason>), CoreError> {
        let policy = self.config().spools;
        if policy.preflight == PreflightMode::Off {
            return Ok((
                Preflight {
                    file_name: file_name.as_str().to_owned(),
                    tools: Vec::new(),
                    verdict: PreflightVerdict::Unchecked,
                    checked_at: self.clock().now(),
                },
                None,
            ));
        }
        let preflight = self.preflight(file_name).await?;
        let Some(reason) = refusal(&preflight, policy.analysis_wait_s) else {
            return Ok((preflight, None));
        };
        Ok(match policy.preflight {
            PreflightMode::Warn => (let_through(preflight, LetThrough::Warned), None),
            _ if asked == Override::Asked && actor.class() == ActorClass::Operator => {
                (let_through(preflight, LetThrough::Overridden), None)
            }
            _ => (preflight, Some(reason)),
        })
    }
}
