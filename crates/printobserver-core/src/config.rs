//! What a running supervisor is configured with.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::records::{MaterialName, SafetyEnvelope, SheetId};
use printobserver_types::PrintId;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_vision_api::WebAddress;

/// The placeholder [`CoreConfig::context_command`] substitutes the print for.
pub const PRINT_ID_PLACEHOLDER: &str = "{print_id}";

/// How often the expiry driver looks for a bounded intervention that is due.
pub const DEFAULT_EXPIRY_POLL: Duration = Duration::from_millis(250);

/// How many of a print's own events a supervision turn is given.
pub const DEFAULT_RECENT_EVENTS: u32 = 20;

/// How long after the agent's last adjustment to a print the detector paused
/// that print is resumed, when its turn has not ended first.
///
/// Long enough for an agent to ask for the two or three changes it decided on
/// one after the other, so that they reach the print together as it moves
/// again; short enough that a paused hot end is not left sitting.
pub const DEFAULT_DETECTOR_RESUME_GRACE: Duration = Duration::from_secs(20);

/// How often every active print is observed when no detector has said
/// anything about it.
///
/// A frozen progress, a heater that went cold mid-print, a fan cut at a bridge
/// and an under-extruding wall all stay below what a failure detector alerts
/// on, so a print whose only trigger was the detector would never be looked at
/// for any of them. Two minutes is the rate the user chose: often enough to
/// catch a stall before it has printed far into the air, and an eight-hour
/// print is 240 turns.
pub const DEFAULT_OBSERVATION_INTERVAL: Duration = Duration::from_secs(120);

/// The longest one look may wait for something to happen, in seconds.
///
/// Below the two minutes a supervising agent's own shell gives one command, so
/// that a look is never cut off by the tool that asked for it. A longer wait is
/// refused rather than shortened.
pub const MAX_LOOK_WAIT_S: u32 = 90;

/// What a start does when the filament check finds a spool short, unverified,
/// or a file with no analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
pub enum PreflightMode {
    /// Refuse the start, unless the actor overrides the check.
    #[default]
    Refuse,
    /// Let the start go ahead, and record that the check warned.
    Warn,
    /// Make no check at all.
    Off,
}

/// How spools are checked before a print, and the margin asked for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", default, deny_unknown_fields)]
pub struct SpoolPolicy {
    /// What a start does when the check finds a problem.
    pub preflight: PreflightMode,
    /// The margin asked for beyond a print's requirement, as a percentage of
    /// it.
    pub buffer_percent: f64,
    /// The least margin asked for, in grams, however small the print.
    pub buffer_min_g: f64,
    /// How long a start waits for `OctoPrint`'s analysis of a file that has
    /// none yet, in whole seconds; zero waits not at all.
    pub analysis_wait_s: u64,
}

impl Default for SpoolPolicy {
    fn default() -> Self {
        Self {
            preflight: PreflightMode::Refuse,
            buffer_percent: 10.0,
            buffer_min_g: 5.0,
            analysis_wait_s: 300,
        }
    }
}

/// The build sheets each material may be printed on when the configuration
/// names no table of its own.
///
/// PLA on smooth PEI and PETG on textured PEI: PLA lifts off textured PEI's
/// grain, and PETG can bond to smooth PEI hard enough to tear the sheet
/// without glue.
pub const DEFAULT_SHEETS: [(&str, &[&str]); 2] =
    [("PLA", &["smooth-pei"]), ("PETG", &["textured-pei"])];

/// Which build sheets each material may be printed on, keyed by material.
///
/// Materials and sheets are matched trimmed and without regard to case, as a
/// spool's own material is written by whoever registered it. A table is
/// never empty in either direction: every material lists at least one sheet
/// and nothing is blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetTable {
    /// Each material, as written, and the sheets it may be printed on.
    entries: BTreeMap<MaterialName, Vec<SheetId>>,
}

impl SheetTable {
    /// A table of these entries.
    ///
    /// # Errors
    ///
    /// Returns why the entries are no table: a blank material or sheet, a
    /// material listing no sheet, or two materials that differ only in case
    /// or surrounding space.
    pub fn new(entries: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut table: BTreeMap<MaterialName, Vec<SheetId>> = BTreeMap::new();
        for (material, sheets) in entries {
            let material = MaterialName::try_from(material)?;
            if table.keys().any(|listed| listed.same(&material)) {
                return Err(format!(
                    "{material} is listed twice, once differing only in case or space"
                ));
            }
            if sheets.is_empty() {
                return Err(format!(
                    "{material} lists no sheet; leave a material out rather than list it on none"
                ));
            }
            let sheets = sheets
                .into_iter()
                .map(SheetId::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|why| format!("{material} lists a sheet that is blank: {why}"))?;
            table.insert(material, sheets);
        }
        Ok(Self { entries: table })
    }

