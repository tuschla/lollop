use super::*;
use crate::gps::{MAX_FIX_GAP_SEC, unix_to_rfc3339_utc as iso};
use crate::import::number::kt_to_double;
use crate::import::raw::hr_bpm;
use crate::import::time::parse_import_time_sec;
use crate::import::xml::{MAX_IMPORT_XML_DEPTH, SNIFF_BYTES, decode_utf16};
use crate::test_support::fit_writer::{ENUM, FIT_EPOCH, FitWriter, I32, U8, U32, semi};
use crate::test_support::{M_PER_DEG, SplitMix};

/// One fix of a test file.
#[derive(Clone, Copy)]
struct Fix {
    lat: f64,
    lon: f64,
    t: i64,
    hr: Option<i32>,
}

/// A fix without a heart-rate sample.
fn at(lat: f64, lon: f64, t: i64) -> Fix {
    Fix {
        lat,
        lon,
        t,
        hr: None,
    }
}

/// A track ending at `end`: `metres_per_fix` apart, one fix per 10 s, heading
/// north from the equator.
fn track(end: i64, metres_per_fix: f64, fixes: usize) -> Vec<Fix> {
    let start = end - (fixes as i64 - 1) * 10;
    (0..fixes)
        .map(|k| {
            at(
                k as f64 * metres_per_fix / M_PER_DEG,
                0.0,
                start + k as i64 * 10,
            )
        })
        .collect()
}

/// An easy 6:00 /km run: 27.8 m every 10 s for 10 minutes.
fn easy_run(end: i64) -> Vec<Fix> {
    track(end, 27.8, 61)
}

/// A seeded wandering 1 Hz run at ~3 m/s.
fn run(n: usize, t0: i64, seed: u64) -> Vec<Fix> {
    let mut rng = SplitMix(seed);
    let (mut lat, mut lon) = (47.37, 8.54);
    (0..n)
        .map(|k| {
            lat += 3.0 * (0.9 + 0.2 * rng.unit()) / M_PER_DEG;
            lon += (k as f64 * 0.05).sin() * 1.5 / (M_PER_DEG * lat.to_radians().cos());
            Fix {
                lat,
                lon,
                t: t0 + k as i64,
                hr: Some(120 + (k % 40) as i32),
            }
        })
        .collect()
}

const T0: i64 = 1_750_000_000;

/// A GPX 1.1 document: one `<trk>` (typed `kind`) of `segments`, each fix
/// with an `<hdop>0.9</hdop>` when `hdop` and its heart rate when it has one.
fn gpx(segments: &[Vec<Fix>], kind: Option<&str>, hdop: bool) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"fuzz\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\" \
         xmlns:gpxtpx=\"http://www.garmin.com/xmlschemas/TrackPointExtension/v1\">\n\
         <metadata><name>m</name></metadata>\n<trk><name>t</name>",
    );
    if let Some(kind) = kind {
        s += &format!("<type>{kind}</type>");
    }
    s += "\n";
    for seg in segments {
        s += "<trkseg>\n";
        for f in seg {
            s += &format!(
                "<trkpt lat=\"{}\" lon=\"{}\"><ele>400.0</ele><time>{}</time>",
                f.lat,
                f.lon,
                iso(f.t)
            );
            if hdop {
                s += "<hdop>0.9</hdop>";
            }
            if let Some(hr) = f.hr {
                s += &format!(
                    "<extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>{hr}</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions>"
                );
            }
            s += "</trkpt>\n";
        }
        s += "</trkseg>\n";
    }
    s + "</trk>\n</gpx>\n"
}

/// A TCX document: one `<Activity>` per `(sport, laps)`, one `<Lap>` and
/// `<Track>` per lap (an empty lap writes an empty track).
fn tcx(activities: &[(&str, Vec<Vec<Fix>>)]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TrainingCenterDatabase \
         xmlns=\"http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2\">\n<Activities>\n",
    );
    for (sport, laps) in activities {
        let first = laps
            .iter()
            .flatten()
            .next()
            .expect("an activity with a fix");
        s += &format!("<Activity Sport=\"{sport}\"><Id>{}</Id>\n", iso(first.t));
        for lap in laps {
            s += &format!(
                "<Lap StartTime=\"{}\"><TotalTimeSeconds>1</TotalTimeSeconds><Track>\n",
                iso(lap.first().unwrap_or(first).t)
            );
            for f in lap {
                s += &format!(
                    "<Trackpoint><Time>{}</Time><Position><LatitudeDegrees>{}</LatitudeDegrees>\
                     <LongitudeDegrees>{}</LongitudeDegrees></Position>",
                    iso(f.t),
                    f.lat,
                    f.lon
                );
                if let Some(hr) = f.hr {
                    s += &format!("<HeartRateBpm><Value>{hr}</Value></HeartRateBpm>");
                }
                s += "</Trackpoint>\n";
            }
            s += "</Track></Lap>\n";
        }
        s += "</Activity>\n";
    }
    s + "</Activities>\n</TrainingCenterDatabase>\n"
}

/// The `record` fields every test FIT writes: timestamp, position_lat,
/// position_long, heart_rate (0xFF, the invalid value, without a sample).
const FIT_RECORD: [(u8, u8, u8); 4] = [(253, 4, U32), (0, 4, I32), (1, 4, I32), (3, 1, U8)];

/// A FIT `record` body for `f` with `extra` fields after [`FIT_RECORD`]'s.
fn fit_record(f: &Fix, extra: &[u8]) -> Vec<u8> {
    let mut body = ((f.t - FIT_EPOCH) as u32).to_le_bytes().to_vec();
    body.extend_from_slice(&semi(f.lat).to_le_bytes());
    body.extend_from_slice(&semi(f.lon).to_le_bytes());
    body.push(f.hr.map_or(0xFF, |hr| hr as u8));
    body.extend_from_slice(extra);
    body
}

/// Close `w` with a session of `sport` (1 running, 2 cycling) at the end, as
/// devices write it.
fn finish_with_session(mut w: FitWriter, sport: u8) -> Vec<u8> {
    w.def(15, 18, false, &[(5, 1, ENUM), (6, 1, ENUM)], &[]);
    w.msg(15, &[sport, 0]);
    w.finish(true)
}

/// A FIT activity of `fixes` and a session with `sport`.
fn fit(fixes: &[Fix], sport: u8) -> Vec<u8> {
    let mut w = FitWriter::default();
    w.def(0, 20, false, &FIT_RECORD, &[]);
    for f in fixes {
        w.msg(0, &fit_record(f, &[]));
    }
    finish_with_session(w, sport)
}

/// The file's verdict as runs, or the reason (and sport) it was refused.
fn verdict(bytes: &[u8]) -> Result<Vec<ImportedRun>, (ImportRefusal, Option<String>)> {
    let v = import_run_file(bytes);
    match v.refusal {
        None => {
            assert!(!v.runs.is_empty() && v.sport.is_none(), "{v:?}");
            Ok(v.runs)
        }
        Some(reason) => {
            assert!(v.runs.is_empty(), "{v:?}");
            Err((reason, v.sport))
        }
    }
}

fn refused(bytes: &[u8]) -> ImportRefusal {
    verdict(bytes).expect_err("refused").0
}

fn one_run(bytes: &[u8]) -> ImportedRun {
    let mut runs = verdict(bytes).expect("imports");
    assert_eq!(runs.len(), 1);
    runs.remove(0)
}

/// [`gpx`] of one segment, untyped, without hdop.
fn plain_gpx(fixes: &[Fix]) -> String {
    gpx(&[fixes.to_vec()], None, false)
}

/// A 300-fix TCX run of two laps whose second lap starts `gap` seconds after
/// the first one's last fix: its points and segment starts.
fn two_laps(gap: i64) -> (usize, Vec<u32>) {
    let a = run(300, T0, 2);
    let shift = gap - (a[150].t - a[149].t);
    let second: Vec<Fix> = a[150..]
        .iter()
        .map(|f| Fix {
            t: f.t + shift,
            ..*f
        })
        .collect();
    let r = one_run(tcx(&[("Running", vec![a[..150].to_vec(), second])]).as_bytes());
    (r.points.len(), r.segment_starts)
}

