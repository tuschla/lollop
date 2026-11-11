//! Run import from files: the one pipeline behind the bridge's `ParseRunFile`
//! op. A file's format is sniffed from its CONTENT (the FIT header magic, else
//! the XML root element; extensions and MIME types lie), it is read into runs
//! (one per GPX `<trk>`, TCX `<Activity>`, or FIT file: [`gpx`], [`tcx`],
//! [`crate::fit`]), and each run passes the same gates (sport, timestamps,
//! GNSS dropouts, plausible pace, the keep threshold) before it becomes the
//! fixes of a `LogRunTrack`, decimated like a live save. The verdict is the
//! runs to log or the one reason the file gave none ([`ImportVerdict`]).

use serde::Serialize;

use crate::fit::{self, FitError};
use crate::gps::{self, GpsPoint};
use crate::wire::Wire;

mod gpx;
mod tcx;
mod time;
mod xml;

/// The largest file an import reads: real GPX/TCX exports are well under it,
/// and it keeps a wrong-type or endless source from exhausting memory. The
/// shells stop reading a file past it.
pub const MAX_RUN_IMPORT_BYTES: u32 = 32 * 1024 * 1024;

/// The largest FIT file an import reads: 8 MB still fits a day-long 1 Hz
/// recording (86 400 records under 90 bytes each).
const MAX_FIT_IMPORT_BYTES: usize = 8 * 1024 * 1024;

/// Accuracy (m) given to an imported fix whose file names none (TCX; GPX
/// without `<hdop>`; FIT without `gps_accuracy`). A sentinel, not a
/// measurement: at the QC gate, so the fix passes as the least-trusted
/// acceptable one.
const UNKNOWN_ACCURACY_M: f32 = gps::MAX_GPS_ACCURACY_M;

/// Physiologically plausible heart-rate samples (bpm) an import keeps.
const PLAUSIBLE_HR_BPM: std::ops::RangeInclusive<i32> = 20..=250;

/// Fastest believable average moving pace for an imported run, seconds per km
/// (2:50 /km): anything faster is a bike ride, a car trip or a broken clock,
/// never running volume.
const FASTEST_PLAUSIBLE_SEC_PER_KM: f64 = 170.0;

/// How far after an imported track's last fix its file-named logged-at stamp
/// may sit and still be believed: a day, longer than any trimmed-off finish or
/// standing-still tail of one run.
const MAX_LOGGED_AT_AFTER_LAST_FIX_SEC: i64 = 24 * 3600;

/// The most fixes in a row [`drop_gnss_excursions`] drops as one dropout.
const MAX_GNSS_EXCURSION_FIXES: usize = 30;

/// Why a file gave no run. The shell shows each as plain-language copy beside
/// the file's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Wire)]
#[serde(rename_all = "snake_case")]
pub enum ImportRefusal {
    /// Over [`MAX_RUN_IMPORT_BYTES`] (a FIT file over its own smaller cap).
    TooLarge,
    /// Neither FIT nor GPX nor TCX.
    WrongType,
    /// No track with two GPS fixes.
    NoTrack,
    /// Under the keep threshold ([`gps::MIN_RUN_KM`] or [`gps::MIN_RUN_SEC`]).
    TooShort,
    /// Fixes without times: a planned route, not a recorded run.
    NoTimestamps,
    /// Malformed or cut off.
    Damaged,
    /// Another sport ([`ImportVerdict::sport`] names it when the file does).
    NotARun,
    /// A moving pace faster than any run.
    ImplausiblePace,
}

/// One run read from an import file, ready to log as a `LogRunTrack`.
#[derive(Clone, Debug, PartialEq, Serialize, Wire)]
pub struct ImportedRun {
    /// The fixes to log: in range, timed, without GNSS dropouts, decimated to
    /// [`gps::TRACK_DECIMATION_CAP`] like a live save (segment boundaries and
    /// the ends kept). A fix whose file names no accuracy has the QC bound.
    pub points: Vec<GpsPoint>,
    /// Indices into `points` that begin a new recording segment.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub segment_starts: Vec<u32>,
    /// When the run happened: lollop's own export names the run's logged-at
    /// stamp, which may follow the last fix by up to a day; any other file is
    /// stamped at its last fix.
    pub observed_at: i64,
    /// Time-weighted average of the file's heart-rate samples (bpm), each
    /// weighed by the seconds to the next fix; `None` without samples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_hr_bpm: Option<f64>,
}

