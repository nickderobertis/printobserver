//! One check image of a recorded frame, for the real-print suite's image probe.
//!
//! `printobserver-server-compose --config <path> --gcode <path> --frame <path>
//! --position <bytes> --out <path>`
//! loads a server configuration exactly as `printobserver server` does, indexes
//! the G-code with the real index builder — spooling it, as the server spools a
//! print's file — and composes the three-panel check image a print's first
//! check at that file position is shown ([`compose_at`]), with the
//! configuration's calibration and camera snapshot address. It writes the
//! image to `--out` and prints as JSON its width, height and bytes, and the
//! line a check shown it carries in its situation.
//!
//! Exit 0 with the image written; 2 for arguments it cannot read; 1 for a
//! configuration, a file or a frame it cannot use, the reason on standard
//! error.

use std::path::PathBuf;
use std::process::ExitCode;

use printobserver_core::IndexBuilder;
use printobserver_printer_api::GcodeSink as _;
use printobserver_server::{PlanAt, ServerConfig, compose_at, composite_note};
use printobserver_types::Timestamp;

/// What the composer was asked for.
struct Asked {
    /// The server configuration.
    config: PathBuf,
    /// The print's G-code.
    gcode: PathBuf,
    /// The camera frame.
    frame: PathBuf,
    /// The check's file position.
    position: u64,
    /// Where the image is written.
    out: PathBuf,
}

/// The five options, each once and none empty.
fn asked(mut arguments: impl Iterator<Item = String>) -> Result<Asked, String> {
    let (mut config, mut gcode, mut frame, mut position, mut out) = (None, None, None, None, None);
    while let Some(flag) = arguments.next() {
        let slot = match flag.as_str() {
            "--config" => &mut config,
            "--gcode" => &mut gcode,
            "--frame" => &mut frame,
            "--position" => &mut position,
            "--out" => &mut out,
            other => return Err(format!("`{other}` is no option of this composer")),
        };
        let value = arguments
            .next()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{flag} takes a value"))?;
        if slot.replace(value).is_some() {
            return Err(format!("{flag} is given twice"));
        }
    }
    // Each missing option is named in the order the usage names them.
    let config = PathBuf::from(config.ok_or("--config is required")?);
    let gcode = PathBuf::from(gcode.ok_or("--gcode is required")?);
    let frame = PathBuf::from(frame.ok_or("--frame is required")?);
    let position = position.ok_or("--position is required")?;
    Ok(Asked {
        config,
        gcode,
        frame,
        position: position
            .parse()
            .map_err(|_| format!("--position {position} is no count of bytes"))?,
        out: PathBuf::from(out.ok_or("--out is required")?),
    })
}

/// Compose the image and say its size.
fn composed(asked: &Asked) -> Result<String, String> {
    let config = ServerConfig::load(&asked.config).map_err(|error| error.to_string())?;
    let calibration = config
        .camera_calibration
        .as_deref()
        .ok_or("the configuration names no camera.calibration")?;
    let url = config
        .camera_snapshot_url
        .as_ref()
        .ok_or("the configuration names no camera.snapshot_url")?;
    let read = |path: &PathBuf| {
        std::fs::read(path).map_err(|error| format!("{} is unreadable: {error}", path.display()))
    };
    let gcode = read(&asked.gcode)?;
    let frame = read(&asked.frame)?;
    let spool = asked.out.with_extension("spool.gcode");
    let mut builder = IndexBuilder::spooling(spool.clone())
        .map_err(|error| format!("{} cannot be written: {error}", spool.display()))?;
    let _ = builder.take(&gcode);
    let index = builder.finish();
    let image = compose_at(
        PlanAt {
            index: &index,
            file_position_bytes: asked.position,
        },
        (&frame, Timestamp::now()),
        None,
        url.as_str(),
        calibration,
    );
    let _ = std::fs::remove_file(&spool);
    let image = image.map_err(|reason| format!("no check image: {reason}"))?;
    std::fs::write(&asked.out, &image.jpeg)
        .map_err(|error| format!("{} cannot be written: {error}", asked.out.display()))?;
    Ok(serde_json::json!({
        "width": image.width,
        "height": image.height,
        "bytes": image.jpeg.len(),
        "note": composite_note(true),
    })
    .to_string())
}

fn main() -> ExitCode {
    let asked = match asked(std::env::args().skip(1)) {
        Ok(asked) => asked,
        Err(error) => {
            eprintln!("printobserver-server-compose: {error}");
            return ExitCode::from(2);
        }
    };
    match composed(&asked) {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("printobserver-server-compose: {error}");
            ExitCode::FAILURE
        }
    }
}
