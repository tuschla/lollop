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

/// One GPS+HR fix extracted from a FIT `record` message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitFix {
    pub lat: f64,
    pub lon: f64,
    pub time_sec: i64,
    pub hr_bpm: Option<u16>,
    /// Horizontal position accuracy in metres, as recorded by the device in the
    /// FIT `record.gps_accuracy` field (field 31, `UInt8`, scale 1 / offset 0,
    /// units "m" per the FIT profile, verified in fitparser 0.11's generated
    /// `record_message_gps_accuracy_field`). `None` when the file carries no
    /// such field: NOT a fabricated stand-in. The shell decides what accuracy
    /// to hand the core's QC gate for a `None` fix (see the sentinel in
    /// `Gpx.kt`'s `importedRunEvent`); this module never invents one.
    pub accuracy_m: Option<f32>,
}

/// A parsed FIT activity: fixes grouped into segments by the recorder's
/// timer stop_all/start events (see [`parse_fit`] for the exact split rule).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FitTrack {
    pub segments: Vec<Vec<FitFix>>,
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
/// rather than erroring the whole file. Returns `Err` with a plain-language
/// message when the bytes don't parse as FIT at all, or when fewer than two
/// positioned records survive (nothing worth logging as a track).
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
pub fn parse_fit(bytes: &[u8]) -> Result<FitTrack, String> {
    if bytes.is_empty() {
        return Err("Not a FIT file".to_string());
    }
    let records = fitparser::from_bytes(bytes).map_err(|_| "Not a FIT file".to_string())?;

    let mut segments: Vec<Vec<FitFix>> = Vec::new();
    let mut current: Vec<FitFix> = Vec::new();

    for record in &records {
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
        return Err("No GPS track in this FIT file".to_string());
    }

    Ok(FitTrack { segments })
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
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_is_a_plain_error_not_a_panic() {
        assert_eq!(parse_fit(&[]), Err("Not a FIT file".to_string()));
    }

    #[test]
    fn garbage_bytes_is_a_plain_error_not_a_panic() {
        let garbage = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 255, 254, 253];
        assert_eq!(parse_fit(&garbage), Err("Not a FIT file".to_string()));
    }

    #[test]
    fn a_fit_looking_header_with_no_data_still_errors_cleanly() {
        // A syntactically plausible 12-byte FIT header (size, protocol,
        // profile version, data size = 0, ".FIT" tag) but no CRC/data
        // records: exercises the "parses far enough to not be garbage, but
        // still not a usable track" path without needing a real fixture.
        let mut header = vec![12u8, 0x10, 0x94, 0x08, 0, 0, 0, 0];
        header.extend_from_slice(b".FIT");
        assert!(parse_fit(&header).is_err());
    }

    #[test]
    fn semicircle_conversion_is_exact_at_known_reference_points() {
        // 2^31 semicircles == 180 degrees exactly (the FIT spec's defining
        // identity); i32::MIN/MAX are the extreme raw values a real file can
        // carry, so pin the conversion there rather than only at 0.
        assert_eq!(semicircles_to_degrees(0), 0.0);
        assert_eq!(semicircles_to_degrees(i32::MAX), 179.99999991618097);
        assert_eq!(semicircles_to_degrees(i32::MIN), -180.0);

        // A real-world reference: Berlin Alexanderplatz is documented at
        // roughly 52.5219 N, 13.4132 E. Round-trip a semicircle value close
        // to that back to degrees and check it lands in the right place
        // within FIT's own precision (~2.1 cm at the equator).
        let berlin_lat_semicircles: i32 = ((52.5219_f64) * 2_147_483_648.0 / 180.0) as i32;
        let deg = semicircles_to_degrees(berlin_lat_semicircles);
        assert!((deg - 52.5219).abs() < 1e-6, "got {deg}");
    }

    /// Real device-recorded FIT files, borrowed from fitparser's own
    /// (MIT-licensed, same repo) test fixtures at
    /// `fitparse-rs/fitparser/tests/fixtures/*.fit`, cheaper and more
    /// faithful than hand-encoding a minimal FIT byte stream (correct
    /// CRC-16/ANT, message definitions, field layout) from scratch.
    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"))
    }

    #[test]
    fn happy_path_extracts_a_real_gps_and_hr_track() {
        // Garmin Fenix 5 bike ride: 19 recorded fixes, all with heart rate,
        // no stop_all/start pair in this short file so it stays one segment
        // (the documented single-segment fallback: see `parse_fit` docs).
        let bytes = fixture("garmin-fenix-5-bike.fit");
        let track = parse_fit(&bytes).expect("real device file should parse");

        assert_eq!(track.segments.len(), 1, "no stop_all/start pair in this file");
        let fixes = &track.segments[0];
        assert_eq!(fixes.len(), 19);

        let first = fixes[0];
        assert!((first.lat - 37.41116).abs() < 1e-4, "lat: {first:?}");
        assert!((first.lon - (-122.06907)).abs() < 1e-4, "lon: {first:?}");
        assert_eq!(first.time_sec, 1_497_283_762);
        assert_eq!(first.hr_bpm, Some(77));

        // This real device file records NO `gps_accuracy` field on its record
        // messages, so accuracy is honestly unknown, `None`, never a
        // fabricated figure. (See `synthetic_gps_accuracy_field_flows_through`
        // for the present-field path.)
        assert!(
            fixes.iter().all(|f| f.accuracy_m.is_none()),
            "fenix-5 fixture carries no gps_accuracy → must stay None, not a stand-in"
        );

        // Timestamps are monotonic non-decreasing across the whole segment -
        // a sanity check that we read `timestamp`, not some other field.
        assert!(fixes.windows(2).all(|w| w[1].time_sec >= w[0].time_sec));
    }

    #[test]
    fn happy_path_second_fixture_also_parses_without_hr() {
        // A different real device file with no heart-rate strap: hr_bpm
        // must come back None, not a fabricated/default value.
        let bytes = fixture("Activity.fit");
        let track = parse_fit(&bytes).expect("real device file should parse");
        let total: usize = track.segments.iter().map(Vec::len).sum();
        assert_eq!(total, 14);
        assert!(
            track.segments.iter().flatten().all(|f| f.hr_bpm.is_none()),
            "this fixture has no HR strap data"
        );
        assert!(
            track.segments.iter().flatten().all(|f| f.accuracy_m.is_none()),
            "this fixture has no gps_accuracy field → accuracy stays None"
        );
    }

    /// FIT CRC-16 (ANT variant) over a byte slice, mirrors fitparser's own
    /// `de::crc::get_crc`. Needed to hand-encode a valid FIT file below (the
    /// trailing 2-byte data CRC is validated by `from_bytes` by default).
    fn fit_crc(data: &[u8]) -> u16 {
        const TABLE: [u16; 16] = [
            0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00,
            0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
        ];
        let mut crc: u16 = 0;
        for &byte in data {
            let mut tmp = TABLE[(crc & 0xF) as usize];
            crc = (crc >> 4) & 0x0FFF;
            crc = crc ^ tmp ^ TABLE[(byte & 0xF) as usize];
            tmp = TABLE[(crc & 0xF) as usize];
            crc = (crc >> 4) & 0x0FFF;
            crc = crc ^ tmp ^ TABLE[((byte >> 4) & 0xF) as usize];
        }
        crc
    }

    #[test]
    fn synthetic_gps_accuracy_field_flows_through() {
        // Neither real-device fixture in shared/tests/fixtures/ records
        // `gps_accuracy`, so hand-encode a minimal valid FIT that DOES: the
        // only way to prove the field is read and surfaced (not fabricated).
        //
        // One `record` definition (global mesg 20, little-endian) with fields
        // timestamp(253,u32) position_lat(0,i32) position_long(1,i32)
        // gps_accuracy(31,u8), then two data records carrying 8 m and 12 m.
        const FIT_EPOCH_OFFSET: i64 = 631_065_600; // unix secs at 1989-12-31Z
        let to_semi = |deg: f64| -> i32 { (deg * 2_147_483_648.0 / 180.0) as i32 };

        let mut data: Vec<u8> = Vec::new();
        // --- definition message (local type 0) ---
        data.push(0x40); // definition, local type 0
        data.push(0x00); // reserved
        data.push(0x00); // architecture: little-endian
        data.extend_from_slice(&20u16.to_le_bytes()); // global mesg num = record
        data.push(4); // field count
        data.extend_from_slice(&[253, 4, 0x86]); // timestamp: u32
        data.extend_from_slice(&[0, 4, 0x85]); // position_lat: i32
        data.extend_from_slice(&[1, 4, 0x85]); // position_long: i32
        data.extend_from_slice(&[31, 1, 0x02]); // gps_accuracy: u8

        let push_record = |unix: i64, lat: f64, lon: f64, acc: u8, out: &mut Vec<u8>| {
            out.push(0x00); // data message, local type 0
            out.extend_from_slice(&((unix - FIT_EPOCH_OFFSET) as u32).to_le_bytes());
            out.extend_from_slice(&to_semi(lat).to_le_bytes());
            out.extend_from_slice(&to_semi(lon).to_le_bytes());
            out.push(acc);
        };
        push_record(1_500_000_000, 52.5200, 13.4050, 8, &mut data);
        push_record(1_500_000_030, 52.5210, 13.4060, 12, &mut data);

        // --- 12-byte header (no header CRC → header bytes join the data CRC) ---
        let mut file: Vec<u8> = Vec::new();
        file.push(12); // header size
        file.push(0x10); // protocol version 1.0
        file.extend_from_slice(&100u16.to_le_bytes()); // profile version (arbitrary)
        file.extend_from_slice(&(data.len() as u32).to_le_bytes()); // data size
        file.extend_from_slice(b".FIT");
        file.extend_from_slice(&data);
        // Trailing data CRC covers header + all messages.
        let crc = fit_crc(&file);
        file.extend_from_slice(&crc.to_le_bytes());

        let track = parse_fit(&file).expect("hand-encoded FIT should parse");
        let fixes: Vec<FitFix> = track.segments.into_iter().flatten().collect();
        assert_eq!(fixes.len(), 2);
        // The device's real recorded accuracy comes through unchanged, in metres.
        assert_eq!(fixes[0].accuracy_m, Some(8.0));
        assert_eq!(fixes[1].accuracy_m, Some(12.0));
        assert!((fixes[0].lat - 52.5200).abs() < 1e-4, "lat: {:?}", fixes[0]);
    }

    /// Build a raw `record` `FitDataField` (name-matched by `fix_from_record`).
    fn field(name: &str, number: u8, value: Value) -> fitparser::FitDataField {
        fitparser::FitDataField::new(name.to_string(), number, None, value, String::new())
    }

    /// Assemble a `record` message from raw fields. `timestamp` is encoded as a
    /// plain `UInt32` of unix seconds, `fix_from_record` matches on field NAME
    /// and `TryInto<i64>` accepts any integer variant, so this needs no chrono.
    fn record(fields: Vec<fitparser::FitDataField>) -> FitDataRecord {
        let mut r = FitDataRecord::new(MesgNum::Record);
        for f in fields {
            r.push(f);
        }
        r
    }

    #[test]
    fn sentinel_position_value_yields_no_fix() {
        // fitparser normally strips FIT invalid sentinels before we see them,
        // but `fix_from_record` guards defensively (like the gps_accuracy
        // guard). Feed it a raw record directly: a `position_lat`/`position_long`
        // carrying the SInt32 invalid sentinel (`i32::MAX`) must yield NO fix -
        // without the guard it converts to ~180° and injects a multi-thousand-km
        // phantom fix into distance/pace/splits.
        let sentinel_lat = record(vec![
            field("timestamp", 253, Value::UInt32(1_500_000_000)),
            field("position_lat", 0, Value::SInt32(i32::MAX)),
            field("position_long", 1, Value::SInt32(157_000_000)),
        ]);
        assert_eq!(fix_from_record(&sentinel_lat), None, "sentinel lat drops the fix");

        let sentinel_lon = record(vec![
            field("timestamp", 253, Value::UInt32(1_500_000_000)),
            field("position_lat", 0, Value::SInt32(626_000_000)),
            field("position_long", 1, Value::SInt32(i32::MAX)),
        ]);
        assert_eq!(fix_from_record(&sentinel_lon), None, "sentinel lon drops the fix");

        // A valid pair still yields a fix: the guard must not over-reject.
        let good = record(vec![
            field("timestamp", 253, Value::UInt32(1_500_000_000)),
            field("position_lat", 0, Value::SInt32(626_000_000)),
            field("position_long", 1, Value::SInt32(157_000_000)),
        ]);
        assert!(fix_from_record(&good).is_some(), "a valid record still yields a fix");
    }

    #[test]
    fn invalid_heart_rate_sentinel_drops_hr_but_keeps_fix() {
        // `heart_rate == 0xFF` is the FIT UInt8 invalid sentinel (a dropped
        // strap read). The fix survives on its valid position, but `hr_bpm`
        // must be None, never a fabricated 255 bpm.
        let sentinel_hr = record(vec![
            field("timestamp", 253, Value::UInt32(1_500_000_000)),
            field("position_lat", 0, Value::SInt32(626_000_000)),
            field("position_long", 1, Value::SInt32(157_000_000)),
            field("heart_rate", 3, Value::UInt8(u8::MAX)),
        ]);
        let fix = fix_from_record(&sentinel_hr).expect("valid position keeps the fix");
        assert_eq!(fix.hr_bpm, None, "0xFF sentinel must not surface as 255 bpm");

        // A real strap reading comes through.
        let good_hr = record(vec![
            field("timestamp", 253, Value::UInt32(1_500_000_000)),
            field("position_lat", 0, Value::SInt32(626_000_000)),
            field("position_long", 1, Value::SInt32(157_000_000)),
            field("heart_rate", 3, Value::UInt8(150)),
        ]);
        let fix = fix_from_record(&good_hr).expect("valid record");
        assert_eq!(fix.hr_bpm, Some(150));
    }
}
