//! On-demand GPX export: `view()` keeps a registry of every exportable GPS
//! run's QC'd route, and the `exportGpx` FFI query builds the ~0.5 MB document
//! for the one run the shell inspects, so it never rides the `ViewModel`.
//! Compiled only where the JNI exports exist.

use super::*;
use std::collections::{HashMap, HashSet};

/// One GPS run's export source: the QC'd track + segment boundaries + the
/// track-name label, everything [`running::export_gpx_seg`] needs besides the
/// run's logged-at stamp (its registry key).
#[derive(Clone)]
struct GpxSource {
    /// The run's content fingerprint: an unchanged run keeps its entry.
    content_key: u64,
    track: std::sync::Arc<Vec<GpsPoint>>,
    seg_starts: Vec<u32>,
    label: String,
}

/// Keyed by the run identity `(entry_id, observed_at)` `find_run` matches on.
type Registry = HashMap<(u64, i64), GpxSource>;

// Process-global in shipping builds: the shell's export call may land on a
// different thread than the `view()` that filled it. Poison is recovered
// (plain cache data, no invariant). Per-thread under `cargo test`, where
// parallel tests each view their own model: each test then owns its live
// set and the shipping prune path runs unchanged.
#[cfg(not(test))]
static REGISTRY: std::sync::LazyLock<std::sync::Mutex<Registry>> =
    std::sync::LazyLock::new(Default::default);
#[cfg(test)]
thread_local! {
    static REGISTRY: std::cell::RefCell<Registry> = std::cell::RefCell::default();
}

fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    #[cfg(not(test))]
    {
        let mut reg = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut reg)
    }
    #[cfg(test)]
    {
        REGISTRY.with(|reg| f(&mut reg.borrow_mut()))
    }
}

/// Bring the registry in step with the live run history, once per
/// `view()`: a run no longer live is dropped, a run with no track is removed,
/// and an unchanged run (same content key) is skipped, so a steady-state
/// refresh re-clones nothing. A run with an exportable route (>= 2 QC'd
/// fixes) exports that route; a GPS run QC left unmeasurable (`gps_unmeasurable`)
/// exports its recorded fixes, so the shell's Undo of its delete re-logs it
/// exactly as recorded (the same fixes fail QC the same way again).
pub(in crate::app) fn refresh_gpx_registry(runs: &[LoggedRun], facts: &RunFacts) {
    with_registry(|reg| {
        let live: HashSet<(u64, i64)> = runs.iter().map(|r| (r.entry_id, r.observed_at)).collect();
        reg.retain(|id, _| live.contains(id));
        for ((r, &content_key), q) in runs.iter().zip(&facts.keys).zip(&facts.qc) {
            let id = (r.entry_id, r.observed_at);
            let Some(q) = q.as_ref() else {
                reg.remove(&id);
                continue;
            };
            if reg.get(&id).is_some_and(|s| s.content_key == content_key) {
                continue;
            }
            let (track, seg_starts) = if q.track.len() >= 2 {
                (q.track.clone(), q.seg_starts.clone())
            } else {
                (r.track.clone(), r.track_segment_starts.clone())
            };
            reg.insert(
                id,
                GpxSource {
                    content_key,
                    track: std::sync::Arc::new(track),
                    seg_starts,
                    label: format!("Run {:.1} km", q.distance_km),
                },
            );
        }
    });
}

/// The GPX document for one logged run by identity; empty when the run is
/// unknown or has no track. For a run with a route (`has_gpx`) it is the QC'd
/// route the shell maps and shares; for an unmeasurable GPS run
/// (`gps_unmeasurable`) it is the recorded fixes, read only by Undo.
/// Reads the registry the most recent `view()` refreshed; the shell always
/// renders the run list before a run can be tapped to export. The source is
/// cloned out (an `Arc` bump) so the lock is not held while the document
/// builds. The document carries `observed_at` as the run's logged-at stamp, so
/// re-importing it is recognised as this run.
pub(crate) fn export_run_gpx(entry_id: u64, observed_at: i64) -> String {
    match with_registry(|reg| reg.get(&(entry_id, observed_at)).cloned()) {
        Some(s) => running::export_gpx_seg(&s.track, &s.label, &s.seg_starts, Some(observed_at)),
        None => String::new(),
    }
}
