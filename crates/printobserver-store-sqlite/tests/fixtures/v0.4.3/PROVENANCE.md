# Fixture: a database written by printobserver v0.4.3

`printobserver.sqlite3` was written by printobserver release **v0.4.3** (tag
`v0.4.3`, commit `97291a9d8147d380c711551d63111005b8cd9603`), through that
release's own `printobserver-store-sqlite` and `printobserver-core` crates'
public API (`SqliteStore::open`, `PrintStore`, `EventStore`, `FilamentStore`)
and its typed payloads from `printobserver_core::kinds`. Every row is one
v0.4.3 wrote; nothing was inserted or edited by hand. It is at schema
`user_version` 5.

## The generator

`Cargo.toml`, in a directory of its own beside a checkout of the tag. The
dependency paths are the one edit to the file as run: they are written here
relative to `<checkout of v0.4.3>`. The tag's own `Cargo.lock` was copied into
the directory before the build, so the dependencies resolved as v0.4.3 locked
them.

```toml
[package]
name = "v043-fixture"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
printobserver-core = { path = "<checkout of v0.4.3>/crates/printobserver-core" }
printobserver-printer-api = { path = "<checkout of v0.4.3>/crates/printobserver-printer-api" }
printobserver-store-sqlite = { path = "<checkout of v0.4.3>/crates/printobserver-store-sqlite" }
printobserver-types = { path = "<checkout of v0.4.3>/crates/printobserver-types" }
rusqlite = { version = "0.37", features = ["bundled"] }
```

`src/main.rs`:

