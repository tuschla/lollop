//! Garmin/ANT `.fit` activity-file parsing: pure data ingestion, no
//! model/event-log involvement. Extracts GPS+HR fixes from a byte blob; the
//! Android shell turns the result into its `LogRunTrack` event. Nothing here
//! is recommendation-bearing, so no `Recommended`/`ClaimId` is attached.
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
    /// Horizontal position accuracy in metres from the FIT `record.gps_accuracy`
    /// field (field 31, `UInt8`, scale 1). `None` when the file has no such
    /// field; the shell picks the accuracy it hands the core's QC gate for a
    /// `None` fix (`Gpx.kt`'s `importedRunEvent`).
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
}

/// deg = semicircles × 180 / 2^31 (FIT profile scale 1, so fitparser returns raw
/// `Value::SInt32` semicircles).
const SEMICIRCLE_TO_DEGREES: f64 = 180.0 / 2_147_483_648.0;

fn semicircles_to_degrees(raw: i32) -> f64 {
    f64::from(raw) * SEMICIRCLE_TO_DEGREES
}

/// Parse a Garmin/ANT `.fit` activity file into GPS+HR fixes, grouped into
/// segments: a timer `stop_all` closes the current segment and the next `start`
/// opens one; plain `stop`/`start` (auto-pause) does not split, so most runs
/// are one segment.
///
/// Records without a usable position or timestamp are skipped. Errors:
/// [`FitError::NotARun`] when a `session`/`lap`/`sport` message names a
/// non-running sport (checked before GPS), [`FitError::NotFit`] for non-FIT
/// bytes, [`FitError::Damaged`]/[`FitError::NoTrack`] when fewer than two
/// positioned records survive.
pub fn parse_fit(bytes: &[u8]) -> Result<FitTrack, FitError> {
    validate_fit_header(bytes)?;

    // Salvage-decode: keep every record decoded before the first stream error so
    // a watch that died mid-run (file ends without its trailing CRC) or an
    // exporter that appended padding still yields the fixes it did record,
    // instead of failing the whole import on the tail (`from_bytes` is
    // all-or-nothing).
    let (mut scan, truncated) = scan_fit(bytes);
    if let Some(sport) = scan.non_run_sport.take() {
        return Err(FitError::NotARun(sport));
    }
    scan.into_track(truncated)
}

/// What [`parse_fit`] keeps of a FIT stream, folded in one record at a time so
/// no decoded message outlives its own step: the positioned `record` fixes
/// grouped into segments (split rule: see [`parse_fit`]) and the first
/// non-running sport named. Peak heap is the fixes (one per positioned record,
/// each at least 9 wire bytes) plus one decoded message, never the whole file's
/// messages: a 32 MB file of messages the importer never reads costs one
/// message's worth.
#[derive(Default)]
struct TrackScan {
    segments: Vec<Vec<FitFix>>,
    current: Vec<FitFix>,
    non_run_sport: Option<String>,
}

impl TrackScan {
    fn push(&mut self, record: &FitDataRecord) {
        match record.kind() {
            MesgNum::Record => {
                if let Some(fix) = fix_from_record(record) {
                    self.current.push(fix);
                }
            }
            // A "start" after a stop_all needs no bookkeeping here: the next
            // Record simply accumulates into the fresh `current`.
            MesgNum::Event if is_timer_event(record, "stop_all") && !self.current.is_empty() => {
                self.segments.push(std::mem::take(&mut self.current));
            }
            MesgNum::Session | MesgNum::Lap | MesgNum::Sport if self.non_run_sport.is_none() => {
                self.non_run_sport = non_run_sport(record);
            }
            _ => {}
        }
    }

