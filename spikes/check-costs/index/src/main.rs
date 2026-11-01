//! A throwaway prototype of the G-code index the context change builds, and
//! the measurement of building it.
//!
//! One streaming pass maps a byte position in the file to the layer it is in,
//! that layer's Z (`;LAYER_CHANGE` then `;Z:`), the `;TYPE:` labels printed in
//! that layer, and the normal-mode `M73 ... R<minutes>` in effect there; plus
//! the final layer's height.
//!
//! ```text
//! gcode-index-spike local PATH
//! gcode-index-spike octoprint BASE_URL API_KEY FILE_NAME
//! gcode-index-spike octoprint-extruded-to BASE_URL API_KEY FILE_NAME SIZE
//! ```
//!
//! Each prints one JSON line: wall time, peak heap (a counting global
//! allocator), the process's peak resident set (`VmHWM`), the index's entry
//! count and resident size, and the layer printing the most types.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size > layout.size() {
                let now = CURRENT.fetch_add(new_size - layout.size(), Ordering::Relaxed)
                    + (new_size - layout.size());
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// One layer: where it starts in the file, its Z, and the types printed in it
/// as a bit set over [`Index::types`].
#[derive(Debug, Clone, Copy)]
struct Layer {
    start: u64,
    z_mm: f32,
    types: u32,
}

/// One normal-mode progress marker: where it is, and the minutes it says are left.
#[derive(Debug, Clone, Copy)]
struct Remaining {
    at: u64,
    minutes: u32,
}

#[derive(Debug, Default)]
struct Index {
    /// Every `;TYPE:` label seen, in first-seen order; a layer's bit `i` is `types[i]`.
    types: Vec<String>,
    layers: Vec<Layer>,
    remaining: Vec<Remaining>,
    final_z_mm: Option<f32>,
}

impl Index {
    fn layer_at(&self, position: u64) -> Option<&Layer> {
        let after = self.layers.partition_point(|layer| layer.start <= position);
        after.checked_sub(1).map(|i| &self.layers[i])
    }

    fn remaining_at(&self, position: u64) -> Option<u32> {
        let after = self.remaining.partition_point(|marker| marker.at <= position);
        after.checked_sub(1).map(|i| self.remaining[i].minutes)
    }

    fn type_names(&self, bits: u32) -> Vec<&str> {
        (0..self.types.len())
            .filter(|i| bits & (1 << i) != 0)
            .map(|i| self.types[i].as_str())
            .collect()
    }

    /// Heap bytes the built index holds, by capacity.
    fn resident_bytes(&self) -> usize {
        size_of::<Self>()
            + self.layers.capacity() * size_of::<Layer>()
            + self.remaining.capacity() * size_of::<Remaining>()
            + self.types.capacity() * size_of::<String>()
            + self.types.iter().map(String::capacity).sum::<usize>()
    }

    fn entries(&self) -> usize {
        self.layers.len() + self.remaining.len() + self.types.len()
    }
}

/// The streaming builder: takes the body in pieces of any size.
#[derive(Default)]
struct Builder {
    index: Index,
    carry: Vec<u8>,
    offset: u64,
    in_layer_change: bool,
}

impl Builder {
    fn take(&mut self, mut bytes: &[u8]) {
        while let Some(newline) = bytes.iter().position(|&b| b == b'\n') {
            let (line, rest) = bytes.split_at(newline + 1);
            if self.carry.is_empty() {
                self.line(line);
            } else {
                let mut whole = std::mem::take(&mut self.carry);
                whole.extend_from_slice(line);
                self.line(&whole);
                whole.clear();
                self.carry = whole;
            }
            bytes = rest;
        }
        self.carry.extend_from_slice(bytes);
    }

    fn end(mut self) -> Index {
        if !self.carry.is_empty() {
            let last = std::mem::take(&mut self.carry);
            self.line(&last);
        }
        self.index.layers.shrink_to_fit();
        self.index.remaining.shrink_to_fit();
        self.index
    }

    fn line(&mut self, raw: &[u8]) {
        let start = self.offset;
        self.offset += raw.len() as u64;
        let line = raw.strip_suffix(b"\n").unwrap_or(raw);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.first() == Some(&b';') {
            if line == b";LAYER_CHANGE" {
                self.index.layers.push(Layer { start, z_mm: f32::NAN, types: 0 });
                self.in_layer_change = true;
            } else if let Some(z) = line.strip_prefix(b";Z:") {
                if let Some(z) = parse_f32(z) {
                    if self.in_layer_change {
                        if let Some(layer) = self.index.layers.last_mut() {
                            layer.z_mm = z;
                        }
                        self.in_layer_change = false;
                    }
                    self.index.final_z_mm = Some(z);
                }
            } else if let Some(label) = line.strip_prefix(b";TYPE:") {
                let label = String::from_utf8_lossy(label);
                let bit = match self.index.types.iter().position(|t| *t == label) {
                    Some(i) => i,
                    None => {
                        self.index.types.push(label.into_owned());
                        self.index.types.len() - 1
                    }
                };
                if let Some(layer) = self.index.layers.last_mut() {
                    layer.types |= 1 << bit.min(31);
                }
            }
        } else if let Some(rest) = line.strip_prefix(b"M73 P") {
            // Normal mode only: `M73 P<percent> R<minutes>`; silent mode is `Q.. S..`.
            if let Some(r) = rest.split(|&b| b == b' ').find_map(|w| w.strip_prefix(b"R")) {
                if let Some(minutes) = std::str::from_utf8(r).ok().and_then(|r| r.parse().ok()) {
                    self.index.remaining.push(Remaining { at: start, minutes });
                }
            }
        }
    }
}