    /// The sheets the table allows a material on, when it lists the material.
    #[must_use]
    pub fn allowed_for(&self, material: &MaterialName) -> Option<&[SheetId]> {
        self.entries
            .iter()
            .find(|(listed, _)| listed.same(material))
            .map(|(_, sheets)| sheets.as_slice())
    }

    /// Whether any entry lists a sheet.
    #[must_use]
    pub fn lists(&self, sheet: &SheetId) -> bool {
        self.entries
            .values()
            .flatten()
            .any(|listed| listed.same(sheet))
    }

    /// Every sheet some entry lists, once each, in order.
    #[must_use]
    pub fn sheets(&self) -> Vec<SheetId> {
        let mut known: Vec<SheetId> = Vec::new();
        for sheet in self.entries.values().flatten() {
            if !known.iter().any(|listed| listed.same(sheet)) {
                known.push(sheet.clone());
            }
        }
        known.sort();
        known
    }
}

impl Default for SheetTable {
    fn default() -> Self {
        Self::new(
            DEFAULT_SHEETS
                .iter()
                .map(|(material, sheets)| {
                    (
                        (*material).to_owned(),
                        sheets.iter().map(|sheet| (*sheet).to_owned()).collect(),
                    )
                })
                .collect(),
        )
        .unwrap_or_else(|why| unreachable!("the default sheet table is a table: {why}"))
    }
}

/// The server configuration supervision reads.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreConfig {
    /// The operator's safety envelope: what any actor may ask for at all.
    pub envelope: SafetyEnvelope,
    /// The command a supervision turn runs to read the print's context.
    ///
    /// [`PRINT_ID_PLACEHOLDER`] in it is replaced with the print's own
    /// identifier, so that a turn reads the context of the print it is about.
    pub context_command: String,
    /// How many of the print's own events a turn is given, newest first.
    pub recent_events: u32,
    /// How often the expiry driver looks for an intervention that is due.
    pub expiry_poll: Duration,
    /// Where a fresh frame of the print is fetched from, when a camera is
    /// configured.
    pub camera_snapshot_url: Option<WebAddress>,
    /// How long after the agent's last adjustment a print the detector paused
    /// is resumed.
    pub detector_resume_grace: Duration,
    /// How often every active print is given a periodic observation.
    pub observation_interval: Duration,
    /// How spools are checked before a print, and the margin asked for.
    pub spools: SpoolPolicy,
    /// Which build sheets each material may be printed on.
    pub sheets: SheetTable,
    /// How often the printer watch reads the machine.
    pub printer_poll: Duration,
    /// How long after an `interrupted` or `failed` ending the printer acting
    /// on its own is recorded.
    pub unattended_window: Duration,
    /// The printer's own facts a recovery is checked against; absent, every
    /// recovery action is refused as not configured.
    pub recovery: Option<crate::records::RecoveryConfig>,
    /// Who may ask for which recovery action.
    pub recovery_grants: crate::records::RecoveryGrants,
}

impl CoreConfig {
    /// The configuration for one envelope, with this crate's own defaults.
    #[must_use]
    pub fn new(envelope: SafetyEnvelope, context_command: String) -> Self {
        Self {
            envelope,
            context_command,
            recent_events: DEFAULT_RECENT_EVENTS,
            expiry_poll: DEFAULT_EXPIRY_POLL,
            camera_snapshot_url: None,
            detector_resume_grace: DEFAULT_DETECTOR_RESUME_GRACE,
            observation_interval: DEFAULT_OBSERVATION_INTERVAL,
            spools: SpoolPolicy::default(),
            sheets: SheetTable::default(),
            printer_poll: crate::watch::DEFAULT_PRINTER_POLL,
            unattended_window: crate::watch::DEFAULT_UNATTENDED_WINDOW,
            recovery: None,
            recovery_grants: crate::records::RecoveryGrants::default(),
        }
    }

    /// The context command for one print.
    #[must_use]
    pub fn context_command_for(&self, print_id: PrintId) -> String {
        self.context_command
            .replace(PRINT_ID_PLACEHOLDER, &print_id.to_string())
    }
}