    /// The track, or why there is none. `truncated` picks the honest message
    /// when too few fixes survive.
    fn into_track(mut self, truncated: bool) -> Result<FitTrack, FitError> {
        if !self.current.is_empty() {
            self.segments.push(self.current);
        }
        let total_fixes: usize = self.segments.iter().map(Vec::len).sum();
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
        Ok(FitTrack {
            segments: self.segments,
        })
    }
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

/// Stream a FIT byte slice into a [`TrackScan`], stopping at the first stream
/// error or structurally invalid header and reporting whether one occurred.
///
/// Mirrors `fitparser::from_bytes`'s object loop but keeps what decoded before
/// an error instead of discarding the whole file, so a truncated recording still
/// surfaces its fixes (see [`parse_fit`]). Each decoded message is folded into
/// the scan and dropped at once (bounded heap, see [`TrackScan`]).
///
/// A FIT stream may CHAIN several sub-files, each introduced by its own header.
/// `fitparser` slices `input[0..header_size - 2]` on the raw wire byte for EVERY
/// header, so a chained oversized header panics just like a bad first one.
/// fitparser opens a new header after each `Crc`, and also WITHOUT a CRC as soon
/// as a message runs past the header's declared data size (its position passes
/// `end_of_messages` with bytes left). Track the same byte offset here and
/// validate the header at each such boundary (stream start, after each `Crc`,
/// and after an overrun) before handing the buffer to `fitparser`.
///
/// Developer fields reach fitparser only as opaque native byte fields
/// ([`opaque_developer_fields`]): it trusts a developer field's size against
/// its declared base type (an odd size for a `uint16` overflows its `u8` byte
/// count, a panic in debug, the rest of the file eaten in release), it fails the
/// stream on a developer field without a description, and it hands decoded
/// developer fields back in `HashMap` order under any name an app picks
/// (`"heart_rate"`). The importer reads none of them. A sub-file whose
/// definitions were rewritten has its data CRC checked here, over its original
/// bytes, instead of by fitparser (which saw the rewritten ones).
fn scan_fit(bytes: &[u8]) -> (TrackScan, bool) {
    use fitparser::de::{DecodeOption, FitObject, FitStreamProcessor};

    let mut processor = FitStreamProcessor::new();
    let mut scan = TrackScan::default();
    let mut buffer = bytes;
    let mut truncated = false;
    let mut expect_header = true;
    // Bytes of `bytes` handed to fitparser so far; equals fitparser's own stream
    // position because every header it sees passed `validate_fit_header`, so its
    // `header_size` is exactly the bytes it consumed, and every rewritten
    // definition is as long as the original.
    let mut consumed = 0usize;
    // Offset where the current sub-file's messages end (fitparser's
    // `end_of_messages`): its CRC sits there.
    let mut end_of_messages = 0usize;
    // Where the current sub-file's data CRC starts (its header too unless the
    // header carries its own CRC), and whether this loop checks that CRC.
    let mut crc_from = 0usize;
    let mut own_crc_check = false;
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
        let rewritten = if consumed < end_of_messages {
            match opaque_developer_fields(buffer) {
                Ok(def) => def,
                // Too many fields to renumber: stop, keeping what decoded.
                Err(TooManyFields) => {
                    truncated = true;
                    break;
                }
            }
        } else {
            None
        };
        if rewritten.is_some() && !own_crc_check {
            processor.add_option(DecodeOption::SkipDataCrcValidation);
            own_crc_check = true;
        }
        let input = rewritten.as_deref().unwrap_or(buffer);
        let (rest, obj) = match processor.deserialize_next(input) {
            Ok(next) => next,
            // Missing trailing CRC, a cut-off final record, or trailing padding:
            // stop, keeping everything decoded so far.
            Err(_) => {
                truncated = true;
                break;
            }
        };
        let advanced = input.len() - rest.len();
        match obj {
            // End of one sub-file; the next bytes (if any) open a new header.
            FitObject::Crc(stored) => {
                if own_crc_check && fit_crc(&bytes[crc_from..end_of_messages]) != stored {
                    truncated = true;
                    break;
                }
                processor.reset();
                expect_header = true;
            }
            FitObject::DataMessage(msg) => match processor.decode_message(msg) {
                Ok(rec) => scan.push(&rec),
                Err(_) => {
                    truncated = true;
                    break;
                }
            },
            FitObject::Header(header) => {
                let header_size = usize::from(header.header_size());
                end_of_messages = consumed
                    .saturating_add(header_size)
                    .saturating_add(header.data_size() as usize);
                crc_from = if header.crc().unwrap_or(0) > 0 {
                    consumed + header_size
                } else {
                    consumed
                };
                if own_crc_check {
                    processor.remove_option(DecodeOption::SkipDataCrcValidation);
                    own_crc_check = false;
                }
            }
            FitObject::DefinitionMessage(..) => {}
        }
        // Defensive: a non-advancing decode would spin the loop forever; treat a
        // stalled stream as truncated.
        if advanced == 0 {
            truncated = true;
            break;
        }
        consumed += advanced;
        // A message overran the declared data size: fitparser parses the next
        // bytes as a chained header without any CRC first.
        if consumed > end_of_messages {
            expect_header = true;
        }
        buffer = &buffer[advanced..];
    }
    (scan, truncated)
}