#[test]
fn consecutive_autolap_tracks_merge_into_one_segment() {
    // A lap starting 0 s up to the live recorder's segment-break gap after the
    // previous one is an autolap, not a pause.
    for gap in [0, 1, MAX_FIX_GAP_SEC] {
        assert_eq!(two_laps(gap), (300, vec![]), "gap {gap} s");
    }
}

#[test]
fn a_real_pause_or_a_backwards_clock_step_still_splits_the_segment() {
    for gap in [MAX_FIX_GAP_SEC + 1, 300, -100] {
        assert_eq!(two_laps(gap), (300, vec![150]), "gap {gap} s");
    }
}

#[test]
fn empty_tracks_are_skipped() {
    // Empty laps around and between two autolaps neither split nor add fixes.
    let a = run(300, T0, 2);
    let laps = vec![vec![], a[..150].to_vec(), vec![], a[150..].to_vec(), vec![]];
    let r = one_run(tcx(&[("Running", laps)]).as_bytes());
    assert_eq!((r.points.len(), r.segment_starts), (300, vec![]));
}

#[test]
fn hr_is_kept_only_as_a_finite_plausible_bpm() {
    let (lo, hi) = (*PLAUSIBLE_HR_BPM.start(), *PLAUSIBLE_HR_BPM.end());
    let cases: Vec<(String, Option<i32>)> = vec![
        ("120".into(), Some(120)),
        (" 155 ".into(), Some(155)),
        ("155.7".into(), Some(155)),
        (lo.to_string(), Some(lo)),
        (hi.to_string(), Some(hi)),
        ((lo - 1).to_string(), None),
        ((hi + 1).to_string(), None),
        ("1000000000".into(), None),
        ("0".into(), None),
        ("-40".into(), None),
        ("Infinity".into(), None),
        ("-Infinity".into(), None),
        ("NaN".into(), None),
        ("".into(), None),
        ("abc".into(), None),
    ];
    for (raw, want) in &cases {
        assert_eq!(hr_bpm(raw), *want, "{raw:?}");
    }
}

#[test]
fn hdop_converts_to_metres_and_anything_else_is_unknown() {
    // accuracy ≈ hdop × 5 m (nominal 1-sigma UERE). Missing, garbage or
    // non-positive hdop is unknown: the QC bound, never 0.
    let pts = run(300, T0, 1);
    let first_accuracy = |hdop: Option<&str>| {
        let doc = gpx(std::slice::from_ref(&pts), Some("running"), hdop.is_some());
        let doc = doc.replacen(
            "<hdop>0.9</hdop>",
            &format!("<hdop>{}</hdop>", hdop.unwrap_or_default()),
            1,
        );
        one_run(doc.as_bytes()).points[0].accuracy_m
    };
    assert_eq!(first_accuracy(Some("2.5")), 12.5);
    assert_eq!(first_accuracy(Some(" 0.8 ")), 4.0);
    for unknown in [
        None,
        Some(""),
        Some("abc"),
        Some("0"),
        Some("-1"),
        Some("NaN"),
        Some("Infinity"),
    ] {
        assert_eq!(
            first_accuracy(unknown),
            gps::MAX_GPS_ACCURACY_M,
            "{unknown:?}"
        );
    }
}

#[test]
fn numbers_read_as_the_jvm_reads_them() {
    let cases: &[(&str, Option<f64>)] = &[
        ("47.0", Some(47.0)),
        (" 47.3 ", Some(47.3)),
        ("\t47.0\n", Some(47.0)),
        ("+47", Some(47.0)),
        ("-0", Some(-0.0)),
        ("1.34072E1", Some(13.4072)),
        ("47.", Some(47.0)),
        (".5", Some(0.5)),
        ("47.0d", Some(47.0)),
        ("47f", Some(47.0)),
        ("0x1p3", Some(8.0)),
        ("0X1.8P1", Some(3.0)),
        ("1e400", Some(f64::INFINITY)),
        ("4,7", None),
        ("", None),
        (".", None),
        ("e5", None),
        ("1e", None),
        ("inf", None),
        ("infinity", None),
        ("nan", None),
        ("0x1", None),
        ("4 7", None),
        ("47\u{A0}", None),
    ];
    for (raw, want) in cases {
        let got = kt_to_double(raw);
        assert_eq!(
            got.map(f64::to_bits),
            want.map(f64::to_bits),
            "{raw:?}: {got:?}"
        );
    }
    assert!(kt_to_double("NaN").is_some_and(f64::is_nan));
    assert_eq!(kt_to_double(&"4".repeat(5000)), Some(f64::INFINITY));
}

#[test]
fn trimming_is_the_jvm_kotlin_whitespace_set() {
    assert_eq!(kt_trim("\u{1C}\u{A0} run \u{2003}\u{3000}"), "run");
    assert_eq!(kt_trim("\u{85}run"), "\u{85}run");
}

#[test]
fn spec_utc_timestamps_parse_to_their_instant() {
    assert_eq!(parse_import_time_sec("2026-08-30T10:00:00Z"), 1_788_084_000);
    // An explicit offset resolves to the same instant (10:00+02:00 == 08:00Z).
    assert_eq!(
        parse_import_time_sec("2026-08-30T10:00:00+02:00"),
        1_788_076_800
    );
}

#[test]
fn colonless_offset_parses_to_its_instant() {
    assert_eq!(
        parse_import_time_sec("2026-08-30T10:00:00+0200"),
        1_788_076_800
    );
    assert_eq!(
        parse_import_time_sec("2026-08-30T10:00:00.000+0000"),
        1_788_084_000
    );
}

#[test]
fn a_zone_naive_timestamp_parses_as_utc_not_dropped() {
    // A dropped time made a recorded run read as "a planned route".
    assert_eq!(parse_import_time_sec("2026-08-30T10:00:00"), 1_788_084_000);
}

#[test]
fn a_missing_or_garbage_timestamp_is_zero() {
    for s in ["", "   ", "not-a-timestamp"] {
        assert_eq!(parse_import_time_sec(s), 0, "{s:?}");
    }
}

#[test]
fn import_times_read_exactly_as_java_time_read_them() {
    // `fixtures/import/times.json`: stamps and what java.time's parsers (the
    // shell's original reader) made of them.
    let cases: Vec<(String, i64)> =
        serde_json::from_str(include_str!("../../tests/fixtures/import/times.json"))
            .expect("fixture");
    assert!(cases.len() > 1000);
    let mismatches: Vec<String> = cases
        .iter()
        .filter(|(s, want)| parse_import_time_sec(s) != *want)
        .map(|(s, want)| format!("{s:?}: java.time {want}, core {}", parse_import_time_sec(s)))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} differ: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(10)]
    );
}

#[test]
fn utf16_decodes_exactly_as_the_jdk() {
    // `fixtures/import/utf16.json`: byte strings and the JDK's decoding of them
    // (big-endian flag, hex bytes, the decoded text's UTF-16 units).
    let cases: Vec<(bool, String, Vec<u16>)> =
        serde_json::from_str(include_str!("../../tests/fixtures/import/utf16.json"))
            .expect("fixture");
    assert!(cases.len() > 1000);
    for (big_endian, hex, units) in &cases {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|k| u8::from_str_radix(&hex[k..k + 2], 16).unwrap())
            .collect();
        // A lone surrogate the JDK keeps as a char reads as U+FFFD here: a Rust
        // string has none.
        let want: String = char::decode_utf16(units.iter().copied())
            .map(|c| c.unwrap_or('\u{FFFD}'))
            .collect();
        assert_eq!(
            decode_utf16(&bytes, *big_endian),
            want,
            "{hex} be={big_endian}"
        );
    }
}

