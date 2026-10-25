//! Seeded mutation fuzzing of the byte-level paths the shell drives over JNI:
//!
//! - the FIT import (`fit::parse_fit` + `fit::import_gate`, the body of
//!   `ffi::parse_fit_json`, serialized exactly as it does);
//! - the GPX export (`export_run_gpx` → `running::export_gpx_seg`, what
//!   `Core.exportGpx` returns);
//! - the track-figures preview (`Event` decode + `preview_track_figures`, the
//!   body of `ffi::track_figures_json`);
//! - the JSON bridge (`Bridge<Engine, JsonFfiFormat>` update/view, the bodies of
//!   `ffi::process_event` / `ffi::current_view` minus their panic firewall).
//!
//! Seeds: the real device fixtures in `shared/tests/fixtures` plus generated
//! FIT files (little/big endian, 12/14-byte headers, compressed timestamps,
//! developer fields, stop_all/start segments, several sessions/sports).
//! Mutations: bit flips, interesting-byte and extreme-integer overwrites,
//! insert/delete, chunk duplication, truncation at sampled offsets, huge
//! header sizes/counts, wrong and re-stamped CRCs.
//!
//! Invariants per input: no panic, a time bound, either a known failure kind
//! or a track whose fixes are finite and in range, a payload that survives the
//! JSON wire, and the same answer on every call.
//!
//! `PARSER_FUZZ_ITERS=<n>` sets the mutations per seed (default keeps the file
//! well under a minute in debug); `PARSER_FUZZ_SEED=<u64>` replays one case.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use super::*;
use crate::fit::{self, FitError, FitTrack};
use crux_core::bridge::{Bridge, JsonFfiFormat};

// ── Deterministic randomness ────────────────────────────────────────────────

/// SplitMix64: tiny, seedable, good enough to drive mutations reproducibly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

fn iters(default: usize) -> usize {
    env_u64("PARSER_FUZZ_ITERS").map_or(default, |n| n as usize)
}

// ── Panic capture (quiet, this thread only) ─────────────────────────────────

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Install (once) a panic hook that, on a thread currently inside [`guarded`],
/// records the panic's message + location instead of printing it; every other
/// panic goes to the previous hook unchanged, so other tests are unaffected.
fn install_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if QUIET.with(Cell::get) {
                LAST_PANIC.with(|p| *p.borrow_mut() = info.to_string());
            } else {
                prev(info);
            }
        }));
    });
}

/// Run `f`, returning its value or the captured panic text.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    install_hook();
    QUIET.with(|q| q.set(true));
    let out = catch_unwind(AssertUnwindSafe(f));
    QUIET.with(|q| q.set(false));
    out.map_err(|_| LAST_PANIC.with(|p| p.borrow().clone()))
}

// ── FIT encoding ────────────────────────────────────────────────────────────

const FIT_EPOCH: i64 = 631_065_600; // unix seconds at 1989-12-31T00:00:00Z

/// FIT CRC-16 (the SDK's nibble-table form).
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

const ENUM: u8 = 0x00;
const U8: u8 = 0x02;
const I32: u8 = 0x85;
const U32: u8 = 0x86;
const STR: u8 = 0x07;

/// Message-level FIT writer: definitions and data messages, then a header and
/// a trailing CRC from [`FitWriter::finish`].
#[derive(Default)]
struct FitWriter {
    data: Vec<u8>,
    big_endian: [bool; 16],
}

impl FitWriter {
    /// Define local type `local` as global `global` with `(num, size, base)`
    /// fields and `(num, size, dev_index)` developer fields.
    fn def(
        &mut self,
        local: u8,
        global: u16,
        big: bool,
        fields: &[(u8, u8, u8)],
        dev: &[(u8, u8, u8)],
    ) {
        let dev_flag = if dev.is_empty() { 0 } else { 0x20 };
        self.data.push(0x40 | dev_flag | local);
        self.data.push(0);
        self.data.push(u8::from(big));
        if big {
            self.data.extend_from_slice(&global.to_be_bytes());
        } else {
            self.data.extend_from_slice(&global.to_le_bytes());
        }
        self.data.push(fields.len() as u8);
        for &(n, s, b) in fields {
            self.data.extend_from_slice(&[n, s, b]);
        }
        if !dev.is_empty() {
            self.data.push(dev.len() as u8);
            for &(n, s, i) in dev {
                self.data.extend_from_slice(&[n, s, i]);
            }
        }
        self.big_endian[local as usize] = big;
    }
    fn msg(&mut self, local: u8, body: &[u8]) {
        self.data.push(local);
        self.data.extend_from_slice(body);
    }
    /// A compressed-timestamp data message (local types 0–3 only).
    fn compressed(&mut self, local: u8, offset: u8, body: &[u8]) {
        self.data.push(0x80 | (local << 5) | (offset & 0x1F));
        self.data.extend_from_slice(body);
    }
    fn u32(&self, local: u8, v: u32) -> [u8; 4] {
        if self.big_endian[local as usize] {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }
    fn i32(&self, local: u8, v: i32) -> [u8; 4] {
        if self.big_endian[local as usize] {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }
    fn finish(&self, header14: bool) -> Vec<u8> {
        let mut f = vec![if header14 { 14 } else { 12 }, 0x20];
        f.extend_from_slice(&2132u16.to_le_bytes());
        f.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        f.extend_from_slice(b".FIT");
        if header14 {
            let c = fit_crc(&f);
            f.extend_from_slice(&c.to_le_bytes());
        }
        f.extend_from_slice(&self.data);
        let crc = fit_crc(&f);
        f.extend_from_slice(&crc.to_le_bytes());
        f
    }
}

fn semi(deg: f64) -> i32 {
    (deg * 2_147_483_648.0 / 180.0).round() as i32
}

/// What a generated activity looks like.
#[derive(Clone)]
struct Spec {
    fixes: usize,
    /// Metres covered per one-second fix.
    mps: f64,
    /// FIT sports written in `session` messages (at the END, as devices do).
    sessions: Vec<u8>,
    /// A `sport` message near the start (some devices write one).
    sport_msg: Option<u8>,
    big_endian: bool,
    header14: bool,
    /// Every n-th fix uses a compressed-timestamp header.
    compressed: bool,
    /// Insert a timer stop_all/start pair every n fixes.
    stop_all_every: Option<usize>,
    /// Developer fields (by name) carried on every record as uint8 values.
    dev_fields: Vec<&'static str>,
    start_unix: i64,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            fixes: 400,
            mps: 3.0,
            sessions: vec![1],
            sport_msg: None,
            big_endian: false,
            header14: true,
            compressed: false,
            stop_all_every: None,
            dev_fields: Vec::new(),
            start_unix: 1_750_000_000,
        }
    }
}

