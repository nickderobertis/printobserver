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
//!
//! # The material and the sheet
//!
//! Beside the weight, a start is held to two more checks, made the same way
//! and under the same mode. The **material** check compares the material the
//! file's own `filament_type` names for tool 0 with the material of the spool
//! loaded there, trimmed and without regard to case; a file naming none, or
//! whose settings cannot be read, is refused like a mismatch. The **sheet**
//! check reads the build sheet the start's manifest declares against the
//! configured [`SheetTable`](crate::SheetTable), keyed by that spool's
//! material: a sheet the table does not allow the material on, a material the
//! table has no entry for, and a sheet no entry lists are each refused. With
//! no tracked spool on tool 0 there is nothing to compare, and both read
//! unchecked.
//!
//! An operator overrides each check by its own flag, exactly as the weight is
//! overridden, and the start's reason is the override's. One refusal takes no
//! override and no mode: a manifest that declares no sheet at all.

use std::time::Duration;

use printobserver_printer_api::FileFacts;
use printobserver_types::FileName;

use crate::accounting::{NOTHING_MM, lengths_of, spool_on_tool};
use crate::clock::seconds_between;
use crate::config::PreflightMode;
use crate::delay::Delay;
use crate::error::CoreError;
use crate::filament::{buffer_g, grams_for_length};
use crate::records::{
    ActionKind, Actor, ActorClass, DoubtedSpool, MaterialComparison, MaterialName,
    MaterialObjection, MaterialResult, Preflight, PreflightNeed, PreflightObjection,
    PreflightSpool, PreflightTool, PreflightToolResult, PreflightVerdict, RejectionReason,
    SheetComparison, SheetId, SheetObjection, SheetResult, ShortSpool, SpoolRecord, SpoolTrust,
    UnanalysedSpool, UncomparedMaterial, UnverifiedMarks,
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

/// Which of the three checks a start asked to go ahead despite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Overrides {
    /// The filament weight check.
    pub(crate) filament: Override,
    /// The material check.
    pub(crate) material: Override,
    /// The sheet check, which never lets through a manifest declaring none.
    pub(crate) sheet: Override,
}

/// The tool the material and sheet checks are about: the one a single-tool
/// print feeds, and the first a multi-tool slicer names.
const CHECKED_TOOL: i64 = 0;

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

/// The refusal the material check makes, when it makes one.
fn material_refusal(preflight: &Preflight) -> Option<RejectionReason> {
    match &preflight.material {
        Some(MaterialResult::Refused { objection }) => Some(RejectionReason::MaterialRefused {
            objection: objection.clone(),
        }),
        _ => None,
    }
}