/// What an import file holds: its runs, or why it holds none. Exactly one of
/// `runs` (non-empty) and `refusal` is set. Inside a multi-run file a run that
/// fails is skipped as long as another imports; when none does, the first
/// run's reason is the file's.
#[derive(Clone, Debug, PartialEq, Serialize, Wire)]
pub struct ImportVerdict {
    pub runs: Vec<ImportedRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<ImportRefusal>,
    /// The other sport a [`ImportRefusal::NotARun`] file names, as plain
    /// lowercase words ("cycling"); `None` when it names none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sport: Option<String>,
}

/// A refused file or run: the reason, and the sport for `NotARun`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Refused(ImportRefusal, Option<String>);

impl From<ImportRefusal> for Refused {
    fn from(reason: ImportRefusal) -> Self {
        Refused(reason, None)
    }
}

pub fn import_run_file(bytes: &[u8]) -> ImportVerdict {
    let result = read_runs(bytes).and_then(|runs| {
        let mut first_refusal = None;
        let built: Vec<ImportedRun> = runs
            .iter()
            .filter_map(|run| {
                build_run(run)
                    .map_err(|refused| {
                        first_refusal.get_or_insert(refused);
                    })
                    .ok()
            })
            .collect();
        match first_refusal {
            Some(refused) if built.is_empty() => Err(refused),
            _ if built.is_empty() => Err(ImportRefusal::NoTrack.into()),
            _ => Ok(built),
        }
    });
    match result {
        Ok(runs) => ImportVerdict {
            runs,
            refusal: None,
            sport: None,
        },
        Err(Refused(reason, sport)) => ImportVerdict {
            runs: Vec::new(),
            refusal: Some(reason),
            sport,
        },
    }
}

/// One fix as a file names it, before the gates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RawFix {
    lat: f64,
    lon: f64,
    /// Unix seconds; 0 when the file names no (readable) time.
    time_sec: i64,
    hr_bpm: Option<i32>,
    /// The file's accuracy (FIT `gps_accuracy`, GPX `<hdop>`), else `None`.
    accuracy_m: Option<f64>,
}

impl Default for RawFix {
    fn default() -> Self {
        Self {
            lat: f64::NAN,
            lon: f64::NAN,
            time_sec: 0,
            hr_bpm: None,
            accuracy_m: None,
        }
    }
}

/// One run as a file names it: its fixes per recording segment, the sport it
/// names (verbatim: TCX `<Activity Sport>`, GPX `<trk><type>`; FIT's sport is
/// gated in [`crate::fit`]) and lollop's own logged-at stamp.
#[derive(Clone, Debug, PartialEq)]
struct RawRun {
    segments: Vec<Vec<RawFix>>,
    sport: Option<String>,
    logged_at_sec: Option<i64>,
}

/// The runs `bytes` hold, by format: FIT by its header magic (".FIT" at
/// offset 8), else XML by its root element within the first
/// [`xml::SNIFF_BYTES`], so a wrong-type file is refused without decoding all
/// of it. XML that breaks off or is malformed past the root (a cut-off
/// download) is damaged, and so is a document that declares a DTD.
fn read_runs(bytes: &[u8]) -> Result<Vec<RawRun>, Refused> {
    if bytes.len() > MAX_RUN_IMPORT_BYTES as usize {
        return Err(ImportRefusal::TooLarge.into());
    }
    if bytes.get(8..12) == Some(b".FIT") {
        if bytes.len() > MAX_FIT_IMPORT_BYTES {
            return Err(ImportRefusal::TooLarge.into());
        }
        return read_fit(bytes).map(|run| vec![run]);
    }
    let (charset, bom) = xml::charset_of(bytes);
    let body = &bytes[bom..];
    let head = xml::decode(&body[..body.len().min(xml::SNIFF_BYTES)], charset);
    let is_tcx = head.contains("<TrainingCenterDatabase");
    if !is_tcx && !head.contains("<gpx") {
        return Err(ImportRefusal::WrongType.into());
    }
    let text = xml::decode(body, charset);
    if xml::declares_dtd(&text) {
        return Err(ImportRefusal::Damaged.into());
    }
    let runs = if is_tcx {
        tcx::parse_tcx_activities(&text)
    } else {
        gpx::parse_gpx_runs(&text)
    };
    runs.map_err(|_| ImportRefusal::Damaged.into())
}

