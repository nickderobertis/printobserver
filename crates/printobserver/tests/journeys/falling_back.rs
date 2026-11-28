//! A print's status says, while it holds, that its checks are shown the plain
//! camera frame, and why, and says nothing of it once the three-panel image is
//! back.
//!
//! Each reason is one row of a table, and each row is a world of its own: the
//! supervisor this program runs, over a stood-in machine running the healthy
//! PETG jar top's slice, calibrated by the committed `core-one-2026-09-27.json`
//! that draws on that print's frames unshifted, its camera a host on the
//! loopback address showing whichever recorded frame — a plain copy from
//! `printobserver-panels`' fixtures — the row puts in front of it. Checks come
//! every second, as the periodic observation the supervisor schedules itself.
//! A row brings its reason on the way it comes on a real print — the job
//! somewhere, a frame, the printer paused, a spool gone, a calibration missing
//! — and then ends it the way an operator or the print would, and reads the
//! print's status after each, three ways: this program's labelled lines, its
//! `--json` document, and the supervisor's own HTTP answer.
//!
//! A reason no row drives is one whose condition no check of a print can end
//! while the print runs, or that no check of a print is ever given:
//! `no_print`, which a check of a print never is; `no_layer`,
//! `alignment_unavailable` and `no_position`, which the print's own G-code and
//! job fix for its whole life; `no_stream`, a server configured with no
//! camera address, whose periodic checks take no frame at all; and `refused`,
//! the composer's own refusal no frame or plan here provokes. The status reads
//! every reason from the same record, so a row for each would test that
//! record again rather than another path to it.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use printobserver_core::gcode_index::index_of;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::machine::{Machine, Reports};
use crate::proxy::Proxy;
use crate::world::{
    CREDENTIAL, JAR_TOP, OTHER_CREDENTIAL, Printer, World, committed_skill, jar_top, supervising,
    toml_of,
};

use super::rechecking::{Camera, committed, root};
use super::running;

/// A row waits on several checks a second apart, each composing an image,
/// which a slow host takes seconds over.
const PATIENCE: Duration = Duration::from_secs(90);

/// The healthy first layer, which the calibration draws on unshifted.
const FIRST_LAYER: &str = "jar-top-petg-healthy/agent-007-obs-121230-p01.3.jpg";

/// The same printer's bed before the print laid anything.
const BARE_BED: &str = "jar-top-petg-healthy/agent-001-obs-120024-p00.0.jpg";

/// Another printer's 1280x720 frame, of a size the calibration was not fitted
/// on.
const OTHER_STREAM: &str = "fan-cut-bridge/01-healthy-before-fault-p41.jpg";

const REASON: &str = "the camera was put back where its calibration has it";

/// One reason a check of a print is shown the plain frame, how a print comes
/// to it and how it ends.
struct Fallback {
    code: &'static str,
    /// Words the reason's message carries.
    says: &'static str,
    /// The supervisor's configuration as this row edits it before it starts.
    configured: fn(&mut Value),
    /// Whether the reason comes on after the print's checks were shown the
    /// three-panel image.
    after_three_panel: bool,
    onset: fn(&mut Stage),
    /// End it, so the next check is shown the three-panel image.
    clears: fn(&mut Stage),
}

/// One row's world and what it puts in front of the supervisor.
struct Stage {
    world: World,
    camera: Camera,
    calibration: PathBuf,
    /// A position just past the first layer.
    past_first: u64,
    /// A position inside the first object layer, before it is down.
    before_first: u64,
    /// Holds the calibration file.
    _held: TempDir,
}

impl Stage {
    fn machine(&self) -> &Machine {
        let Printer::StoodIn(machine) = &self.world.printer else {
            panic!("the {JAR_TOP} world is not a stood-in machine");
        };
        machine
    }

    fn at(&self, position: u64) {
        self.machine().stands_at.store(position, Ordering::SeqCst);
    }