```rust
//! Writes a printobserver v0.4.3 state directory through v0.4.3's own store API.
//!
//! Usage: v043-fixture <empty state directory>

use std::str::FromStr;

use printobserver_core::filament::grams_for_length;
use printobserver_core::kinds::{
    ChargePendedPayload, FilamentChangeSignalledPayload, FilamentConsumedPayload,
    PreflightCheckedPayload, PrintEndedPayload, PrintStartedPayload, SpoolCommandRejectedPayload,
    SpoolRetiredPayload, operator_source, system_source,
};
use printobserver_core::store::{
    Charge, EventDraft, EventStore, FilamentStore, NewSpool, Pend, PrintStore, Settlement,
    SpoolChange, SpoolMutation,
};
use printobserver_core::{
    Actor, ChargeKey, ChargeMethod, FilePosition, LoadedSpool, PendingCharge, PolicyDecision,
    Preflight, PreflightTool, PreflightVerdict, PrintOutcome, RejectionReason, SpoolCommand,
    SpoolId, SpoolRecord, SpoolTrust, block_on,
};
use printobserver_printer_api::PrinterState;
use printobserver_store_sqlite::{DATABASE_FILE_NAME, SqliteStore};
use printobserver_types::{EventBody, EventKind, EventPayload, EventSource, PrintId, Timestamp};

/// The spool id the rejected load names, which nothing registers.
const UNKNOWN_SPOOL: &str = "01950000-0000-7000-8000-000000000001";

fn at(minute: u32) -> Timestamp {
    Timestamp::from_str(&format!("2026-03-01T12:{minute:02}:00Z")).expect("a fixed instant")
}

fn draft<P: EventPayload>(
    print_id: Option<PrintId>,
    source: EventSource,
    received_at: Timestamp,
    payload: &P,
) -> EventDraft {
    EventDraft {
        print_id,
        source,
        received_at,
        body: EventBody::of(payload).expect("a declared payload serializes"),
        raw: None,
    }
}

fn register(
    store: &SqliteStore,
    name: &str,
    material: &str,
    density_g_cm3: f64,
    remaining_g: f64,
    loaded_on_tool: Option<i64>,
    minute: u32,
) -> SpoolRecord {
    block_on(store.register_spool(
        NewSpool {
            name: name.to_owned(),
            material: material.to_owned(),
            color: Some("black".to_owned()),
            diameter_mm: 1.75,
            density_g_cm3,
            initial_g: 1_000.0,
            remaining_g,
            tare_g: Some(250.0),
            loaded_on_tool,
            trust: SpoolTrust::Verified,
            registered_at: at(minute),
        },
        operator_source(),
        Actor::Operator,
        "a new spool on the shelf".to_owned(),
    ))
    .expect("the spool registers")
    .0
}

fn append(store: &SqliteStore, event: EventDraft) {
    block_on(store.append_event(event)).expect("the event appends");
}

fn main() {
    let state_dir = std::env::args().nth(1).expect("usage: v043-fixture <state dir>");
    let store = SqliteStore::open(&state_dir).expect("the store opens");

    // Spools.
    let retired = register(&store, "PLA retired-on-tool", "PLA", 1.24, 640.0, Some(0), 0);
    let loaded = register(&store, "PETG loaded", "PETG", 1.27, 80.0, Some(1), 1);
    let shelf = register(&store, "PLA shelf", "PLA", 1.24, 1_000.0, None, 2);
    block_on(store.apply_spool_changes(
        vec![SpoolMutation {
            spool_id: retired.id,
            change: SpoolChange::Retire,
            at: at(3),
            event: draft(
                None,
                operator_source(),
                at(3),
                &SpoolRetiredPayload {
                    spool_id: retired.id,
                    remaining_g: retired.remaining_g,
                    actor: Actor::Operator,
                    reason: "the spool is empty".to_owned(),
                },
            ),
        }],
        Vec::new(),
        Vec::new(),
    ))
    .expect("the retirement applies");

    // A preflight check, printer-wide: one insufficient tool, one unchecked.
    append(
        &store,
        draft(
            None,
            system_source(),
            at(4),
            &PreflightCheckedPayload {
                preflight: Preflight {
                    file_name: "bracket.gcode".to_owned(),
                    tools: vec![
                        PreflightTool {
                            tool: 1,
                            spool_id: Some(loaded.id),
                            spool_name: Some(loaded.name.clone()),
                            required_g: Some(120.0),
                            buffer_g: Some(12.0),
                            remaining_g: Some(80.0),
                            marks: Vec::new(),
                            verdict: PreflightVerdict::Insufficient,
                        },
                        PreflightTool {
                            tool: 2,
                            spool_id: None,
                            spool_name: None,
                            required_g: None,
                            buffer_g: None,
                            remaining_g: None,
                            marks: Vec::new(),
                            verdict: PreflightVerdict::Unchecked,
                        },
                    ],
                    verdict: PreflightVerdict::Insufficient,
                    checked_at: at(4),
                },
                action_id: None,
            },
        ),
    );

    // A load naming a spool nothing registered, refused, printer-wide.
    let unknown = SpoolId::from_str(UNKNOWN_SPOOL).expect("a version 7 identifier");
    append(
        &store,
        draft(
            None,
            operator_source(),
            at(5),
            &SpoolCommandRejectedPayload {
                command: SpoolCommand::LoadSpool {
                    spool_id: unknown,
                    tool: 1,
                    reason: "swapping to the new roll".to_owned(),
                    actor: Actor::Operator,
                },
                decision: PolicyDecision::Rejected(RejectionReason::SpoolRefused {
                    detail: format!(
                        "there is no spool {unknown}; list the spools to find the one meant"
                    ),
                }),
            },
        ),
    );

    // The print.
    let print = block_on(store.open_print(None, Some("bracket.gcode".to_owned())))
        .expect("the print opens");
    let print_id = print.id;
    append(
        &store,
        draft(
            Some(print_id),
            system_source(),
            at(10),
            &PrintStartedPayload {
                file_name: Some("bracket.gcode".to_owned()),
                started_by: None,
                loaded: vec![LoadedSpool {
                    tool: 1,
                    spool_id: loaded.id,
                }],
                preflight: Some(Preflight {
                    file_name: "bracket.gcode".to_owned(),
                    tools: vec![PreflightTool {
                        tool: 1,
                        spool_id: Some(loaded.id),
                        spool_name: Some(loaded.name.clone()),
                        required_g: Some(120.0),
                        buffer_g: Some(12.0),
                        remaining_g: Some(80.0),
                        marks: Vec::new(),
                        verdict: PreflightVerdict::Overridden,
                    }],
                    verdict: PreflightVerdict::Overridden,
                    checked_at: at(10),
                }),
            },
        ),
    );

    let obico = EventKind::new("obico_printer_notification").expect("a kind name");
    let position = FilePosition {
        bytes: 40_960,
        size_bytes: Some(163_840),
        observed_at: at(20),
    };
    append(
        &store,
        draft(
            Some(print_id),
            system_source(),
            at(20),
            &FilamentChangeSignalledPayload {
                tool: Some(1),
                spool_id: Some(loaded.id),
                source_kind: obico.clone(),
                position: Some(position.clone()),
                position_read_at_signal: true,
            },
        ),
    );
    append(
        &store,
        draft(
            Some(print_id),
            system_source(),
            at(25),
            &FilamentChangeSignalledPayload {
                tool: None,
                spool_id: None,
                source_kind: obico,
                position: None,
                position_read_at_signal: false,
            },
        ),
    );

    // An unobserved print, recorded uncharged.
    let unobserved = ChargeKey::Unobserved {
        origin: "local".to_owned(),
        file_name: "benchy.gcode".to_owned(),
        generation: 0,
        print_number: 3,
        tool: 0,
    };
    block_on(store.apply_spool_changes(
        Vec::new(),
        vec![Charge {
            key: unobserved.clone(),
            tool: 0,
            print_id: None,
            spool_id: None,
            length_mm: 5_120.0,
            grams: None,
            at: at(30),
            event: draft(
                None,
                system_source(),
                at(30),
                &FilamentConsumedPayload {
                    charge_key: unobserved,
                    tool: 0,
                    spool_id: None,
                    method: ChargeMethod::UnobservedEstimate,
                    length_mm: 5_120.0,
                    grams: None,
                    diameter_mm: None,
                    density_g_cm3: None,
                    analysed_length_mm: Some(5_120.0),
                    position: None,
                    remaining_before_g: None,
                    remaining_after_g: None,
                    note: None,
                },
            ),
        }],
        Vec::new(),
    ))
    .expect("the unobserved charge applies");

    // Settle the print: segment 0 charged against the loaded spool, segment 1
    // held pending, and its print_ended event.
    let length_mm = 4_000.0;
    let grams = grams_for_length(length_mm, loaded.diameter_mm, loaded.density_g_cm3);
    let segment0 = ChargeKey::Print {
        print_id,
        tool: 1,
        segment: 0,
    };
    let segment1 = ChargeKey::Print {
        print_id,
        tool: 1,
        segment: 1,
    };
    let pending = PendingCharge {
        charge_key: segment1,
        print_id,
        tool: 1,
        length_mm: 250.0,
        method: ChargeMethod::GcodeExtrusion,
        analysed_length_mm: Some(4_250.0),
        position: Some(FilePosition {
            bytes: 163_840,
            size_bytes: Some(163_840),
            observed_at: at(40),
        }),
        note: Some("fed after a filament change nobody confirmed".to_owned()),
        pended_at: at(40),
    };
    let settlement = block_on(store.settle_print(
        print_id,
        PrintOutcome::Completed,
        draft(
            Some(print_id),
            system_source(),
            at(40),
            &PrintEndedPayload {
                outcome: PrintOutcome::Completed,
                state: PrinterState::Operational,
                reason: "finished".to_owned(),
                last_position: Some(FilePosition {
                    bytes: 163_840,
                    size_bytes: Some(163_840),
                    observed_at: at(40),
                }),
            },
        ),
        vec![Charge {
            key: segment0.clone(),
            tool: 1,
            print_id: Some(print_id),
            spool_id: Some(loaded.id),
            length_mm,
            grams: Some(grams),
            at: at(40),
            event: draft(
                Some(print_id),
                system_source(),
                at(40),
                &FilamentConsumedPayload {
                    charge_key: segment0,
                    tool: 1,
                    spool_id: Some(loaded.id),
                    method: ChargeMethod::GcodeExtrusion,
                    length_mm,
                    grams: Some(grams),
                    diameter_mm: Some(loaded.diameter_mm),
                    density_g_cm3: Some(loaded.density_g_cm3),
                    analysed_length_mm: Some(4_250.0),
                    position: Some(position),
                    remaining_before_g: Some(loaded.remaining_g),
                    remaining_after_g: Some(loaded.remaining_g - grams),
                    note: Some("charged up to the filament change".to_owned()),
                },
            ),
        }],
        vec![Pend {
            pending: pending.clone(),
            event: draft(
                Some(print_id),
                system_source(),
                at(40),
                &ChargePendedPayload { pending },
            ),
        }],
        Vec::new(),
    ))
    .expect("the print settles");
    assert!(matches!(settlement, Settlement::Settled { .. }));
    block_on(store.end_print(
        print_id,
        PrinterState::Operational,
        at(40),
        "finished".to_owned(),
    ))
    .expect("the print ends");

    println!("print {print_id}");
    println!("spool {} {}", retired.id, retired.name);
    println!("spool {} {}", loaded.id, loaded.name);
    println!("spool {} {}", shelf.id, shelf.name);
    println!("unknown spool {unknown}");
    drop(store);

    // One self-contained file: fold the write-ahead log in and leave WAL mode.
    let connection = rusqlite::Connection::open(std::path::Path::new(&state_dir).join(DATABASE_FILE_NAME))
        .expect("the database reopens");
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .expect("the checkpoint runs");
    let mode: String = connection
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .expect("the journal mode changes");
    assert_eq!(mode, "delete");
}
```

