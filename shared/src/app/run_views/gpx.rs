//! On-demand GPX export: the bridge's `ExportGpx` op builds the ~0.5 MB
//! document for the one run the shell inspects, so it never rides the
//! `ViewModel`. Compiled only where the bridge exists (Android, or the `ffi`
//! feature).

use crate::app::format::fmt_km_or_mi;
use crate::app::model::Model;
use crate::app::run_metrics::run_qc_keyed;
use crate::gps;
use crate::memo::fingerprint;

/// The GPX document for the logged run `(entry_id, observed_at)` (the
/// identity `find_entry` matches on; the last such run when a legacy id
/// repeats); empty when the run is unknown or has no track. For a run with a
/// route (`has_gpx`, >= 2 QC'd fixes) it is the QC'd route the shell maps and
/// shares; for a GPS run QC left unmeasurable (`gps_unmeasurable`) it is the
/// recorded fixes, so the shell's Undo of its delete re-logs it exactly as
/// recorded (the same fixes fail QC the same way again). The document carries
/// `observed_at` as the run's logged-at stamp, so re-importing it is
/// recognised as this run.
pub(crate) fn export_run_gpx(model: &Model, entry_id: u64, observed_at: i64) -> String {
    let Some(run) = model
        .runs
        .iter()
        .rev()
        .find(|r| r.entry_id == entry_id && r.observed_at == observed_at)
    else {
        return String::new();
    };
    if run.track.is_empty() {
        return String::new();
    }
    let q = run_qc_keyed(run, fingerprint(run));
    let (track, seg_starts) = if q.track.len() >= 2 {
        (&q.track, &q.seg_starts)
    } else {
        (&run.track, &run.track_segment_starts)
    };
    let label = format!("Run {}", fmt_km_or_mi(q.distance_km, false));
    gps::export_gpx(track, &label, seg_starts, Some(observed_at))
}