    /// Where the print's G-code is spooled.
    fn spool(&self) -> PathBuf {
        self.world
            .root
            .path()
            .join("state")
            .join("gcode-spool")
            .join(format!("{}.gcode", self.world.print_id()))
    }
}

fn frame(file: &str) -> Vec<u8> {
    std::fs::read(
        root()
            .join("crates/printobserver-panels/tests/fixtures/frames")
            .join(file),
    )
    .expect("the recorded frame is there")
}

fn changed(bytes: &[u8], change: impl FnOnce(&mut image::RgbImage)) -> Vec<u8> {
    let mut decoded = image::load_from_memory(bytes).expect("a frame").to_rgb8();
    change(&mut decoded);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
        .encode_image(&decoded)
        .expect("a frame encodes");
    out
}

/// The first layer's frame moved `by` pixels right: the camera knocked out of
/// place past its drift tolerance, inside every landmark's search.
fn knocked(by: u32) -> Vec<u8> {
    changed(&frame(FIRST_LAYER), |source| {
        let moved = image::RgbImage::from_fn(source.width(), source.height(), |x, y| {
            if x < by {
                image::Rgb([0, 0, 0])
            } else {
                *source.get_pixel(x - by, y)
            }
        });
        *source = moved;
    })
}

/// The first layer's frame with every landmark of the calibration painted
/// over, as something standing in front of them would hide them.
fn landmarks_hidden() -> Vec<u8> {
    let centres: Vec<(u32, u32)> = committed()["landmarks"]
        .as_array()
        .expect("landmarks")
        .iter()
        .map(|landmark| {
            let at = |axis: usize| {
                format!(
                    "{:.0}",
                    landmark["center_px"][axis].as_f64().expect("a centre")
                )
                .parse::<u32>()
                .expect("a pixel of the frame")
            };
            (at(0), at(1))
        })
        .collect();
    changed(&frame(FIRST_LAYER), |decoded| {
        for (u, v) in centres {
            for y in v.saturating_sub(240)..(v + 240).min(decoded.height()) {
                for x in u.saturating_sub(240)..(u + 240).min(decoded.width()) {
                    decoded.put_pixel(x, y, image::Rgb([128, 128, 128]));
                }
            }
        }
    })
}

fn camera_section(stage_camera: &str, calibration: &std::path::Path) -> Value {
    json!({
        "snapshot_url": stage_camera,
        "calibration": calibration.display().to_string(),
    })
}

/// Stand one row's world up, the job at the start of its file and the
/// machine idle.
fn staged(row: &Fallback) -> Stage {
    let camera = Camera::showing(frame(FIRST_LAYER));
    let held = TempDir::new().expect("a directory for the calibration");
    let calibration = held.path().join("calibration.json");
    let mut written = committed();
    written["camera_snapshot_url"] = json!(camera.url());
    std::fs::write(&calibration, written.to_string()).expect("the calibration is written");
    let section = camera_section(&camera.url(), &calibration);
    let world = World::configured(JAR_TOP, &committed_skill(), None, |document| {
        document["camera"] = section;
        document["supervisor"]["observation_interval_s"] = json!(1);
        (row.configured)(document);
    });
    let bytes = jar_top();
    let index = index_of(&[&bytes]);
    let size = u64::try_from(bytes.len()).expect("a size");
    let past_first = (0..size)
        .step_by(500)
        .find(|position| {
            index
                .layer_at(*position)
                .and_then(|layer| index.layer_z_mm(layer))
                .is_some_and(|z| z >= 0.6 - 1e-6)
        })
        .expect("a layer above the first")
        + 100;
    let before_first = index.first_object_layer().expect("a first layer").1 - 50;
    Stage {
        world,
        camera,
        calibration,
        past_first,
        before_first,
        _held: held,
    }
}

