//! Reading one stored row back as the record the contracts declare.
//!
//! Each select names its columns in the order the mapper beside it reads them,
//! so a column added to the schema without being read shows up as a mapper that
//! no longer matches rather than as a value silently dropped.

use printobserver_core::store::{ChargeIdentity, ChargeRecord, FileTally};
use printobserver_core::{
    ActionRecord, ActionRequest, ChargedFilament, ImageRecord, Intervention, PrintRecord, SpoolId,
    SpoolLifecycle, SpoolRecord,
};
use printobserver_supervisor_api::SupervisionSession;
use printobserver_types::{EventBody, EventRecord, ImageRef, PrintId};
use rusqlite::Row;
use rusqlite::types::Type;

use crate::ledger::key_from_canonical;
use crate::values::{from_json, from_json_option, from_tag, parsed, parsed_option};

/// Every column of one print, in the order [`print_from_row`] reads them.
pub(crate) const PRINT_SELECT: &str = "SELECT id, provider_print_id, file_name, state, opened_at, \
     ended_at, end_reason, narrowings, job_started_at, job_print_time_s FROM prints";

/// Every column of one event, with the image it arrived with.
///
/// The image is read back from the images table rather than held on the event,
/// because an image row names the event it arrived with and a second copy of
/// that edge on the event could disagree with it.
pub(crate) const EVENT_SELECT: &str = "SELECT e.id, e.print_id, e.source, e.received_at, \
     e.payload, e.raw, \
     (SELECT i.id FROM images i WHERE i.event_id = e.id ORDER BY i.fetched_at, i.id LIMIT 1), \
     (SELECT i.sha256 FROM images i WHERE i.event_id = e.id ORDER BY i.fetched_at, i.id LIMIT 1) \
     FROM events e";

/// Every column of one image, in the order [`image_from_row`] reads them.
pub(crate) const IMAGE_SELECT: &str = "SELECT id, print_id, event_id, source_url, fetched_at, \
     content_type, byte_len, sha256, relative_path FROM images";

/// Every column of one action, with the execution outcome it may have.
pub(crate) const ACTION_SELECT: &str = "SELECT a.id, a.print_id, a.action, a.actor, \
     a.requested_at, a.decision, x.executed_at, x.outcome \
     FROM actions a LEFT JOIN executions x ON x.action_id = a.id";

/// Every column of one intervention.
pub(crate) const INTERVENTION_SELECT: &str = "SELECT id, print_id, action_id, adjustable, \
     prior_value, applied_value, applied_at, expires_at, restored_at, outcome FROM interventions";

/// Every column of one supervision session.
pub(crate) const SESSION_SELECT: &str = "SELECT print_id, session_name, harness_identity, \
     created_at, last_turn_at, closed_at, close_reason FROM sessions";

/// One print, as [`PRINT_SELECT`] answers it.
pub(crate) fn print_from_row(row: &Row<'_>) -> rusqlite::Result<PrintRecord> {
    Ok(PrintRecord {
        id: parsed(row, 0)?,
        provider_print_id: row.get(1)?,
        file_name: row.get(2)?,
        state: from_json(row, 3)?,
        opened_at: parsed(row, 4)?,
        ended_at: parsed_option(row, 5)?,
        end_reason: row.get(6)?,
        narrowings: from_json(row, 7)?,
        job_started_at: parsed_option(row, 8)?,
        job_print_time_s: row.get(9)?,
    })
}

/// One event, as [`EVENT_SELECT`] answers it.
pub(crate) fn event_from_row(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
    let image = match (parsed_option(row, 6)?, row.get::<_, Option<String>>(7)?) {
        (Some(id), Some(sha256)) => Some(ImageRef { id, sha256 }),
        _ => None,
    };
    Ok(EventRecord {
        id: parsed(row, 0)?,
        print_id: parsed_option(row, 1)?,
        source: from_tag(row, 2)?,
        received_at: parsed(row, 3)?,
        image,
        body: printobserver_core::upgrade::upgraded(from_json::<EventBody>(row, 4)?),
        raw: row
            .get::<_, Option<Vec<u8>>>(5)?
            .map(printobserver_types::RawBytes::new),
    })
}

