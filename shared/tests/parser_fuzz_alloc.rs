//! Peak heap of the FIT import (`shared::fit::parse_fit` + `import_gate`, what
//! `Core.parseFit` runs) relative to the input size, measured with a counting
//! global allocator (its own test binary, so no other test shares the count).
//!
//! The shell caps an import at 32 MB (`RunImport.kt` `MAX_RUN_IMPORT_BYTES`,
//! "keeps a pathological or wrong-type file from OOMing the process"); a Rust
//! allocation failure aborts the process (no unwind, no firewall), so the
//! parser's peak per input byte times that cap is the import's worst case.
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

fn fit_crc(data: &[u8]) -> u16 {
    const T: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];
    let mut crc: u16 = 0;
    for &b in data {
        let mut tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[(b & 0xF) as usize];
        tmp = T[(crc & 0xF) as usize];
        crc = (crc >> 4) & 0x0FFF;
        crc = crc ^ tmp ^ T[((b >> 4) & 0xF) as usize];
    }
    crc
}

fn finish(data: &[u8]) -> Vec<u8> {
    let mut f = vec![14u8, 0x20];
    f.extend_from_slice(&2132u16.to_le_bytes());
    f.extend_from_slice(&(data.len() as u32).to_le_bytes());
    f.extend_from_slice(b".FIT");
    let c = fit_crc(&f);
    f.extend_from_slice(&c.to_le_bytes());
    f.extend_from_slice(data);
    let c = fit_crc(&f);
    f.extend_from_slice(&c.to_le_bytes());
    f
}

fn semi(d: f64) -> i32 {
    (d * 2_147_483_648.0 / 180.0).round() as i32
}

/// A plain 1 Hz run: `record` = timestamp, lat, lon, heart_rate (14 bytes).
fn run_fit(fixes: u32) -> Vec<u8> {
    let mut d = vec![0x40, 0, 0];
    d.extend_from_slice(&20u16.to_le_bytes());
    d.push(4);
    d.extend_from_slice(&[253, 4, 0x86, 0, 4, 0x85, 1, 4, 0x85, 3, 1, 0x02]);
    for i in 0..fixes {
        d.push(0);
        d.extend_from_slice(&(1_100_000_000 + i).to_le_bytes());
        d.extend_from_slice(&semi(47.0 + f64::from(i % 50_000) * 3.0 / 111_195.0).to_le_bytes());
        d.extend_from_slice(&semi(8.0).to_le_bytes());
        d.push(140);
    }
    finish(&d)
}

/// The same size spent on what a device writes besides positions: an unknown
/// message (global 0xFF00) carrying one 255-byte uint8 array per message, the
/// worst case for decoded size per wire byte.
fn array_fit(messages: u32) -> Vec<u8> {
    let mut d = vec![0x40, 0, 0];
    d.extend_from_slice(&0xFF00u16.to_le_bytes());
    d.push(1);
    d.extend_from_slice(&[0, 255, 0x02]);
    for i in 0..messages {
        d.push(0);
        d.extend((0..255u32).map(|k| ((i + k) % 250) as u8));
    }
    finish(&d)
}

fn import(bytes: &[u8]) -> bool {
    shared::fit::parse_fit(bytes)
        .and_then(|t| shared::fit::import_gate(&t).map(|()| t))
        .is_ok()
}

/// Heap per input byte the import may use: at the 32 MB shell cap this is a
/// 512 MB native peak, beyond which a mid-range phone's process is at risk.
/// A file of messages the importer never reads (no fixes come out) needs no
/// more than its own bytes; a positioned run needs its fixes.
const MAX_BYTES_PER_INPUT_BYTE: usize = 16;

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
            "{} B input peaked at {peak} B: {per} B per input byte, {} MB at the 32 MB cap",
            bytes.len(),
            per * 32
        );
    }
}