/// Local types: 0 record (full), 1 record (no timestamp, for compressed
/// headers), 2 event, 3 session, 4 file_id, 5 developer_data_id, 6
/// field_description, 7 sport.
fn gen_fit(spec: &Spec) -> Vec<u8> {
    let mut w = FitWriter::default();
    let be = spec.big_endian;
    // file_id: type = activity (4).
    w.def(4, 0, be, &[(0, 1, ENUM)], &[]);
    w.msg(4, &[4]);
    if let Some(sport) = spec.sport_msg {
        w.def(7, 12, be, &[(0, 1, ENUM)], &[]);
        w.msg(7, &[sport]);
    }
    let dev: Vec<(u8, u8, u8)> = (0..spec.dev_fields.len())
        .map(|i| (i as u8, 1, 0))
        .collect();
    if !dev.is_empty() {
        // developer_data_id: developer_data_index (3) = 0.
        w.def(5, 207, be, &[(3, 1, U8)], &[]);
        w.msg(5, &[0]);
        // field_description: developer_data_index, field_definition_number,
        // fit_base_type_id (uint8), field_name (16-byte string).
        w.def(
            6,
            206,
            be,
            &[(0, 1, U8), (1, 1, U8), (2, 1, U8), (3, 16, STR)],
            &[],
        );
        for (i, name) in spec.dev_fields.iter().enumerate() {
            let mut body = vec![0, i as u8, U8];
            let mut s = name.as_bytes().to_vec();
            s.resize(16, 0);
            body.extend_from_slice(&s);
            w.msg(6, &body);
        }
    }
    w.def(
        0,
        20,
        be,
        &[
            (253, 4, U32),
            (0, 4, I32),
            (1, 4, I32),
            (3, 1, U8),
            (31, 1, U8),
        ],
        &dev,
    );
    w.def(1, 20, be, &[(0, 4, I32), (1, 4, I32), (3, 1, U8)], &dev);
    w.def(2, 21, be, &[(253, 4, U32), (0, 1, ENUM), (1, 1, ENUM)], &[]);
    let (lat0, lon0) = (47.37, 8.54);
    let deg_per_m = 1.0 / 111_195.0;
    let event = |w: &mut FitWriter, t: u32, kind: u8| {
        let mut body = w.u32(2, t).to_vec();
        body.extend_from_slice(&[0, kind]); // event = timer, event_type
        w.msg(2, &body);
    };
    let t0 = (spec.start_unix - FIT_EPOCH) as u32;
    event(&mut w, t0, 0); // start
    for i in 0..spec.fixes {
        let t = t0 + i as u32;
        if let Some(n) = spec.stop_all_every
            && i > 0
            && i % n == 0
        {
            event(&mut w, t, 4); // stop_all
            event(&mut w, t, 0); // start
        }
        let lat = semi(lat0 + spec.mps * i as f64 * deg_per_m);
        let lon = semi(lon0 + (i as f64 * 0.37).sin() * 2.0 * deg_per_m);
        let hr = 120 + (i % 40) as u8;
        let devs: Vec<u8> = (0..spec.dev_fields.len())
            .map(|k| 100 + 50 * k as u8)
            .collect();
        if spec.compressed && i % 4 != 0 {
            let mut body = w.i32(1, lat).to_vec();
            body.extend_from_slice(&w.i32(1, lon));
            body.push(hr);
            body.extend_from_slice(&devs);
            w.compressed(1, (t & 0x1F) as u8, &body);
        } else {
            let mut body = w.u32(0, t).to_vec();
            body.extend_from_slice(&w.i32(0, lat));
            body.extend_from_slice(&w.i32(0, lon));
            body.push(hr);
            body.push(5);
            body.extend_from_slice(&devs);
            w.msg(0, &body);
        }
    }
    let t_end = t0 + spec.fixes as u32;
    event(&mut w, t_end, 4); // stop_all
    w.def(3, 18, be, &[(253, 4, U32), (5, 1, ENUM), (6, 1, ENUM)], &[]);
    for &sport in &spec.sessions {
        let mut body = w.u32(3, t_end).to_vec();
        body.extend_from_slice(&[sport, 0]);
        w.msg(3, &body);
    }
    w.finish(spec.header14)
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"))
}

/// Every seed the FIT fuzzers mutate, named for failure messages.
fn fit_seeds() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("Activity.fit", fixture("Activity.fit")),
        (
            "garmin-fenix-5-bike.fit",
            fixture("garmin-fenix-5-bike.fit"),
        ),
        ("gen run", gen_fit(&Spec::default())),
        (
            "gen big-endian 12-byte header compressed",
            gen_fit(&Spec {
                fixes: 240,
                big_endian: true,
                header14: false,
                compressed: true,
                ..Spec::default()
            }),
        ),
        (
            "gen segmented + dev field",
            gen_fit(&Spec {
                fixes: 300,
                stop_all_every: Some(100),
                dev_fields: vec!["cadence_x"],
                ..Spec::default()
            }),
        ),
        (
            "gen brick (run + cycling sessions)",
            gen_fit(&Spec {
                fixes: 300,
                sessions: vec![1, 2],
                sport_msg: Some(1),
                ..Spec::default()
            }),
        ),
    ]
}

// ── The parse_fit_json body and its invariants ──────────────────────────────

/// What `ffi::parse_fit_json` decides for some bytes, before serialization.
#[derive(Debug, Clone, PartialEq)]
enum FitOutcome {
    Track(FitTrack),
    Fail(FitError),
}

fn decide(bytes: &[u8]) -> FitOutcome {
    match fit::parse_fit(bytes).and_then(|t| fit::import_gate(&t).map(|()| t)) {
        Ok(t) => FitOutcome::Track(t),
        Err(e) => FitOutcome::Fail(e),
    }
}

/// The wire payload `parse_fit_json` would return for `out` (same shapes).
fn payload(out: &FitOutcome) -> serde_json::Value {
    match out {
        FitOutcome::Track(t) => serde_json::to_value(t).expect("a track serializes"),
        FitOutcome::Fail(e) => serde_json::json!({
            "error": {"kind": e.kind(), "context": e.context(), "message": e.message()}
        }),
    }
}