#[test]
fn a_dtd_in_the_prolog_is_found_past_declarations_and_comments() {
    let doc = gpx(&[run(300, T0, 1)], Some("running"), true);
    let body = doc.split_once("?>\n").unwrap().1;
    let after_comment = format!("<?xml version=\"1.0\"?>\n<!-- c --> <!DOCTYPE gpx>{body}");
    assert_eq!(refused(after_comment.as_bytes()), ImportRefusal::Damaged);
    // A DTD only inside a comment declares none.
    let commented = format!("<!-- <!DOCTYPE gpx> -->{body}");
    assert_eq!(one_run(commented.as_bytes()).points.len(), 300);
}

/// Two fixes 0.0054° (≈ 600 m) apart in 200 s on the equator: past the keep
/// threshold at a running pace.
fn short_run() -> Vec<Fix> {
    vec![at(0.0, 0.0, 1000), at(0.0, 0.0054, 1200)]
}

#[test]
fn unknown_accuracy_gets_the_qc_bound_never_a_fabricated_good_fix() {
    let run = one_run(plain_gpx(&short_run()).as_bytes());
    assert_eq!(run.points.len(), 2);
    assert!(
        run.points
            .iter()
            .all(|p| p.accuracy_m == gps::MAX_GPS_ACCURACY_M)
    );
}

#[test]
fn a_real_accuracy_passes_through_unchanged() {
    // A FIT `gps_accuracy` (field 31, whole metres) per record.
    let mut w = FitWriter::default();
    let mut fields = FIT_RECORD.to_vec();
    fields.push((31, 1, U8));
    w.def(0, 20, false, &fields, &[]);
    for (k, acc) in [8, 12].into_iter().enumerate() {
        let f = at(0.0, k as f64 * 0.0054, T0 + 200 * k as i64);
        w.msg(0, &fit_record(&f, &[acc]));
    }
    let run = one_run(&finish_with_session(w, 1));
    assert_eq!(
        run.points.iter().map(|p| p.accuracy_m).collect::<Vec<_>>(),
        [8.0, 12.0]
    );
}

/// A fix on the equator at `lon` with heart rate `hr`.
fn hr_fix(lon: f64, t: i64, hr: Option<i32>) -> Fix {
    Fix {
        hr,
        ..at(0.0, lon, t)
    }
}

#[test]
fn the_hr_average_is_time_weighted_so_a_long_held_sample_dominates() {
    // The first sample is held 300 s, the second 5 s: a plain mean (142.5)
    // would sit ~27 bpm below the long-held one.
    let run = one_run(
        plain_gpx(&[
            hr_fix(0.000, 1000, Some(170)),
            hr_fix(0.005, 1300, Some(115)),
            hr_fix(0.00502, 1305, None),
        ])
        .as_bytes(),
    );
    let avg = run.avg_hr_bpm.unwrap();
    assert!((avg - 170.0).abs() < 1.0, "{avg}");
}

#[test]
fn evenly_spaced_hr_samples_match_the_plain_mean() {
    let run = one_run(
        plain_gpx(&[
            hr_fix(0.0000, 1000, Some(100)),
            hr_fix(0.0030, 1100, Some(120)),
            hr_fix(0.0060, 1200, Some(140)),
        ])
        .as_bytes(),
    );
    assert_eq!(run.avg_hr_bpm, Some(120.0));
    assert_eq!(one_run(plain_gpx(&short_run()).as_bytes()).avg_hr_bpm, None);
}

#[test]
fn the_heart_rate_is_a_share_of_a_measured_hr_max_only_capped_at_100() {
    assert!((imported_hr_pct_max(Some(152.0), Some(190.0)) - 80.0).abs() < 1e-9);
    // No measured HRmax: no figure (never the Tanaka estimate).
    assert_eq!(imported_hr_pct_max(Some(152.0), None), 0.0);
    assert_eq!(imported_hr_pct_max(Some(152.0), Some(0.0)), 0.0);
    assert_eq!(imported_hr_pct_max(Some(152.0), Some(-190.0)), 0.0);
    assert_eq!(imported_hr_pct_max(Some(152.0), Some(140.0)), 100.0);
    assert_eq!(imported_hr_pct_max(None, Some(190.0)), 0.0);
}

#[test]
fn a_named_non_running_sport_is_recognised_and_anything_else_stays_importable() {
    for bike in [
        "Biking",
        "cycling",
        "Ride",
        "VirtualRide",
        "road_biking",
        "mountain-biking",
        "ebike",
        "1 cycling",
    ] {
        assert_eq!(non_run_sport(Some(bike)), Some("cycling"), "{bike}");
    }
    assert_eq!(non_run_sport(Some("lap_swimming")), Some("swimming"));
    assert_eq!(non_run_sport(Some("alpine_skiing")), Some("skiing"));
    assert_eq!(non_run_sport(Some("hiking")), Some("hiking"));
    assert_eq!(non_run_sport(Some("Nordic Walking")), Some("walking"));
    // Running in any word wins, in any language or mixed label; unnamed,
    // generic or unknown labels import.
    let runs = [
        "",
        "Running",
        "trail_running",
        "treadmill_running",
        "Run",
        "jogging",
        "Other",
        "9",
        "1",
        "strides",
        "Trail run",
        "TrailRun",
        "VirtualRun",
        "Laufen",
        "Traillauf",
        "Course à pied",
        "Trail",
        "Carrera",
        "Correr",
        "Corsa",
        "Hardlopen",
        "Löpning",
        "Løb",
        "Juoksu",
        "Bieganie",
        "Běh",
        "Futás",
        "Koşu",
        "Бег",
        "ランニング",
        "跑步",
        "Skriešana",
        "Running/Walking",
        "Walk/Run",
        "Run/Walk",
        "Hike & Run",
        "Swimrun",
        "street_running",
        "Skyrunning",
        "Fell running",
        "Orienteering",
        "Treadmill",
    ];
    let refused: Vec<String> = runs
        .iter()
        .filter_map(|r| non_run_sport(Some(r)).map(|s| format!("{r}→{s}")))
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(non_run_sport(None), None);
}

#[test]
fn strava_compound_ride_and_ski_types_are_recognised() {
    let compounds = [
        ("MountainBikeRide", "cycling"),
        ("GravelRide", "cycling"),
        ("EMountainBikeRide", "cycling"),
        ("EBikeRide", "cycling"),
        ("Handcycle", "cycling"),
        ("Velomobile", "cycling"),
        ("NordicSki", "skiing"),
        ("AlpineSki", "skiing"),
        ("BackcountrySki", "skiing"),
        ("RollerSki", "skiing"),
    ];
    for (label, sport) in compounds {
        assert_eq!(non_run_sport(Some(label)), Some(sport), "{label}");
    }
}

#[test]
fn a_ride_named_by_its_file_is_refused_by_name() {
    let ride = gpx(&[short_run()], Some("Biking"), false);
    assert_eq!(
        verdict(ride.as_bytes()),
        Err((ImportRefusal::NotARun, Some("cycling".into())))
    );
}

#[test]
fn a_file_named_logged_at_stamp_is_kept_only_when_it_follows_the_track() {
    // lollop's own export names the run's logged-at stamp.
    let points: Vec<GpsPoint> = short_run()
        .iter()
        .map(|f| GpsPoint {
            lat: f.lat,
            lon: f.lon,
            observed_at: f.t,
            accuracy_m: 5.0,
        })
        .collect();
    let stamped =
        |logged_at| one_run(gps::export_gpx(&points, "run", &[], logged_at).as_bytes()).observed_at;
    // A standing-still finish or a trimmed end: the run's stamp follows its last fix.
    assert_eq!(stamped(Some(1500)), 1500);
    // None, one before the track ends, or one days later: the last fix.
    assert_eq!(stamped(None), 1200);
    assert_eq!(stamped(Some(1100)), 1200);
    assert_eq!(stamped(Some(1200 + 3 * 86_400)), 1200);
}

