//! The printer port is implementable, dyn-compatible and shareable.
//!
//! What establishes those three is not an assertion about the trait but this:
//! a test-only implementation, held behind the same shared trait object the
//! supervision core will hold it behind, with every method the trait declares
//! called and awaited and each asserted to answer that method's declared
//! success type rather than an error.
//!
//! It overrides none of the file reads and not the position read, which every implementation
//! inherits answering a refusal until it implements them, and those are held
//! to that refusal here.
//!
//! It behaves trivially rather than erroring, because a stub answering a
//! not-yet-implemented error would need a variant in the error vocabulary that
//! no real implementation can ever produce, and every consumer would still have
//! to match it.

#[path = "support/block_on.rs"]
mod block_on;

use std::sync::Arc;
use std::thread;

use block_on::block_on;
use printobserver_printer_api::{
    BoxFuture, JobSnapshot, PrinterError, PrinterPort, PrinterSnapshot,
};
use printobserver_types::FileName;
use printobserver_types::contract::Sample;

/// A printer that answers every method with the success type it declares.
struct TrivialPrinter;

impl PrinterPort for TrivialPrinter {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        Box::pin(async { Ok(PrinterSnapshot::sample_minimal()) })
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        Box::pin(async { Ok(JobSnapshot::sample_minimal()) })
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = file_name;
        Box::pin(async { Ok(()) })
    }

    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async { Ok(()) })
    }

    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async { Ok(()) })
    }

    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        Box::pin(async { Ok(()) })
    }

    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = factor;
        Box::pin(async { Ok(()) })
    }

    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = factor;
        Box::pin(async { Ok(()) })
    }

    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = (tool, target_c);
        Box::pin(async { Ok(()) })
    }

    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = target_c;
        Box::pin(async { Ok(()) })
    }

    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let _ = percent;
        Box::pin(async { Ok(()) })
    }
}

/// Every method answers its declared success type, behind a shared trait object.
#[test]
fn every_method_answers_its_declared_success_type() {
    let port: Arc<dyn PrinterPort> = Arc::new(TrivialPrinter);

    assert_eq!(
        block_on(port.snapshot()),
        Ok(PrinterSnapshot::sample_minimal())
    );
    assert_eq!(block_on(port.job()), Ok(JobSnapshot::sample_minimal()));
    assert_eq!(block_on(port.start(FileName::sample_full())), Ok(()));
    assert_eq!(block_on(port.pause()), Ok(()));
    assert_eq!(block_on(port.resume()), Ok(()));
    assert_eq!(block_on(port.cancel()), Ok(()));
    assert_eq!(block_on(port.set_feedrate_factor(1.0)), Ok(()));
    assert_eq!(block_on(port.set_flowrate_factor(1.0)), Ok(()));
    assert_eq!(block_on(port.set_tool_target_c(0, 215.0)), Ok(()));
    assert_eq!(block_on(port.set_bed_target_c(60.0)), Ok(()));
    assert_eq!(block_on(port.set_fan_percent(100.0)), Ok(()));
}

/// A printer overriding none of the file reads, nor the position read, answers each with the
/// port's own refusal: status 501, naming the read it cannot answer.
#[test]
fn the_file_reads_a_printer_does_not_implement_are_refused_with_501() {
    let port: Arc<dyn PrinterPort> = Arc::new(TrivialPrinter);
    let refused = |method: &str| PrinterError::Refused {
        status: 501,
        detail: format!("{method} is not supported by this printer"),
    };
    assert_eq!(
        block_on(port.file_facts(FileName::sample_full())),
        Err(refused("file_facts"))
    );
    assert_eq!(
        block_on(port.printed_files()),
        Err(refused("printed_files"))
    );
    assert_eq!(
        block_on(port.extruded_to(FileName::sample_full(), 4_096)),
        Err(refused("extruded_to"))
    );
    assert_eq!(
        block_on(port.printed_extent(FileName::sample_full(), 4_096)),
        Err(refused("printed_extent"))
    );
    assert_eq!(
        block_on(port.read_position()),
        Err(refused("read_position"))
    );
}

/// A printer overriding neither the push channel nor the command answers each
/// with the port's own refusal: status 501, naming the method.
#[test]
fn the_push_channel_and_the_command_a_printer_does_not_implement_are_refused_with_501() {
    let port: Arc<dyn PrinterPort> = Arc::new(TrivialPrinter);
    let refused = |method: &str| PrinterError::Refused {
        status: 501,
        detail: format!("{method} is not supported by this printer"),
    };
    assert_eq!(
        block_on(port.push_events()).err(),
        Some(refused("push_events"))
    );
    assert_eq!(
        block_on(port.send_command(printobserver_printer_api::PrinterCommand::ReportPosition)),
        Err(refused("send_command"))
    );
    for command in [
        printobserver_printer_api::PrinterCommand::MoveZ { mm: 5.0 },
        printobserver_printer_api::PrinterCommand::MoveXy {
            x_mm: 10.0,
            y_mm: 200.0,
        },
        printobserver_printer_api::PrinterCommand::Home {
            axes: printobserver_printer_api::HomeAxes::Xy,
        },
        printobserver_printer_api::PrinterCommand::Home {
            axes: printobserver_printer_api::HomeAxes::Z,
        },
    ] {
        assert_eq!(
            block_on(port.send_command(command)),
            Err(refused("send_command")),
            "{command:?} reached a printer that takes no command"
        );
    }
}

/// The same trait object is shareable across threads, which is what core needs.
#[test]
fn the_trait_object_is_shareable_across_threads() {
    let port: Arc<dyn PrinterPort> = Arc::new(TrivialPrinter);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = Arc::clone(&port);
            thread::spawn(move || block_on(shared.snapshot()))
        })
        .collect();
    for handle in handles {
        assert_eq!(
            handle.join().expect("the thread completes"),
            Ok(PrinterSnapshot::sample_minimal())
        );
    }
}

/// Every variant of this port's error vocabulary says what it is.
#[test]
fn every_error_variant_says_what_it_is() {
    let variants = [
        PrinterError::Unreachable {
            detail: "no route".to_owned(),
        },
        PrinterError::Unauthorized {
            detail: "bad key".to_owned(),
        },
        PrinterError::Refused {
            status: 409,
            detail: "printing".to_owned(),
        },
        PrinterError::StateConflict {
            detail: "already paused".to_owned(),
        },
        PrinterError::Unsupported {
            adjustable: printobserver_printer_api::Adjustable::ToolTarget { tool: 3 },
        },
        PrinterError::Malformed {
            detail: "not JSON".to_owned(),
        },
    ];
    for variant in variants {
        let message = variant.to_string();
        assert!(!message.is_empty(), "{variant:?} says nothing");
    }
}