fn parse_f32(bytes: &[u8]) -> Option<f32> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
}

fn report(index: &Index, wall: f64, bytes: u64, mode: &str) {
    let most = index
        .layers
        .iter()
        .enumerate()
        .max_by_key(|(i, l)| (l.types.count_ones(), std::cmp::Reverse(*i)))
        .map(|(i, l)| {
            serde_json::json!({
                "layer_number": i + 1,
                "layer_count": index.layers.len(),
                "z_mm": l.z_mm,
                "types": index.type_names(l.types),
                "slicer_minutes_left": index.remaining_at(l.start),
                "start_byte": l.start,
                "lookup_check": index.layer_at(l.start).map(|f| f.start) == Some(l.start),
            })
        });
    let out = serde_json::json!({
        "mode": mode,
        "bytes_read": bytes,
        "wall_s": wall,
        "peak_heap_bytes": PEAK.load(Ordering::Relaxed),
        "peak_rss_kib": peak_rss_kib(),
        "index_entries": index.entries(),
        "index_layers": index.layers.len(),
        "index_m73_markers": index.remaining.len(),
        "index_types": index.types,
        "index_resident_bytes": index.resident_bytes(),
        "final_z_mm": index.final_z_mm,
        "layer_with_most_types": most,
    });
    println!("{out}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["local", path] => {
            let started = Instant::now();
            let mut file = std::fs::File::open(path).expect("the G-code opens");
            let mut builder = Builder::default();
            let mut buffer = vec![0u8; 64 * 1024];
            let mut bytes = 0u64;
            loop {
                let n = file.read(&mut buffer).expect("the G-code reads");
                if n == 0 {
                    break;
                }
                bytes += n as u64;
                builder.take(&buffer[..n]);
            }
            drop(buffer);
            let index = builder.end();
            report(&index, started.elapsed().as_secs_f64(), bytes, "local");
        }
        ["octoprint", url, key, name] => {
            let printer = printer(url, key);
            let file = printobserver_types::FileName::new(*name).expect("a file name");
            let started = Instant::now();
            let mut builder = Builder::default();
            let mut bytes = 0u64;
            printer
                .spike_stream_gcode(&file, &mut |chunk| {
                    bytes += chunk.len() as u64;
                    builder.take(chunk);
                })
                .expect("the G-code streams from OctoPrint");
            let index = builder.end();
            report(&index, started.elapsed().as_secs_f64(), bytes, "octoprint");
        }
        ["octoprint-extruded-to", url, key, name, size] => {
            use printobserver_printer_api::PrinterPort as _;
            let printer = printer(url, key);
            let file = printobserver_types::FileName::new(*name).expect("a file name");
            let size: i64 = size.parse().expect("a size");
            let started = Instant::now();
            // The adapter's future is synchronous inside: one poll answers it.
            let mut future = printer.extruded_to(file, size);
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            let std::task::Poll::Ready(fed) = future.as_mut().poll(&mut context) else {
                panic!("the adapter's read did not answer on its first poll");
            };
            let fed = fed.expect("read_gcode_to answers");
            let wall = started.elapsed().as_secs_f64();
            println!(
                "{}",
                serde_json::json!({
                    "mode": "octoprint-extruded-to",
                    "wall_s": wall,
                    "peak_heap_bytes": PEAK.load(Ordering::Relaxed),
                    "peak_rss_kib": peak_rss_kib(),
                    "answer": format!("{fed:?}"),
                })
            );
        }
        _ => {
            eprintln!("usage: gcode-index-spike local PATH | octoprint URL KEY NAME | octoprint-extruded-to URL KEY NAME SIZE");
            std::process::exit(2);
        }
    }
}

fn printer(url: &str, key: &str) -> printobserver_octoprint::OctoPrintPrinter {
    let config = printobserver_octoprint::OctoPrintConfig::new(url, key.to_owned())
        .expect("an OctoPrint configuration")
        .with_timeout(std::time::Duration::from_secs(120));
    printobserver_octoprint::OctoPrintPrinter::new(config)
}