#[test]
fn an_implausibly_fast_track_is_refused() {
    // 12 m/s ≈ 1:23 /km, far faster than the 2:50 /km gate: a ride or a drive.
    assert_eq!(
        refused(plain_gpx(&track(10_000, 120.0, 61)).as_bytes()),
        ImportRefusal::ImplausiblePace
    );
    assert!(verdict(plain_gpx(&easy_run(20_000)).as_bytes()).is_ok());
}

#[test]
fn a_fragment_under_the_keep_threshold_is_too_short() {
    // A 13 s, 3 m blip; 0.6 km in 2:30; and two 80 s pieces split by a long
    // pause (the bridge counts neither distance nor time, so 2:40 in all).
    let blip = vec![vec![at(0.0, 0.0, 9_987), at(3.0 / M_PER_DEG, 0.0, 10_000)]];
    let piece = |from: i64, t0: i64| -> Vec<Fix> {
        (0..9)
            .map(|k| at((from + k) as f64 * 30.0 / M_PER_DEG, 0.0, t0 + k * 10))
            .collect()
    };
    let split = vec![piece(0, 40_000), piece(100, 41_000)];
    for segments in [blip, vec![track(20_000, 40.0, 16)], split] {
        assert_eq!(
            refused(gpx(&segments, None, false).as_bytes()),
            ImportRefusal::TooShort
        );
    }
    // Just over both thresholds: kept.
    assert!(verdict(plain_gpx(&track(50_000, 30.0, 20)).as_bytes()).is_ok());
}

#[test]
fn the_gate_judges_moving_pace_so_stops_cannot_hide_a_speed() {
    // 3:20 /km (5 m/s) is a fast but real run.
    let brisk: Vec<Fix> = (0..61)
        .map(|k| at(k as f64 * 50.0 / M_PER_DEG, 0.0, 10_000 + k * 10))
        .collect();
    assert!(verdict(plain_gpx(&brisk).as_bytes()).is_ok());
    // 2:30 /km while moving, standing still every other 10 s: 5:00 /km
    // elapsed, but the moving pace is what counts.
    let stop_start: Vec<Fix> = (0..41)
        .map(|k| at(((k + 1) / 2) as f64 * 66.7 / M_PER_DEG, 0.0, 1_000 + k * 10))
        .collect();
    assert_eq!(
        refused(plain_gpx(&stop_start).as_bytes()),
        ImportRefusal::ImplausiblePace
    );
}

#[test]
fn untimed_and_out_of_range_fixes_are_dropped_before_the_gates() {
    let mut fixes = easy_run(20_000);
    fixes[4].lat = 91.0;
    fixes[5].lon = -180.5;
    let untimed = plain_gpx(&fixes).replacen(&format!("<time>{}</time>", iso(fixes[3].t)), "", 1);
    assert_eq!(one_run(untimed.as_bytes()).points.len(), 58);
    let no_times =
        plain_gpx(&[at(0.0, 0.0, 1000), at(0.0, 0.1, 1200)]).replace("<time>", "<time>x");
    assert_eq!(refused(no_times.as_bytes()), ImportRefusal::NoTimestamps);
    assert_eq!(
        refused(plain_gpx(&[at(0.0, 0.0, 1000)]).as_bytes()),
        ImportRefusal::NoTrack
    );
}

#[test]
fn a_single_gnss_teleport_is_dropped_and_a_jump_never_returned_from_is_kept() {
    let mut fixes = easy_run(20_000);
    fixes[30].lat = 10.0;
    let run = one_run(plain_gpx(&fixes).as_bytes());
    assert_eq!(run.points.len(), 60);
    assert!(run.points.iter().all(|p| p.lat < 1.0));
    // A car ride: the track never comes back, so the jump stays and the pace gate judges it.
    let mut drive = easy_run(30_000);
    for f in &mut drive[31..] {
        f.lat += 0.5;
    }
    assert_eq!(
        refused(plain_gpx(&drive).as_bytes()),
        ImportRefusal::ImplausiblePace
    );
}

#[test]
fn a_huge_track_is_decimated_with_its_boundaries_and_ends() {
    let fixes: Vec<Fix> = (0..20_000)
        .map(|k| at(k as f64 * 3.0 / M_PER_DEG, 0.0, T0 + k))
        .collect();
    let (a, b) = fixes.split_at(12_345);
    let b: Vec<Fix> = b.iter().map(|f| Fix { t: f.t + 600, ..*f }).collect();
    let run = one_run(gpx(&[a.to_vec(), b], None, false).as_bytes());
    assert!(
        run.points.len() <= gps::TRACK_DECIMATION_CAP as usize + 2,
        "{}",
        run.points.len()
    );
    assert_eq!(run.points[0].observed_at, T0);
    assert_eq!(run.observed_at, T0 + 19_999 + 600);
    assert_eq!(run.segment_starts.len(), 1);
    let start = run.points[run.segment_starts[0] as usize];
    assert_eq!(start.observed_at, T0 + 12_345 + 600);
}

#[test]
fn each_gpx_track_is_its_own_run_with_its_own_sport() {
    // A 10 km run and a 5 km ride in one GPX: the run imports, the ride is refused.
    let trkpts = |n: usize, lat0: f64, t0: i64, mps: f64| -> String {
        (0..n)
            .map(|k| {
                format!(
                    "<trkpt lat=\"{}\" lon=\"8.54\"><time>{}</time></trkpt>",
                    lat0 + k as f64 * mps / M_PER_DEG,
                    iso(t0 + k as i64)
                )
            })
            .collect()
    };
    let ride_start = T0 + 3000 + 60;
    let doc = format!(
        "<?xml version=\"1.0\"?><gpx version=\"1.1\"><trk><type>running</type><trkseg>{}</trkseg></trk>\
         <trk><type>cycling</type><trkseg>{}</trkseg></trk></gpx>",
        trkpts(3000, 47.37, T0, 3.33),
        trkpts(720, 47.37 + 3000.0 * 3.33 / M_PER_DEG, ride_start, 7.0)
    );
    let imported = verdict(doc.as_bytes()).unwrap();
    assert_eq!(
        imported.iter().map(|r| r.observed_at).collect::<Vec<_>>(),
        [T0 + 2999]
    );
    // Only rides: the first one's reason is the file's.
    let rides = doc.replace("<type>running</type>", "<type>Ride</type>");
    assert_eq!(
        verdict(rides.as_bytes()),
        Err((ImportRefusal::NotARun, Some("cycling".into())))
    );
}

#[test]
fn a_file_that_is_neither_fit_nor_gpx_nor_tcx_is_refused_from_its_header() {
    assert_eq!(
        refused(b"name,distance\nmonday,5"),
        ImportRefusal::WrongType
    );
    assert_eq!(refused(b""), ImportRefusal::WrongType);
    // The root's `<gpx` must sit whole in the first SNIFF_BYTES.
    let root_at = |at: usize| format!("<!--{}--><gpx></gpx>", "x".repeat(at - 7));
    assert_eq!(
        refused(root_at(SNIFF_BYTES - 3).as_bytes()),
        ImportRefusal::WrongType
    );
    assert_eq!(
        refused(root_at(SNIFF_BYTES - 4).as_bytes()),
        ImportRefusal::NoTrack
    );
}

#[test]
fn a_file_over_the_cap_is_refused_unread() {
    let mut fit_header = vec![0u8; MAX_FIT_IMPORT_BYTES + 1];
    fit_header[8..12].copy_from_slice(b".FIT");
    assert_eq!(refused(&fit_header), ImportRefusal::TooLarge);
    // A FIT at its cap reaches the FIT reader (all zeros past the magic: no track).
    fit_header.pop();
    assert_ne!(refused(&fit_header), ImportRefusal::TooLarge);
    let huge = vec![b' '; MAX_RUN_IMPORT_BYTES as usize + 1];
    assert_eq!(refused(&huge), ImportRefusal::TooLarge);
}