/// One image, as [`IMAGE_SELECT`] answers it.
pub(crate) fn image_from_row(row: &Row<'_>) -> rusqlite::Result<ImageRecord> {
    Ok(ImageRecord {
        id: parsed(row, 0)?,
        print_id: parsed(row, 1)?,
        event_id: parsed(row, 2)?,
        source_url: row.get(3)?,
        fetched_at: parsed(row, 4)?,
        content_type: row.get(5)?,
        byte_len: row.get(6)?,
        sha256: row.get(7)?,
        relative_path: row.get(8)?,
    })
}

/// One action, as [`ACTION_SELECT`] answers it.
pub(crate) fn action_from_row(row: &Row<'_>) -> rusqlite::Result<ActionRecord> {
    Ok(ActionRecord {
        id: parsed(row, 0)?,
        print_id: parsed(row, 1)?,
        request: ActionRequest {
            action: from_json(row, 2)?,
            actor: from_json(row, 3)?,
            requested_at: parsed(row, 4)?,
        },
        decision: from_json(row, 5)?,
        executed_at: parsed_option(row, 6)?,
        outcome: from_json_option(row, 7)?,
    })
}

/// One intervention, as [`INTERVENTION_SELECT`] answers it.
pub(crate) fn intervention_from_row(row: &Row<'_>) -> rusqlite::Result<Intervention> {
    Ok(Intervention {
        id: parsed(row, 0)?,
        print_id: parsed(row, 1)?,
        action_id: parsed(row, 2)?,
        adjustable: parsed(row, 3)?,
        prior_value: row.get(4)?,
        applied_value: row.get(5)?,
        applied_at: parsed(row, 6)?,
        expires_at: parsed(row, 7)?,
        restored_at: parsed_option(row, 8)?,
        outcome: from_json(row, 9)?,
    })
}

/// One supervision session, as [`SESSION_SELECT`] answers it.
pub(crate) fn session_from_row(row: &Row<'_>) -> rusqlite::Result<SupervisionSession> {
    Ok(SupervisionSession {
        print_id: parsed(row, 0)?,
        session_name: row.get(1)?,
        harness_identity: row.get(2)?,
        created_at: parsed(row, 3)?,
        last_turn_at: parsed(row, 4)?,
        closed_at: parsed_option(row, 5)?,
        close_reason: row.get(6)?,
    })
}

/// Every column of one spool, in the order [`spool_from_row`] reads them.
pub(crate) const SPOOL_SELECT: &str = "SELECT id, name, material, color, diameter_mm, \
     density_g_cm3, initial_g, remaining_g, tare_g, loaded_on_tool, trust, registered_at, \
     updated_at, retired_at FROM spools";

/// Every column of one charge, in the order [`charge_from_row`] reads them.
pub(crate) const CHARGE_SELECT: &str = "SELECT charge_key, tool, print_id, spool_id, length_mm, \
     grams, charged_at, event_id, diameter_mm, density_g_cm3, remaining_before_g, \
     remaining_after_g FROM charges";

/// Every column of one file's tally, in the order [`tally_from_row`] reads them.
pub(crate) const TALLY_SELECT: &str = "SELECT file_name, origin, generation, successes, failures, \
     observed_at FROM file_tallies";

/// One spool, as [`SPOOL_SELECT`] answers it.
///
/// The loaded tool and the retirement are two columns held to one state by the
/// schema's triggers; a row holding both is reported rather than read.
pub(crate) fn spool_from_row(row: &Row<'_>) -> rusqlite::Result<SpoolRecord> {
    let id: String = row.get(0)?;
    let lifecycle = match (row.get::<_, Option<i64>>(9)?, parsed_option(row, 13)?) {
        (None, None) => SpoolLifecycle::Shelved,
        (Some(tool), None) => SpoolLifecycle::Loaded { tool },
        (None, Some(at)) => SpoolLifecycle::Retired { at },
        (Some(tool), Some(_)) => {
            return Err(reported(
                9,
                Type::Integer,
                format!("spool {id} is held both retired and loaded on tool {tool}"),
            ));
        }
    };
    Ok(SpoolRecord {
        id: parsed(row, 0)?,
        name: row.get(1)?,
        material: row.get(2)?,
        color: row.get(3)?,
        diameter_mm: row.get(4)?,
        density_g_cm3: row.get(5)?,
        initial_g: row.get(6)?,
        remaining_g: row.get(7)?,
        tare_g: row.get(8)?,
        lifecycle,
        trust: from_json(row, 10)?,
        registered_at: parsed(row, 11)?,
        updated_at: parsed(row, 12)?,
    })
}