/// A definition message whose developer fields can't all be renumbered as
/// native fields (see [`opaque_developer_fields`]).
struct TooManyFields;

/// Native field numbers no message of the FIT profile fitparser 0.11 decodes
/// uses (its generated `profile::decode` names numbers up to 210, then 250,
/// 253 and 254): a developer field renumbered to one of these decodes as an
/// `unknown_field_N` byte array, never as a named native field.
const OPAQUE_FIELD_NUMBERS: std::ops::RangeInclusive<u8> = 211..=249;

/// FIT base type `byte` (0x0D): an array of raw bytes, valid at any size.
const FIT_BASE_TYPE_BYTE: u8 = 0x0D;

/// If `buffer` starts with a complete definition message that declares
/// developer fields, a copy of it with every developer field turned into a
/// native `byte` field of the same size (numbered from
/// [`OPAQUE_FIELD_NUMBERS`], skipping the definition's own field numbers) and a
/// developer-field count of 0. Same length, and the fields keep their order
/// (native first, then the former developer fields), so every data message of
/// this local type still decodes to the same bytes, just never as a developer
/// field. `None` when there is nothing to rewrite (another message, or a
/// definition cut short, which fitparser reports itself).
fn opaque_developer_fields(buffer: &[u8]) -> Result<Option<Vec<u8>>, TooManyFields> {
    // Record header: bit 7 clear (normal header), bit 6 definition, bit 5
    // developer data. Then reserved, architecture, global number (2), the
    // native field count and 3 bytes per field, the developer field count and 3
    // bytes per developer field.
    let Some(&header) = buffer.first() else {
        return Ok(None);
    };
    if header & 0xE0 != 0x60 || buffer.len() < 6 {
        return Ok(None);
    }
    let native = usize::from(buffer[5]);
    let dev_count_at = 6 + 3 * native;
    let Some(&dev_count) = buffer.get(dev_count_at) else {
        return Ok(None);
    };
    let dev = usize::from(dev_count);
    let end = dev_count_at + 1 + 3 * dev;
    if dev == 0 || buffer.len() < end {
        return Ok(None);
    }
    let Ok(total) = u8::try_from(native + dev) else {
        return Err(TooManyFields);
    };
    let taken: Vec<u8> = buffer[6..dev_count_at]
        .chunks_exact(3)
        .map(|f| f[0])
        .collect();
    let mut free = OPAQUE_FIELD_NUMBERS.filter(|n| !taken.contains(n));
    let mut out = Vec::with_capacity(end);
    out.extend_from_slice(&buffer[..5]);
    out.push(total);
    out.extend_from_slice(&buffer[6..dev_count_at]);
    for field in buffer[dev_count_at + 1..end].chunks_exact(3) {
        let number = free.next().ok_or(TooManyFields)?;
        out.extend_from_slice(&[number, field[1], FIT_BASE_TYPE_BYTE]);
    }
    out.push(0);
    Ok(Some(out))
}

/// The FIT SDK's CRC-16 (nibble-table form), as fitparser checks a sub-file's
/// data CRC.
fn fit_crc(data: &[u8]) -> u16 {
    const T: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];
    data.iter().fold(0u16, |crc, &b| {
        let crc = (crc >> 4) ^ T[usize::from(crc & 0xF)] ^ T[usize::from(b & 0xF)];
        (crc >> 4) ^ T[usize::from(crc & 0xF)] ^ T[usize::from(b >> 4)]
    })
}

/// The sport a `session`, `lap` or `sport` message names when it is not
/// running-family, as plain lowercase words (`cross_country_skiing` →
/// `"cross country skiing"`); `None` when it is a run or names no concrete
/// sport. [`TrackScan`] keeps the first such sport of the file.
///
/// fitparser resolves a named enum value to its snake_case FIT-profile string
/// and leaves an unnamed one numeric, so a non-string value is an unknown sport
/// and stays importable, as do `generic` and `all` (no concrete sport) and an
/// absent field. `sub_sport == "treadmill"` is the FIT profile's
/// run-on-fitness-equipment marker, so it counts as a run whatever the parent
/// sport says; any other sub-sport of `running` (trail, street, track, …) is a
/// run by its parent.
fn non_run_sport(record: &FitDataRecord) -> Option<String> {
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

/// A FIT `SInt32` field's value, or `None` for the `i32::MAX` "invalid"
/// sentinel, so a stray sentinel never becomes a ~180° phantom fix.
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