#[test]
fn a_fit_refusal_keeps_its_reason() {
    let pts = run(900, T0, 8);
    let header_only = [14, 0x20, 0, 0, 0, 0, 0, 0, b'.', b'F', b'I', b'T', 0, 0];
    let cases: [(Vec<u8>, ImportRefusal, Option<&str>); 5] = [
        (fit(&pts, 2), ImportRefusal::NotARun, Some("cycling")),
        (fit(&pts[..100], 1), ImportRefusal::TooShort, None),
        (fit(&pts[..1], 1), ImportRefusal::NoTrack, None),
        (fit(&pts, 1)[..30].to_vec(), ImportRefusal::Damaged, None),
        (header_only.to_vec(), ImportRefusal::NoTrack, None),
    ];
    for (bytes, reason, sport) in cases {
        assert_eq!(
            verdict(&bytes),
            Err((reason, sport.map(str::to_owned))),
            "{:02x?}",
            &bytes[..bytes.len().min(16)]
        );
    }
}

#[test]
fn the_same_run_as_fit_gpx_and_tcx_gives_the_same_fixes_and_figures() {
    let pts = run(900, T0, 7);
    let runs = [
        ("fit", one_run(&fit(&pts, 1))),
        (
            "gpx",
            one_run(gpx(std::slice::from_ref(&pts), Some("running"), false).as_bytes()),
        ),
        (
            "tcx",
            one_run(tcx(&[("Running", vec![pts.clone()])]).as_bytes()),
        ),
    ];
    let reference = &runs[1].1;
    for (what, r) in &runs {
        assert_eq!(r.observed_at, reference.observed_at, "{what}");
        assert_eq!(r.avg_hr_bpm, reference.avg_hr_bpm, "{what}");
        assert_eq!(r.points.len(), pts.len(), "{what}");
        for (p, f) in r.points.iter().zip(&pts) {
            assert_eq!(p.observed_at, f.t);
            // The FIT path's fixes are the semicircle-rounded source fixes (~1 cm).
            assert!(
                (p.lat - f.lat).abs() < 1e-7 && (p.lon - f.lon).abs() < 1e-7,
                "{what} {p:?}"
            );
        }
        let (km, min) = crate::app::preview_track_figures(r.points.clone(), &r.segment_starts);
        let (ref_km, ref_min) =
            crate::app::preview_track_figures(reference.points.clone(), &reference.segment_starts);
        assert!(
            (km - ref_km).abs() <= ref_km * 1e-4,
            "{what} {km} vs {ref_km}"
        );
        assert!((min - ref_min).abs() < 1e-9, "{what}");
    }
}

#[test]
fn a_tcx_bike_ride_and_a_gpx_typed_as_cycling_are_refused_as_not_a_run() {
    let pts = run(900, T0, 3);
    let ride = tcx(&[("Biking", vec![pts.clone()])]);
    assert_eq!(
        verdict(ride.as_bytes()),
        Err((ImportRefusal::NotARun, Some("cycling".into())))
    );
    let typed = gpx(&[pts], Some("cycling"), true);
    assert_eq!(
        verdict(typed.as_bytes()),
        Err((ImportRefusal::NotARun, Some("cycling".into())))
    );
}

#[test]
fn a_tcx_activity_is_a_run_and_its_courses_never_join_it() {
    let a = run(300, T0, 2);
    let b = run(300, T0 + 100_000, 3);
    let doc = tcx(&[("Running", vec![a[..150].to_vec(), a[150..].to_vec()]), ("Running", vec![b])]).replace(
        "</Activities>",
        "</Activities><Courses><Course><Track><Trackpoint><Time>2026-03-06T00:00:00Z</Time><Position>\
         <LatitudeDegrees>1</LatitudeDegrees><LongitudeDegrees>2</LongitudeDegrees></Position></Trackpoint>\
         </Track></Course></Courses>",
    );
    let runs = verdict(doc.as_bytes()).unwrap();
    // Two laps 1 s apart are one segment (an autolap), each activity one run.
    assert_eq!(
        runs.iter()
            .map(|r| (r.points.len(), r.segment_starts.len()))
            .collect::<Vec<_>>(),
        [(300, 0), (300, 0)]
    );
}

#[test]
fn a_stray_trkpt_after_the_trkseg_or_outside_the_trk_is_ignored() {
    // Two plausible next fixes of the run, one after its `</trkseg>` and one
    // after its `</trk>`: neither joins it.
    let pts = run(300, T0, 4);
    let last = pts[299];
    let stray = |k: i64| {
        format!(
            "<trkpt lat=\"{}\" lon=\"{}\"><time>{}</time></trkpt>\n",
            last.lat + 3.0 * k as f64 / M_PER_DEG,
            last.lon,
            iso(last.t + k)
        )
    };
    let doc = gpx(std::slice::from_ref(&pts), Some("running"), true)
        .replacen("</trkseg>\n", &format!("</trkseg>\n{}", stray(1)), 1)
        .replacen("</trk>\n", &format!("</trk>\n{}", stray(2)), 1);
    let r = one_run(doc.as_bytes());
    assert_eq!((r.points.len(), r.observed_at), (300, last.t));
}

#[test]
fn a_route_only_or_a_standing_still_file_ends_in_its_own_reason() {
    let route_only = "<?xml version=\"1.0\"?><gpx version=\"1.1\"><rte><rtept lat=\"47.0\" lon=\"8.0\"/>\
        <rtept lat=\"47.1\" lon=\"8.1\"/></rte></gpx>";
    assert_eq!(refused(route_only.as_bytes()), ImportRefusal::NoTrack);
    let still: Vec<Fix> = (0..600).map(|k| at(47.0, 8.0, T0 + 10 * k)).collect();
    assert_eq!(
        refused(gpx(&[still], None, true).as_bytes()),
        ImportRefusal::TooShort
    );
    let untimed = gpx(&[run(300, T0, 1)], None, true).replace("<time>", "<time>x");
    assert_eq!(refused(untimed.as_bytes()), ImportRefusal::NoTimestamps);
    let one = gpx(&[run(1, T0, 1)], None, true);
    assert_eq!(refused(one.as_bytes()), ImportRefusal::NoTrack);
}

#[test]
fn lollops_own_gpx_reads_back_with_its_stamp_times_segments_and_accuracy() {
    let points: Vec<GpsPoint> = run(400, T0, 10)
        .iter()
        .enumerate()
        .map(|(k, f)| GpsPoint {
            lat: f.lat,
            lon: f.lon,
            observed_at: f.t,
            accuracy_m: 3.0 + (k % 7) as f32 + 0.1,
        })
        .collect();
    let logged_at = T0 + 399 + 600;
    let doc = gps::export_gpx(&points, "Run 1.2 km", &[100, 250], Some(logged_at));
    let r = one_run(doc.as_bytes());
    assert_eq!(r.points, points, "bit for bit");
    assert_eq!(r.segment_starts, [100, 250]);
    assert_eq!(r.observed_at, logged_at);
    // A year past 9999 writes java.time's signed year, which reads back.
    let far: Vec<GpsPoint> = points
        .iter()
        .map(|p| GpsPoint {
            observed_at: p.observed_at + 251_652_300_800,
            ..*p
        })
        .collect();
    let doc = gps::export_gpx(&far, "far", &[], None);
    assert!(doc.contains("<time>+10000-"));
    assert_eq!(one_run(doc.as_bytes()).points, far);
}

