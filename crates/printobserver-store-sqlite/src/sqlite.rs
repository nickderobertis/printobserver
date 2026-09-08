//! The durable store: one embedded database, and image bytes beside it.

use core::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use printobserver_store_api::{
    AuditPage, BoxFuture, EventDraft, HistoryQuery, ImageLookup, SettleOutcome, StoreError,
    StorePort, resolve_history_limit,
};
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, EventId, EventRecord, ExecutionOutcome,
    ImageId, ImageRecord, Intervention, InterventionId, InterventionOutcome, JobManifest,
    ManifestNarrowing, PolicyDecision, PrintId, PrintRecord, PrinterState, RawBytes,
    SupervisionSession, Timestamp,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params, params_from_iter};

use crate::hold::{HoldPoints, settle_label};
use crate::images::{resolve, store_bytes};
use crate::rows::{
    ACTION_SELECT, EVENT_SELECT, IMAGE_SELECT, INTERVENTION_SELECT, PRINT_SELECT, SESSION_SELECT,
    action_from_row, event_from_row, image_from_row, intervention_from_row, print_from_row,
    session_from_row,
};
use crate::schema::{DATABASE_FILE_NAME, connect, open_database};
use crate::values::{
    database_error, instant_text, is_constraint_violation, json_text, not_found, refused, tag_text,
};

/// The one durable implementation of the store port.
///
/// One embedded database under the configured state directory, with image bytes
/// on the filesystem beside it and rows pointing at them.
#[derive(Debug)]
pub struct SqliteStore {
    /// The configured state directory, which every stored path is relative to.
    state_dir: PathBuf,
    /// Connections not in use. The server is one process but not one thread, so
    /// a read taken while a writer holds an open transaction is a read on a
    /// connection of its own — which, under write-ahead logging, answers the
    /// pre-write state rather than waiting for the commit.
    idle: Mutex<Vec<Connection>>,
    /// The two places this store's own tests hold a call at.
    points: HoldPoints,
}

impl SqliteStore {
    /// Open the store in a state directory, creating and migrating it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when the state directory cannot be created,
    /// and [`StoreError::Database`] when the database cannot be opened, or is
    /// from a later schema version than this build understands.
    pub fn open(state_dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        let state_dir = state_dir.as_ref().to_path_buf();
        let connection = open_database(&state_dir)?;
        Ok(Self {
            state_dir,
            idle: Mutex::new(vec![connection]),
            points: HoldPoints::default(),
        })
    }

    /// The state directory every stored path is relative to.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// The places this store holds a call at, for a test that arms one.
    #[must_use]
    pub const fn hold_points(&self) -> &HoldPoints {
        &self.points
    }

