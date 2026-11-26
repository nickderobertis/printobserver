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
//!
//! # A file with planned colour changes
//!
//! A single-nozzle print sliced with colour changes is fed by one spool per
//! stretch between them, so its whole length is no one spool's. The file is
//! read once through the G-code index ([`crate::gcode_index`]): when it plans a
//! change, the spool on tool 0 is checked against segment 0's filament alone,
//! every segment's filament is recorded with the spool meant for it, and each
//! later segment a start's manifest names a spool for in `segment_spools` is
//! checked against that spool, a short or doubted one refused naming the
//! segment. A file planning no change, or one that cannot be read, is checked
//! as a whole, as before.

use std::time::Duration;

use printobserver_printer_api::FileFacts;
use printobserver_types::FileName;

use crate::accounting::{NOTHING_MM, lengths_of, spool_on_tool};
use crate::clock::seconds_between;
use crate::config::{PreflightMode, SpoolPolicy};
use crate::delay::Delay;
use crate::error::CoreError;
use crate::filament::{buffer_g, grams_for_length};
use crate::gcode_index::{GcodeIndex, IndexBuilder, PlannedSegment};
use crate::records::{
    ActionKind, Actor, ActorClass, DoubtedSpool, MaterialComparison, MaterialName,
    MaterialObjection, MaterialResult, Preflight, PreflightNeed, PreflightObjection,
    PreflightSegment, PreflightSpool, PreflightTool, PreflightToolResult, PreflightVerdict,
    RejectionReason, SegmentSpool, SheetComparison, SheetId, SheetObjection, SheetResult,
    ShortSpool, SpoolRecord, SpoolTrust, UnanalysedSpool, UncomparedMaterial, UnverifiedMarks,
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
    // With planned changes, tool 0 was checked against segment 0 alone.
    let segment_of =
        |tool: i64| (tool == CHECKED_TOOL && !preflight.segments.is_empty()).then_some(0);
    let short = preflight.tools.iter().find_map(|tool| match &tool.result {
        PreflightToolResult::Insufficient(short) => Some(RejectionReason::InsufficientFilament {
            tool: tool.tool,
            spool_id: short.spool.id,
            spool_name: short.spool.name.clone(),
            required_g: short.need.required_g,
            buffer_g: short.need.buffer_g,
            remaining_g: short.remaining_g,
            unverified: short.marks.first().map(|mark| mark.reason.clone()),
            segment: segment_of(tool.tool),
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
                    segment: segment_of(tool.tool),
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

/// The refusal one later segment's named spool makes, judged against that
/// segment's filament: short, else in doubt.
fn segment_refusal(segment: u32, result: &PreflightToolResult) -> Option<RejectionReason> {
    match result {
        PreflightToolResult::Insufficient(short) => Some(RejectionReason::InsufficientFilament {
            tool: CHECKED_TOOL,
            spool_id: short.spool.id,
            spool_name: short.spool.name.clone(),
            required_g: short.need.required_g,
            buffer_g: short.need.buffer_g,
            remaining_g: short.remaining_g,
            unverified: short.marks.first().map(|mark| mark.reason.clone()),
            segment: Some(segment),
        }),
        PreflightToolResult::UnverifiedSpool(doubted) => Some(RejectionReason::UnverifiedSpool {
            tool: CHECKED_TOOL,
            spool_id: doubted.spool.id,
            spool_name: doubted.spool.name.clone(),
            reason: doubted.marks.oldest().reason.clone(),
            segment: Some(segment),
        }),
        _ => None,
    }
}

/// What one spool must hold for one segment: its filament as that spool's
/// grams, and the buffer the policy asks beyond them.
fn need_for(segment: &PlannedSegment, spool: &SpoolRecord, policy: &SpoolPolicy) -> PreflightNeed {
    let required_g = grams_for_length(segment.filament_mm, spool.diameter_mm, spool.density_g_cm3);
    PreflightNeed {
        required_g,
        buffer_g: buffer_g(required_g, policy),
    }
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
// llmlint: ignore-block[invalid_states_unrepresentable] suppressions.toml has the reason.
fn judged_material(
    file_name: &FileName,
    spool: &SpoolRecord,
    loaded: MaterialName,
    read: Result<Vec<Option<String>>, String>,
) -> MaterialResult {
    // llmlint: ignore-end[invalid_states_unrepresentable]
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
        Ok(self.preflight_for(file_name, sheet, &[]).await?.0)
    }

    /// The check of one file, with every later segment the manifest names a
    /// spool for checked against that spool, and the refusal the first short
    /// or doubted one of them makes.
    async fn preflight_for(
        &self,
        file_name: &FileName,
        sheet: Option<&SheetId>,
        named: &[SegmentSpool],
    ) -> Result<(Preflight, Option<RejectionReason>), CoreError> {
        let facts = self.read_file_facts(file_name.clone()).await.ok();
        let loaded = self.stores().filament.loaded_spools().await?;
        let mut preflight = self.judge(file_name, facts.as_ref(), &loaded);
        let refused = match self.planned_index_of(file_name).await {
            Some(index) => {
                self.judge_segments(&mut preflight, &index, &loaded, named)
                    .await?
            }
            None => None,
        };
        preflight.material = Some(self.judge_material(file_name, &loaded).await);
        preflight.sheet = sheet.map(|sheet| self.judge_sheet(sheet, &loaded));
        Ok((preflight, refused))
    }

    /// The index of a file a start names, when it can be read and plans a
    /// change: read through the same builder a print's index is, holding no
    /// spool, and kept no longer than the check.
    async fn planned_index_of(&self, file_name: &FileName) -> Option<GcodeIndex> {
        let mut builder = IndexBuilder::new();
        self.read_gcode(file_name.clone(), &mut builder)
            .await
            .ok()?;
        let index = builder.finish();
        index.planned_change(0).is_some().then_some(index)
    }

    /// Check tool 0 against segment 0 alone, record every segment with the
    /// spool meant for it, and check each later segment the manifest names a
    /// spool for against that spool. Answers the refusal the first short or
    /// doubted named spool makes.
    async fn judge_segments(
        &self,
        preflight: &mut Preflight,
        index: &GcodeIndex,
        loaded: &[SpoolRecord],
        named: &[SegmentSpool],
    ) -> Result<Option<RejectionReason>, CoreError> {
        let policy = self.config().spools;
        let segments = index.planned_segments();
        let first = spool_on_tool(loaded, CHECKED_TOOL);
        if let (Some(spool), Some(segment)) = (first, segments.first()) {
            let result = judged(spool, need_for(segment, spool, &policy));
            match preflight
                .tools
                .iter_mut()
                .find(|tool| tool.tool == CHECKED_TOOL)
            {
                Some(tool) => tool.result = result,
                None => preflight.tools.insert(
                    0,
                    PreflightTool {
                        tool: CHECKED_TOOL,
                        result,
                    },
                ),
            }
        }
        let mut refused = None;
        let mut checked = Vec::with_capacity(segments.len());
        for segment in &segments {
            let spool = if segment.segment == 0 {
                first.cloned()
            } else {
                match named.iter().find(|named| named.segment == segment.segment) {
                    Some(named) => self.stores().filament.spool(named.spool_id).await?,
                    None => None,
                }
            };
            if let Some(spool) = spool.as_ref().filter(|_| segment.segment > 0) {
                let result = judged(spool, need_for(segment, spool, &policy));
                refused = refused.or_else(|| segment_refusal(segment.segment, &result));
                checked.push(result.verdict());
            }
            preflight.segments.push(PreflightSegment {
                segment: segment.segment,
                color: segment.color.clone(),
                filament_mm: segment.filament_mm,
                required_g: spool
                    .as_ref()
                    .map(|spool| need_for(segment, spool, &policy).required_g),
                spool_id: spool.as_ref().map(|spool| spool.id),
            });
        }
        preflight.verdict = checked
            .into_iter()
            .chain(Some(worst(&preflight.tools)))
            .min_by_key(|verdict| severity(*verdict))
            .unwrap_or(PreflightVerdict::Unchecked);
        Ok(refused)
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
            segments: Vec::new(),
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
        segment_spools: &[SegmentSpool],
        actor: &Actor,
        asked: Overrides,
    ) -> Result<(Preflight, Option<RejectionReason>), CoreError> {
        let policy = self.config().spools;
        // A blank sheet names none, and is refused as declaring none.
        let declared = sheet.and_then(|sheet| SheetId::try_from(sheet.to_owned()).ok());
        let declared = declared.as_ref();
        let (mut preflight, segment_refused) = if policy.preflight == PreflightMode::Off {
            let unchecked = Preflight {
                file_name: file_name.as_str().to_owned(),
                tools: Vec::new(),
                verdict: PreflightVerdict::Unchecked,
                material: Some(MaterialResult::Unchecked),
                sheet: Some(SheetResult::Unchecked),
                segments: Vec::new(),
                checked_at: self.clock().now(),
            };
            (unchecked, None)
        } else {
            self.preflight_for(file_name, declared, segment_spools)
                .await?
        };
        if declared.is_none() {
            preflight.sheet = Some(SheetResult::Undeclared);
            return Ok((preflight, Some(RejectionReason::SheetNotDeclared)));
        }
        if policy.preflight == PreflightMode::Off {
            return Ok((preflight, None));
        }
        let segment_let_through = segment_refused.is_some();
        let refusals = [
            (
                refusal(&preflight, policy.analysis_wait_s).or(segment_refused),
                asked.filament,
            ),
            (material_refusal(&preflight), asked.material),
            (sheet_refusal(&preflight), asked.sheet),
        ];
        if refusals.iter().all(|(refused, _)| refused.is_none()) {
            return Ok((preflight, None));
        }
        // A later segment's spool lets the start through as tool 0's would,
        // and the verdict says so whatever tool 0 was found to hold.
        let through = |preflight: Preflight, how: LetThrough| {
            let mut preflight = let_all_through(preflight, how);
            if segment_let_through {
                let let_through = match how {
                    LetThrough::Overridden => PreflightVerdict::Overridden,
                    LetThrough::Warned => PreflightVerdict::Warned,
                };
                if severity(let_through) < severity(preflight.verdict) {
                    preflight.verdict = let_through;
                }
            }
            preflight
        };
        if policy.preflight == PreflightMode::Warn {
            return Ok((through(preflight, LetThrough::Warned), None));
        }
        let operator = actor.class() == ActorClass::Operator;
        let standing = refusals.into_iter().find_map(|(refused, asked)| {
            refused.filter(|_| !(operator && asked == Override::Asked))
        });
        Ok(match standing {
            Some(reason) => (preflight, Some(reason)),
            None => (through(preflight, LetThrough::Overridden), None),
        })
    }
}