/// Offsets to cut `doc` at, as a download cut off mid-file: about 150 evenly
/// spaced plus about 150 tag ends (a cut right after a tag is the case a pull
/// parser does not notice), never the whole document.
fn truncation_cuts(doc: &[u8]) -> Vec<usize> {
    let until = doc.len() - 1;
    let step = (doc.len() / 150).max(1);
    let tag_stride = (doc.iter().filter(|&&b| b == b'>').count() / 150).max(1);
    let mut cuts: Vec<usize> = (0..until).step_by(step).collect();
    cuts.extend(
        (1..until)
            .filter(|&k| doc[k - 1] == b'>')
            .step_by(tag_stride),
    );
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

#[test]
fn a_gpx_or_tcx_cut_at_any_offset_is_damaged_or_the_wrong_type_never_a_shorter_run() {
    let gpx_doc = gpx(
        &[run(60, T0, 1), run(60, T0 + 200, 1)],
        Some("running"),
        true,
    );
    let tcx_doc = tcx(&[
        ("Running", vec![run(150, T0, 4)]),
        ("Running", vec![run(150, T0 + 100_000, 5)]),
    ]);
    let after_first = tcx_doc.find("</Activity>").unwrap() + "</Activity>".len();
    for (doc, extra) in [
        (gpx_doc.as_bytes(), None),
        (tcx_doc.as_bytes(), Some(after_first)),
    ] {
        assert!(read_runs(doc).is_ok(), "the whole document reads");
        let mut cuts = truncation_cuts(doc);
        cuts.extend(extra);
        for cut in cuts {
            let reason = refused(&doc[..cut]);
            assert!(
                matches!(reason, ImportRefusal::Damaged | ImportRefusal::WrongType),
                "cut at {cut}: {reason:?}"
            );
        }
    }
}

#[test]
fn a_declared_dtd_is_refused_so_entities_never_expand_or_resolve() {
    let pts = run(300, T0, 1);
    let body = gpx(std::slice::from_ref(&pts), Some("&x;"), true);
    let body = body.split_once("?>\n").unwrap().1;
    for dtd in [
        "<!DOCTYPE gpx [<!ENTITY x SYSTEM \"file:///etc/hostname\">]>",
        "<!DOCTYPE gpx [<!ENTITY x PUBLIC \"-//x\" \"file:///etc/hostname\">]>",
        "<!DOCTYPE gpx SYSTEM \"file:///tmp/x.dtd\">",
        "<!DOCTYPE gpx [<!ENTITY % p SYSTEM \"file:///tmp/x.dtd\"> %p;]>",
        "<!DOCTYPE gpx [<!ENTITY x SYSTEM \"http://127.0.0.1:9/x\">]>",
    ] {
        let doc = format!("<?xml version=\"1.0\"?>{dtd}{body}");
        assert_eq!(refused(doc.as_bytes()), ImportRefusal::Damaged, "{dtd}");
    }
    // Billion laughs: 6 levels would be 3 MB of "lol".
    let mut laughs =
        String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE gpx [\n<!ENTITY l0 \"lol\">\n");
    for k in 1..=6 {
        laughs += &format!(
            "<!ENTITY l{k} \"{}\">\n",
            format!("&l{};", k - 1).repeat(10)
        );
    }
    laughs += "]>\n";
    laughs += &body.replace("&x;", "&l6;");
    assert_eq!(refused(laughs.as_bytes()), ImportRefusal::Damaged);
    // Without a DTD an undefined entity is malformed.
    assert_eq!(refused(body.as_bytes()), ImportRefusal::Damaged);
}

#[test]
fn bom_crlf_latin1_declared_and_utf16_files_import_the_same_run() {
    let base = gpx(&[run(300, T0, 1)], Some("running"), true);
    let reference = one_run(base.as_bytes());
    let latin1: Vec<u8> = base
        .replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"")
        .replace("<name>t</name>", "<name>Lauf am S\u{e9}e</name>")
        .chars()
        .map(|c| c as u8)
        .collect();
    let utf16 = base.replace("encoding=\"UTF-8\"", "encoding=\"UTF-16\"");
    let mut utf16le = vec![0xFF, 0xFE];
    utf16le.extend(utf16.encode_utf16().flat_map(u16::to_le_bytes));
    let mut utf16be = vec![0xFE, 0xFF];
    utf16be.extend(utf16.encode_utf16().flat_map(u16::to_be_bytes));
    let variants: [(&str, Vec<u8>); 6] = [
        (
            "UTF-8 BOM",
            [&[0xEF, 0xBB, 0xBF][..], base.as_bytes()].concat(),
        ),
        ("CRLF", base.replace('\n', "\r\n").into_bytes()),
        ("ISO-8859-1 declared, Latin-1 name", latin1),
        (
            "no XML declaration",
            base.split_once("?>\n").unwrap().1.as_bytes().to_vec(),
        ),
        ("UTF-16LE", utf16le),
        ("UTF-16BE", utf16be),
    ];
    for (what, bytes) in variants {
        assert_eq!(one_run(&bytes), reference, "{what}");
    }
}

#[test]
fn non_finite_huge_and_out_of_range_coordinates_are_dropped_at_the_source() {
    let pts = run(300, T0, 1);
    let invalid = [
        "NaN",
        "Infinity",
        "-Infinity",
        "1e308",
        "-1e308",
        "1e400",
        "-180.0001",
        "4,7",
        "",
    ];
    let long = "4".repeat(5000);
    for bad in invalid.iter().copied().chain([long.as_str(), "91.5"]) {
        // One bad latitude (fix 10) and, when it is no valid longitude either,
        // one bad longitude (fix 20): each dropped, the run still imports.
        let mut doc = gpx(std::slice::from_ref(&pts), Some("running"), true).replacen(
            &format!("lat=\"{}\"", pts[10].lat),
            &format!("lat=\"{bad}\""),
            1,
        );
        let also_lon = bad != "91.5";
        if also_lon {
            doc = doc.replacen(
                &format!("lon=\"{}\"", pts[20].lon),
                &format!("lon=\"{bad}\""),
                1,
            );
        }
        let r = one_run(doc.as_bytes());
        assert_eq!(
            r.points.len(),
            if also_lon { 298 } else { 299 },
            "{}",
            &bad[..bad.len().min(20)]
        );
        assert!(
            r.points
                .iter()
                .all(|p| p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0)
        );
    }
}

#[test]
fn a_single_null_island_fix_does_not_refuse_the_whole_run() {
    let pts = run(1800, T0, 11);
    let doc = gpx(std::slice::from_ref(&pts), Some("running"), true).replacen(
        &format!("lat=\"{}\" lon=\"{}\"", pts[900].lat, pts[900].lon),
        "lat=\"0.0\" lon=\"0.0\"",
        1,
    );
    assert_eq!(one_run(doc.as_bytes()).points.len(), 1799);
}

#[test]
fn out_of_order_duplicated_and_future_timestamps_still_give_sane_runs() {
    let mut pts = run(400, T0, 1);
    pts[50] = pts[49];
    pts[51].t = pts[10].t;
    pts[300].t = 4_102_444_800; // 2100-01-01
    pts.insert(0, pts[5]);
    let later: Vec<Fix> = run(400, T0, 1)
        .iter()
        .map(|f| Fix {
            t: f.t + 2_350_000_000,
            ..*f
        })
        .collect();
    for doc in [
        gpx(std::slice::from_ref(&pts), Some("running"), true),
        gpx(&[later], Some("running"), true),
        tcx(&[("Running", vec![pts.clone()])]),
    ] {
        assert_sane(&import_run_file(doc.as_bytes()));
    }
}

#[test]
fn deeply_nested_and_giant_elements_finish_quickly() {
    let pts = run(200, T0, 1);
    let base = gpx(std::slice::from_ref(&pts), Some("running"), true);
    let nested = |n: usize| {
        base.replacen(
            "<extensions>",
            &format!("<extensions>{}{}", "<x>".repeat(n), "</x>".repeat(n)),
            1,
        )
    };
    assert_eq!(refused(nested(100_000).as_bytes()), ImportRefusal::Damaged);
    // The deepest nesting the reader allows reads; one level more is refused.
    let nested_to = |depth: usize| {
        base.replacen(
            "<trk>",
            &format!(
                "<trk>{}{}",
                "<x>".repeat(depth - 2),
                "</x>".repeat(depth - 2)
            ),
            1,
        )
    };
    assert!(verdict(nested_to(MAX_IMPORT_XML_DEPTH).as_bytes()).is_ok());
    assert_eq!(
        refused(nested_to(MAX_IMPORT_XML_DEPTH + 1).as_bytes()),
        ImportRefusal::Damaged
    );
    let giant = base.replacen(
        "<name>t</name>",
        &format!("<name>{}</name>", "n".repeat(4_000_000)),
        1,
    );
    assert!(verdict(giant.as_bytes()).is_ok());
    let many_segments: Vec<Vec<Fix>> = pts.iter().map(|f| vec![*f]).collect();
    assert_sane(&import_run_file(
        gpx(&many_segments, Some("running"), true).as_bytes(),
    ));
}