/// Per-input time bound (debug build). A real 4.6 KB file parses in ~1 ms.
const TIME_BOUND: Duration = Duration::from_secs(3);

/// Check one input: no panic, bounded time, sane result, deterministic.
/// `Err` carries a description of the violated invariant.
fn check_fit(bytes: &[u8]) -> Result<FitOutcome, String> {
    let start = Instant::now();
    let first = guarded(|| decide(bytes)).map_err(|p| format!("PANIC: {p}"))?;
    let took = start.elapsed();
    if took > TIME_BOUND {
        return Err(format!("took {took:?} for {} bytes", bytes.len()));
    }
    match &first {
        FitOutcome::Track(t) => {
            let total: usize = t.segments.iter().map(Vec::len).sum();
            if total < 2 || t.segments.iter().any(Vec::is_empty) {
                return Err(format!("track with an empty segment or < 2 fixes: {t:?}"));
            }
            // Every fix is one positioned `record`, each at least 9 bytes.
            if total > bytes.len() / 9 + 1 {
                return Err(format!("{total} fixes from {} bytes", bytes.len()));
            }
            for f in t.segments.iter().flatten() {
                let sane = f.lat.is_finite()
                    && f.lon.is_finite()
                    && f.lat.abs() <= 180.0
                    && f.lon.abs() <= 180.0
                    && (FIT_EPOCH..=FIT_EPOCH + i64::from(u32::MAX)).contains(&f.time_sec)
                    && f.hr_bpm.is_none_or(|h| h < 255)
                    && f.accuracy_m
                        .is_none_or(|a| a.is_finite() && (0.0..255.0).contains(&a));
                if !sane {
                    return Err(format!("insane fix {f:?}"));
                }
            }
        }
        FitOutcome::Fail(e) => {
            let ok_ctx = match e {
                FitError::NotARun(s) => !s.is_empty() && !s.contains('_'),
                FitError::TooShort => e.context() == "too_short",
                _ => e.context().is_empty(),
            };
            if !ok_ctx || e.message().is_empty() {
                return Err(format!("bad error envelope {e:?}"));
            }
        }
    }
    // The wire: what the shell decodes must come back bit for bit (serde_json's
    // `float_roundtrip` parser is correctly rounded, as is the shell's kotlinx
    // parser; see `a_logged_track_survives_the_event_wire_bit_for_bit`).
    let wire = serde_json::to_string(&payload(&first)).map_err(|e| e.to_string())?;
    let back: serde_json::Value = serde_json::from_str(&wire).map_err(|e| e.to_string())?;
    if back != payload(&first) {
        return Err(format!("payload does not round-trip: {wire}"));
    }
    if let FitOutcome::Track(t) = &first {
        let segs = back["segments"].as_array().map(Vec::len);
        if segs != Some(t.segments.len()) {
            return Err(format!("segments lost on the wire: {wire}"));
        }
    }
    // Same input, same answer.
    let again = guarded(|| decide(bytes)).map_err(|p| format!("PANIC on 2nd call: {p}"))?;
    if again != first {
        return Err(format!("nondeterministic: {first:?} then {again:?}"));
    }
    Ok(first)
}

// ── Mutators ────────────────────────────────────────────────────────────────

const INTERESTING: [u8; 12] = [
    0x00, 0x01, 0x0C, 0x0E, 0x20, 0x40, 0x7F, 0x80, 0x9F, 0xE0, 0xFE, 0xFF,
];

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let len = b.len().max(1);
        match rng.below(10) {
            0 => {
                let i = rng.below(len);
                if let Some(x) = b.get_mut(i) {
                    *x ^= 1 << rng.below(8);
                }
            }
            1 => {
                let i = rng.below(len);
                if let Some(x) = b.get_mut(i) {
                    *x = INTERESTING[rng.below(INTERESTING.len())];
                }
            }
            2 => {
                let i = rng.below(len + 1).min(b.len());
                let n = 1 + rng.below(8);
                let ins: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                b.splice(i..i, ins);
            }
            3 => {
                let i = rng.below(len).min(b.len());
                let n = (1 + rng.below(16)).min(b.len() - i);
                b.drain(i..i + n);
            }
            4 if !b.is_empty() => {
                let i = rng.below(b.len());
                let n = (1 + rng.below(64)).min(b.len() - i);
                let chunk = b[i..i + n].to_vec();
                let j = rng.below(b.len() + 1);
                b.splice(j..j, chunk);
            }
            5 => {
                let cut = rng.below(len + 1).min(b.len());
                b.truncate(cut);
            }
            6 if b.len() >= 4 => {
                let i = rng.below(b.len() - 3);
                let v: u32 = [0, u32::MAX, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF][rng.below(5)];
                b[i..i + 4].copy_from_slice(&v.to_le_bytes());
            }
            7 if b.len() >= 12 => {
                // Header data_size: zero, huge, or off by a little.
                let v: u32 = [0, u32::MAX, (b.len() as u32).wrapping_mul(3), 1][rng.below(4)];
                b[4..8].copy_from_slice(&v.to_le_bytes());
            }
            8 if !b.is_empty() => {
                let (i, j) = (rng.below(b.len()), rng.below(b.len()));
                b.swap(i, j);
            }
            _ => {
                // Field definition count / size bytes in a definition message:
                // any byte after a 0x40-ish header becomes huge.
                if let Some(i) = b.iter().position(|&x| x & 0xF0 == 0x40) {
                    let k = (i + 5 + rng.below(8)).min(b.len().saturating_sub(1));
                    if let Some(x) = b.get_mut(k) {
                        *x = 0xFF;
                    }
                }
            }
        }
    }
    // Half the time re-stamp a valid trailing CRC (and header CRC), so the
    // mutation is judged on its content, not stopped at the checksum.
    if rng.chance(50) && b.len() >= 16 && matches!(b[0], 12 | 14) {
        if b[0] == 14 {
            let c = fit_crc(&b[..12]);
            b[12..14].copy_from_slice(&c.to_le_bytes());
        }
        let n = b.len() - 2;
        let c = fit_crc(&b[..n]);
        b[n..].copy_from_slice(&c.to_le_bytes());
    }
    b
}

fn hex_head(b: &[u8]) -> String {
    b.iter().take(48).map(|x| format!("{x:02x}")).collect()
}

// ── FIT tests ───────────────────────────────────────────────────────────────

