//! On-demand GPX export: `view()` keeps a registry of every exportable GPS
//! run's QC'd route, and the `exportGpx` FFI query builds the ~0.5 MB document
//! for the one run the shell inspects, so it never rides the `ViewModel`.
//! Compiled only where the JNI exports exist.

use super::*;
use std::collections::{HashMap, HashSet};

/// One GPS run's export source: the QC'd track + segment boundaries + the
/// track-name label, everything [`running::export_gpx_seg`] needs.
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
/// `view()`: a run no longer live is dropped, a run without an exportable
/// route (<2 QC'd fixes) is removed, and an unchanged run (same content
/// key) is skipped, so a steady-state refresh re-clones nothing.
pub(in crate::app) fn refresh_gpx_registry(runs: &[LoggedRun], facts: &RunFacts) {
    with_registry(|reg| {
        let live: HashSet<(u64, i64)> = runs.iter().map(|r| (r.entry_id, r.observed_at)).collect();
        reg.retain(|id, _| live.contains(id));
        for ((r, &content_key), q) in runs.iter().zip(&facts.keys).zip(&facts.qc) {
            let id = (r.entry_id, r.observed_at);
            let Some(q) = q.as_ref().filter(|q| q.track.len() >= 2) else {
                reg.remove(&id);
                continue;
            };
            if reg.get(&id).is_some_and(|s| s.content_key == content_key) {
                continue;
            }
            reg.insert(
                id,
                GpxSource {
                    content_key,
                    track: std::sync::Arc::new(q.track.clone()),
                    seg_starts: q.seg_starts.clone(),
                    label: format!("Run {:.1} km", q.distance_km),
                },
            );
        }
    });
}

/// The GPX document for one logged run by identity; empty when the run is
/// unknown or has no exportable route (the shell's `has_gpx` gate agrees).
/// Reads the registry the most recent `view()` refreshed; the shell always
/// renders the run list before a run can be tapped to export. The source is
/// cloned out (an `Arc` bump) so the lock is not held while the document
/// builds.
pub(crate) fn export_run_gpx(entry_id: u64, observed_at: i64) -> String {
    match with_registry(|reg| reg.get(&(entry_id, observed_at)).cloned()) {
        Some(s) => running::export_gpx_seg(&s.track, &s.label, &s.seg_starts),
        None => String::new(),
    }
}
