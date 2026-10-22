//! Garmin/ANT `.fit` activity-file parsing, pure data ingestion, no
//! model/event-log involvement. This module only extracts GPS+HR fixes from
//! a byte blob (mirrors how [`crate::running::GpsPoint`] treats an already-
//! decoded track); the Android shell is responsible for turning the result
//! into its existing `LogRunTrack` event. Nothing here is recommendation-
//! bearing, so no `Evidence`/`ConfidenceTag` is attached (evidence gating
//! covers coaching claims, not raw sensor ingest).
//!
//! Pure function: no clocks, no IO beyond the given byte slice.

use fitparser::{FitDataRecord, Value, profile::MesgNum};

/// One GPS+HR fix extracted from a FIT `record` message. Its serialization is
/// the per-fix wire shape of the shell's `parseFit` payload (`ffi.rs`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct FitFix {
    pub lat: f64,
    pub lon: f64,
    pub time_sec: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hr_bpm: Option<u16>,
    /// Horizontal position accuracy in metres, as recorded by the device in the
    /// FIT `record.gps_accuracy` field (field 31, `UInt8`, scale 1 / offset 0,
    /// units "m" per the FIT profile, verified in fitparser 0.11's generated
    /// `record_message_gps_accuracy_field`). `None` when the file carries no
    /// such field: NOT a fabricated stand-in. The shell decides what accuracy
    /// to hand the core's QC gate for a `None` fix (see the sentinel in
    /// `Gpx.kt`'s `importedRunEvent`); this module never invents one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<f32>,
}

/// A parsed FIT activity: fixes grouped into segments by the recorder's
/// timer stop_all/start events (see [`parse_fit`] for the exact split rule).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct FitTrack {
    pub segments: Vec<Vec<FitFix>>,
}

/// Why a FIT file can't be imported as a run. Each variant maps to one stable
/// [`FitError::kind`] the shell keys its per-file import reason on.
#[derive(Debug, Clone, PartialEq)]
pub enum FitError {
    /// The bytes are not a FIT file (bad/oversized header, missing `.FIT` tag).
    NotFit,
    /// A stream/decode error or rejected chained header left too few fixes.
    Damaged,
    /// A cleanly decoded file with fewer than two positioned records.
    NoTrack,
    /// A track too short to keep as a run: under [`MIN_IMPORT_M`] of GPS
    /// distance or [`MIN_IMPORT_SEC`] of recorded time, the shell tracker's
    /// accidental-recording thresholds. Reported as `no_track` / `too_short`.
    TooShort,
    /// The file records a non-running activity. Carries the FIT sport name in
    /// plain lowercase words (e.g. `"cycling"`, `"cross country skiing"`).
    NotARun(String),
}

/// The shortest GPS distance (metres, summed within segments) an imported
/// track must cover to count as a run: the tracker's short-run threshold.
pub const MIN_IMPORT_M: f64 = 500.0;
/// The shortest recorded time (seconds, summed within segments) an imported
/// track must span to count as a run: the tracker's short-run threshold.
pub const MIN_IMPORT_SEC: i64 = 180;

impl FitError {
    /// Stable machine-readable kind for the shell's error envelope.
    pub fn kind(&self) -> &'static str {
        match self {
            FitError::NotFit => "not_fit",
            FitError::Damaged => "damaged",
            FitError::NoTrack | FitError::TooShort => "no_track",
            FitError::NotARun(_) => "not_a_run",
        }
    }

    /// Detail for the kind: the sport for [`FitError::NotARun`], `"too_short"`
    /// for [`FitError::TooShort`], else empty.
    pub fn context(&self) -> &str {
        match self {
            FitError::NotARun(sport) => sport.as_str(),
            FitError::TooShort => "too_short",
            FitError::NotFit | FitError::Damaged | FitError::NoTrack => "",
        }
    }

    /// The plain-language message shown to the user.
    pub fn message(&self) -> String {
        match self {
            FitError::NotFit => "Not a FIT file".to_string(),
            FitError::Damaged => {
                "FIT file is damaged or incomplete; no usable GPS track".to_string()
            }
            FitError::NoTrack => "No GPS track in this FIT file".to_string(),
            FitError::TooShort => "Too short to import as a run".to_string(),
            FitError::NotARun(sport) => format!("This FIT file records {sport}, not a run"),
        }
    }
}

/// FIT position fields are signed 32-bit "semicircles". Verified against
/// fitparse-rs' generated decoder (`record_message_position_lat_field` /
/// `..._position_long_field` in its build-time-generated `profile::decode`):
/// the FIT profile declares scale=1, offset=0 for these fields, so
/// `fitparser` hands them back as raw `Value::SInt32` semicircles: it does
/// NOT convert to degrees. The conversion is done by hand here:
/// `deg = semicircles * 180 / 2^31`.
const SEMICIRCLE_TO_DEGREES: f64 = 180.0 / 2_147_483_648.0; // 180.0 / 2^31

