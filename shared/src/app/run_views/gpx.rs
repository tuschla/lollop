//! On-demand route exports for the one run the shell inspects, so they never
//! ride the `ViewModel`: the bridge's `ExportGpx` op builds the ~0.5 MB GPX
//! document, its `RunTrack` op the same fixes as structured segments.
//! Compiled only where the bridge exists (Android, or the `ffi` feature).

use crate::app::format::fmt_km_or_mi;
use crate::app::model::Model;
use crate::app::run_metrics::run_qc_keyed;
use crate::gps::{self, GpsPoint};
use crate::memo::fingerprint;
use crate::profile::DistanceUnit;

/// The fixes the export writes for the logged run `(entry_id, observed_at)`
/// (the identity `find_entry` matches on; the last such run when a legacy id
/// repeats), handed to `f` with their segment starts and the run's distance;
/// `None` when the run is unknown or has no route (`has_gpx`: fewer than 2
/// fixes pass QC). They are the QC'd route the shell maps and shares.
fn with_exported_fixes<R>(
    model: &Model,
    entry_id: u64,
    observed_at: i64,
    f: impl FnOnce(&[GpsPoint], &[u32], f64) -> R,
) -> Option<R> {
    let run = model
        .runs
        .iter()
        .rev()
        .find(|r| r.entry_id == entry_id && r.observed_at == observed_at)?;
    if run.track.is_empty() {
        return None;
    }
    let q = run_qc_keyed(run, fingerprint(run));
    (q.track.len() >= 2).then(|| f(&q.track, &q.seg_starts, q.distance_km))
}

/// The GPX document of the run's exported fixes ([`with_exported_fixes`]);
/// empty when there are none. The document carries `observed_at` as the run's
/// logged-at stamp, so re-importing it is recognised as this run.
pub(crate) fn export_run_gpx(model: &Model, entry_id: u64, observed_at: i64) -> String {
    with_exported_fixes(model, entry_id, observed_at, |track, seg_starts, km| {
        let label = format!("Run {}", fmt_km_or_mi(km, DistanceUnit::Km));
        gps::export_gpx(track, &label, seg_starts, Some(observed_at))
    })
    .unwrap_or_default()
}

/// The run's exported fixes ([`with_exported_fixes`]) as one list per
/// recording segment; empty when there are none.
pub(crate) fn run_track_segments(
    model: &Model,
    entry_id: u64,
    observed_at: i64,
) -> Vec<Vec<GpsPoint>> {
    with_exported_fixes(model, entry_id, observed_at, |track, seg_starts, _| {
        gps::segments(track, seg_starts)
            .into_iter()
            .filter(|seg| !seg.is_empty())
            .map(<[GpsPoint]>::to_vec)
            .collect()
    })
    .unwrap_or_default()
}