    /// Run one unit of work on a connection of this store's own.
    fn on_connection<T>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let taken = self
            .idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        let mut connection = match taken {
            Some(connection) => connection,
            None => connect(&self.state_dir.join(DATABASE_FILE_NAME))?,
        };
        let answer = work(&mut connection);
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(connection);
        answer
    }

    /// Which reference a refused write named, by asking which row is absent.
    ///
    /// The refusal is the database's; this only reads back which of the rows the
    /// write named does not exist, so the caller is told the reference rather
    /// than the driver's own word for a violated constraint.
    fn refusal(
        connection: &Connection,
        error: &rusqlite::Error,
        candidates: &[(&str, &str, String)],
    ) -> StoreError {
        if !is_constraint_violation(error) {
            return database_error(error);
        }
        for (constraint, table, identifier) in candidates {
            let query = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)");
            let present = connection
                .query_row(&query, params![identifier], |row| row.get::<_, bool>(0))
                .unwrap_or(true);
            if !present {
                return refused(constraint);
            }
        }
        refused(
            candidates
                .first()
                .map_or("a declared constraint", |(c, _, _)| c),
        )
    }

    /// Open a print, minting its identifier.
    fn insert_print(
        &self,
        obico_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> Result<PrintRecord, StoreError> {
        let record = PrintRecord {
            id: PrintId::new(),
            obico_print_id,
            file_name,
            state: PrinterState::Printing,
            opened_at: Timestamp::now(),
            ended_at: None,
            end_reason: None,
            narrowings: Vec::new(),
        };
        let state = json_text(&record.state)?;
        let narrowings = json_text(&record.narrowings)?;
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO prints \
                     (id, obico_print_id, file_name, state, opened_at, ended_at, \
                      end_reason, narrowings) \
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)",
                    params![
                        record.id.to_string(),
                        record.obico_print_id,
                        record.file_name,
                        state,
                        instant_text(record.opened_at),
                        narrowings,
                    ],
                )
                .map_err(|error| database_error(&error))?;
            Ok(record.clone())
        })
    }

    /// Read the newest print matching one clause over its own columns.
    fn read_print(
        &self,
        clause: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Option<PrintRecord>, StoreError> {
        let query =
            format!("{PRINT_SELECT} WHERE {clause} ORDER BY opened_at DESC, id DESC LIMIT 1");
        self.on_connection(|connection| {
            connection
                .query_row(&query, parameters, print_from_row)
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(database_error(&other)),
                })
        })
    }

    /// One print, or the refusal that there is no such print.
    fn require_print(&self, print_id: PrintId) -> Result<PrintRecord, StoreError> {
        let identifier = print_id.to_string();
        self.read_print("id = ?1", params![identifier])?
            .ok_or_else(|| not_found(&format!("print {print_id}")))
    }

    /// End a print, in a terminal state, at an instant, for a reason.
    fn write_end(
        &self,
        print_id: PrintId,
        state: &PrinterState,
        ended_at: Timestamp,
        reason: &str,
    ) -> Result<PrintRecord, StoreError> {
        let state = json_text(state)?;
        self.on_connection(|connection| {
            let changed = connection
                .execute(
                    "UPDATE prints SET state = ?2, ended_at = ?3, end_reason = ?4 WHERE id = ?1",
                    params![print_id.to_string(), state, instant_text(ended_at), reason],
                )
                .map_err(|error| database_error(&error))?;
            if changed == 0 {
                return Err(not_found(&format!("print {print_id}")));
            }
            Ok(())
        })?;
        self.require_print(print_id)
    }

    /// Record that a manifest range was narrowed to the envelope's.
    ///
    /// Read and written in one transaction, so that two narrowings recorded at
    /// once are both kept rather than one overwriting the other's list.
    fn write_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> Result<PrintRecord, StoreError> {
        let identifier = print_id.to_string();
        let query = format!("{PRINT_SELECT} WHERE id = ?1");
        self.on_connection(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| database_error(&error))?;
            let mut record = transaction
                .query_row(&query, params![identifier], print_from_row)
                .optional()
                .map_err(|error| database_error(&error))?
                .ok_or_else(|| not_found(&format!("print {print_id}")))?;
            record.narrowings.push(narrowing);
            let narrowings = json_text(&record.narrowings)?;
            transaction
                .execute(
                    "UPDATE prints SET narrowings = ?2 WHERE id = ?1",
                    params![print_id.to_string(), narrowings],
                )
                .map_err(|error| database_error(&error))?;
            transaction
                .commit()
                .map_err(|error| database_error(&error))?;
            Ok(record)
        })
    }

    /// Append one event, minting its identifier.
    fn insert_event(&self, draft: EventDraft) -> Result<EventRecord, StoreError> {
        let record = EventRecord {
            id: EventId::new(),
            print_id: draft.print_id,
            source: draft.source,
            received_at: draft.received_at,
            image: None,
            payload: draft.payload,
            raw: draft.raw,
        };
        let source = tag_text(&record.source)?;
        let kind = tag_text(&record.kind())?;
        let payload = json_text(&record.payload)?;
        let raw = record.raw.as_ref().map(|bytes| bytes.as_slice().to_vec());
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO events (id, print_id, source, received_at, kind, payload, raw) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        record.id.to_string(),
                        record.print_id.map(|id| id.to_string()),
                        source,
                        instant_text(record.received_at),
                        kind,
                        payload,
                        raw,
                    ],
                )
                .map_err(|error| {
                    let candidates: Vec<(&str, &str, String)> = record
                        .print_id
                        .map(|id| vec![("events.print_id", "prints", id.to_string())])
                        .unwrap_or_default();
                    Self::refusal(connection, &error, &candidates)
                })?;
            Ok(record.clone())
        })
    }

    /// Store one image's bytes, minting its identifier.
    fn insert_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: &RawBytes,
    ) -> Result<ImageRecord, StoreError> {
        let stored = store_bytes(&self.state_dir, bytes.as_slice(), &self.points)?;
        let record = ImageRecord {
            id: ImageId::new(),
            print_id,
            event_id,
            source_url,
            fetched_at: Timestamp::now(),
            content_type,
            byte_len: stored.byte_len,
            sha256: stored.sha256,
            relative_path: stored.relative_path,
        };
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO images (id, print_id, event_id, source_url, fetched_at, \
                     content_type, byte_len, sha256, relative_path) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        record.id.to_string(),
                        record.print_id.to_string(),
                        record.event_id.to_string(),
                        record.source_url,
                        instant_text(record.fetched_at),
                        record.content_type,
                        record.byte_len,
                        record.sha256,
                        record.relative_path,
                    ],
                )
                .map_err(|error| {
                    Self::refusal(
                        connection,
                        &error,
                        &[
                            ("images.print_id", "prints", print_id.to_string()),
                            ("images.event_id", "events", event_id.to_string()),
                        ],
                    )
                })?;
            Ok(record.clone())
        })
    }

    /// Read one image, saying whether its file is where the row says it is.
    fn read_image(&self, image_id: ImageId) -> Result<ImageLookup, StoreError> {
        let identifier = image_id.to_string();
        let query = format!("{IMAGE_SELECT} WHERE id = ?1");
        let record = self.on_connection(|connection| {
            connection
                .query_row(&query, params![identifier], image_from_row)
                .optional()
                .map_err(|error| database_error(&error))
        })?;
        let record = record.ok_or_else(|| not_found(&format!("image {image_id}")))?;
        let path = resolve(&self.state_dir, &record.relative_path);
        if path.is_file() {
            Ok(ImageLookup::Found { record, path })
        } else {
            Ok(ImageLookup::FileMissing { record })
        }
    }
}