#[test]
fn every_seed_parses_to_its_known_outcome() {
    let kinds: Vec<(&str, String)> = fit_seeds()
        .into_iter()
        .map(|(name, bytes)| {
            let out = check_fit(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let kind = match &out {
                FitOutcome::Track(t) => format!("track {} segs", t.segments.len()),
                FitOutcome::Fail(e) => format!("{} {}", e.kind(), e.context()),
            };
            (name, kind)
        })
        .collect();
    let expected = [
        ("Activity.fit", "no_track too_short"),
        ("garmin-fenix-5-bike.fit", "not_a_run cycling"),
        ("gen run", "track 1 segs"),
        ("gen big-endian 12-byte header compressed", "track 1 segs"),
        ("gen segmented + dev field", "track 3 segs"),
        ("gen brick (run + cycling sessions)", "not_a_run cycling"),
    ];
    assert_eq!(
        kinds,
        expected.map(|(a, b)| (a, b.to_string())).to_vec(),
        "seed outcomes"
    );
}

#[test]
fn generated_fit_keeps_every_fix_time_and_position() {
    // Compressed timestamps rebuild the full clock: every fix one second apart.
    for spec in [
        Spec::default(),
        Spec {
            compressed: true,
            ..Spec::default()
        },
        Spec {
            compressed: true,
            big_endian: true,
            header14: false,
            ..Spec::default()
        },
    ] {
        let FitOutcome::Track(t) = decide(&gen_fit(&spec)) else {
            panic!("generated run should import");
        };
        let fixes: Vec<_> = t.segments.concat();
        assert_eq!(fixes.len(), spec.fixes);
        for (i, f) in fixes.iter().enumerate() {
            assert_eq!(f.time_sec, spec.start_unix + i as i64, "fix {i} {f:?}");
            let lat = 47.37 + spec.mps * i as f64 / 111_195.0;
            assert!((f.lat - lat).abs() < 1e-6, "fix {i} lat {} vs {lat}", f.lat);
            assert_eq!(f.hr_bpm, Some(120 + (i % 40) as u16));
        }
    }
}

#[test]
fn fit_mutation_fuzz_never_panics_and_yields_sane_outcomes() {
    let per_seed = iters(450);
    let only = env_u64("PARSER_FUZZ_SEED");
    let mut tracks = 0usize;
    for (si, (name, seed)) in fit_seeds().into_iter().enumerate() {
        for i in 0..per_seed {
            let case = 0xF17_0000_0000 + ((si as u64) << 24) + i as u64;
            if only.is_some_and(|s| s != case) {
                continue;
            }
            let mut rng = Rng(case);
            let bytes = mutate(&mut rng, &seed);
            match check_fit(&bytes) {
                Ok(FitOutcome::Track(_)) => tracks += 1,
                Ok(FitOutcome::Fail(_)) => {}
                Err(e) => panic!(
                    "seed {name}, case {case} (PARSER_FUZZ_SEED={case}), {} bytes {}…: {e}",
                    bytes.len(),
                    hex_head(&bytes)
                ),
            }
        }
    }
    eprintln!("fit fuzz: {tracks} tracks");
    if only.is_none() {
        assert!(
            tracks > 0,
            "the fuzzer should still reach importable tracks"
        );
    }
}

#[test]
fn truncation_at_every_sampled_offset_only_ever_drops_fixes() {
    // A cut-off file never invents a fix: its fixes are a prefix of the whole
    // file's fixes (the salvage decoder keeps what decoded before the cut).
    for (name, seed) in fit_seeds() {
        let whole = crate::fit::parse_fit(&seed)
            .ok()
            .map(|t| t.segments.concat());
        let step = (seed.len() / 120).max(1);
        for cut in (0..seed.len()).step_by(step) {
            let part = &seed[..cut];
            let out = check_fit(part).unwrap_or_else(|e| panic!("{name} cut at {cut}: {e}"));
            if cut < 12 {
                assert_eq!(
                    out,
                    FitOutcome::Fail(FitError::NotFit),
                    "{name} cut at {cut}"
                );
            }
            if let (Ok(t), Some(whole)) = (crate::fit::parse_fit(part), &whole) {
                let got = t.segments.concat();
                assert!(
                    got.len() <= whole.len() && got[..] == whole[..got.len()],
                    "{name} cut at {cut}: {} fixes not a prefix of the whole file's",
                    got.len()
                );
            }
        }
    }
}

#[test]
fn hostile_headers_and_counts_are_refused_quickly() {
    let good = gen_fit(&Spec {
        fixes: 50,
        ..Spec::default()
    });
    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    for hs in [0u8, 1, 11, 13, 15, 0x7F, 0xFF] {
        let mut b = good.clone();
        b[0] = hs;
        cases.push((format!("header_size {hs}"), b));
    }
    for ds in [0u32, 1, u32::MAX, 0x7FFF_FFFF] {
        let mut b = good.clone();
        b[4..8].copy_from_slice(&ds.to_le_bytes());
        cases.push((format!("data_size {ds}"), b));
    }
    // Wrong trailing CRC, wrong header CRC, magic damaged.
    let mut b = good.clone();
    let n = b.len();
    b[n - 1] ^= 0xFF;
    cases.push(("wrong data CRC".into(), b));
    let mut b = good.clone();
    b[12] ^= 0xFF;
    cases.push(("wrong header CRC".into(), b));
    let mut b = good.clone();
    b[9] = b'f';
    cases.push(("magic .fIT".into(), b));
    // A definition with 255 fields of 255 bytes, and with 255 developer fields.
    let mut w = FitWriter::default();
    let many: Vec<(u8, u8, u8)> = (0..255).map(|i| (i as u8, 255, 0x0D)).collect();
    w.def(0, 20, false, &many, &[]);
    w.msg(0, &vec![0xAB; 255 * 255]);
    cases.push(("255×255-byte fields".into(), w.finish(true)));
    let mut w = FitWriter::default();
    w.def(0, 20, false, &[(0, 4, I32)], &[(0, 255, 0); 255]);
    w.msg(0, &[0; 4]);
    cases.push(("255 undescribed developer fields".into(), w.finish(true)));
    // Data messages referencing undefined local types; a definition-only file.
    let mut w = FitWriter::default();
    for l in 0..16 {
        w.msg(l, &[0; 8]);
    }
    cases.push(("undefined local types".into(), w.finish(true)));
    let mut w = FitWriter::default();
    for l in 0..16 {
        w.def(l, 20, false, &[(0, 4, I32)], &[]);
    }
    cases.push(("definitions only".into(), w.finish(false)));
    // 10 000 chained empty sub-files (each a valid header + CRC).
    let mut chain = Vec::new();
    for _ in 0..10_000 {
        chain.extend_from_slice(&FitWriter::default().finish(true));
    }
    cases.push(("10k chained empty sub-files".into(), chain));

    for (what, bytes) in cases {
        let out = check_fit(&bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
        if let FitOutcome::Track(t) = &out {
            let n: usize = t.segments.iter().map(Vec::len).sum();
            assert!(n <= 50, "{what}: {n} fixes from a 50-fix file");
        }
    }
}

#[test]
fn timestamps_out_of_order_duplicated_and_far_future_stay_in_range() {
    // Fix times are whatever the file says (QC is the core's job later), but
    // they must stay inside the FIT clock's range and never break the gate.
    let mut w = FitWriter::default();
    w.def(
        0,
        20,
        false,
        &[(253, 4, U32), (0, 4, I32), (1, 4, I32)],
        &[],
    );
    let times: [u32; 8] = [1000, 1000, 900, u32::MAX - 1, 5, 0x0FFF_FFFF, 2000, 2000];
    for (i, &t) in times.iter().enumerate() {
        let mut body = t.to_le_bytes().to_vec();
        body.extend_from_slice(&semi(10.0 + i as f64 * 0.01).to_le_bytes());
        body.extend_from_slice(&semi(20.0).to_le_bytes());
        w.msg(0, &body);
    }
    let bytes = w.finish(true);
    let out = check_fit(&bytes).expect("sane");
    let FitOutcome::Track(t) = out else {
        panic!("8 fixes spanning ~8 km and a far-future clock import: {out:?}")
    };
    let got: Vec<i64> = t.segments.concat().iter().map(|f| f.time_sec).collect();
    assert_eq!(got, times.map(|t| FIT_EPOCH + i64::from(t)).to_vec());
}

#[test]
fn a_developer_field_named_like_a_native_field_does_not_override_it() {
    // A Connect IQ app may name its own field anything, e.g. "heart_rate". The
    // native heart_rate (120–159 here) is the device's reading; the developer
    // value (100, 150, …) is the app's private data and must not replace it.
    let bytes = gen_fit(&Spec {
        fixes: 300,
        dev_fields: vec!["heart_rate"],
        ..Spec::default()
    });
    let FitOutcome::Track(t) = decide(&bytes) else {
        panic!("should import")
    };
    let hrs: Vec<Option<u16>> = t.segments.concat().iter().map(|f| f.hr_bpm).collect();
    let native: Vec<Option<u16>> = (0..300).map(|i| Some(120 + (i % 40) as u16)).collect();
    assert_eq!(
        hrs[..10],
        native[..10],
        "native HR, not the developer field's 100"
    );
}

#[test]
fn two_developer_fields_with_one_name_give_the_same_answer_every_call() {
    // Two developer fields both named "heart_rate" (values 100 and 150) on
    // every record: which one parse_fit reports depends on fitparser's
    // developer-field HashMap iteration order, which is randomized per map.
    let bytes = gen_fit(&Spec {
        fixes: 300,
        dev_fields: vec!["heart_rate", "heart_rate"],
        ..Spec::default()
    });
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let FitOutcome::Track(t) = decide(&bytes) else {
            panic!("should import")
        };
        seen.insert(t.segments[0][0].hr_bpm);
    }
    assert_eq!(seen.len(), 1, "one answer for one file, got {seen:?}");
}

#[test]
fn a_developer_field_whose_size_is_not_a_multiple_of_its_type_keeps_the_rest_of_the_file() {
    // field_description declares a uint16 developer field; one record's
    // definition gives it 255 bytes (odd). fitparser checks size agreement for
    // native fields only, so its u8 `bytes_consumed += 2` overflows: a panic in
    // a debug build; in the shipped release build it wraps and the decoder eats
    // the rest of the file, so the 600 good fixes after that record vanish
    // without a Damaged report (`cargo run --release` probe: 300 of 900).
    let mut w = FitWriter::default();
    w.def(
        6,
        206,
        false,
        &[(0, 1, U8), (1, 1, U8), (2, 1, U8), (3, 8, STR)],
        &[],
    );
    let mut body = vec![0, 0, 0x84];
    body.extend_from_slice(b"power\0\0\0");
    w.msg(6, &body);
    w.def(
        1,
        20,
        false,
        &[(253, 4, U32), (0, 4, I32), (1, 4, I32)],
        &[],
    );
    w.def(
        0,
        20,
        false,
        &[(253, 4, U32), (0, 4, I32), (1, 4, I32)],
        &[(0, 255, 0)],
    );
    let rec = |w: &mut FitWriter, local: u8, i: u32| {
        let mut body = (1_100_000_000 + i).to_le_bytes().to_vec();
        body.extend_from_slice(&semi(47.0 + f64::from(i) * 3.0 / 111_195.0).to_le_bytes());
        body.extend_from_slice(&semi(8.0).to_le_bytes());
        if local == 0 {
            body.extend_from_slice(&[0u8; 255]);
        }
        w.msg(local, &body);
    };
    for i in 0..300 {
        rec(&mut w, 1, i);
    }
    rec(&mut w, 0, 300);
    for i in 301..901 {
        rec(&mut w, 1, i);
    }
    let bytes = w.finish(true);
    let out =
        guarded(|| fit::parse_fit(&bytes)).unwrap_or_else(|p| panic!("parse_fit panicked: {p}"));
    let fixes = out.map_or(0, |t| t.segments.concat().len());
    assert!(
        fixes >= 900,
        "{fixes} of the file's 900 well-formed fixes survive"
    );
}

// ── GPX export ──────────────────────────────────────────────────────────────

/// Minimal XML well-formedness for the export's own subset: balanced tags,
/// quoted attributes, no raw `<`/`&` in text, no XML 1.0-illegal characters.
fn xml_well_formed(doc: &str) -> Result<(), String> {
    if doc
        .chars()
        .any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err("control character".into());
    }
    let body = doc
        .strip_prefix("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
        .ok_or("no XML declaration")?;
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = body;
    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];
        if text.contains('>') {
            return Err(format!("stray > in {text:?}"));
        }
        for (i, _) in text.match_indices('&') {
            let ent = &text[i..];
            if !["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"]
                .iter()
                .any(|e| ent.starts_with(e))
            {
                return Err(format!("bare & in {text:?}"));
            }
        }
        let gt = rest[lt..].find('>').ok_or("unclosed tag")? + lt;
        let tag = &rest[lt + 1..gt];
        if let Some(close) = tag.strip_prefix('/') {
            if stack.pop() != Some(close) {
                return Err(format!("mismatched </{close}>"));
            }
        } else {
            let name = tag.split([' ', '/']).next().unwrap_or("");
            if tag.matches('"').count() % 2 != 0 {
                return Err(format!("unbalanced quotes in <{tag}>"));
            }
            if !tag.ends_with('/') {
                stack.push(name);
            }
        }
        rest = &rest[gt + 1..];
    }
    if !stack.is_empty() {
        return Err(format!("unclosed {stack:?}"));
    }
    Ok(())
}