fn read_fit(bytes: &[u8]) -> Result<RawRun, Refused> {
    let track = fit::parse_fit(bytes).and_then(|track| fit::import_gate(&track).map(|()| track));
    let track = track.map_err(|err| match err {
        FitError::NotFit => Refused::from(ImportRefusal::WrongType),
        FitError::Damaged => ImportRefusal::Damaged.into(),
        FitError::NoTrack => ImportRefusal::NoTrack.into(),
        FitError::TooShort => ImportRefusal::TooShort.into(),
        FitError::NotARun(sport) => Refused(
            ImportRefusal::NotARun,
            Some(sport).filter(|s| !kt_trim(s).is_empty()),
        ),
    })?;
    let segments = track
        .segments
        .into_iter()
        .map(|seg| {
            seg.into_iter()
                .map(|f| RawFix {
                    lat: f.lat,
                    lon: f.lon,
                    time_sec: f.time_sec,
                    hr_bpm: f
                        .hr_bpm
                        .map(i32::from)
                        .filter(|bpm| PLAUSIBLE_HR_BPM.contains(bpm)),
                    accuracy_m: f.accuracy_m.map(f64::from),
                })
                .collect()
        })
        .collect();
    Ok(RawRun {
        segments,
        sport: None,
        logged_at_sec: None,
    })
}

/// One run's gates and the fixes it is logged with:
/// - refused as [`ImportRefusal::NotARun`] when the file names another sport
///   ([`non_run_sport`]), checked first, as FIT's gate does;
/// - untimed fixes (a leading 0 stamp would add a ~1.7e9 s leg), out-of-range
///   coordinates and GNSS dropouts ([`drop_gnss_excursions`]) are dropped, so
///   the gates judge the legs the core keeps;
/// - decimated to [`gps::TRACK_DECIMATION_CAP`] like a live save, keeping
///   segment boundaries and the ends, so one import can't bloat the log;
/// - refused as [`ImportRefusal::ImplausiblePace`] when its moving pace is
///   faster than any run, and as [`ImportRefusal::TooShort`] under the keep
///   threshold.
fn build_run(run: &RawRun) -> Result<ImportedRun, Refused> {
    if let Some(sport) = non_run_sport(run.sport.as_deref()) {
        return Err(Refused(ImportRefusal::NotARun, Some(sport.to_owned())));
    }
    if run.segments.iter().map(Vec::len).sum::<usize>() < 2 {
        return Err(ImportRefusal::NoTrack.into());
    }
    let timed: Vec<Vec<RawFix>> = run
        .segments
        .iter()
        .map(|seg| {
            let kept: Vec<RawFix> = seg
                .iter()
                .copied()
                .filter(|f| f.time_sec > 0 && f.lat.abs() <= 90.0 && f.lon.abs() <= 180.0)
                .collect();
            drop_gnss_excursions(kept)
        })
        .filter(|seg| !seg.is_empty())
        .collect();
    let all: Vec<RawFix> = timed.iter().flatten().copied().collect();
    let Some(last) = all.last().filter(|_| all.len() >= 2) else {
        return Err(ImportRefusal::NoTimestamps.into());
    };
    // Each segment's first index in `all`: the core's `segment_starts`
    // contract (indices that BEGIN a new segment; one unbroken track sends none).
    let boundaries: Vec<usize> = timed
        .iter()
        .scan(0, |at, seg| {
            let start = *at;
            *at += seg.len();
            Some(start)
        })
        .skip(1)
        .collect();
    let keep = gps::decimation_keep(all.len(), gps::TRACK_DECIMATION_CAP as usize, &boundaries);
    let points: Vec<GpsPoint> = keep
        .iter()
        .map(|&k| {
            let f = all[k];
            GpsPoint {
                lat: f.lat,
                lon: f.lon,
                observed_at: f.time_sec,
                accuracy_m: f.accuracy_m.map_or(UNKNOWN_ACCURACY_M, |a| a as f32),
            }
        })
        .collect();
    let segment_starts = gps::remap_starts(&keep, &boundaries);
    let legs = LegTotals::of(&points, &segment_starts);
    if legs.implausibly_fast() {
        return Err(ImportRefusal::ImplausiblePace.into());
    }
    if legs.too_short() {
        return Err(ImportRefusal::TooShort.into());
    }
    let observed_at = run
        .logged_at_sec
        .filter(|&at| {
            (last.time_sec..=last.time_sec + MAX_LOGGED_AT_AFTER_LAST_FIX_SEC).contains(&at)
        })
        .unwrap_or(last.time_sec);
    Ok(ImportedRun {
        points,
        segment_starts,
        observed_at,
        avg_hr_bpm: time_weighted_hr(&all),
    })
}