fn semicircles_to_degrees(raw: i32) -> f64 {
    f64::from(raw) * SEMICIRCLE_TO_DEGREES
}

/// Parse a Garmin/ANT `.fit` activity file into GPS+HR fixes, grouped into
/// segments.
///
/// Records without a usable position (`position_lat`/`position_long`
/// missing or the FIT "invalid value" sentinel) or timestamp are skipped
/// rather than erroring the whole file. Returns [`FitError::NotARun`] when a
/// `session`/`lap`/`sport` message names a non-running sport (see
/// `non_run_sport`), checked before the GPS so an indoor ride without GPS is
/// still reported as "not a run". Returns [`FitError::NotFit`] when the bytes
/// don't parse as FIT at all, and [`FitError::Damaged`]/[`FitError::NoTrack`]
/// when fewer than two positioned records survive (nothing worth logging as a
/// track).
///
/// ## Segment splitting
///
/// FIT `event` messages with `event == "timer"` mark start/stop boundaries.
/// A `stop_all` closes the current segment; the next `start` opens a new
/// one. Plain `stop`/`start` (the recorder's ordinary auto-pause at a red
/// light, GPS loss, etc.) does NOT split: it is not a genuinely separate
/// activity leg. LIMITATION: `stop_all`/`start` pairs are how multi-activity
/// recordings (e.g. brick workouts, a paused-and-resumed-later session) mark
/// segment boundaries; the overwhelming majority of single-activity runs
/// never emit them, so those files fall back to exactly one segment: this
/// is the documented single-segment case, not a bug.
pub fn parse_fit(bytes: &[u8]) -> Result<FitTrack, FitError> {
    validate_fit_header(bytes)?;

    // Salvage-decode: keep every record decoded before the first stream error so
    // a watch that died mid-run (file ends without its trailing CRC) or an
    // exporter that appended padding still yields the fixes it did record,
    // instead of failing the whole import on the tail (`from_bytes` is
    // all-or-nothing).
    let (records, truncated) = decode_records_salvaging(bytes);
    if let Some(sport) = non_run_sport(&records) {
        return Err(FitError::NotARun(sport));
    }
    track_from_records(&records, truncated)
}

