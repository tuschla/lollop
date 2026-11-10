mod card;
#[cfg(any(target_os = "android", feature = "ffi", test))]
mod gpx;
mod progress;

use crate::app::run_metrics::QC_TRACK_CACHE;
#[cfg(test)]
pub(super) use card::{RUN_VIEW_BUILDS, split_verdict_view};
pub(super) use card::{RUN_VIEW_CACHE, run_views, runs_fingerprint, to_run_view};
#[cfg(any(target_os = "android", feature = "ffi", test))]
pub(crate) use gpx::export_run_gpx;
#[cfg(any(target_os = "android", feature = "ffi", test))]
pub(super) use gpx::refresh_gpx_registry;
use progress::BEST_EFFORT_CACHE;
pub(super) use progress::{annotate_run_views, build_run_progress};

/// Drop any per-run memo entry whose fingerprint is no longer among the live
/// runs (a deleted or amended-away run), so steady-state memory is bounded by
/// the live history. The memos are per-thread: this prunes the calling
/// thread's copies. Output is identical whether or not it runs.
pub(super) fn prune_run_memos(keys: &[u64]) {
    let live: std::collections::HashSet<u64> = keys.iter().copied().collect();
    RUN_VIEW_CACHE.with(|c| c.borrow_mut().retain(|k| live.contains(k)));
    QC_TRACK_CACHE.with(|c| c.borrow_mut().retain(|k| live.contains(k)));
    BEST_EFFORT_CACHE.with(|c| c.borrow_mut().retain(|k| live.contains(k)));
}