/// A row this store reads back that is not one its records can hold.
fn reported(column: usize, kind: Type, detail: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, kind, detail.into())
}

/// One charge, as [`CHARGE_SELECT`] answers it.
///
/// The key is the debit's identity and the print and tool columns are its
/// projection, so a row whose projection disagrees with its key — or that
/// names a spool with half a calculation — is reported rather than read.
pub(crate) fn charge_from_row(row: &Row<'_>) -> rusqlite::Result<ChargeRecord> {
    let canonical: String = row.get(0)?;
    let key = key_from_canonical(&canonical).ok_or_else(|| {
        reported(
            0,
            Type::Text,
            format!("{canonical} is no charge key's canonical spelling"),
        )
    })?;
    let tool: i64 = row.get(1)?;
    if tool != key.tool() {
        return Err(reported(
            1,
            Type::Integer,
            format!("charge {canonical} stores tool {tool}, which its key does not name"),
        ));
    }
    let print_id: Option<PrintId> = parsed_option(row, 2)?;
    let identity = match (key.watched_print(), print_id) {
        (Some(named), Some(stored)) if named == stored => ChargeIdentity::of(key),
        (None, None) => ChargeIdentity::of(key),
        (None, Some(linked)) => ChargeIdentity::linked(key, linked).map_err(|_| {
            reported(
                2,
                Type::Text,
                format!("charge {canonical} cannot be linked"),
            )
        })?,
        (Some(_), stored) => {
            let stored = stored.map_or_else(|| "none".to_owned(), |id| id.to_string());
            return Err(reported(
                2,
                Type::Text,
                format!("charge {canonical} stores print {stored}, which its key does not name"),
            ));
        }
    };
    let spool_id: Option<SpoolId> = parsed_option(row, 3)?;
    let figures = (
        row.get::<_, Option<f64>>(5)?,
        row.get::<_, Option<f64>>(8)?,
        row.get::<_, Option<f64>>(9)?,
        row.get::<_, Option<f64>>(10)?,
        row.get::<_, Option<f64>>(11)?,
    );
    let charged = match (spool_id, figures) {
        (None, _) => None,
        (
            Some(spool_id),
            (
                Some(grams),
                Some(diameter_mm),
                Some(density_g_cm3),
                Some(remaining_before_g),
                Some(remaining_after_g),
            ),
        ) => Some(ChargedFilament {
            spool_id,
            grams,
            diameter_mm,
            density_g_cm3,
            remaining_before_g,
            remaining_after_g,
        }),
        (Some(spool_id), _) => {
            return Err(reported(
                3,
                Type::Text,
                format!(
                    "charge {canonical} names spool {spool_id} without the whole calculation of \
                     what it took"
                ),
            ));
        }
    };
    Ok(ChargeRecord {
        identity,
        length_mm: row.get(4)?,
        charged,
        at: parsed(row, 6)?,
        event_id: parsed(row, 7)?,
    })
}

/// One file's tally, as [`TALLY_SELECT`] answers it.
pub(crate) fn tally_from_row(row: &Row<'_>) -> rusqlite::Result<FileTally> {
    let generation: i64 = row.get(2)?;
    Ok(FileTally {
        file_name: row.get(0)?,
        origin: row.get(1)?,
        generation: u32::try_from(generation).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(2, Type::Integer, Box::new(error))
        })?,
        successes: row.get(3)?,
        failures: row.get(4)?,
        observed_at: parsed(row, 5)?,
    })
}