/// Every `<trkpt>` lat/lon an xsd:decimal (no NaN/inf/exponent) in range, every
/// `<time>` a UTC stamp the shell importer (java.time `Instant.parse`) reads: a
/// four-digit year, or past 9999 the ISO 8601 expanded year with its `+` sign.
fn gpx_values_sane(doc: &str) -> Result<usize, String> {
    let mut n = 0;
    for pt in doc.split("<trkpt ").skip(1) {
        for key in ["lat=\"", "lon=\""] {
            let s = pt.find(key).ok_or("missing attribute")? + key.len();
            let v = &pt[s..s + pt[s..].find('"').ok_or("unterminated")?];
            let decimal = !v.is_empty()
                && v.trim_start_matches('-')
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.');
            let x: f64 = v.parse().map_err(|_| format!("{key}{v}"))?;
            if !decimal || !x.is_finite() {
                return Err(format!("{key}{v} is not an xsd:decimal"));
            }
        }
        let s = pt.find("<time>").ok_or("missing time")? + 6;
        let t = &pt[s..s + pt[s..].find('<').ok_or("unterminated time")?];
        let (year, rest) = t.split_at(t.get(1..).and_then(|r| r.find('-')).map_or(0, |i| i + 1));
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        let year_ok = (year.len() == 4 && digits(year))
            || (year.len() >= 6 && year.starts_with('+') && digits(&year[1..]));
        let ok = year_ok && rest.len() == 16 && rest.starts_with('-') && rest.ends_with('Z');
        if !ok {
            return Err(format!(
                "<time>{t}</time> is not a UTC stamp java.time reads"
            ));
        }
        n += 1;
    }
    Ok(n)
}