## The command

```console
cp <checkout of v0.4.3>/Cargo.lock .
mkdir state
cargo run -- state
```

Run with the tag's own toolchain (`rust-toolchain.toml`: 1.97.1).

## The checkpoint and journal step

The last lines of `main.rs`, after the store is dropped: on a fresh
connection to `state/printobserver.sqlite3`, `PRAGMA wal_checkpoint(TRUNCATE)`
and then `PRAGMA journal_mode = DELETE`. The file is self-contained; no `-wal`
or `-shm` file goes with it. Only `state/printobserver.sqlite3` was copied
here.

## What it holds

Fixed instants are `2026-03-01T12:MM:00Z`. The store mints every identifier
(UUID version 7) and the print's `opened_at` at the moment it writes them, so
those are what this run minted rather than chosen values; `prints.opened_at`
is `2026-10-26T05:52:20.618169179Z`, the wall clock of the run.

| Identifier | Value |
|---|---|
| print (`bracket.gcode`, settled `completed`, ended) | `01a176c4-548a-7bb1-80e5-a5a058ca2ad2` |
| spool "PLA retired-on-tool" (tool 0, retired, `loaded_on_tool` still 0) | `01a176c4-5487-73b0-a80f-753b6f3fdc64` |
| spool "PETG loaded" (tool 1) | `01a176c4-5488-7532-8297-73e157be17bc` |
| spool "PLA shelf" (no tool) | `01a176c4-5488-7532-8297-740be582f6d9` |
| unknown spool the rejected load names | `01950000-0000-7000-8000-000000000001` |

| Table | Rows |
|---|---|
| `spools` | 3 |
| `prints` | 1 |
| `events` | 13 |
| `charges` | 2 |
| `pending_charges` | 1 |
| `print_settlements` | 1 |
| `actions`, `executions`, `file_tallies`, `images`, `interventions`, `manifests`, `print_positions`, `sessions` | 0 |

| Event kind | Count | Print-scoped |
|---|---|---|
| `spool_registered` | 3 | no |
| `spool_retired` | 1 | no |
| `preflight_checked` | 1 | no |
| `spool_command_rejected` | 1 | no |
| `print_started` | 1 | yes |
| `filament_change_signalled` | 2 | yes |
| `filament_consumed` | 2 | one (`print:01a176c4-548a-7bb1-80e5-a5a058ca2ad2:1:0`, charged); one not (`unobserved:local:0:3:0:benchy.gcode`, uncharged) |
| `charge_pended` | 1 | yes (`print:01a176c4-548a-7bb1-80e5-a5a058ca2ad2:1:1`) |
| `print_ended` | 1 | yes |