/// The refusal the sheet check makes, when it makes one.
fn sheet_refusal(preflight: &Preflight) -> Option<RejectionReason> {
    match &preflight.sheet {
        Some(SheetResult::Refused { objection }) => Some(RejectionReason::SheetRefused {
            objection: objection.clone(),
        }),
        _ => None,
    }
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

/// The same check, with each check that refused let through.
fn let_all_through(mut preflight: Preflight, how: LetThrough) -> Preflight {
    preflight = let_through(preflight, how);
    if let Some(MaterialResult::Refused { objection }) = preflight.material.clone() {
        preflight.material = Some(match how {
            LetThrough::Overridden => MaterialResult::Overridden { objection },
            LetThrough::Warned => MaterialResult::Warned { objection },
        });
    }
    if let Some(SheetResult::Refused { objection }) = preflight.sheet.clone() {
        preflight.sheet = Some(match how {
            LetThrough::Overridden => SheetResult::Overridden { objection },
            LetThrough::Warned => SheetResult::Warned { objection },
        });
    }
    preflight
}

/// The material a spool was registered as, which a registration refuses to
/// leave blank — so a spool always has one.
fn material_of(spool: &SpoolRecord) -> Option<MaterialName> {
    MaterialName::try_from(spool.material.clone()).ok()
}

/// Whether the spool's material is the one the file was sliced for, by what
/// the printer read of the file's settings.
fn judged_material(
    file_name: &FileName,
    spool: &SpoolRecord,
    loaded: MaterialName,
    read: Result<Vec<Option<String>>, String>,
) -> MaterialResult {
    let types = match read {
        Ok(types) => types,
        Err(detail) => {
            return MaterialResult::Refused {
                objection: MaterialObjection::Unread {
                    tool: CHECKED_TOOL,
                    spool: named(spool),
                    loaded,
                    file_name: file_name.clone(),
                    detail,
                },
            };
        }
    };
    let declared = types
        .into_iter()
        .next()
        .flatten()
        .and_then(|named| MaterialName::try_from(named).ok());
    let Some(sliced_for) = declared else {
        return MaterialResult::Refused {
            objection: MaterialObjection::Undeclared(UncomparedMaterial {
                tool: CHECKED_TOOL,
                spool: named(spool),
                loaded,
                file_name: file_name.clone(),
            }),
        };
    };
    let compared = MaterialComparison {
        tool: CHECKED_TOOL,
        spool: named(spool),
        loaded,
        sliced_for,
    };
    if compared.sliced_for.same(&compared.loaded) {
        MaterialResult::Passed(compared)
    } else {
        MaterialResult::Refused {
            objection: MaterialObjection::Mismatch(compared),
        }
    }
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
    pub async fn preflight(
        &self,
        file_name: &FileName,
        sheet: Option<&SheetId>,
    ) -> Result<Preflight, CoreError> {
        let facts = self.read_file_facts(file_name.clone()).await.ok();
        let loaded = self.stores().filament.loaded_spools().await?;
        let mut preflight = self.judge(file_name, facts.as_ref(), &loaded);
        preflight.material = Some(self.judge_material(file_name, &loaded).await);
        preflight.sheet = sheet.map(|sheet| self.judge_sheet(sheet, &loaded));
        Ok(preflight)
    }

    /// Whether the file was sliced for the material of the spool on tool 0:
    /// unchecked when no tracked spool is loaded there.
    async fn judge_material(&self, file_name: &FileName, loaded: &[SpoolRecord]) -> MaterialResult {
        let Some((spool, material)) = spool_on_tool(loaded, CHECKED_TOOL)
            .and_then(|spool| Some((spool, material_of(spool)?)))
        else {
            return MaterialResult::Unchecked;
        };
        let read = self
            .read_filament_types(file_name.clone())
            .await
            .map_err(|error| error.to_string());
        judged_material(file_name, spool, material, read)
    }

    /// Whether the table allows the material of the spool on tool 0 on a
    /// sheet: unchecked when no tracked spool is loaded there.
    fn judge_sheet(&self, sheet: &SheetId, loaded: &[SpoolRecord]) -> SheetResult {
        let Some(material) = spool_on_tool(loaded, CHECKED_TOOL).and_then(material_of) else {
            return SheetResult::Unchecked;
        };
        let table = &self.config().sheets;
        let sheet = sheet.clone();
        let objection = if !table.lists(&sheet) {
            SheetObjection::UnknownSheet {
                sheet,
                material,
                known: table.sheets(),
            }
        } else if let Some(allowed) = table.allowed_for(&material) {
            let compared = SheetComparison {
                allowed: allowed.to_vec(),
                sheet,
                material,
            };
            if allowed.iter().any(|listed| listed.same(&compared.sheet)) {
                return SheetResult::Passed(compared);
            }
            SheetObjection::NotAllowed(compared)
        } else {
            SheetObjection::UnlistedMaterial { sheet, material }
        };
        SheetResult::Refused { objection }
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
            material: None,
            sheet: None,
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
    /// A manifest declaring no sheet is refused whatever the mode and whoever
    /// overrides. Otherwise mode `off` checks nothing and records `unchecked`,
    /// and a short, unverified or still-unanalysed tool, a material mismatch
    /// or a sheet the table does not allow refuses the start in mode `refuse`
    /// — unless an operator overrode every check that refused, each by its own
    /// flag, which proceeds as `overridden` — and proceeds as `warned` in mode
    /// `warn`.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the loaded spools could not be read.
    pub(crate) async fn check_start(
        &self,
        file_name: &FileName,
        sheet: Option<&str>,
        actor: &Actor,
        asked: Overrides,
    ) -> Result<(Preflight, Option<RejectionReason>), CoreError> {
        let policy = self.config().spools;
        // A blank sheet names none, and is refused as declaring none.
        let declared = sheet.and_then(|sheet| SheetId::try_from(sheet.to_owned()).ok());
        let declared = declared.as_ref();
        let mut preflight = if policy.preflight == PreflightMode::Off {
            Preflight {
                file_name: file_name.as_str().to_owned(),
                tools: Vec::new(),
                verdict: PreflightVerdict::Unchecked,
                material: Some(MaterialResult::Unchecked),
                sheet: Some(SheetResult::Unchecked),
                checked_at: self.clock().now(),
            }
        } else {
            self.preflight(file_name, declared).await?
        };
        if declared.is_none() {
            preflight.sheet = Some(SheetResult::Undeclared);
            return Ok((preflight, Some(RejectionReason::SheetNotDeclared)));
        }
        if policy.preflight == PreflightMode::Off {
            return Ok((preflight, None));
        }
        let refusals = [
            (refusal(&preflight, policy.analysis_wait_s), asked.filament),
            (material_refusal(&preflight), asked.material),
            (sheet_refusal(&preflight), asked.sheet),
        ];
        if refusals.iter().all(|(refused, _)| refused.is_none()) {
            return Ok((preflight, None));
        }
        if policy.preflight == PreflightMode::Warn {
            return Ok((let_all_through(preflight, LetThrough::Warned), None));
        }
        let operator = actor.class() == ActorClass::Operator;
        let standing = refusals.into_iter().find_map(|(refused, asked)| {
            refused.filter(|_| !(operator && asked == Override::Asked))
        });
        Ok(match standing {
            Some(reason) => (preflight, Some(reason)),
            None => (let_all_through(preflight, LetThrough::Overridden), None),
        })
    }
}
