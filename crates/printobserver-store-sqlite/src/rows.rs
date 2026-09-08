//! Reading one stored row back as the record the contracts declare.
//!
//! Each select names its columns in the order the mapper beside it reads them,
//! so a column added to the schema without being read shows up as a mapper that
//! no longer matches rather than as a value silently dropped.

use printobserver_types::{
    ActionRecord, ActionRequest, EventRecord, ImageRecord, ImageRef, Intervention, PrintRecord,
    SupervisionSession,
};
use rusqlite::Row;

use crate::values::{from_json, from_json_option, from_tag, parsed, parsed_option};

/// Every column of one print, in the order [`print_from_row`] reads them.
pub(crate) const PRINT_SELECT: &str = "SELECT id, obico_print_id, file_name, state, opened_at, \
     ended_at, end_reason, narrowings FROM prints";

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
        obico_print_id: row.get(1)?,
        file_name: row.get(2)?,
        state: from_json(row, 3)?,
        opened_at: parsed(row, 4)?,
        ended_at: parsed_option(row, 5)?,
        end_reason: row.get(6)?,
        narrowings: from_json(row, 7)?,
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
        payload: from_json(row, 4)?,
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
