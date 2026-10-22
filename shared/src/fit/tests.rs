use super::*;

/// One fix `dlat` degrees north of a base point, `t` seconds in.
fn gate_fix(dlat: f64, t: i64) -> FitFix {
    FitFix {
        lat: 52.52 + dlat,
        lon: 13.405,
        time_sec: 1_500_000_000 + t,
        hr_bpm: None,
        accuracy_m: None,
    }
}

#[test]
fn the_import_gate_refuses_a_fragment_and_keeps_a_real_run() {
    // 0.0054° of latitude ≈ 600 m.
    let run = FitTrack {
        segments: vec![vec![gate_fix(0.0, 0), gate_fix(0.0054, 200)]],
    };
    assert_eq!(import_gate(&run), Ok(()));
    let short_time = FitTrack {
        segments: vec![vec![gate_fix(0.0, 0), gate_fix(0.0054, 120)]],
    };
    assert_eq!(import_gate(&short_time), Err(FitError::TooShort));
    let short_distance = FitTrack {
        segments: vec![vec![gate_fix(0.0, 0), gate_fix(0.0009, 600)]],
    };
    assert_eq!(import_gate(&short_distance), Err(FitError::TooShort));
    // Two 100 s legs add up; the pause between the segments does not count.
    let split = FitTrack {
        segments: vec![
            vec![gate_fix(0.0, 0), gate_fix(0.0027, 100)],
            vec![gate_fix(0.0027, 5_000), gate_fix(0.0054, 5_100)],
        ],
    };
    assert_eq!(import_gate(&split), Ok(()));
    let err = FitError::TooShort;
    assert_eq!((err.kind(), err.context()), ("no_track", "too_short"));
}

#[test]
fn empty_bytes_is_a_plain_error_not_a_panic() {
    assert_eq!(parse_fit(&[]), Err(FitError::NotFit));
}

#[test]
fn garbage_bytes_is_a_plain_error_not_a_panic() {
    let garbage = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 255, 254, 253];
    assert_eq!(parse_fit(&garbage), Err(FitError::NotFit));
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
fn an_oversized_header_size_is_rejected_before_fitparser_can_panic() {
    // header_size = 0xFF with a valid ".FIT" tag: fitparser slices
    // input[0..0xFF-2] on this short buffer and panics on the out-of-bounds
    // range. Pre-validation must return the plain error instead, so the
    // shell shows "Not a FIT file", never a caught-panic toast + logged stack.
    let mut bytes = vec![0xFFu8, 0x10, 0x94, 0x08, 0, 0, 0, 0];
    bytes.extend_from_slice(b".FIT");
    bytes.extend_from_slice(&[0u8; 4]);
    assert_eq!(parse_fit(&bytes), Err(FitError::NotFit));
}

#[test]
fn a_recording_cut_off_before_its_crc_still_imports_its_fixes() {
    // A watch dying at the end of a run writes every record but never the
    // trailing CRC; cutting into the tail makes `fitparser::from_bytes`
    // fail the whole file. Salvage must keep the fixes decoded before the cut.
    let mut bytes = fixture("garmin-fenix-5-bike.fit");
    bytes.truncate(bytes.len() - 40);
    let track = track_any_sport(&bytes).expect("fixes decoded before the cut survive");
    let total: usize = track.segments.iter().map(Vec::len).sum();
    assert!(total >= 2, "salvaged the decoded fixes, got {total}");
}

#[test]
fn a_file_truncated_to_no_usable_fixes_reports_damage_honestly() {
    // Truncated three bytes past the header: the first record is cut off, so
    // zero fixes survive. The error must name the damage, not the misleading
    // generic "Not a FIT file", and must not falsely claim fixes were imported.
    let mut bytes = fixture("garmin-fenix-5-bike.fit");
    let keep = bytes[0] as usize + 3;
    bytes.truncate(keep);
    assert_eq!(parse_fit(&bytes), Err(FitError::Damaged));
}

