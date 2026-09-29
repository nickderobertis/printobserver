//! The per-command half of the printer-smoke relay: what the smoke runs as its program.
//!
//! The relay itself (`relay.py`) is one long-lived process a test's `World`
//! starts. This is the thin client of it that the smoke runs once per command:
//! it hands the relay its arguments and its standard input over a loopback
//! connection, and answers with whatever the relay answers — the program's
//! exit status and both of its streams. It is compiled rather than
//! interpreted because the start-up of an interpreter per command is exactly
//! the cost the relay exists to take out of every command's bound.
//!
//! The wire, every integer big-endian:
//!
//! * to the relay: the argument count (`u32`), each argument as a frame, then
//!   standard input as frames, ending with an empty one;
//! * from the relay: the exit status (`i32`), then standard output and
//!   standard error as one frame each.
//!
//! A frame is a `u32` length followed by that many bytes. `relay.py` is the
//! other half of this wire, and every test in `test_relay.py` drives this
//! client against it, so the two cannot drift apart without one failing.

use std::env;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process;
use std::thread;

/// The variable naming where the relay listens, as `host:port`: `relay.py`'s
/// `ADDRESS`, which `test_relay.py` holds this to.
const ADDRESS: &str = "SMOKE_RELAY_ADDRESS";

/// The exit of a command the relay never answered: none of the program's own.
// llmlint: ignore[cli_output_contract] suppressions.toml has the reason.
const UNANSWERED: i32 = 70;

/// The most bytes one frame from the relay may carry: `relay.py`'s
/// `MOST_FRAME_BYTES`, which `test_relay.py` holds this to.
const MOST_FRAME_BYTES: u32 = 64 * 1024 * 1024;

fn main() {
    let code = match relay() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("the smoke's relay did not answer: {error}");
            UNANSWERED
        }
    };
    process::exit(code);
}

/// Hand this command to the relay, write what it answered, and return its exit.
fn relay() -> io::Result<i32> {
    let address = env::var(ADDRESS)
        .map_err(|_| io::Error::other(format!("{ADDRESS} names no relay to hand this to")))?;
    // The relay is always this host's own, so an address that names another
    // host is refused before anything of this command is sent to it.
    let relay: SocketAddr = address
        .parse()
        .ok()
        .filter(|relay: &SocketAddr| relay.ip().is_loopback())
        .ok_or_else(|| {
            io::Error::other(format!(
                "{ADDRESS} is {address:?}, not a loopback host:port"
            ))
        })?;
    let mut stream = TcpStream::connect(relay)?;
    stream.set_nodelay(true)?;

    // Refused rather than converted: a lossy conversion would run the program
    // with arguments the smoke never gave it.
    let arguments = env::args_os()
        .skip(1)
        .map(|argument| {
            argument.into_string().map_err(|argument| {
                io::Error::other(format!("the argument {argument:?} is not UTF-8"))
            })
        })
        .collect::<io::Result<Vec<String>>>()?;
    let mut header = Vec::new();
    header.extend_from_slice(&length(arguments.len())?.to_be_bytes());
    for argument in &arguments {
        frame(&mut header, argument.as_bytes())?;
    }
    stream.write_all(&header)?;

    // Standard input is forwarded as it arrives rather than read to its end
    // first: an input nothing ever closes would otherwise hold the command up
    // forever, where the program it reaches never reads it at all.
    let mut upstream = stream.try_clone()?;
    thread::spawn(move || forward_input(&mut upstream));

    let mut status = [0_u8; 4];
    stream.read_exact(&mut status)?;
    let output = read_frame(&mut stream)?;
    let error = read_frame(&mut stream)?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&output)?;
    stdout.flush()?;
    let mut stderr = io::stderr().lock();
    stderr.write_all(&error)?;
    stderr.flush()?;
    Ok(i32::from_be_bytes(status))
}

/// Copy standard input to the relay until it ends, then say that it has.
fn forward_input(upstream: &mut TcpStream) {
    let mut buffer = [0_u8; 8192];
    let mut stdin = io::stdin().lock();
    loop {
        // An input that cannot be read is one that has ended, as far as the
        // program on the far side can tell.
        let read = stdin.read(&mut buffer).unwrap_or(0);
        let mut chunk = Vec::with_capacity(read + 4);
        if frame(&mut chunk, &buffer[..read]).is_err()
            || upstream.write_all(&chunk).is_err()
            || read == 0
        {
            return;
        }
    }
}

fn frame(into: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    into.extend_from_slice(&length(bytes.len())?.to_be_bytes());
    into.extend_from_slice(bytes);
    Ok(())
}

/// Read one frame from `stream`, refusing one past `MOST_FRAME_BYTES` before
/// allocating anything for it.
fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut size = [0_u8; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size);
    if size > MOST_FRAME_BYTES {
        return Err(io::Error::other(format!(
            "the relay sent a frame of {size} bytes, over the most this takes ({MOST_FRAME_BYTES})"
        )));
    }
    let mut bytes = vec![0_u8; size as usize];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn length(size: usize) -> io::Result<u32> {
    u32::try_from(size).map_err(|_| io::Error::other(format!("{size} bytes is too long to frame")))
}