/// The time-weighted average of the fixes' heart-rate samples: each sample
/// weighs the seconds until the next fix (the last one the median gap), so a
/// brief spike counts briefly. `None` without samples.
fn time_weighted_hr(all: &[RawFix]) -> Option<f64> {
    if all.iter().all(|f| f.hr_bpm.is_none()) {
        return None;
    }
    let gaps: Vec<f64> = all
        .windows(2)
        .map(|w| (w[1].time_sec.saturating_sub(w[0].time_sec) as f64).max(0.0))
        .collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(f64::total_cmp);
    let m = sorted.len();
    let median = match m {
        0 => f64::NAN,
        _ if m % 2 == 1 => sorted[m / 2],
        _ => (sorted[m / 2 - 1] + sorted[m / 2]) / 2.0,
    };
    let (mut num, mut den) = (0.0, 0.0);
    for (k, f) in all.iter().enumerate() {
        let Some(hr) = f.hr_bpm else { continue };
        let w = gaps.get(k).copied().unwrap_or(median);
        num += f64::from(hr) * w;
        den += w;
    }
    if den > 0.0 {
        return Some(num / den);
    }
    let samples: Vec<f64> = all.iter().filter_map(|f| f.hr_bpm.map(f64::from)).collect();
    Some(samples.iter().sum::<f64>() / samples.len() as f64)
}

/// An imported run's `hr_pct_max`: the file's average heart rate
/// ([`ImportedRun::avg_hr_bpm`]) against a MEASURED HRmax, capped at 100 %
/// (a stale low HRmax never states more). Never against an estimate: without
/// samples or a measured HRmax it is 0.0, "no HR", as a live save sends.
pub fn imported_hr_pct_max(avg_hr_bpm: Option<f64>, measured_hr_max: Option<f64>) -> f64 {
    match (avg_hr_bpm, measured_hr_max) {
        (Some(avg), Some(max)) if max > 0.0 => (avg / max * 100.0).min(100.0),
        _ => 0.0,
    }
}

/// Implied ground speed (m/s) from `a` to `b`, or `None` when `b` isn't
/// stamped after `a`.
fn speed_mps(a: &RawFix, b: &RawFix) -> Option<f64> {
    let dt = b.time_sec.saturating_sub(a.time_sec);
    (dt > 0).then(|| gps::haversine_m(raw_point(a), raw_point(b)) / dt as f64)
}

fn raw_point(f: &RawFix) -> GpsPoint {
    GpsPoint {
        lat: f.lat,
        lon: f.lon,
        observed_at: f.time_sec,
        accuracy_m: 0.0,
    }
}

