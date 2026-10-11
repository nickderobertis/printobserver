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
    ActionKind, Actor, ActorClass, Preflight, PreflightTool, PreflightVerdict, RejectionReason,
    SpoolRecord, SpoolTrust,
};
use crate::supervisor::Supervisor;

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
        .map(|tool| tool.verdict)
        .min_by_key(|verdict| severity(*verdict))
        .unwrap_or(PreflightVerdict::Unchecked)
}

/// Whether a verdict is one a start in mode `refuse` is refused for.
const fn objects(verdict: PreflightVerdict) -> bool {
    matches!(
        verdict,
        PreflightVerdict::Insufficient
            | PreflightVerdict::UnverifiedSpool
            | PreflightVerdict::NoAnalysis
    )
}

/// The marks a spool holds, oldest first.
fn marks(spool: &SpoolRecord) -> Vec<crate::records::UnverifiedMark> {
    match &spool.trust {
        SpoolTrust::Verified => Vec::new(),
        SpoolTrust::Unverified { marks } => marks.clone(),
    }
}

/// The refusal one check makes, when it makes one: the first short tool, else
/// the first unverified one, else a file still unanalysed.
fn refusal(preflight: &Preflight, waited_s: u64) -> Option<RejectionReason> {
    let short = preflight.tools.iter().find_map(|tool| {
        match (
            tool.verdict,
            tool.spool_id,
            &tool.spool_name,
            tool.required_g,
            tool.buffer_g,
            tool.remaining_g,
        ) {
            (
                PreflightVerdict::Insufficient,
                Some(spool_id),
                Some(spool_name),
                Some(required_g),
                Some(buffer_g),
                Some(remaining_g),
            ) => Some(RejectionReason::InsufficientFilament {
                tool: tool.tool,
                spool_id,
                spool_name: spool_name.clone(),
                required_g,
                buffer_g,
                remaining_g,
                unverified: tool.marks.first().map(|mark| mark.reason.clone()),
            }),
            _ => None,
        }
    });
    let unverified = || {
        preflight.tools.iter().find_map(|tool| {
            match (
                tool.verdict,
                tool.spool_id,
                &tool.spool_name,
                tool.marks.first(),
            ) {
                (
                    PreflightVerdict::UnverifiedSpool,
                    Some(spool_id),
                    Some(spool_name),
                    Some(mark),
                ) => Some(RejectionReason::UnverifiedSpool {
                    tool: tool.tool,
                    spool_id,
                    spool_name: spool_name.clone(),
                    reason: mark.reason.clone(),
                }),
                _ => None,
            }
        })
    };
    let unanalysed = || {
        preflight
            .tools
            .iter()
            .any(|tool| tool.verdict == PreflightVerdict::NoAnalysis)
            .then(|| RejectionReason::AnalysisUnavailable {
                file_name: preflight.file_name.clone(),
                waited_s,
            })
    };
    short.or_else(unverified).or_else(unanalysed)
}

/// The same check, with every objecting tool's verdict replaced by one that
/// let the start go ahead.
fn let_through(mut preflight: Preflight, verdict: PreflightVerdict) -> Preflight {
    for tool in &mut preflight.tools {
        if objects(tool.verdict) {
            tool.verdict = verdict;
        }
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
                .find(|spool| spool.loaded_on_tool == Some(tool))
        };
        let named = |spool: Option<&SpoolRecord>, tool: i64, verdict| PreflightTool {
            tool,
            spool_id: spool.map(|spool| spool.id),
            spool_name: spool.map(|spool| spool.name.clone()),
            required_g: None,
            buffer_g: None,
            remaining_g: spool.map(|spool| spool.remaining_g),
            marks: spool.map(marks).unwrap_or_default(),
            verdict,
        };
        let tools: Vec<PreflightTool> = match facts.and_then(|facts| facts.filament.as_deref()) {
            Some(filament) => lengths_of(filament)
                .into_iter()
                .filter(|(_, length)| *length > NOTHING_MM)
                .map(|(tool, length)| match on_tool(tool) {
                    None => named(None, tool, PreflightVerdict::Unchecked),
                    Some(spool) => {
                        let required =
                            grams_for_length(length, spool.diameter_mm, spool.density_g_cm3);
                        let buffer = buffer_g(required, policy);
                        let marks = marks(spool);
                        let verdict = if spool.remaining_g < required + buffer {
                            PreflightVerdict::Insufficient
                        } else if marks.is_empty() {
                            PreflightVerdict::Passed
                        } else {
                            PreflightVerdict::UnverifiedSpool
                        };
                        PreflightTool {
                            required_g: Some(required),
                            buffer_g: Some(buffer),
                            ..named(Some(spool), tool, verdict)
                        }
                    }
                })
                .collect(),
            None => loaded
                .iter()
                .filter_map(|spool| {
                    let tool = spool.loaded_on_tool?;
                    let verdict = if marks(spool).is_empty() {
                        PreflightVerdict::NoAnalysis
                    } else {
                        PreflightVerdict::UnverifiedSpool
                    };
                    Some(named(Some(spool), tool, verdict))
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
            Delay::new(ANALYSIS_POLL).await;
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
    // llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
    pub(crate) async fn check_start(
        &self,
        file_name: &FileName,
        actor: &Actor,
        override_filament_check: bool,
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
            PreflightMode::Warn => (let_through(preflight, PreflightVerdict::Warned), None),
            _ if override_filament_check && actor.class() == ActorClass::Operator => {
                (let_through(preflight, PreflightVerdict::Overridden), None)
            }
            _ => (preflight, Some(reason)),
        })
    }
}