/// Every run of `v` holds finite, in-range, timed fixes within the save cap,
/// sorted unique in-range segment starts, a positive stamp and a plausible
/// heart rate.
fn assert_sane(v: &ImportVerdict) {
    assert_eq!(v.runs.is_empty(), v.refusal.is_some(), "{v:?}");
    for r in &v.runs {
        assert!(r.points.len() >= 2);
        assert!(r.points.len() <= gps::TRACK_DECIMATION_CAP as usize + r.segment_starts.len() + 1);
        for p in &r.points {
            assert!(
                p.lat.is_finite()
                    && p.lon.is_finite()
                    && p.lat.abs() <= 90.0
                    && p.lon.abs() <= 180.0,
                "{p:?}"
            );
            assert!(
                p.observed_at > 0 && p.accuracy_m.is_finite() && p.accuracy_m >= 0.0,
                "{p:?}"
            );
        }
        assert!(r.segment_starts.windows(2).all(|w| w[0] < w[1]));
        assert!(
            r.segment_starts
                .iter()
                .all(|&s| (1..r.points.len()).contains(&(s as usize)))
        );
        assert!(r.observed_at > 0);
        let (lo, hi) = (*PLAUSIBLE_HR_BPM.start(), *PLAUSIBLE_HR_BPM.end());
        assert!(
            r.avg_hr_bpm
                .is_none_or(|hr| (f64::from(lo)..=f64::from(hi)).contains(&hr))
        );
    }
}