fn pt(lat: f64, lon: f64, t: i64, acc: f32) -> GpsPoint {
    GpsPoint {
        lat,
        lon,
        observed_at: t,
        accuracy_m: acc,
    }
}

/// A plausible 10-minute run at 3 m/s, 1 Hz.
fn walk(n: usize, t0: i64) -> Vec<GpsPoint> {
    (0..n)
        .map(|i| pt(47.37 + 3.0 * i as f64 / 111_195.0, 8.54, t0 + i as i64, 5.0))
        .collect()
}

/// Log `points` as a GPS run in a fresh model, view it (fills the export
/// registry) and return its GPX document.
fn export_logged(points: Vec<GpsPoint>, segment_starts: Vec<u32>, observed_at: i64) -> String {
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(
        log_run_track_seg(points, 0.0, 0.0, observed_at, 4242, segment_starts),
        &mut model,
    );
    let vm = app.view(&model);
    vm.runs
        .first()
        .map(|r| export_run_gpx(r.entry_id, r.observed_at))
        .unwrap_or_default()
}

#[test]
fn gpx_export_of_adversarial_tracks_is_well_formed_with_sane_values() {
    let mut rng = Rng(0x6E_0000);
    let weird_f64 = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e308,
        -1e308,
        1e12,
        5e-324,
        -0.0,
        91.0,
        181.0,
        1e-7,
    ];
    let weird_acc = [f32::NAN, f32::INFINITY, -1.0, 0.0, 1e30, 1e-30, 30.0];
    for case in 0..iters(300) {
        let mut pts = walk(120 + rng.below(200), 1_750_000_000);
        for _ in 0..rng.below(6) {
            let i = rng.below(pts.len());
            match rng.below(4) {
                0 => pts[i].lat = weird_f64[rng.below(weird_f64.len())],
                1 => pts[i].lon = weird_f64[rng.below(weird_f64.len())],
                2 => pts[i].accuracy_m = weird_acc[rng.below(weird_acc.len())],
                _ => {
                    // Duplicated / backwards / far-apart times.
                    let j = rng.below(pts.len());
                    pts[i].observed_at = pts[j].observed_at - rng.below(3) as i64;
                }
            }
        }
        let starts: Vec<u32> = (0..rng.below(4))
            .map(|_| {
                [
                    0,
                    1,
                    rng.below(pts.len()) as u32,
                    pts.len() as u32,
                    u32::MAX,
                ][rng.below(5)]
            })
            .collect();
        let doc = export_logged(pts.clone(), starts.clone(), 1_750_000_400);
        if doc.is_empty() {
            continue;
        }
        xml_well_formed(&doc).unwrap_or_else(|e| panic!("case {case}: {e}\n{doc}"));
        let n = gpx_values_sane(&doc).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert!(
            n <= pts.len(),
            "case {case}: {n} exported fixes from {}",
            pts.len()
        );
        assert_eq!(
            doc,
            export_logged(pts, starts, 1_750_000_400),
            "case {case}: same run, same document"
        );
    }
}

#[test]
fn gpx_export_track_name_with_xml_specials_stays_well_formed() {
    let doc = crate::running::export_gpx(&walk(5, 1_750_000_000), "<\"a&b'> ]]> <!--");
    xml_well_formed(&doc).unwrap();
    assert!(
        doc.contains("<name>&lt;&quot;a&amp;b&apos;&gt; ]]&gt; &lt;!--</name>"),
        "{doc}"
    );
}

#[test]
fn a_far_future_fix_the_importer_accepted_exports_a_time_it_can_read_back() {
    // Gpx.kt's parseImportTimeSec reads "+10000-01-01T00:00:00Z" (java.time
    // ISO_INSTANT needs the sign past four digits), so such a run can be logged;
    // its export must name a time the same importer reads.
    let t0 = 253_402_300_800; // 10000-01-01T00:00:00Z
    let doc = export_logged(walk(300, t0), Vec::new(), t0 + 300);
    assert!(!doc.is_empty(), "the run is logged and exportable");
    gpx_values_sane(&doc).unwrap();
}

