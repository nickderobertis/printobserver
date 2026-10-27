//! The drivers of the printer watch.
//!
//! Core decides what a read of the machine means and writes it down
//! ([`Supervisor::watch_printer`], [`Supervisor::note_push`],
//! [`Supervisor::check_detector`]); this is what asks it to, and what holds
//! the two things core may not: the printer's push channel, and the one
//! command the watch sends.
//!
//! * **The reads**, once per
//!   [`CoreConfig::printer_poll`](printobserver_core::CoreConfig), whether or
//!   not a print is active, each turn they claimed run as an observation's is.
//! * **The push reader**, which opens the printer's push channel, hands each
//!   reset and error to core as it arrives and keeps each terminal line for the
//!   position probe — and, when the channel drops or cannot be opened, tells
//!   core it is down and opens it again after [`REOPEN_AFTER`].
//! * **The position probe**, which core asks only once a read has found no job
//!   running: it sends [`PrinterCommand::ReportPosition`] and reads the reply
//!   off the push channel's terminal lines, waiting up to [`POSITION_WAIT`].
//!   With the channel down it sends nothing and answers no position.
//! * **The detector check**, once per observation interval, when
//!   `obico.ml_api_url` is configured.
//!
//! `send_command` is called here and nowhere else in this program: no
//! operation, agent or client can reach it.
//!
//! Every core call is made on a blocking thread, as the observation driver
//! makes its own, because the ports' futures block the thread that polls them.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use printobserver_core::{
    DetectorProbe, HeadPosition, PendingTurn, PositionProbe, Supervisor, block_on,
};
use printobserver_printer_api::{BoxFuture, PrinterCommand, PrinterPort, PrinterPush};
use tokio::task::JoinHandle;

/// How long after the push channel drops, or refuses to open, it is opened
/// again.
pub const REOPEN_AFTER: Duration = Duration::from_secs(5);

/// How long the position probe waits for the reply to its command.
pub const POSITION_WAIT: Duration = Duration::from_secs(3);

/// How many terminal lines the position probe keeps, newest last.
const KEPT_LINES: usize = 64;

/// The printer watch's tasks, aborted together and when dropped.
#[derive(Debug)]
pub struct Watching {
    /// Each task.
    tasks: Vec<JoinHandle<()>>,
}

impl Watching {
    /// Stop every task.
    pub fn abort(&self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.abort();
    }
}

/// What the push reader shares with the position probe.
#[derive(Debug, Default)]
struct Terminal {
    /// Whether the push channel is open now.
    open: AtomicBool,
    /// The terminal lines read since the probe last asked, newest last.
    lines: Mutex<VecDeque<String>>,
}

impl Terminal {
    /// Keep one line.
    fn keep(&self, line: String) {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        lines.push_back(line);
        while lines.len() > KEPT_LINES {
            lines.pop_front();
        }
    }

    /// Take every line kept.
    fn take(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }
}

/// Reads the head's position by asking the printer and reading the reply off
/// its push channel.
struct Probe {
    /// The printer.
    printer: Arc<dyn PrinterPort>,
    /// What the push reader keeps.
    terminal: Arc<Terminal>,
}