/// Stop the supervisor, edit its configuration as an operator does, and
/// start it again over the same state directory, as a service restarted
/// after an edit is.
fn restarted(world: &mut World, edit: impl FnOnce(&mut Value)) {
    let _ = world.server.kill();
    let _ = world.server.wait();
    let configuration = world.root.path().join("server.toml");
    let mut document: Value = toml::from_str(
        &std::fs::read_to_string(&configuration).expect("the configuration is readable"),
    )
    .expect("the configuration is TOML");
    edit(&mut document);
    std::fs::write(&configuration, toml_of(&document)).expect("the configuration is writable");
    let search = world.root.path().join("no-harness-on-this-path");
    let (server, address, said) = supervising(&configuration, search.as_os_str());
    world.server = server;
    world.proxy = Proxy::in_front_of(address);
    world.said = std::sync::Mutex::new(said);
    for (config, credential) in [
        (world.client_config(), CREDENTIAL),
        (world.other_client_config(), OTHER_CREDENTIAL),
    ] {
        std::fs::write(
            config,
            format!(
                "[client]\nserver = \"{}\"\ncredential = \"{credential}\"\n",
                world.proxy.url()
            ),
        )
        .expect("the client configuration is writable");
    }
}

/// The print's status as the supervisor itself answers it over HTTP.
fn answered(world: &World, print_id: &str) -> Value {
    let address = world.proxy.address;
    let mut stream = std::net::TcpStream::connect(address).expect("the supervisor is reachable");
    write!(
        stream,
        "GET {}/prints/{print_id}/status HTTP/1.0\r\nHost: {address}\r\nAuthorization: Bearer {CREDENTIAL}\r\n\r\n",
        printobserver_server::VERSION_PREFIX
    )
    .expect("the request is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    let (head, body) = answer
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("an answer with a body: {answer}"));
    assert!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "{answer}"
    );
    printobserver_types::serde_json::from_str(body)
        .unwrap_or_else(|error| panic!("{error}: {body}"))
}

/// The print's status, read three ways at one moment.
struct Read {
    /// This program's `--json` document.
    document: Value,
    /// The supervisor's own answer.
    answer: Value,
    /// This program's labelled lines.
    lines: String,
}

impl Read {
    /// The value of one labelled line, when the lines carry it.
    fn line(&self, label: &str) -> Option<&str> {
        self.lines
            .lines()
            .find_map(|line| line.strip_prefix(label)?.strip_prefix(": "))
    }
}

