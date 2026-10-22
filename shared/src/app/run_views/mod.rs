use super::*;

mod card;
#[cfg(any(target_os = "android", feature = "jni-host", test))]
mod gpx;
mod progress;
mod qc;

pub(super) use card::*;
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(crate) use gpx::export_run_gpx;
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(super) use gpx::refresh_gpx_registry;
pub(super) use progress::*;
pub(super) use qc::*;

/// Cap on the per-run memos (view, QC track, best efforts). Live runs are
/// pruned every `view()` ([`prune_run_memos`]); the cap is a backstop for a
/// thread that derives runs without viewing. No real history approaches it.
const RUN_MEMO_CAP: usize = 4096;

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