impl Probe {
    /// Ask once, waiting for the reply.
    async fn position(&self) -> Option<HeadPosition> {
        if !self.terminal.open.load(Ordering::SeqCst) {
            return None;
        }
        // What arrived before the command is not its reply.
        let _ = self.terminal.take();
        self.printer
            .send_command(PrinterCommand::ReportPosition)
            .await
            .ok()?;
        let deadline = Instant::now() + POSITION_WAIT;
        while Instant::now() < deadline {
            let found = self
                .terminal
                .take()
                .iter()
                .find_map(|line| printobserver_octoprint::push::position_of(line));
            if let Some([x, y, z]) = found {
                return Some(HeadPosition { x, y, z });
            }
            // A blocking thread of the runtime's own, which is where the watch
            // runs: waiting here holds no asynchronous worker.
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl PositionProbe for Probe {
    fn read_position(&self) -> BoxFuture<'_, Option<HeadPosition>> {
        Box::pin(self.position())
    }
}

/// Start the printer watch: its reads, its push reader and, when a detector
/// check is given, that check.
pub fn start(
    supervisor: &Arc<Supervisor>,
    printer: Arc<dyn PrinterPort>,
    detector: Option<Arc<dyn DetectorProbe>>,
) -> Watching {
    let terminal = Arc::new(Terminal::default());
    let probe = Arc::new(Probe {
        printer: Arc::clone(&printer),
        terminal: Arc::clone(&terminal),
    });
    let mut tasks = vec![
        reading(Arc::downgrade(supervisor), probe),
        pushing(Arc::downgrade(supervisor), printer, terminal),
    ];
    if let Some(detector) = detector {
        tasks.push(checking(Arc::downgrade(supervisor), detector));
    }
    Watching { tasks }
}

/// Run each claimed turn on a blocking thread of its own.
fn run_turns(supervisor: &Arc<Supervisor>, turns: Vec<PendingTurn>) {
    for turn in turns {
        let runner = Arc::clone(supervisor);
        tokio::spawn(async move {
            let supervised = tokio::task::spawn_blocking(move || {
                let _ = block_on(runner.run_supervision(turn));
            })
            .await;
            debug_assert!(supervised.is_ok(), "one watch turn panicked");
        });
    }
}

/// Read the machine once per poll, and run what the reads claimed.
fn reading(weak: Weak<Supervisor>, probe: Arc<Probe>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Some(poll) = weak.upgrade().map(|held| held.config().printer_poll) else {
                return;
            };
            tokio::time::sleep(poll).await;
            let Some(supervisor) = weak.upgrade() else {
                return;
            };
            let watching = Arc::clone(&supervisor);
            let probing = Arc::clone(&probe);
            let watched = tokio::task::spawn_blocking(move || {
                block_on(watching.watch_printer(probing.as_ref()))
            })
            .await;
            // A round the store refused recorded nothing; the next one reads
            // the machine again.
            if let Ok(Ok(watched)) = watched {
                run_turns(&supervisor, watched.turns);
            }
        }
    })
}

/// Keep the printer's push channel open, handing what it pushes to core.
fn pushing(
    weak: Weak<Supervisor>,
    printer: Arc<dyn PrinterPort>,
    terminal: Arc<Terminal>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let opening = Arc::clone(&printer);
            let opened = tokio::task::spawn_blocking(move || block_on(opening.push_events())).await;
            let Some(supervisor) = weak.upgrade() else {
                return;
            };
            match opened {
                Ok(Ok(mut stream)) => {
                    terminal.open.store(true, Ordering::SeqCst);
                    supervisor.note_push_channel(Ok(()));
                    drop(supervisor);
                    while let Some(push) =
                        poll_fn(|context| stream.as_mut().poll_next(context)).await
                    {
                        match push {
                            PrinterPush::Log { line } => terminal.keep(line),
                            other => {
                                let Some(supervisor) = weak.upgrade() else {
                                    return;
                                };
                                let noted = tokio::task::spawn_blocking(move || {
                                    block_on(supervisor.note_push(other))
                                })
                                .await;
                                debug_assert!(noted.is_ok(), "noting one push panicked");
                            }
                        }
                    }
                    terminal.open.store(false, Ordering::SeqCst);
                    let Some(supervisor) = weak.upgrade() else {
                        return;
                    };
                    supervisor.note_push_channel(Err("the push channel dropped".to_owned()));
                }
                Ok(Err(error)) => {
                    terminal.open.store(false, Ordering::SeqCst);
                    supervisor.note_push_channel(Err(error.to_string()));
                }
                Err(_) => {
                    terminal.open.store(false, Ordering::SeqCst);
                    supervisor
                        .note_push_channel(Err("opening the push channel panicked".to_owned()));
                }
            }
            tokio::time::sleep(REOPEN_AFTER).await;
        }
    })
}

/// Check the detector once per observation interval, and run the turn going
/// unavailable claimed.
fn checking(weak: Weak<Supervisor>, detector: Arc<dyn DetectorProbe>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Some(interval) = weak
                .upgrade()
                .map(|held| held.config().observation_interval)
            else {
                return;
            };
            tokio::time::sleep(interval).await;
            let Some(supervisor) = weak.upgrade() else {
                return;
            };
            let checking = Arc::clone(&supervisor);
            let probe = Arc::clone(&detector);
            let checked = tokio::task::spawn_blocking(move || {
                block_on(checking.check_detector(probe.as_ref()))
            })
            .await;
            if let Ok(Ok(Some(turn))) = checked {
                run_turns(&supervisor, vec![turn]);
            }
        }
    })
}