/// A FIT-like 1 Hz track: coordinates are semicircle conversions, the values
/// the FIT importer and a phone GNSS hand the core.
fn semicircle_track(n: usize, t0: i64) -> Vec<GpsPoint> {
    let deg = |s: i32| f64::from(s) * (180.0 / 2_147_483_648.0);
    (0..n)
        .map(|i| {
            let lat = deg(semi(47.37) + 1737 * i as i32);
            let lon = deg(semi(8.54) + (i as f64 * 0.3).sin().mul_add(800.0, 0.0) as i32);
            pt(lat, lon, t0 + i as i64, 4.0)
        })
        .collect()
}

#[test]
fn a_logged_track_survives_the_event_wire_bit_for_bit() {
    // running.rs promises the export's shortest-decimal coordinates rebuild the
    // route bit for bit (Undo, re-import). The shell re-sends those digits as
    // JSON numbers (kotlinx writes the shortest decimal; its parser is exact),
    // so the core's Event decode must read them back to the same f64.
    let pts = semicircle_track(400, 1_750_000_000);
    let app = Engine;
    let mut model = Model::default();
    let _ = app.update(log_run_track(pts, 0.0, 0.0, 1_750_000_400, 77), &mut model);
    let _ = app.view(&model);
    let doc = export_run_gpx(77, 1_750_000_400);
    // The exact value each exported decimal denotes (Rust's `str::parse` and
    // kotlinx are correctly rounded), and what the core's decode makes of it.
    let mut exact = Vec::new();
    let mut json_points = Vec::new();
    for p in doc.split("<trkpt ").skip(1) {
        let attr = |k: &str| {
            let s = p.find(k).unwrap() + k.len();
            p[s..s + p[s..].find('"').unwrap()].to_string()
        };
        let (lat, lon) = (attr("lat=\""), attr("lon=\""));
        exact.push((lat.parse::<f64>().unwrap(), lon.parse::<f64>().unwrap()));
        json_points.push(format!(
            r#"{{"lat":{lat},"lon":{lon},"observed_at":0,"accuracy_m":4.0}}"#
        ));
    }
    let wire = format!(
        r#"{{"LogRunTrack":{{"points":[{}],"hr_pct_max":0.0,"longest_recent_km":0.0}}}}"#,
        json_points.join(",")
    );
    let Ok(Event::LogRunTrack { points, .. }) = serde_json::from_str::<Event>(&wire) else {
        panic!("decodes")
    };
    assert!(exact.len() >= 300, "{} exported fixes", exact.len());
    // The export itself is lossless against the stored (pre-QC) track.
    let stored: std::collections::HashSet<(u64, u64)> = model.runs[0]
        .track
        .iter()
        .map(|p| (p.lat.to_bits(), p.lon.to_bits()))
        .collect();
    assert!(
        exact
            .iter()
            .all(|(a, b)| stored.contains(&(a.to_bits(), b.to_bits())))
    );
    let moved: Vec<(f64, f64)> = exact
        .iter()
        .zip(&points)
        .flat_map(|(a, b)| [(a.0, b.lat), (a.1, b.lon)])
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .collect();
    assert!(
        moved.is_empty(),
        "{} of {} coordinates came back changed, e.g. {:?}",
        moved.len(),
        2 * exact.len(),
        moved.first()
    );
}

// ── trackFigures body ───────────────────────────────────────────────────────

