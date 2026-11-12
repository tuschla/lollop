//! TCX (Garmin Training Center XML) track reader for runs exported as TCX
//! instead of GPX. Reads only `<Activities>/<Activity>/<Lap>/<Track>/<Trackpoint>`,
//! each Activity's `Sport` attribute, each Trackpoint's `<Time>`, `<Position>`
//! (`LatitudeDegrees`/`LongitudeDegrees`) and an optional
//! `<HeartRateBpm><Value>` sample. Tag names are matched by local name (any
//! namespace prefix). Everything else (lap summaries, extensions, author,
//! courses) is ignored. TCX names no accuracy, so every fix has none.

use super::number::{kt_to_double, kt_trim};
use super::raw::{RawFix, RawRun, hr_bpm};
use super::time::parse_import_time_sec;
use super::xml::{Event, Malformed, pull_tags};
use crate::gps::MAX_FIX_GAP_SEC;

/// One [`RawRun`] per `<Activity>` (a multi-sport day is several runs, not
/// pauses inside one), with its `Sport` attribute; its `<Track>`s are merged
/// into recording segments by [`merge_tcx_tracks`]. Trackpoints without a
/// `<Position>` (TCX writes these during pauses) are skipped.
pub(super) fn parse_tcx_activities(text: &str) -> Result<Vec<RawRun>, Malformed> {
    let mut activities = Vec::new();
    let mut sport: Option<String> = None;
    // Every `<Track>`'s fix list, by index: a list joins the activity's
    // `tracks` when its `</Track>` closes and keeps taking fixes until the
    // next `<Track>` opens one, as the reader always has.
    let mut pool: Vec<Vec<RawFix>> = vec![Vec::new()];
    let mut segment = 0;
    let mut tracks: Vec<usize> = Vec::new();
    // Track/Trackpoint handling is gated on being INSIDE an `<Activity>`: a TCX
    // can carry a `<Courses><Course><Track>` block after `</Activities>`, whose
    // trackpoints would otherwise append into the last activity, merging a
    // planned course into a recorded run.
    let mut in_activity = false;
    let mut in_trackpoint = false;
    let mut in_position = false;
    let mut in_heart_rate = false;
    let mut fix = RawFix::default();
    pull_tags(text, "TrainingCenterDatabase", |reader, event, name| {
        match event {
            Event::Start => match name {
                "Activity" => {
                    in_activity = true;
                    tracks = Vec::new();
                    sport = reader
                        .attribute("Sport")
                        .map(kt_trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned);
                }
                "Track" if in_activity => {
                    pool.push(Vec::new());
                    segment = pool.len() - 1;
                }
                "Trackpoint" if in_activity => {
                    in_trackpoint = true;
                    fix = RawFix::default();
                }
                "Position" if in_trackpoint => in_position = true,
                "LatitudeDegrees" if in_trackpoint && in_position => {
                    fix.lat = kt_to_double(kt_trim(&reader.next_text()?)).unwrap_or(f64::NAN);
                }
                "LongitudeDegrees" if in_trackpoint && in_position => {
                    fix.lon = kt_to_double(kt_trim(&reader.next_text()?)).unwrap_or(f64::NAN);
                }
                "Time" if in_trackpoint => {
                    fix.time_sec = parse_import_time_sec(&reader.next_text()?)
                }
                "HeartRateBpm" if in_trackpoint => in_heart_rate = true,
                "Value" if in_trackpoint && in_heart_rate => {
                    fix.hr_bpm = hr_bpm(&reader.next_text()?)
                }
                _ => {}
            },
            Event::End => match name {
                "Position" => in_position = false,
                "HeartRateBpm" => in_heart_rate = false,
                "Trackpoint" if in_activity => {
                    in_trackpoint = false;
                    if fix.lat.is_finite() && fix.lon.is_finite() {
                        pool[segment].push(fix);
                    }
                }
                "Track" if in_activity && !pool[segment].is_empty() => tracks.push(segment),
                "Activity" => {
                    in_activity = false;
                    // The activity's tracks stay its own until the next `<Activity>`
                    // opens: a nested one closing first does not clear them.
                    let merged =
                        merge_tcx_tracks(tracks.iter().map(|&k| pool[k].clone()).collect());
                    if !merged.is_empty() {
                        activities.push(RawRun {
                            segments: merged,
                            sport: sport.clone(),
                            logged_at_sec: None,
                        });
                    }
                }
                _ => {}
            },
            Event::EndDocument => {}
        }
        Ok(())
    })?;
    Ok(activities)
}

/// Fold consecutive per-`<Track>` fix lists into recording segments.
/// Garmin/Strava TCX writes one `<Track>` per `<Lap>`, autolaps on continuous
/// running included, and the core drops the entering leg of every segment from
/// distance and moving time (~−130 m / −40 s on an autolap-per-km marathon).
/// So adjacent Tracks with a gap of 0 to [`MAX_FIX_GAP_SEC`] (the live
/// recorder's segment-break gap) merge; a larger gap or a backwards clock step
/// is a pause and opens a new segment. Empty tracks are skipped.
pub(super) fn merge_tcx_tracks(tracks: Vec<Vec<RawFix>>) -> Vec<Vec<RawFix>> {
    let mut segments: Vec<Vec<RawFix>> = Vec::new();
    for track in tracks {
        let (Some(first), Some(prev)) = (track.first(), segments.last_mut()) else {
            if !track.is_empty() {
                segments.push(track);
            }
            continue;
        };
        let gap = prev
            .last()
            .map_or(-1, |last| first.time_sec.saturating_sub(last.time_sec));
        if (0..=MAX_FIX_GAP_SEC).contains(&gap) {
            prev.extend(track);
        } else {
            segments.push(track);
        }
    }
    segments
}