#[test]
fn a_chained_oversized_header_is_rejected_without_panicking() {
    // A complete FIT file followed by a chained 14-byte header whose
    // header_size byte is 0xFF: fitparser slices input[0..0xFF-2] on that
    // second header and panics. Validating every header position (not just the
    // first) must stop before fitparser sees it — salvaging the real file's
    // fixes rather than aborting or panicking.
    let mut bytes = fixture("garmin-fenix-5-bike.fit");
    bytes.extend_from_slice(&[
        0xFF, 0x10, 0, 0, 0, 0, 0, 0, b'.', b'F', b'I', b'T', 0x34, 0x12,
    ]);
    let track = track_any_sport(&bytes).expect("the real file's fixes survive the bad trailer");
    let total: usize = track.segments.iter().map(Vec::len).sum();
    assert!(total >= 2, "salvaged the real sub-file, got {total}");
}

#[test]
fn a_message_overrunning_the_declared_data_size_cannot_smuggle_a_bad_header() {
    // The header declares 1 data byte but a 6-byte definition follows, so
    // fitparser ends past its end-of-messages with no CRC and parses the next
    // bytes as a chained header, here one with header_size 0xFF that it would
    // slice on and panic. A direct call (no catch_unwind): a panic fails it.
    let mut bytes = fit_header(1);
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0]); // definition: file_id, no fields
    bytes.extend_from_slice(&[
        0xFF, 0x10, 0, 0, 0, 0, 0, 0, b'.', b'F', b'I', b'T', 0x34, 0x12,
    ]);
    assert_eq!(parse_fit(&bytes), Err(FitError::Damaged));
}

#[test]
fn fixes_decoded_before_a_data_size_overrun_are_kept() {
    // Declared data size one byte short of the real messages: the second
    // record overruns it, then a bad header follows. Both fixes survive.
    let data = gps_accuracy_messages();
    let mut bytes = fit_header(data.len() as u32 - 1);
    bytes.extend_from_slice(&data);
    bytes.extend_from_slice(&[
        0xFF, 0x10, 0, 0, 0, 0, 0, 0, b'.', b'F', b'I', b'T', 0x34, 0x12,
    ]);
    let track = parse_fit(&bytes).expect("fixes before the overrun survive");
    let total: usize = track.segments.iter().map(Vec::len).sum();
    assert_eq!(total, 2);
}

#[test]
fn a_real_bike_ride_is_reported_as_not_a_run() {
    // The Fenix 5 fixture is a cycling activity with a full GPS track: it
    // must not be importable as a run.
    let bytes = fixture("garmin-fenix-5-bike.fit");
    let err = parse_fit(&bytes).expect_err("cycling is not a run");
    assert_eq!(err, FitError::NotARun("cycling".to_string()));
    assert_eq!(err.kind(), "not_a_run");
    assert_eq!(err.context(), "cycling");
}

#[test]
fn session_sport_gates_import() {
    // FIT sport ids: running 1, cycling 2, fitness_equipment 4,
    // cross_country_skiing 12; sub_sport ids: generic 0, treadmill 1, trail 3,
    // elliptical 15.
    let with_session = |sport: u8, sub_sport: u8| {
        let mut data = gps_accuracy_messages();
        data.extend_from_slice(&session_messages(sport, sub_sport));
        let mut file = fit_header(data.len() as u32);
        file.extend_from_slice(&data);
        let crc = fit_crc(&file);
        file.extend_from_slice(&crc.to_le_bytes());
        parse_fit(&file)
    };
    assert!(with_session(1, 0).is_ok(), "road run");
    assert!(with_session(1, 3).is_ok(), "trail run");
    assert!(with_session(1, 1).is_ok(), "treadmill run");
    assert!(
        with_session(4, 1).is_ok(),
        "treadmill logged as fitness equipment"
    );
    assert!(
        with_session(0, 0).is_ok(),
        "generic sport is unknown, not a refusal"
    );
    assert!(
        with_session(200, 0).is_ok(),
        "an unnamed sport id is unknown"
    );
    assert_eq!(
        with_session(2, 0),
        Err(FitError::NotARun("cycling".to_string()))
    );
    assert_eq!(
        with_session(4, 15),
        Err(FitError::NotARun("fitness equipment".to_string()))
    );
    assert_eq!(
        with_session(12, 0),
        Err(FitError::NotARun("cross country skiing".to_string()))
    );
}

