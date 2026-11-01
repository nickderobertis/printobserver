//! SPIKE ONLY (check-costs, never landed): the `context` answer the agent
//! reads today, from the real server, for a print part-way through a file.
//!
//! `SPIKE_FILE`, `SPIKE_SIZE`, `SPIKE_POSITION` and `SPIKE_PRINT_TIME_S` name
//! the file the printer stand-in runs and where it is in it.
//!
//! `cargo test -p printobserver-server --test api spike_context -- --ignored --nocapture`

use std::sync::Arc;

use printobserver_printer_api::PrinterState;

use crate::agent::StandInAgent;
use crate::printer::RecordingPrinter;
use crate::world::{World, manifest_write};

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spike measurement, run by spikes/check-costs"]
async fn spike_context() {
    let file = var("SPIKE_FILE");
    let size: i64 = var("SPIKE_SIZE").parse().expect("a size");
    let position: i64 = var("SPIKE_POSITION").parse().expect("a position");
    let print_time: i64 = var("SPIKE_PRINT_TIME_S").parse().expect("a time");
    let printer = RecordingPrinter::printing();
    printer.holds_file(&file, size, None);
    printer.runs_job(&file, PrinterState::Printing, Some(print_time));
    printer.at_byte(position);
    let world = World::open_with(Arc::clone(&printer), StandInAgent::new()).await;
    let print_id = world.open_print().await;
    let manifest = printobserver_types::serde_json::to_value(
        <printobserver_core::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders");
    let path = |name: &str| {
        printobserver_server::operation(name)
            .expect("the operation is declared")
            .full_path()
    };
    let (status, written) = world
        .put(&world.operation_url(&path("manifest_set"), print_id), &manifest_write(&manifest))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{written}");
    let (status, context) = world
        .get(&world.operation_url(&path("context"), print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{context}");
    world.server.stop().await;
    println!("SPIKE_CONTEXT {context}");
}