/// Group the positioned `record` fixes into segments (split rule: see
/// [`parse_fit`]). `truncated` picks the honest message when too few survive.
fn track_from_records(records: &[FitDataRecord], truncated: bool) -> Result<FitTrack, FitError> {
    let mut segments: Vec<Vec<FitFix>> = Vec::new();
    let mut current: Vec<FitFix> = Vec::new();

    for record in records {
        match record.kind() {
            MesgNum::Record => {
                if let Some(fix) = fix_from_record(record) {
                    current.push(fix);
                }
            }
            // A "start" after a stop_all needs no bookkeeping here: the next
            // Record simply accumulates into the fresh `current`.
            MesgNum::Event if is_timer_event(record, "stop_all") && !current.is_empty() => {
                segments.push(std::mem::take(&mut current));
            }
            _ => {}
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let total_fixes: usize = segments.iter().map(Vec::len).sum();
    if total_fixes < 2 {
        // Fewer than two positioned records survived, so there is no track worth
        // logging. A stream/decode error or a rejected chained header means the
        // file is damaged/incomplete; an otherwise-clean file simply has no GPS.
        return Err(if truncated {
            FitError::Damaged
        } else {
            FitError::NoTrack
        });
    }

    Ok(FitTrack { segments })
}

/// The import gate on a parsed track: [`FitError::TooShort`] when its fixes
/// cover under [`MIN_IMPORT_M`] or span under [`MIN_IMPORT_SEC`], each summed
/// within segments (a pause between segments counts as neither).
pub fn import_gate(track: &FitTrack) -> Result<(), FitError> {
    let point = |f: &FitFix| crate::running::GpsPoint {
        lat: f.lat,
        lon: f.lon,
        observed_at: f.time_sec,
        accuracy_m: f.accuracy_m.unwrap_or(0.0),
    };
    let mut metres = 0.0;
    let mut seconds = 0i64;
    for seg in &track.segments {
        metres += seg
            .windows(2)
            .map(|w| crate::running::haversine_m(point(&w[0]), point(&w[1])))
            .filter(|d| d.is_finite())
            .sum::<f64>();
        if let (Some(first), Some(last)) = (seg.first(), seg.last()) {
            seconds = seconds.saturating_add(last.time_sec.saturating_sub(first.time_sec).max(0));
        }
    }
    if metres < MIN_IMPORT_M || seconds < MIN_IMPORT_SEC {
        return Err(FitError::TooShort);
    }
    Ok(())
}

/// Structural pre-validation of a FIT file header, returning
/// [`FitError::NotFit`] rather than letting `fitparser` slice on an untrusted
/// `header_size`.
///
/// `fitparser` reads `header_size` as a raw wire byte (validated only as `> 12`)
/// and slices `input[0..header_size - 2]`, so a crafted oversized value (e.g.
/// `0xFF` on a short file) panics on an out-of-bounds range — caught by the FFI
/// firewall, but the user sees a generic panic toast and a stack is logged. The
/// FIT spec fixes `header_size` at 12 or 14 and stamps `.FIT` at bytes 8..12, so
/// reject anything else up front. (A 14-byte header carries a header CRC in
/// bytes 12..14; its correctness is `fitparser`'s job, not ours.)
fn validate_fit_header(bytes: &[u8]) -> Result<(), FitError> {
    if bytes.len() < 12 {
        return Err(FitError::NotFit);
    }
    let header_size = bytes[0] as usize;
    if !matches!(bytes[0], 12 | 14) || &bytes[8..12] != b".FIT" || bytes.len() < header_size {
        return Err(FitError::NotFit);
    }
    Ok(())
}

/// Stream records out of a FIT byte slice, stopping at the first stream error or
/// structurally invalid header and reporting whether one occurred.
///
/// Mirrors `fitparser::from_bytes`'s object loop but keeps the records decoded
/// before an error instead of discarding the whole file, so a truncated
/// recording still surfaces its fixes (see [`parse_fit`]). `parse_fit` uses no
/// decode options, so unknown messages are decoded and dropped downstream by
/// `kind()` matching, exactly as before.
///
/// A FIT stream may CHAIN several sub-files, each introduced by its own header.
/// `fitparser` slices `input[0..header_size - 2]` on the raw wire byte for EVERY
/// header, so a chained oversized header panics just like a bad first one.
/// fitparser opens a new header after each `Crc`, and also WITHOUT a CRC as soon
/// as a message runs past the header's declared data size (its position passes
/// `end_of_messages` with bytes left). Track the same byte offset here and
/// validate the header at each such boundary (stream start, after each `Crc`,
/// and after an overrun) before handing the buffer to `fitparser`.
fn decode_records_salvaging(bytes: &[u8]) -> (Vec<FitDataRecord>, bool) {
    use fitparser::de::{FitObject, FitStreamProcessor};

    let mut processor = FitStreamProcessor::new();
    let mut records = Vec::new();
    let mut buffer = bytes;
    let mut truncated = false;
    let mut expect_header = true;
    // Bytes of `bytes` handed to fitparser so far; equals fitparser's own stream
    // position because every header it sees passed `validate_fit_header`, so its
    // `header_size` is exactly the bytes it consumed.
    let mut consumed = 0usize;
    // Offset where the current sub-file's messages end (fitparser's
    // `end_of_messages`): its CRC sits there.
    let mut end_of_messages = 0usize;
    while !buffer.is_empty() {
        if expect_header {
            // Reject a structurally impossible (or non-FIT) chained header before
            // fitparser slices on its header_size byte; keep what decoded first.
            if validate_fit_header(buffer).is_err() {
                truncated = true;
                break;
            }
            expect_header = false;
        }
        let (rest, obj) = match processor.deserialize_next(buffer) {
            Ok(next) => next,
            // Missing trailing CRC, a cut-off final record, or trailing padding:
            // stop, keeping everything decoded so far.
            Err(_) => {
                truncated = true;
                break;
            }
        };
        match obj {
            // End of one sub-file; the next bytes (if any) open a new header.
            FitObject::Crc(..) => {
                processor.reset();
                expect_header = true;
            }
            FitObject::DataMessage(msg) => match processor.decode_message(msg) {
                Ok(rec) => records.push(rec),
                Err(_) => {
                    truncated = true;
                    break;
                }
            },
            FitObject::Header(header) => {
                end_of_messages = consumed
                    .saturating_add(usize::from(header.header_size()))
                    .saturating_add(header.data_size() as usize);
            }
            FitObject::DefinitionMessage(..) => {}
        }
        // Defensive: a non-advancing decode would spin the loop forever; treat a
        // stalled stream as truncated.
        if rest.len() >= buffer.len() {
            truncated = true;
            break;
        }
        consumed += buffer.len() - rest.len();
        // A message overran the declared data size: fitparser parses the next
        // bytes as a chained header without any CRC first.
        if consumed > end_of_messages {
            expect_header = true;
        }
        buffer = rest;
    }
    (records, truncated)
}

/// The first sport named by a `session`, `lap` or `sport` message that is not
/// running-family, as plain lowercase words (`cross_country_skiing` →
/// `"cross country skiing"`); `None` when every such message is a run or names
/// no concrete sport.
///
/// fitparser resolves a named enum value to its snake_case FIT-profile string
/// and leaves an unnamed one numeric, so a non-string value is an unknown sport
/// and stays importable, as do `generic` and `all` (no concrete sport) and an
/// absent field. `sub_sport == "treadmill"` is the FIT profile's
/// run-on-fitness-equipment marker, so it counts as a run whatever the parent
/// sport says; any other sub-sport of `running` (trail, street, track, …) is a
/// run by its parent.
fn non_run_sport(records: &[FitDataRecord]) -> Option<String> {
    records
        .iter()
        .filter(|r| matches!(r.kind(), MesgNum::Session | MesgNum::Lap | MesgNum::Sport))
        .find_map(|record| {
            let mut sport: Option<&str> = None;
            let mut treadmill = false;
            for field in record.fields() {
                match (field.name(), field.value()) {
                    ("sport", Value::String(s)) => sport = Some(s.as_str()),
                    ("sub_sport", Value::String(s)) => treadmill = s.as_str() == "treadmill",
                    _ => {}
                }
            }
            match sport {
                None | Some("running" | "generic" | "all") => None,
                Some(_) if treadmill => None,
                Some(other) => Some(other.replace('_', " ")),
            }
        })
}

/// True when a FIT `event` message is `event == "timer"` with the given
/// `event_type` (fitparser resolves both to their named-variant strings, so
/// this string compare is exact, see the FIT profile's `Event`/`EventType`
/// enums).
fn is_timer_event(record: &FitDataRecord, event_type: &str) -> bool {
    let mut is_timer = false;
    let mut matches_type = false;
    for field in record.fields() {
        match field.name() {
            "event" => is_timer = field.value().to_string() == "timer",
            "event_type" => matches_type = field.value().to_string() == event_type,
            _ => {}
        }
    }
    is_timer && matches_type
}

/// A FIT `SInt32` field's value, or `None` if it is the `i32::MAX` "invalid"
/// sentinel. fitparser normally drops invalid-valued fields before we see
/// them, but guard it anyway so a stray sentinel never becomes a phantom
/// value, e.g. a ~180° position fix injecting a multi-thousand-km jump into
/// distance/pace/splits.
fn valid_i32(v: &Value) -> Option<i32> {
    match v {
        Value::SInt32(v) if *v != i32::MAX => Some(*v),
        _ => None,
    }
}

/// A FIT `UInt8` field's value, or `None` if it is the `0xFF` "invalid"
/// sentinel (same rationale as [`valid_i32`]), so a dropped strap read never
/// surfaces as a fake "255 bpm" and a missing reading never becomes a fake
/// "255 m".
fn valid_u8(v: &Value) -> Option<u8> {
    match v {
        Value::UInt8(v) if *v != u8::MAX => Some(*v),
        _ => None,
    }
}

fn fix_from_record(record: &FitDataRecord) -> Option<FitFix> {
    let mut lat_raw: Option<i32> = None;
    let mut lon_raw: Option<i32> = None;
    let mut time_sec: Option<i64> = None;
    let mut hr_bpm: Option<u16> = None;
    let mut accuracy_m: Option<f32> = None;

    for field in record.fields() {
        match field.name() {
            "position_lat" => {
                if let Some(v) = valid_i32(field.value()) {
                    lat_raw = Some(v);
                }
            }
            "position_long" => {
                if let Some(v) = valid_i32(field.value()) {
                    lon_raw = Some(v);
                }
            }
            "timestamp" => {
                // fitparser resolves `timestamp` to `Value::Timestamp(DateTime<Local>)`;
                // its `TryInto<i64>` calls `.timestamp()`, which is unix seconds
                // (UTC) regardless of the `Local` wrapper: no wall-clock read here,
                // this only reinterprets bytes already in `bytes`.
                if let Ok(t) = field.value().try_into() {
                    time_sec = Some(t);
                }
            }
            "heart_rate" => {
                if let Some(v) = valid_u8(field.value()) {
                    hr_bpm = Some(u16::from(v));
                }
            }
            "gps_accuracy" => {
                // A real device measurement in metres (scale 1, offset 0).
                if let Some(v) = valid_u8(field.value()) {
                    accuracy_m = Some(f32::from(v));
                }
            }
            _ => {}
        }
    }

    Some(FitFix {
        lat: semicircles_to_degrees(lat_raw?),
        lon: semicircles_to_degrees(lon_raw?),
        time_sec: time_sec?,
        hr_bpm,
        accuracy_m,
    })
}

#[cfg(test)]
mod tests;