/// Read the print's status until `wanted` holds of the document and all
/// three readings are of one check, and answer them.
fn until(stage: &Stage, what: &str, wanted: impl Fn(&Value) -> bool) -> Read {
    let world = &stage.world;
    let print_id = world.print_id();
    let deadline = Instant::now() + PATIENCE;
    let mut last = Value::Null;
    while Instant::now() < deadline {
        let document = running::read(world, &["status", "--print-id", &print_id]);
        last = document["last_check_image"].clone();
        if wanted(&last) {
            let answer = answered(world, &print_id);
            let ran = running::command(world, &["status", "--print-id", &print_id]);
            assert_eq!(ran.code, Some(0), "{}", ran.said());
            let read = Read {
                document,
                answer,
                lines: ran.out,
            };
            let at = &read.document["last_check_image"]["at"];
            // A check may land between two readings; read again until all
            // three are of the same one.
            if read.answer["last_check_image"]["at"] == *at
                && read.line("last_check_image.at") == at.as_str()
            {
                return read;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("{what} never happened; the last check image was {last}");
}

/// Wait until the print's checks are shown the three-panel image, and hold
/// every reading to saying nothing of a fallback.
fn three_panel(stage: &Stage, row: &Fallback, when: &str) {
    let read = until(
        stage,
        &format!("{}: three panels {when}", row.code),
        |image| image["kind"] == "three_panel",
    );
    for (said, document) in [("--json", &read.document), ("HTTP", &read.answer)] {
        assert_eq!(
            document["last_check_image"]["fallback"],
            Value::Null,
            "{}: {said} {when}: {document}",
            row.code
        );
    }
    assert_eq!(read.line("last_check_image.kind"), Some("three_panel"));
    assert_eq!(
        read.line("last_check_image.fallback"),
        Some("null"),
        "{}: {when}: {}",
        row.code,
        read.lines
    );
    assert!(
        !read.lines.contains("last_check_image.fallback.")
            && !read.lines.contains(row.code)
            && !read.lines.contains(row.says),
        "{} {when}: the lines say nothing of a fallback: {}",
        row.code,
        read.lines
    );
}

/// Wait until the print's checks are shown the plain frame for the row's
/// reason, and hold every reading to saying so, with its message.
fn falls_back(stage: &Stage, row: &Fallback) {
    let read = until(
        stage,
        &format!("the plain frame for {}", row.code),
        |image| image["fallback"]["code"] == row.code,
    );
    let message = read.document["last_check_image"]["fallback"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.contains(row.says),
        "{}: the message names the reason: {message}",
        row.code
    );
    for (said, document) in [("--json", &read.document), ("HTTP", &read.answer)] {
        let image = &document["last_check_image"];
        assert_eq!(
            image["kind"], "whole_frame",
            "{}: {said}: {document}",
            row.code
        );
        assert_eq!(
            image["fallback"],
            json!({ "code": row.code, "message": message }),
            "{}: {said}: {document}",
            row.code
        );
    }
    assert_eq!(
        (
            read.line("last_check_image.kind"),
            read.line("last_check_image.fallback.code"),
            read.line("last_check_image.fallback.message"),
        ),
        (Some("whole_frame"), Some(row.code), Some(message.as_str())),
        "the lines say the plain frame, its reason and its message: {}",
        read.lines
    );
}

const fn as_installed(_: &mut Value) {}

/// The job past its first layer and printing.
fn printing_past_first(stage: &mut Stage) {
    stage.at(stage.past_first);
    stage.world.wants(Reports::Printing);
}

fn first_layer_again(stage: &mut Stage) {
    stage.camera.shows(frame(FIRST_LAYER));
}

/// Every reason a row drives, with how it comes on and how it ends.
fn fallbacks() -> Vec<Fallback> {
    let mut rows = before_any_pass();
    rows.extend(after_a_pass());
    rows
}

/// The reasons a print's checks are given before its first layer is aligned.
fn before_any_pass() -> Vec<Fallback> {
    vec![
        Fallback {
            code: "no_calibration",
            says: "no camera calibration is configured",
            configured: |document| {
                document["camera"]
                    .as_object_mut()
                    .expect("a camera section")
                    .remove("calibration");
            },
            after_three_panel: false,
            onset: printing_past_first,
            clears: |stage| {
                let calibration = stage.calibration.display().to_string();
                restarted(&mut stage.world, |document| {
                    document["camera"]["calibration"] = json!(calibration);
                });
            },
        },
        Fallback {
            code: "no_object",
            says: "names no extruded object laid at this point",
            configured: as_installed,
            after_three_panel: false,
            onset: |stage| {
                stage.at(200);
                stage.world.wants(Reports::Printing);
            },
            clears: |stage| stage.at(stage.past_first),
        },
        Fallback {
            code: "alignment_pending",
            says: "first-layer alignment is pending",
            configured: as_installed,
            after_three_panel: false,
            onset: |stage| {
                stage.at(stage.before_first);
                stage.world.wants(Reports::Printing);
            },
            clears: |stage| stage.at(stage.past_first),
        },
        Fallback {
            code: "alignment_inconclusive",
            says: "the printed first layer was not clearly found",
            configured: as_installed,
            after_three_panel: false,
            onset: |stage| {
                stage.camera.shows(frame(BARE_BED));
                printing_past_first(stage);
            },
            clears: first_layer_again,
        },
        Fallback {
            code: "alignment_failed",
            says: "the print's first-layer alignment failed",
            configured: as_installed,
            after_three_panel: false,
            onset: |stage| {
                stage.camera.shows(frame(OTHER_STREAM));
                printing_past_first(stage);
            },
            // A failure is latched: the camera put right is judged again only
            // when the operator asks.
            clears: |stage| {
                first_layer_again(stage);
                let print_id = stage.world.print_id();
                let rechecked = running::read(
                    &stage.world,
                    &[
                        "recheck-camera",
                        "--print-id",
                        &print_id,
                        "--actor",
                        "operator",
                        "--reason",
                        REASON,
                    ],
                );
                assert_eq!(rechecked["alignment"]["verdict"], "passed", "{rechecked}");
            },
        },
        Fallback {
            code: "calibration_stale",
            says: "the camera has moved since its calibration, so the outline cannot be drawn reliably; recalibrate this camera",
            configured: as_installed,
            after_three_panel: false,
            onset: |stage| {
                stage.camera.shows(knocked(20));
                printing_past_first(stage);
            },
            clears: first_layer_again,
        },
    ]
}

/// The reasons a print's checks are given after they were shown the
/// three-panel image.
fn after_a_pass() -> Vec<Fallback> {
    vec![
        Fallback {
            code: "toolpath_unread",
            says: "could not be read from the print's spool",
            configured: as_installed,
            after_three_panel: true,
            onset: |stage| {
                let spool = stage.spool();
                std::fs::remove_file(&spool).expect("the print's spool is there");
                std::fs::create_dir_all(&spool).expect("a directory where the spool was");
            },
            clears: |stage| {
                let spool = stage.spool();
                std::fs::remove_dir(&spool).expect("the directory goes");
                std::fs::write(&spool, jar_top()).expect("the spool is written back");
            },
        },
        Fallback {
            code: "landmarks_not_found",
            says: "fewer than two of the landmarks",
            configured: as_installed,
            after_three_panel: true,
            onset: |stage| stage.camera.shows(landmarks_hidden()),
            clears: first_layer_again,
        },
        Fallback {
            code: "view_refit_rejected",
            says: "the camera has moved again since this print's first-layer correction was taken",
            // The next check after this one judges the moved camera afresh,
            // so each check is held long enough to read this one's.
            configured: |document| {
                document["supervisor"]["observation_interval_s"] = json!(3);
            },
            after_three_panel: true,
            onset: |stage| stage.camera.shows(knocked(20)),
            clears: first_layer_again,
        },
        Fallback {
            code: "frame_mismatch",
            says: "the frame's resolution is 1280x720",
            configured: as_installed,
            after_three_panel: true,
            onset: |stage| stage.camera.shows(frame(OTHER_STREAM)),
            clears: first_layer_again,
        },
        Fallback {
            code: "paused",
            says: "the printer reports the print paused",
            configured: as_installed,
            after_three_panel: true,
            onset: |stage| stage.world.wants(Reports::Paused),
            clears: |stage| stage.world.wants(Reports::Printing),
        },
    ]
}

/// `printobserver status` says, for each reason a check of a print is shown
/// the plain frame, that it is the plain frame, the reason and its message —
/// in its labelled lines, in its `--json` document and in the supervisor's own
/// answer — while the reason holds, and nothing of it once the condition ends
/// and the three-panel image is back.
pub fn the_status_says_while_checks_are_shown_the_plain_frame_and_why() {
    for row in fallbacks() {
        let mut stage = staged(&row);
        let print_id = stage.world.print_id();
        let status = running::read(&stage.world, &["status", "--print-id", &print_id]);
        assert_eq!(
            status["last_check_image"],
            Value::Null,
            "{}: no check has been shown an image yet: {status}",
            row.code
        );
        if row.after_three_panel {
            printing_past_first(&mut stage);
            three_panel(&stage, &row, "before the reason comes on");
        }
        (row.onset)(&mut stage);
        falls_back(&stage, &row);
        (row.clears)(&mut stage);
        three_panel(&stage, &row, "once the reason has ended");
    }
}
