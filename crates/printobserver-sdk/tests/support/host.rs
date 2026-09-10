//! A stub supervisor on a real socket, for the generated walk to drive.
//!
//! It is a **host** rather than a stand-in for the client's transport: the
//! client opens a real connection, writes a real request and reads a real
//! answer, and what this records is what arrived over that connection. The
//! layer under test is the client, and nothing here is inside it.
//!
//! The real server is what the printer integration tier drives. This is what
//! the fast tier drives, because a walk over every operation needs an answer of
//! every shape and a real supervisor cannot be put into sixteen states in a
//! second.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;

/// What arrived over one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The method it was made by.
    pub method: String,
    /// The whole request target, question mark and all.
    pub target: String,
    /// The body it carried, empty where it carried none.
    pub body: String,
}

/// A host answering one canned document to whatever it is asked.
pub struct Host {
    /// Where it answers.
    address: String,
    /// What arrived, in the order it arrived.
    arrivals: Receiver<Received>,
    /// The thread serving it, joined when this is dropped.
    serving: Option<JoinHandle<()>>,
}

impl Host {
    /// A host answering `status` with `body`, on a port the system chooses.
    ///
    /// # Panics
    ///
    /// Panics when no loopback port can be taken, which is a machine nothing
    /// could be driven on.
    pub fn answering(status: u16, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener
            .local_addr()
            .expect("the bound address")
            .to_string();
        let (sending, arrivals) = channel();
        let answer = body.to_owned();
        let serving = std::thread::spawn(move || {
            // One connection per call, and the walk makes one call per host:
            // a host that went on serving would outlive the assertion it is
            // for.
            if let Ok((stream, _)) = listener.accept()
                && let Some(received) = serve(stream, status, &answer)
            {
                let _ = sending.send(received);
            }
        });
        Self {
            address,
            arrivals,
            serving: Some(serving),
        }
    }

    /// Where this host answers.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// What arrived over the one connection this host served.
    ///
    /// # Panics
    ///
    /// Panics when nothing arrived, which is a call that was never made.
    pub fn received(&self) -> Received {
        self.arrivals
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the call reached this host")
    }

    /// How many requests reached this host.
    ///
    /// Read without waiting: what a caller asks this is whether a call that
    /// should have been refused before it was made reached the wire, and
    /// waiting for one that never comes would be waiting for a timeout.
    pub fn requests(&self) -> usize {
        self.arrivals.try_iter().count()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(serving) = self.serving.take() {
            // The thread is blocked in `accept` when no call was made, so it
            // is left to the process rather than joined: a walk that waited
            // for a connection nobody was going to make would never finish.
            if serving.is_finished() {
                let _ = serving.join();
            }
        }
    }
}

/// Read one request off a connection and answer the canned document.
fn serve(stream: TcpStream, status: u16, body: &str) -> Option<Received> {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    reader.read_line(&mut request).ok()?;
    let mut words = request.split_whitespace();
    let method = words.next()?.to_owned();
    let target = words.next()?.to_owned();

    let mut length = 0_usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut carried = vec![0_u8; length];
    reader.read_exact(&mut carried).ok()?;

    let answer = format!(
        "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = reader.into_inner();
    stream.write_all(answer.as_bytes()).ok()?;
    stream.flush().ok()?;
    Some(Received {
        method,
        target,
        body: String::from_utf8_lossy(&carried).into_owned(),
    })
}