impl SqliteStore {
    /// Record one request together with the decision taken on it.
    ///
    /// The action binds to the open print — the most recently opened print with
    /// no end recorded — because the request names none and an action recorded
    /// against no print is one nobody can review afterwards.
    fn insert_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> Result<ActionRecord, StoreError> {
        let action = json_text(&request.action)?;
        let actor = json_text(&request.actor)?;
        let decision_text = json_text(&decision)?;
        let requested_at = instant_text(request.requested_at);
        let id = ActionId::new();
        self.on_connection(move |connection| {
            // One transaction, so the print an action binds to cannot end
            // between the two statements that find it and name it.
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| database_error(&error))?;
            let print_id: Option<PrintId> = transaction
                .query_row(
                    "SELECT id FROM prints WHERE ended_at IS NULL \
                     ORDER BY opened_at DESC, id DESC LIMIT 1",
                    [],
                    |row| crate::values::parsed(row, 0),
                )
                .optional()
                .map_err(|error| database_error(&error))?;
            let print_id =
                print_id.ok_or_else(|| not_found("open print to record this action against"))?;
            transaction
                .execute(
                    "INSERT INTO actions (id, print_id, action, actor, requested_at, decision) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        id.to_string(),
                        print_id.to_string(),
                        action,
                        actor,
                        requested_at,
                        decision_text
                    ],
                )
                .map_err(|error| database_error(&error))?;
            transaction
                .commit()
                .map_err(|error| database_error(&error))?;
            Ok(ActionRecord {
                id,
                print_id,
                request,
                decision,
                executed_at: None,
                outcome: None,
            })
        })
    }

    /// Read one action, with whatever the printer made of it.
    fn read_action(&self, action_id: ActionId) -> Result<Option<ActionRecord>, StoreError> {
        let identifier = action_id.to_string();
        let query = format!("{ACTION_SELECT} WHERE a.id = ?1");
        self.on_connection(|connection| {
            connection
                .query_row(&query, params![identifier], action_from_row)
                .optional()
                .map_err(|error| database_error(&error))
        })
    }

    /// Record what the printer made of an action that reached it.
    ///
    /// The outcome is a row of its own whose primary key references the action,
    /// so an outcome against an action nothing minted is refused by the schema
    /// rather than reported as a record that could not be found.
    fn insert_execution(
        &self,
        action_id: ActionId,
        outcome: &ExecutionOutcome,
    ) -> Result<ActionRecord, StoreError> {
        let outcome_text = json_text(outcome)?;
        let executed_at = instant_text(Timestamp::now());
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO executions (action_id, executed_at, outcome) \
                     VALUES (?1, ?2, ?3)",
                    params![action_id.to_string(), executed_at, outcome_text],
                )
                .map_err(|error| {
                    Self::refusal(
                        connection,
                        &error,
                        &[("executions.action_id", "actions", action_id.to_string())],
                    )
                })?;
            Ok(())
        })?;
        self.read_action(action_id)?
            .ok_or_else(|| not_found(&format!("action {action_id}")))
    }

    /// Open a bounded intervention against the print its action belongs to.
    fn insert_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> Result<Intervention, StoreError> {
        let id = InterventionId::new();
        let still_active = json_text(&InterventionOutcome::StillActive)?;
        self.on_connection(|connection| {
            let inserted = connection
                .execute(
                    "INSERT INTO interventions (id, print_id, action_id, adjustable, \
                     prior_value, applied_value, applied_at, expires_at, restored_at, outcome) \
                     SELECT ?1, a.print_id, a.id, ?2, ?3, ?4, ?5, ?6, NULL, ?7 \
                     FROM actions a WHERE a.id = ?8",
                    params![
                        id.to_string(),
                        adjustable.to_string(),
                        prior_value,
                        applied_value,
                        instant_text(applied_at),
                        instant_text(expires_at),
                        still_active,
                        action_id.to_string(),
                    ],
                )
                .map_err(|error| database_error(&error))?;
            if inserted == 0 {
                return Err(refused("interventions.action_id"));
            }
            Ok(())
        })?;
        self.read_intervention(id)?
            .ok_or_else(|| not_found(&format!("intervention {id}")))
    }

    /// Read one intervention.
    fn read_intervention(
        &self,
        intervention_id: InterventionId,
    ) -> Result<Option<Intervention>, StoreError> {
        let identifier = intervention_id.to_string();
        let query = format!("{INTERVENTION_SELECT} WHERE id = ?1");
        self.on_connection(|connection| {
            connection
                .query_row(&query, params![identifier], intervention_from_row)
                .optional()
                .map_err(|error| database_error(&error))
        })
    }

    /// Settle one intervention, or report the outcome that already won.
    ///
    /// The outcome is read, and then written by an update conditional on that
    /// same outcome, so the read and the write are one step: an expiry and a
    /// supersession contending here cannot both take effect, whichever order
    /// they reach this in.
    fn settle(
        &self,
        intervention_id: InterventionId,
        outcome: &InterventionOutcome,
    ) -> Result<SettleOutcome, StoreError> {
        let current = self
            .read_intervention(intervention_id)?
            .ok_or_else(|| not_found(&format!("intervention {intervention_id}")))?
            .outcome;
        self.points.settle().reach(settle_label(outcome));
        if current != InterventionOutcome::StillActive {
            return Ok(SettleOutcome::AlreadySettled { outcome: current });
        }

        let still_active = json_text(&InterventionOutcome::StillActive)?;
        let recorded = json_text(outcome)?;
        let restored_at =
            (*outcome == InterventionOutcome::Restored).then(|| instant_text(Timestamp::now()));
        let changed = self.on_connection(|connection| {
            connection
                .execute(
                    "UPDATE interventions SET outcome = ?2, restored_at = ?3 \
                     WHERE id = ?1 AND outcome = ?4",
                    params![
                        intervention_id.to_string(),
                        recorded,
                        restored_at,
                        still_active
                    ],
                )
                .map_err(|error| database_error(&error))
        })?;
        let settled = self
            .read_intervention(intervention_id)?
            .ok_or_else(|| not_found(&format!("intervention {intervention_id}")))?;
        if changed == 1 {
            Ok(SettleOutcome::Settled {
                intervention: settled,
            })
        } else {
            Ok(SettleOutcome::AlreadySettled {
                outcome: settled.outcome,
            })
        }
    }

    /// Read every intervention matching one clause, in a stated order.
    fn read_interventions(
        &self,
        clause: &str,
        order: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<Intervention>, StoreError> {
        let query = format!("{INTERVENTION_SELECT} WHERE {clause} ORDER BY {order}");
        self.on_connection(|connection| {
            let mut statement = connection
                .prepare(&query)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map(parameters, intervention_from_row)
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }

    /// Store one print's manifest, replacing whatever it had.
    fn write_manifest(&self, print_id: PrintId, manifest: &JobManifest) -> Result<(), StoreError> {
        let manifest = json_text(manifest)?;
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO manifests (print_id, manifest) VALUES (?1, ?2) \
                     ON CONFLICT(print_id) DO UPDATE SET manifest = excluded.manifest",
                    params![print_id.to_string(), manifest],
                )
                .map_err(|error| {
                    Self::refusal(
                        connection,
                        &error,
                        &[("manifests.print_id", "prints", print_id.to_string())],
                    )
                })?;
            Ok(())
        })
    }

    /// Read one print's manifest.
    fn read_manifest(&self, print_id: PrintId) -> Result<Option<JobManifest>, StoreError> {
        self.on_connection(|connection| {
            connection
                .query_row(
                    "SELECT manifest FROM manifests WHERE print_id = ?1",
                    params![print_id.to_string()],
                    |row| crate::values::from_json(row, 0),
                )
                .optional()
                .map_err(|error| database_error(&error))
        })
    }

    /// Store one print's supervision session, replacing whatever it had.
    fn write_session(&self, session: &SupervisionSession) -> Result<(), StoreError> {
        self.on_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO sessions (print_id, session_name, harness_identity, \
                     created_at, last_turn_at, closed_at, close_reason) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                     ON CONFLICT(print_id) DO UPDATE SET \
                     session_name = excluded.session_name, \
                     harness_identity = excluded.harness_identity, \
                     created_at = excluded.created_at, \
                     last_turn_at = excluded.last_turn_at, \
                     closed_at = excluded.closed_at, \
                     close_reason = excluded.close_reason",
                    params![
                        session.print_id.to_string(),
                        session.session_name.clone(),
                        session.harness_identity.clone(),
                        instant_text(session.created_at),
                        instant_text(session.last_turn_at),
                        session.closed_at.map(instant_text),
                        session.close_reason.clone(),
                    ],
                )
                .map_err(|error| {
                    Self::refusal(
                        connection,
                        &error,
                        &[("sessions.print_id", "prints", session.print_id.to_string())],
                    )
                })?;
            Ok(())
        })
    }

    /// Read one print's supervision session.
    fn read_session(&self, print_id: PrintId) -> Result<Option<SupervisionSession>, StoreError> {
        let query = format!("{SESSION_SELECT} WHERE print_id = ?1");
        self.on_connection(|connection| {
            connection
                .query_row(&query, params![print_id.to_string()], session_from_row)
                .optional()
                .map_err(|error| database_error(&error))
        })
    }

    /// Read a print's events, newest first, bounded by the resolved limit.
    fn read_history(&self, query: &HistoryQuery) -> Result<Vec<EventRecord>, StoreError> {
        let limit = query.resolved_limit()?;
        let mut values = vec![Value::Text(query.print_id.to_string())];
        let mut sql = format!("{EVENT_SELECT} WHERE e.print_id = ?1");
        if !query.kinds.is_empty() {
            let mut placeholders = Vec::with_capacity(query.kinds.len());
            for kind in &query.kinds {
                values.push(Value::Text(tag_text(kind)?));
                placeholders.push(format!("?{}", values.len()));
            }
            let _ = write!(sql, " AND e.kind IN ({})", placeholders.join(", "));
        }
        if let Some(since) = query.since {
            values.push(Value::Text(instant_text(since)));
            let _ = write!(sql, " AND e.received_at >= ?{}", values.len());
        }
        if let Some(until) = query.until {
            values.push(Value::Text(instant_text(until)));
            let _ = write!(sql, " AND e.received_at <= ?{}", values.len());
        }
        values.push(Value::Integer(i64::from(limit)));
        let _ = write!(
            sql,
            " ORDER BY e.received_at DESC, e.id DESC LIMIT ?{}",
            values.len()
        );
        self.read_events(&sql, params_from_iter(values.iter()))
    }

    /// Read one page of a print's whole history, oldest first.
    fn read_audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> Result<AuditPage, StoreError> {
        let asked_for = (page_size > 0).then_some(page_size);
        let size = resolve_history_limit(asked_for)?;
        let mut values = vec![Value::Text(print_id.to_string())];
        let mut sql = format!("{EVENT_SELECT} WHERE e.print_id = ?1");
        if let Some(cursor) = after {
            let instant = self.instant_of(cursor)?;
            values.push(Value::Text(instant));
            values.push(Value::Text(cursor.to_string()));
            sql.push_str(" AND (e.received_at, e.id) > (?2, ?3)");
        }
        values.push(Value::Integer(i64::from(size) + 1));
        let _ = write!(
            sql,
            " ORDER BY e.received_at ASC, e.id ASC LIMIT ?{}",
            values.len()
        );

        let mut events = self.read_events(&sql, params_from_iter(values.iter()))?;
        let bounded = size as usize;
        let next = (events.len() > bounded).then(|| events[bounded - 1].id);
        events.truncate(bounded);
        Ok(AuditPage { events, next })
    }

    /// When one event was received, as the cursor a page follows spells it.
    fn instant_of(&self, event_id: EventId) -> Result<String, StoreError> {
        self.on_connection(|connection| {
            connection
                .query_row(
                    "SELECT received_at FROM events WHERE id = ?1",
                    params![event_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| database_error(&error))
        })?
        .ok_or_else(|| not_found(&format!("event {event_id}")))
    }

    /// Run one prepared read of events.
    fn read_events(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<EventRecord>, StoreError> {
        self.on_connection(|connection| {
            let mut statement = connection
                .prepare(sql)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map(parameters, event_from_row)
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }
}

impl StorePort for SqliteStore {
    fn open_print(
        &self,
        obico_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move { self.insert_print(obico_print_id, file_name) })
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move {
            let identifier = print_id.to_string();
            self.read_print("id = ?1", params![identifier])
        })
    }

    fn print_by_obico_id(
        &self,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move { self.read_print("obico_print_id = ?1", params![obico_print_id]) })
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move { self.write_end(print_id, &state, ended_at, &reason) })
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move { self.write_narrowing(print_id, narrowing) })
    }

    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        Box::pin(async move { self.insert_event(draft) })
    }

    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        Box::pin(
            async move { self.insert_image(print_id, event_id, source_url, content_type, &bytes) },
        )
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        Box::pin(async move { self.read_image(image_id) })
    }

    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        Box::pin(async move { self.insert_action(request, decision) })
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        Box::pin(async move { self.insert_execution(action_id, &outcome) })
    }

    fn open_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> BoxFuture<'_, Result<Intervention, StoreError>> {
        Box::pin(async move {
            self.insert_intervention(
                action_id,
                adjustable,
                prior_value,
                applied_value,
                applied_at,
                expires_at,
            )
        })
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        Box::pin(async move { self.settle(intervention_id, &outcome) })
    }

    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        Box::pin(async move {
            let still_active = json_text(&InterventionOutcome::StillActive)?;
            self.read_interventions(
                "outcome = ?1 AND expires_at <= ?2",
                "expires_at ASC, id ASC",
                params![still_active, instant_text(at)],
            )
        })
    }

    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        Box::pin(async move {
            let still_active = json_text(&InterventionOutcome::StillActive)?;
            self.read_interventions(
                "print_id = ?1 AND outcome = ?2",
                "applied_at ASC, id ASC",
                params![print_id.to_string(), still_active],
            )
        })
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move { self.write_manifest(print_id, &manifest) })
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        Box::pin(async move { self.read_manifest(print_id) })
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move { self.read_history(&query) })
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        Box::pin(async move { self.read_audit_page(print_id, after, page_size) })
    }

    fn put_session(&self, session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move { self.write_session(&session) })
    }

    fn session(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        Box::pin(async move { self.read_session(print_id) })
    }
}
