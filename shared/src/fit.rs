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

fn fix_from_record(record: &FitDataRecord) -> Option<FitFix> {
    let mut lat_raw: Option<i32> = None;
    let mut lon_raw: Option<i32> = None;
    let mut time_sec: Option<i64> = None;
    let mut hr_bpm: Option<u16> = None;

    for field in record.fields() {
        match field.name() {
            "position_lat" => {
                if let Value::SInt32(v) = field.value() {
                    lat_raw = Some(*v);
                }
            }
            "position_long" => {
                if let Value::SInt32(v) = field.value() {
                    lon_raw = Some(*v);
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
                if let Value::UInt8(v) = field.value() {
                    hr_bpm = Some(u16::from(*v));
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
    }
}