/// `ffi::track_figures_json`'s body: decode the event, preview the figures.
fn track_figures(bytes: &[u8]) -> Result<(f64, f64), String> {
    match serde_json::from_slice::<Event>(bytes) {
        Ok(Event::LogRunTrack {
            points,
            segment_starts,
            ..
        }) => Ok(preview_track_figures(points, &segment_starts)),
        Ok(_) => Err("not a LogRunTrack".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn track_json(points: &[GpsPoint], starts: &[u32]) -> String {
    serde_json::json!({"LogRunTrack": {
        "points": points, "segment_starts": starts,
        "hr_pct_max": 0.0, "longest_recent_km": 0.0, "observed_at": 1, "entry_id": 1
    }})
    .to_string()
}

#[test]
fn track_figures_of_hostile_points_are_finite_and_bounded() {
    let mut rng = Rng(0x7F_0000);
    let weird = [1e308, -1e308, 1e12, 90.0, -90.0, 180.0, 1e-300, 0.0];
    for case in 0..iters(400) {
        let mut pts = walk(60 + rng.below(300), 1_750_000_000);
        for _ in 0..rng.below(8) {
            let i = rng.below(pts.len());
            match rng.below(4) {
                0 => pts[i].lat = weird[rng.below(weird.len())],
                1 => pts[i].lon = weird[rng.below(weird.len())],
                2 => pts[i].observed_at = [i64::MIN, i64::MAX, 0, -1, 1 << 40][rng.below(5)],
                _ => pts[i].accuracy_m = [f32::MAX, -f32::MAX, 0.0, 29.9, 30.1][rng.below(5)],
            }
        }
        let starts: Vec<u32> = (0..rng.below(5))
            .map(|_| rng.next() as u32 % (pts.len() as u32 + 2))
            .collect();
        let json = track_json(&pts, &starts);
        let (km, min) = guarded(|| track_figures(json.as_bytes()))
            .unwrap_or_else(|p| panic!("case {case} panicked: {p}"))
            .unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert!(
            km.is_finite() && km >= 0.0 && min.is_finite() && min >= 0.0,
            "case {case}: {km} km {min} min"
        );
        // A QC'd leg is at most half the Earth's circumference.
        assert!(km <= pts.len() as f64 * 20_016.0, "case {case}: {km} km");
    }
}

#[test]
fn track_figures_refuses_malformed_json_without_panicking() {
    let good = track_json(&walk(30, 1_750_000_000), &[]);
    let mut inputs: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"null".to_vec(),
        b"\"ClearRuns\"".to_vec(),
        b"{\"LogRunTrack\":{}}".to_vec(),
        b"{\"LogRunTrack\":{\"points\":[{\"lat\":NaN,\"lon\":0,\"observed_at\":0,\"accuracy_m\":1}]}}".to_vec(),
        b"{\"LogRunTrack\":{\"points\":[{\"lat\":1e400,\"lon\":0,\"observed_at\":0,\"accuracy_m\":1}]}}".to_vec(),
        format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes(),
        format!("{}0{}", "{\"a\":".repeat(50_000), "}".repeat(50_000)).into_bytes(),
        vec![0xFF, 0xFE, 0x00],
        good.as_bytes()[..good.len() / 2].to_vec(),
    ];
    let mut rng = Rng(0x7F_1000);
    for _ in 0..iters(400) {
        inputs.push(mutate(&mut rng, good.as_bytes()));
    }
    for (i, inp) in inputs.iter().enumerate() {
        let r =
            guarded(|| track_figures(inp)).unwrap_or_else(|p| panic!("input {i} panicked: {p}"));
        if let Ok((km, min)) = r {
            assert!(
                km.is_finite() && min.is_finite() && km >= 0.0 && min >= 0.0,
                "input {i}"
            );
        }
    }
}

// ── The JSON bridge (update / view) ─────────────────────────────────────────

fn seed_events() -> Vec<String> {
    vec![
        r#"{"SetToday":{"epoch_day":20300,"utc_offset_sec":3600}}"#.into(),
        track_json(&walk(400, 1_753_920_000), &[200]),
        r#"{"LogRun":{"distance_km":10.0,"duration_min":55.0,"hr_pct_max":78.0,"longest_recent_km":0.0,"observed_at":1753900000,"entry_id":7}}"#.into(),
        r#""ClearRuns""#.into(),
        r#"{"DeleteEntry":{"kind":"Run","entry_id":1,"observed_at_fallback":0}}"#.into(),
    ]
}

#[test]
fn bridge_update_and_view_survive_malformed_events_and_keep_state_on_rejection() {
    let mut bridge: Bridge<Engine, JsonFfiFormat> = Bridge::new(crux_core::Core::default());
    let view = |b: &mut Bridge<Engine, JsonFfiFormat>| {
        let mut out = Vec::new();
        guarded(|| b.view(&mut out))
            .expect("view never panics")
            .expect("view serializes");
        out
    };
    let seeds = seed_events();
    for s in &seeds {
        let mut out = Vec::new();
        assert!(
            bridge.update(s.as_bytes(), &mut out).is_ok(),
            "seed event accepted: {s}"
        );
    }
    let mut rng = Rng(0xB0_0000);
    let mut rejected = 0;
    for case in 0..iters(600) {
        let seed = &seeds[rng.below(seeds.len())];
        let bytes = mutate(&mut rng, seed.as_bytes());
        let before = view(&mut bridge);
        let mut out = Vec::new();
        let res = guarded(|| bridge.update(&bytes, &mut out)).unwrap_or_else(|p| {
            panic!(
                "case {case} update panicked: {p}\n{}",
                String::from_utf8_lossy(&bytes)
            )
        });
        let after = view(&mut bridge);
        let v: serde_json::Value = serde_json::from_slice(&after).expect("view is JSON");
        assert!(v.is_object(), "case {case}: view is an object");
        if res.is_err() {
            rejected += 1;
            assert!(
                before == after,
                "case {case}: a rejected event changed the view"
            );
        } else {
            let effects: serde_json::Value =
                serde_json::from_slice(&out).expect("effects are JSON");
            assert!(
                effects.is_array(),
                "case {case}: effect requests are an array"
            );
        }
    }
    assert!(rejected > 0);
    // Pathological documents: deep nesting, huge numbers, invalid UTF-8.
    for bytes in [
        format!("{}{}", "[".repeat(200_000), "]".repeat(200_000)).into_bytes(),
        format!("{{\"LogRunTrack\":{}}}", "{\"points\":".repeat(10_000)).into_bytes(),
        br#"{"SetToday":{"epoch_day":1e999,"utc_offset_sec":0}}"#.to_vec(),
        br#"{"SetToday":{"epoch_day":9223372036854775808,"utc_offset_sec":0}}"#.to_vec(),
        vec![b'"', 0xC3, 0x28, b'"'],
        b"\xEF\xBB\xBF\"ClearRuns\"".to_vec(),
    ] {
        let before = view(&mut bridge);
        let mut out = Vec::new();
        let res = guarded(|| bridge.update(&bytes, &mut out)).expect("no panic");
        assert!(
            res.is_err(),
            "rejected: {}",
            String::from_utf8_lossy(&bytes[..bytes.len().min(40)])
        );
        assert!(before == view(&mut bridge));
    }
}

#[test]
fn extreme_but_valid_events_never_panic_update_or_view() {
    let bridge: Bridge<Engine, JsonFfiFormat> = Bridge::new(crux_core::Core::default());
    let extremes = [
        r#"{"SetToday":{"epoch_day":9223372036854775807,"utc_offset_sec":-9223372036854775808}}"#.to_string(),
        r#"{"SetToday":{"epoch_day":-9223372036854775808,"utc_offset_sec":9223372036854775807}}"#.to_string(),
        r#"{"SetToday":{"epoch_day":20300,"utc_offset_sec":0}}"#.to_string(),
        track_json(
            &[pt(1e308, -1e308, i64::MIN, f32::MAX), pt(-1e308, 1e308, i64::MAX, -f32::MAX), pt(0.0, 0.0, 0, 0.0)],
            &[u32::MAX, 0, 1, 1],
        ),
        track_json(&walk(3, i64::MAX - 10), &[]),
        track_json(&walk(3, i64::MIN), &[]),
        r#"{"LogRun":{"distance_km":1e308,"duration_min":1e-308,"hr_pct_max":1e308,"longest_recent_km":-1e308,"observed_at":-9223372036854775808,"entry_id":18446744073709551615}}"#.into(),
        r#"{"DeleteEntry":{"kind":"Run","entry_id":18446744073709551615,"observed_at_fallback":-9223372036854775808}}"#.into(),
    ];
    for e in &extremes {
        let mut out = Vec::new();
        let r = guarded(|| bridge.update(e.as_bytes(), &mut out))
            .unwrap_or_else(|p| panic!("update panicked on {e}: {p}"));
        let mut v = Vec::new();
        guarded(|| bridge.view(&mut v))
            .unwrap_or_else(|p| panic!("view panicked after {e} (accepted: {}): {p}", r.is_ok()))
            .expect("view serializes");
        let parsed: serde_json::Value = serde_json::from_slice(&v).unwrap();
        assert!(parsed.is_object());
    }
}