/// Track extraction without the sport gate, so the sport-agnostic decode
/// paths can be exercised on the real (cycling) Fenix fixture.
fn track_any_sport(bytes: &[u8]) -> Result<FitTrack, FitError> {
    validate_fit_header(bytes)?;
    let (records, truncated) = decode_records_salvaging(bytes);
    track_from_records(&records, truncated)
}

/// A 12-byte FIT header (no header CRC) declaring `data_size` message bytes.
fn fit_header(data_size: u32) -> Vec<u8> {
    let mut header = vec![12u8, 0x10];
    header.extend_from_slice(&100u16.to_le_bytes()); // profile version (arbitrary)
    header.extend_from_slice(&data_size.to_le_bytes());
    header.extend_from_slice(b".FIT");
    header
}

/// A `session` definition (local type 1, global mesg 18) with sport (5) and
/// sub_sport (6) enum fields, plus one data message carrying them.
fn session_messages(sport: u8, sub_sport: u8) -> Vec<u8> {
    let mut data = vec![0x41, 0x00, 0x00];
    data.extend_from_slice(&18u16.to_le_bytes());
    data.push(2); // field count
    data.extend_from_slice(&[5, 1, 0x00]); // sport: enum
    data.extend_from_slice(&[6, 1, 0x00]); // sub_sport: enum
    data.extend_from_slice(&[0x01, sport, sub_sport]);
    data
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
    let track = track_any_sport(&bytes).expect("real device file should parse");

    assert_eq!(
        track.segments.len(),
        1,
        "no stop_all/start pair in this file"
    );
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
    // A different real device file (sport = running, so the sport gate lets it
    // through) with no heart-rate strap: hr_bpm must come back None, not a
    // fabricated/default value.
    let bytes = fixture("Activity.fit");
    let track = parse_fit(&bytes).expect("real device file should parse");
    let total: usize = track.segments.iter().map(Vec::len).sum();
    assert_eq!(total, 14);
    assert!(
        track.segments.iter().flatten().all(|f| f.hr_bpm.is_none()),
        "this fixture has no HR strap data"
    );
    assert!(
        track
            .segments
            .iter()
            .flatten()
            .all(|f| f.accuracy_m.is_none()),
        "this fixture has no gps_accuracy field → accuracy stays None"
    );
}

/// FIT CRC-16 (ANT variant) over a byte slice, mirrors fitparser's own
/// `de::crc::get_crc`. Needed to hand-encode a valid FIT file below (the
/// trailing 2-byte data CRC is validated by `from_bytes` by default).
fn fit_crc(data: &[u8]) -> u16 {
    const TABLE: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
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

/// One `record` definition (local type 0, global mesg 20, little-endian) with
/// fields timestamp(253,u32) position_lat(0,i32) position_long(1,i32)
/// gps_accuracy(31,u8), then two data records carrying 8 m and 12 m.
fn gps_accuracy_messages() -> Vec<u8> {
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
    data
}

#[test]
fn synthetic_gps_accuracy_field_flows_through() {
    // Neither real-device fixture in shared/tests/fixtures/ records
    // `gps_accuracy`, so hand-encode a minimal valid FIT that DOES: the
    // only way to prove the field is read and surfaced (not fabricated).
    let data = gps_accuracy_messages();
    // 12-byte header (no header CRC → header bytes join the data CRC).
    let mut file = fit_header(data.len() as u32);
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
    assert_eq!(
        fix_from_record(&sentinel_lat),
        None,
        "sentinel lat drops the fix"
    );

    let sentinel_lon = record(vec![
        field("timestamp", 253, Value::UInt32(1_500_000_000)),
        field("position_lat", 0, Value::SInt32(626_000_000)),
        field("position_long", 1, Value::SInt32(i32::MAX)),
    ]);
    assert_eq!(
        fix_from_record(&sentinel_lon),
        None,
        "sentinel lon drops the fix"
    );

    // A valid pair still yields a fix: the guard must not over-reject.
    let good = record(vec![
        field("timestamp", 253, Value::UInt32(1_500_000_000)),
        field("position_lat", 0, Value::SInt32(626_000_000)),
        field("position_long", 1, Value::SInt32(157_000_000)),
    ]);
    assert!(
        fix_from_record(&good).is_some(),
        "a valid record still yields a fix"
    );
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
    assert_eq!(
        fix.hr_bpm, None,
        "0xFF sentinel must not surface as 255 bpm"
    );

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