/// `segment` without its GNSS dropouts: fixes the QC drops as jumps (an
/// implied speed over [`gps::MAX_PLAUSIBLE_SPEED_MPS`] from the last kept fix)
/// when the track comes back, i.e. a later fix (at most
/// [`MAX_GNSS_EXCURSION_FIXES`] on) is plausible from that kept fix again. One
/// multipath or no-fix teleport then never makes the whole run "implausibly
/// fast". A jump the track never returns from (a car ride, a relocation) is
/// kept, so the pace gate still judges it.
fn drop_gnss_excursions(segment: Vec<RawFix>) -> Vec<RawFix> {
    if segment.len() < 3 {
        return segment;
    }
    let mut out = Vec::with_capacity(segment.len());
    out.push(segment[0]);
    let mut k = 1;
    while k < segment.len() {
        let kept = out[out.len() - 1];
        let jumps = speed_mps(&kept, &segment[k]).is_some_and(|v| v > gps::MAX_PLAUSIBLE_SPEED_MPS);
        let back = if jumps {
            (k + 1..segment.len().min(k + 1 + MAX_GNSS_EXCURSION_FIXES)).find(|&j| {
                speed_mps(&kept, &segment[j]).is_some_and(|v| v <= gps::MAX_PLAUSIBLE_SPEED_MPS)
            })
        } else {
            None
        };
        match back {
            Some(j) => k = j,
            None => {
                out.push(segment[k]);
                k += 1;
            }
        }
    }
    out
}

/// A track's legs within each segment (a pause bridge counts none): GPS
/// distance, recorded time and moving time (legs at or above
/// [`gps::MIN_MOVING_SPEED_MPS`]), as FIT's gate and the core's moving time
/// measure them.
struct LegTotals {
    km: f64,
    sec: i64,
    moving_sec: i64,
}

impl LegTotals {
    fn of(points: &[GpsPoint], segment_starts: &[u32]) -> Self {
        let mut totals = LegTotals {
            km: 0.0,
            sec: 0,
            moving_sec: 0,
        };
        for k in 1..points.len() {
            if segment_starts.binary_search(&(k as u32)).is_ok() {
                continue;
            }
            let dt = points[k]
                .observed_at
                .saturating_sub(points[k - 1].observed_at);
            if dt <= 0 {
                continue;
            }
            let leg_km = gps::haversine_m(points[k - 1], points[k]) / 1000.0;
            totals.km += leg_km;
            totals.sec += dt;
            if leg_km * 1000.0 / dt as f64 >= gps::MIN_MOVING_SPEED_MPS {
                totals.moving_sec += dt;
            }
        }
        totals
    }

    /// A moving pace faster than [`FASTEST_PLAUSIBLE_SEC_PER_KM`]: not running volume.
    fn implausibly_fast(&self) -> bool {
        self.km > 0.0
            && self.moving_sec > 0
            && (self.moving_sec as f64) / self.km < FASTEST_PLAUSIBLE_SEC_PER_KM
    }

    fn too_short(&self) -> bool {
        self.km < gps::MIN_RUN_KM || self.sec < gps::MIN_RUN_SEC
    }
}

/// The non-running sport a GPX `<type>` or TCX `Sport` names, as plain
/// lowercase words ("cycling", like FIT's gate); `None` when it names a run or
/// nothing recognised.
///
/// TCX's `Sport` is `Running`/`Biking`/`Other`, but a GPX `<type>` is free
/// text in any language ("running", "Ride", "trail_running", "Laufen", "1").
/// So, unlike the FIT enum, only a recognised non-running sport is refused: an
/// unknown label stays importable (the pace gate still guards fast tracks), and
/// a label that names running in any word wins over everything else.
fn non_run_sport(declared: Option<&str>) -> Option<&'static str> {
    let lower = declared.unwrap_or("").to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|w| w.contains("run") || w.contains("jog")) {
        return None;
    }
    let named = |test: fn(&str) -> bool| words.iter().any(|w| test(w));
    if named(|w| {
        w.contains("cycl")
            || w.starts_with("bik")
            || w.contains("bike")
            || w.contains("biking")
            || RIDE_WORDS.contains(&w)
    }) {
        Some("cycling")
    } else if named(|w| w.starts_with("swim")) {
        Some("swimming")
    } else if named(|w| w.starts_with("ski") || w.ends_with("ski") || w.contains("skiing")) {
        Some("skiing")
    } else if named(|w| w.starts_with("hik")) {
        Some("hiking")
    } else if named(|w| w.starts_with("walk")) {
        Some("walking")
    } else {
        None
    }
}

