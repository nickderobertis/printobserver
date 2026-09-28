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
//! A frame is a `u32` length followed by that many bytes.

use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::process;
use std::thread;

/// The variable naming where the relay listens, as `host:port`.
const ADDRESS: &str = "SMOKE_RELAY_ADDRESS";

/// The exit of a command the relay never answered: none of the program's own.
const UNANSWERED: i32 = 70;

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
    let mut stream = TcpStream::connect(&address)?;
    stream.set_nodelay(true)?;

    let arguments: Vec<String> = env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
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

/// Append `bytes` to `into` as one frame.
fn frame(into: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    into.extend_from_slice(&length(bytes.len())?.to_be_bytes());
    into.extend_from_slice(bytes);
    Ok(())
}

/// Read one frame from `stream`.
fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut size = [0_u8; 4];
    stream.read_exact(&mut size)?;
    let mut bytes = vec![0_u8; u32::from_be_bytes(size) as usize];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A length as the wire carries one.
fn length(size: usize) -> io::Result<u32> {
    u32::try_from(size).map_err(|_| io::Error::other(format!("{size} bytes is too long to frame")))
}
