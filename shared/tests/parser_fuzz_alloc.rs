//! Peak heap of the run import (`shared::import::import_run_file`, what the
//! `ParseRunFile` op runs) relative to the input size, for FIT and GPX, measured
//! with a counting global allocator (its own test binary, so no other test
//! shares the count).
//!
//! An import reads at most `shared::import::MAX_RUN_IMPORT_BYTES` (the shells
//! stop reading there too); a Rust allocation failure aborts the process (no
//! unwind, no firewall), so the import's peak per input byte times that cap is
//! its worst case.
//!
//! `PARSER_FUZZ_ALLOC_MB=<n>` additionally measures an n-MB file (slow in debug).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Serializes the tests: libtest runs them on parallel threads and the counter
/// is process-wide, so a test must hold this while it builds its input too.
static MEASURE: Mutex<()> = Mutex::new(());

fn exclusive() -> MutexGuard<'static, ()> {
    MEASURE.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                let now = LIVE.fetch_add(new - l.size(), Ordering::Relaxed) + (new - l.size());
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Peak heap growth (bytes above the live count at entry) while `f` runs. Takes
/// the [`exclusive`] guard so no other test allocates meanwhile.
fn peak_during<T>(_held: &MutexGuard<'_, ()>, f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = f();
    (out, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

// The unit tests' FIT writer; this binary uses only part of it.
#[allow(dead_code)]
#[path = "../src/test_support/fit_writer.rs"]
mod fit_writer;

use fit_writer::{FitWriter, I32, U8, U32, semi};

/// A plain 1 Hz run: `record` = timestamp, lat, lon, heart_rate (14 bytes).
fn run_fit(fixes: u32) -> Vec<u8> {
    // Metres per degree of latitude on the core's sphere.
    let m_per_deg = shared::gps::EARTH_RADIUS_M.to_radians();
    let mut w = FitWriter::default();
    w.def(
        0,
        20,
        false,
        &[(253, 4, U32), (0, 4, I32), (1, 4, I32), (3, 1, U8)],
        &[],
    );
    for i in 0..fixes {
        let mut body = (1_100_000_000 + i).to_le_bytes().to_vec();
        body.extend_from_slice(&semi(47.0 + f64::from(i % 50_000) * 3.0 / m_per_deg).to_le_bytes());
        body.extend_from_slice(&semi(8.0).to_le_bytes());
        body.push(140);
        w.msg(0, &body);
    }
    w.finish(true)
}

/// The same size spent on what a device writes besides positions: an unknown
/// message (global 0xFF00) carrying one 255-byte uint8 array per message, the
/// worst case for decoded size per wire byte.
fn array_fit(messages: u32) -> Vec<u8> {
    let mut w = FitWriter::default();
    w.def(0, 0xFF00, false, &[(0, 255, U8)], &[]);
    for i in 0..messages {
        let body: Vec<u8> = (0..255u32).map(|k| ((i + k) % 250) as u8).collect();
        w.msg(0, &body);
    }
    w.finish(true)
}

fn import(bytes: &[u8]) -> bool {
    shared::import::import_run_file(bytes).refusal.is_none()
}

const CAP_MB: usize = shared::import::MAX_RUN_IMPORT_BYTES as usize / (1024 * 1024);

/// Heap per input byte the import may use: at the 32 MB cap this is a 512 MB
/// native peak, beyond which a mid-range phone's process is at risk. A file of
/// messages the importer never reads (no fixes come out) needs no more than its
/// own bytes; a positioned run needs its fixes.
const MAX_BYTES_PER_INPUT_BYTE: usize = 16;

#[test]
fn a_gpx_run_imports_within_a_bounded_heap_per_input_byte() {
    let held = exclusive();
    let mut doc = String::from("<?xml version=\"1.0\"?><gpx version=\"1.1\"><trk><trkseg>\n");
    for i in 0..20_000u32 {
        let lat = 47.0 + f64::from(i) * 3.0 / 111_195.0;
        let (h, m, s) = (i / 3600, i / 60 % 60, i % 60);
        doc += &format!(
            "<trkpt lat=\"{lat}\" lon=\"8.0\"><time>2026-03-04T{h:02}:{m:02}:{s:02}Z</time><hdop>0.9</hdop></trkpt>\n"
        );
    }
    doc += "</trkseg></trk></gpx>\n";
    let bytes = doc.into_bytes();
    let (ok, peak) = peak_during(&held, || import(&bytes));
    assert!(ok);
    let per = peak / bytes.len();
    eprintln!("GPX: {} bytes, peak {peak} B ({per} B/B)", bytes.len());
    assert!(
        per <= MAX_BYTES_PER_INPUT_BYTE,
        "{per} B of heap per input byte, {} MB at the {CAP_MB} MB cap",
        per * CAP_MB
    );
}

#[test]
fn a_plain_run_fit_parses_within_a_bounded_heap_per_input_byte() {
    let held = exclusive();
    let bytes = run_fit(40_000); // ~0.6 MB, 11 h at 1 Hz
    let (ok, peak) = peak_during(&held, || import(&bytes));
    assert!(ok);
    let per = peak / bytes.len();
    eprintln!("run FIT: {} bytes, peak {peak} B ({per} B/B)", bytes.len());
    // parse_fit folds decoded messages one at a time (about 2 B/B); keeping
    // every message costs about 38 B/B.
    assert!(
        per <= MAX_BYTES_PER_INPUT_BYTE,
        "{per} B of heap per input byte"
    );
}

#[test]
fn an_array_heavy_fit_parses_within_a_bounded_heap_per_input_byte() {
    let held = exclusive();
    let mb: usize = std::env::var("PARSER_FUZZ_ALLOC_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut sizes = vec![2_000u32]; // ~0.5 MB
    if mb > 0 {
        sizes.push((mb * 1024 * 1024 / 256) as u32);
    }
    for messages in sizes {
        let bytes = array_fit(messages);
        let (ok, peak) = peak_during(&held, || import(&bytes));
        assert!(!ok, "a FIT without fixes must not import");
        let per = peak / bytes.len();
        eprintln!(
            "array FIT: {} bytes, peak {peak} B ({per} B/B)",
            bytes.len()
        );
        assert!(
            per <= MAX_BYTES_PER_INPUT_BYTE,
            "{} B input peaked at {peak} B: {per} B per input byte, {} MB at the {CAP_MB} MB cap",
            bytes.len(),
            per * CAP_MB
        );
    }
}
