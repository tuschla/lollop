//! GPX 1.0/1.1 track reader for runs recorded elsewhere (Garmin / Strava /
//! Komoot exports) and for lollop's own export. Reads only
//! `<trk>/<trkseg>/<trkpt>`, the lat/lon attributes, the optional `<time>` and
//! `<hdop>`, an optional heart-rate sample from the common Garmin
//! TrackPointExtension (`<gpxtpx:hr>`, matched by local name so any namespace
//! prefix works), the track's `<type>` (Strava/Garmin name the sport there)
//! and lollop's own `<trk><extensions><lollop:logged_at>` stamp
//! (`gps::export_gpx`). Each `<trk>` is one run. Everything else (waypoints,
//! routes, metadata) is ignored.

use super::number::{kt_to_double, kt_trim};
use super::raw::{RawFix, RawRun, hr_bpm};
use super::time::parse_import_time_sec;
use super::xml::{Event, Malformed, pull_tags};
use crate::gps::hdop_accuracy_m;

/// One [`RawRun`] per `<trk>` that holds fixes: its segments (one per
/// `<trkseg>`; segments mark recording pauses), its own `<type>` as the sport
/// and lollop's `<lollop:logged_at>` stamp.
pub(super) fn parse_gpx_runs(text: &str) -> Result<Vec<RawRun>, Malformed> {
    let mut runs = Vec::new();
    // Every `<trkseg>`'s fix list, by index: a list joins its track's
    // `segments` when its `</trkseg>` closes and keeps taking the fixes of a
    // `<trkpt>` still open until the next `<trkseg>` opens one.
    let mut pool: Vec<Vec<RawFix>> = vec![Vec::new()];
    let mut segment = 0;
    let mut segments: Vec<usize> = Vec::new();
    // Trackpoint handling is gated on being INSIDE a `<trk>/<trkseg>`: a stray
    // `<trkpt>` outside `<trk>` or after `</trkseg>` would otherwise reuse the
    // last lat/lon and append into the already-committed segment, merging
    // waypoints or route noise into a recorded run.
    let mut in_trk = false;
    let mut trk_depth = 0;
    let mut in_trkseg = false;
    let mut sport: Option<String> = None;
    let mut logged_at_sec: Option<i64> = None;
    let mut in_trkpt = false;
    let mut fix = RawFix::default();
    pull_tags(text, "gpx", |reader, event, name| {
        match event {
            Event::Start => match name {
                "trk" if !in_trk => {
                    in_trk = true;
                    trk_depth = reader.depth();
                    segments = Vec::new();
                    sport = None;
                    logged_at_sec = None;
                }
                // The track's own `<type>` only: a `<link>` inside `<trk>` has a
                // `<type>` (a MIME type) one level deeper, and so does a `<trkpt>`.
                "type" if in_trk && reader.depth() == trk_depth + 1 => {
                    let declared = reader.next_text()?;
                    let declared = kt_trim(&declared);
                    if sport.is_none() && !declared.is_empty() {
                        sport = Some(declared.to_owned());
                    }
                }
                "logged_at" if in_trk && !in_trkseg => {
                    logged_at_sec =
                        Some(parse_import_time_sec(&reader.next_text()?)).filter(|&t| t > 0);
                }
                "trkseg" if in_trk => {
                    in_trkseg = true;
                    pool.push(Vec::new());
                    segment = pool.len() - 1;
                }
                "trkpt" if in_trkseg => {
                    in_trkpt = true;
                    fix = RawFix {
                        lat: reader
                            .attribute("lat")
                            .and_then(kt_to_double)
                            .unwrap_or(f64::NAN),
                        lon: reader
                            .attribute("lon")
                            .and_then(kt_to_double)
                            .unwrap_or(f64::NAN),
                        ..RawFix::default()
                    };
                }
                "time" if in_trkpt => fix.time_sec = parse_import_time_sec(&reader.next_text()?),
                "hr" if in_trkpt => fix.hr_bpm = hr_bpm(&reader.next_text()?),
                // `<hdop>` is a direct child of `<trkpt>` in GPX 1.0/1.1.
                "hdop" if in_trkpt => {
                    fix.accuracy_m =
                        kt_to_double(kt_trim(&reader.next_text()?)).and_then(hdop_accuracy_m);
                }
                _ => {}
            },
            Event::End => match name {
                "trkpt" if in_trkpt => {
                    in_trkpt = false;
                    if fix.lat.is_finite() && fix.lon.is_finite() {
                        pool[segment].push(fix);
                    }
                }
                "trkseg" if in_trkseg => {
                    in_trkseg = false;
                    if !pool[segment].is_empty() {
                        segments.push(segment);
                    }
                }
                "trk" if in_trk && reader.depth() == trk_depth => {
                    in_trk = false;
                    if !segments.is_empty() {
                        runs.push(RawRun {
                            segments: segments.drain(..).map(|k| pool[k].clone()).collect(),
                            sport: sport.take(),
                            logged_at_sec,
                        });
                    }
                }
                _ => {}
            },
            Event::EndDocument => {}
        }
        Ok(())
    })?;
    Ok(runs)
}