/// Strava's ride sport types no stem above catches, lowercased ("Ride",
/// "VirtualRide", "GravelRide", "Velomobile"); the bike/cycle/ski compounds
/// ("MountainBikeRide", "EBikeRide", "Handcycle", "NordicSki", "RollerSki")
/// match the stems.
const RIDE_WORDS: [&str; 4] = ["ride", "virtualride", "gravelride", "velomobile"];

/// A heart-rate sample from a file, kept only when finite and physiologically
/// plausible ([`PLAUSIBLE_HR_BPM`]), truncated to whole bpm: a NaN, an
/// `Infinity` or 1e9 bpm never reaches the averages.
fn hr_bpm(raw: &str) -> Option<i32> {
    let v = kt_to_double(kt_trim(raw))?;
    let (lo, hi) = (*PLAUSIBLE_HR_BPM.start(), *PLAUSIBLE_HR_BPM.end());
    (v.is_finite() && v >= f64::from(lo) && v <= f64::from(hi)).then_some(v as i32)
}

/// Whitespace as the import has always trimmed it (Kotlin's `Char.isWhitespace`
/// on the JVM): Unicode space separators, line and paragraph separators, and
/// the ASCII controls TAB, LF, VT, FF, CR and U+001C–U+001F.
fn kt_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{1C}'..='\u{1F}'
    ) || (c.is_whitespace() && c != '\u{85}')
}

fn kt_trim(s: &str) -> &str {
    s.trim_matches(kt_is_whitespace)
}

/// A number as the JVM's `Double.valueOf` reads one (the grammar the import
/// has always accepted): characters at or below U+0020 trimmed, an optional
/// sign, then `NaN`, `Infinity`, a decimal with optional exponent, or a hex
/// float (`0x1.8p3`), each optionally suffixed `f`/`F`/`d`/`D`. `None` for
/// anything else.
fn kt_to_double(raw: &str) -> Option<f64> {
    let s = raw.trim_matches(|c: char| c <= ' ');
    let (negative, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let magnitude = match body {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        _ => {
            let body = body.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(body);
            if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
                hex_float(hex)?
            } else {
                decimal(body)?
            }
        }
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// `digits[.digits][(e|E)[sign]digits]` with at least one mantissa digit.
fn decimal(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let int_digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if exp_digits == 0 {
            return None;
        }
        i += exp_digits;
    }
    if i != b.len() {
        return None;
    }
    s.parse().ok()
}

/// The part of a Java hex float after `0x`: `hex[.hex](p|P)[sign]digits`, the
/// binary exponent required. Exact for mantissas of up to 53 significant bits.
fn hex_float(s: &str) -> Option<f64> {
    let (mantissa, exponent) = s.split_once(['p', 'P'])?;
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.len() + frac.len() == 0
        || !int
            .chars()
            .chain(frac.chars())
            .all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let exp_digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
    if exp_digits.is_empty() || !exp_digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let exponent: i64 = exponent.parse().unwrap_or(if exponent.starts_with('-') {
        i64::MIN / 2
    } else {
        i64::MAX / 2
    });
    let mut value = 0.0f64;
    for c in int.chars().chain(frac.chars()) {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let shift = exponent
        .saturating_sub(4 * frac.len() as i64)
        .clamp(-2200, 2200) as i32;
    Some(value * 2f64.powi(shift / 2) * 2f64.powi(shift - shift / 2))
}

#[cfg(test)]
mod tests;