/// Seeded byte and XML-structure mutants of the GPX, TCX and FIT seeds: every
/// one ends in sane runs or a named reason, the same each time.
#[test]
fn mutants_always_end_in_sane_runs_or_a_named_reason() {
    let iters: u64 = std::env::var("IMPORT_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let r = run(400, T0, 1);
    let a = run(300, T0, 2);
    let b = run(200, T0 + 100_000, 3);
    let mut second: Vec<Fix> = r[200..].to_vec();
    for f in &mut second {
        f.t += 120;
    }
    let seeds: [(&str, Vec<u8>); 5] = [
        (
            "gpx",
            gpx(&[r[..200].to_vec(), second], Some("running"), true).into_bytes(),
        ),
        (
            "tcx",
            tcx(&[
                ("Running", vec![a[..150].to_vec(), a[150..].to_vec()]),
                ("Running", vec![b]),
            ])
            .into_bytes(),
        ),
        ("fit", fit(&run(400, T0, 1), 1)),
        (
            "Activity.fit",
            include_bytes!("../../tests/fixtures/Activity.fit").to_vec(),
        ),
        (
            "fenix bike",
            include_bytes!("../../tests/fixtures/garmin-fenix-5-bike.fit").to_vec(),
        ),
    ];
    for (name, seed) in &seeds {
        let mut imported = 0;
        for case in 0..iters {
            let mut rng =
                SplitMix(case.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ name.len() as u64 | 1);
            let bytes = if seed.get(8..12) == Some(b".FIT") || rng.index(2) == 0 {
                mutate_bytes(&mut rng, seed)
            } else {
                mutate_xml(&mut rng, std::str::from_utf8(seed).unwrap()).into_bytes()
            };
            let v = import_run_file(&bytes);
            assert_sane(&v);
            assert_eq!(
                v,
                import_run_file(&bytes),
                "{name} case {case}: same input, same answer"
            );
            imported += usize::from(v.refusal.is_none());
        }
        if name != &"fenix bike" {
            assert!(imported > 0, "{name}: some mutants still import");
        }
    }
}

fn mutate_bytes(rng: &mut SplitMix, seed: &[u8]) -> Vec<u8> {
    const INTERESTING: [u8; 12] = [
        0, 1, 0x0C, 0x0E, 0x20, 0x26, 0x3C, 0x3E, 0x40, 0x7F, 0x80, 0xFF,
    ];
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.index(4) {
        let n = b.len().max(1);
        match rng.index(7) {
            0 if !b.is_empty() => {
                let k = rng.index(b.len());
                b[k] ^= 1 << rng.index(8);
            }
            1 if !b.is_empty() => {
                let k = rng.index(b.len());
                b[k] = INTERESTING[rng.index(INTERESTING.len())];
            }
            2 => {
                let k = rng.index(b.len() + 1);
                let junk: Vec<u8> = (0..1 + rng.index(8))
                    .map(|_| rng.index(256) as u8)
                    .collect();
                b.splice(k..k, junk);
            }
            3 if !b.is_empty() => {
                let k = rng.index(b.len());
                let end = (k + 1 + rng.index(16)).min(b.len());
                b.drain(k..end);
            }
            4 if !b.is_empty() => {
                let k = rng.index(b.len());
                let copy = b[k..(k + 1 + rng.index(128)).min(b.len())].to_vec();
                let at = rng.index(b.len() + 1);
                b.splice(at..at, copy);
            }
            5 => b.truncate(rng.index(n + 1)),
            _ if b.len() > 1 => {
                let (k, j) = (rng.index(b.len()), rng.index(b.len()));
                b.swap(k, j);
            }
            _ => {}
        }
    }
    b
}

fn mutate_xml(rng: &mut SplitMix, doc: &str) -> String {
    const WEIRD_NUMBERS: [&str; 13] = [
        "NaN",
        "Infinity",
        "-Infinity",
        "1e308",
        "-1e308",
        "1e400",
        "91.5",
        "-180.0001",
        "0x1p3",
        "4,7",
        "",
        " 47.3 ",
        "-0",
    ];
    const WEIRD_TIMES: [&str; 11] = [
        "",
        "garbage",
        "9999-12-31T23:59:59Z",
        "+10000-01-01T00:00:00Z",
        "1970-01-01T00:00:00Z",
        "1969-12-31T23:59:59Z",
        "2025-06-15T12:00:00+14:00",
        "2025-06-15T12:00:00.999999999Z",
        "2025-06-15T12:00:00",
        "2025-02-30T00:00:00Z",
        "-999999999-01-01T00:00:00Z",
    ];
    const SNIPPETS: [&str; 10] = [
        "&amp;",
        "&#x41;",
        "&#0;",
        "&#xD800;",
        "&undefined;",
        "<![CDATA[<x>]]>",
        "<!-- c -->",
        "<?pi x?>",
        "\u{0}",
        "&",
    ];
    let mut s = doc.to_owned();
    for _ in 0..1 + rng.index(3) {
        s = match rng.index(9) {
            0 | 1 => {
                // A weird value in some coordinate or time.
                let field =
                    ["lat=\"", "lon=\"", "<time>", "<Time>", "<LatitudeDegrees>"][rng.index(5)];
                let weird = if field.contains("ime") {
                    WEIRD_TIMES[rng.index(11)]
                } else {
                    WEIRD_NUMBERS[rng.index(13)]
                };
                let close = if field.ends_with('"') { "\"" } else { "<" };
                let mut out = String::with_capacity(s.len());
                let mut rest = s.as_str();
                while let Some(k) = rest.find(field) {
                    let value_start = k + field.len();
                    let value_end = rest[value_start..]
                        .find(close)
                        .map_or(rest.len(), |e| value_start + e);
                    out += &rest[..value_start];
                    out += if rng.index(8) == 0 {
                        weird
                    } else {
                        &rest[value_start..value_end]
                    };
                    rest = &rest[value_end..];
                }
                out + rest
            }
            2 => {
                // Duplicate an element span.
                let tag = [
                    "trkpt",
                    "trkseg",
                    "trk",
                    "Trackpoint",
                    "Track",
                    "Lap",
                    "Activity",
                    "time",
                    "Time",
                ][rng.index(9)];
                let from = rng.index(s.len());
                match (
                    s[from..].find(&format!("<{tag}")).map(|k| from + k),
                    None::<usize>,
                ) {
                    (Some(k), _) => match s[k..].find(&format!("</{tag}>")).map(|j| k + j) {
                        Some(j) => {
                            let end = j + tag.len() + 3;
                            format!("{}{}{}", &s[..end], &s[k..end], &s[end..])
                        }
                        None => s,
                    },
                    _ => s,
                }
            }
            3 => {
                // Nest an extra element of the same kind, closed at the end.
                let tag = ["trkseg", "trk", "gpx", "Track", "Activity", "Lap"][rng.index(6)];
                match s.find(&format!("<{tag}")) {
                    Some(k) => format!(
                        "{}<{tag}>{}",
                        &s[..k],
                        s[k..].replacen(&format!("</{tag}>"), &format!("</{tag}></{tag}>"), 1)
                    ),
                    None => s,
                }
            }
            4 => s.replace('\n', "\r\n"),
            5 => format!("\u{FEFF}{s}"),
            6 => {
                let mut k = rng.index(s.len() + 1);
                while !s.is_char_boundary(k) {
                    k -= 1;
                }
                format!("{}{}{}", &s[..k], SNIPPETS[rng.index(10)], &s[k..])
            }
            7 => s
                .replacen(
                    "<trkpt ",
                    &format!("<trkpt x=\"{}\" ", "y".repeat(200_000)),
                    1,
                )
                .replacen(
                    "<Trackpoint>",
                    &format!("<Trackpoint data=\"{}\">", "z".repeat(200_000)),
                    1,
                ),
            _ => {
                let mut k = rng.index(s.len() + 1);
                while !s.is_char_boundary(k) {
                    k -= 1;
                }
                s.truncate(k);
                s
            }
        };
    }
    s
}

/// A [`read_runs`] outcome in the fixture's shape: `{"fail": reason}` or
/// `{"runs": [{"sport", "logged_at", "segments": [[[lat, lon, t, hr, acc]]]}]}`.
fn raw_outcome(bytes: &[u8]) -> serde_json::Value {
    match read_runs(bytes) {
        Err(Refused(reason, _)) => serde_json::json!({ "fail": format!("{reason:?}") }),
        Ok(runs) => serde_json::json!({
            "runs": runs.iter().map(|r| serde_json::json!({
                "sport": r.sport,
                "logged_at": r.logged_at_sec,
                "segments": r.segments.iter().map(|s| s.iter().map(|f| serde_json::json!(
                    [f.lat, f.lon, f.time_sec, f.hr_bpm, f.accuracy_m]
                )).collect::<Vec<_>>()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    }
}

/// `fixtures/import/xml_docs.json`: every document the shell's import tests
/// read plus XML edge cases (entities, CDATA, comments, self-closing tags,
/// stray content, declarations, nesting), each cut off at every character, with
/// what the Android platform reader (kXML) the shell imported with made of it,
/// run-length encoded. The core reads each the same: the same runs, fixes and
/// stamps, or the same reason.
#[test]
fn every_document_and_cut_reads_as_the_platform_reader_read_it() {
    #[derive(serde::Deserialize)]
    struct Doc {
        doc: String,
        cuts: Vec<(serde_json::Value, usize)>,
    }
    let docs: Vec<Doc> =
        serde_json::from_str(include_str!("../../tests/fixtures/import/xml_docs.json"))
            .expect("fixture");
    let mut compared = 0;
    let mut diffs = Vec::new();
    for (k, Doc { doc, cuts }) in docs.iter().enumerate() {
        let mut ends: Vec<usize> = doc.char_indices().map(|(at, _)| at).collect();
        ends.push(doc.len());
        let expected = cuts
            .iter()
            .flat_map(|(outcome, n)| std::iter::repeat_n(outcome, *n));
        assert_eq!(
            cuts.iter().map(|(_, n)| n).sum::<usize>(),
            ends.len(),
            "doc {k}"
        );
        for (cut, want) in ends.iter().zip(expected) {
            compared += 1;
            let got = raw_outcome(&doc.as_bytes()[..*cut]);
            if &got != want {
                diffs.push(format!(
                    "doc {k} cut {cut}/{}: kXML {want}, core {got}\n  {:?}",
                    doc.len(),
                    &doc[..*cut]
                ));
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} of {compared} differ:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(compared > 20_000, "{compared}");
}

#[test]
fn the_logged_at_extension_and_the_tracks_type_are_read_only_where_they_belong() {
    // A core-shaped export (namespace declared, extension before <trkseg>) plus
    // a Garmin-style <type> inside <extensions> and a <link><type>: the stamp is
    // read, the nested types are not the track's sport.
    let fixes: String = (0..240).step_by(10).map(fix_line).collect();
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="x" xmlns="http://www.topografix.com/GPX/1/1" xmlns:lollop="urn:lollop:gpx:1" xmlns:gx="urn:other">
  <trk>
    <name>n</name>
    <extensions><gx:meta><type>cycling</type></gx:meta><lollop:logged_at>2026-09-21T14:20:18Z</lollop:logged_at></extensions>
    <link href="http://x"><type>text/html</type></link>
    <trkseg>
{fixes}    </trkseg>
  </trk>
</gpx>"#
    );
    let r = one_run(doc.as_bytes());
    assert_eq!(r.observed_at, 1_790_000_418, "2026-09-21T14:20:18Z");
    assert_eq!((r.points.len(), r.segment_starts), (24, vec![]));
    assert_eq!(non_run_sport(Some("mountainbiking")), Some("cycling"));
}

fn fix_line(t: u32) -> String {
    format!(
        "<trkpt lat=\"0\" lon=\"{}\"><time>2026-09-21T14:{:02}:{:02}Z</time></trkpt>\n",
        f64::from(t) * 3e-5,
        13 + t / 60,
        t % 60
    )
}

#[test]
fn a_gpx_cut_off_anywhere_inside_its_track_is_damaged() {
    let mut full = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"x\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>n</name><trkseg>\n",
    );
    for t in 0..40 {
        full += &fix_line(t);
    }
    full += "</trkseg></trk>\n</gpx>\n";
    let start = full.find("<trkseg>").unwrap() + 8;
    // Only cuts that lose data: before the closing </trkseg>.
    let lost: Vec<String> = (start..full.rfind("</trkseg>").unwrap())
        .filter_map(|cut| {
            let reason = verdict(&full.as_bytes()[..cut]).err().map(|(r, _)| r);
            (reason != Some(ImportRefusal::Damaged))
                .then(|| format!("{reason:?} at +{}", cut - start))
        })
        .collect();
    assert!(
        lost.is_empty(),
        "every data-losing truncation is damaged: {:?}",
        &lost[..lost.len().min(3)]
    );
}

#[test]
fn a_gpx_cut_off_in_its_second_segment_never_imports_a_partial_run() {
    let mut full = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"x\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>n</name>\n<trkseg>\n",
    );
    for t in 0..200 {
        full += &fix_line(t);
    }
    full += "</trkseg>\n<trkseg>\n";
    for t in 300..500 {
        full += &fix_line(t);
    }
    full += "</trkseg></trk>\n</gpx>\n";
    let bytes = full.as_bytes();
    let second = full.rfind("<trkseg>").unwrap() + 8;
    let end = full.rfind("</trkseg>").unwrap();
    // Every tag end (where a cut-off parse can look complete) plus every 7th byte.
    let cuts = (second..end).filter(|&cut| (cut - second) % 7 == 0 || bytes[cut - 1] == b'>');
    let partial: Vec<String> = cuts
        .filter_map(|cut| {
            verdict(&bytes[..cut]).ok().map(|runs| {
                format!(
                    "cut at +{}: {} of 400 fixes",
                    cut - second,
                    runs[0].points.len()
                )
            })
        })
        .collect();
    assert!(
        partial.is_empty(),
        "a truncated file imported as a shorter run: {:?}",
        &partial[..partial.len().min(3)]
    );
}
