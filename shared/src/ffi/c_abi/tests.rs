//! The C ABI wraps `ffi` without changing its answers: these drive the
//! `extern "C"` functions exactly as a C caller would (raw pointers in, owned
//! buffers out, one free each). They share the one global bridge with the
//! `ffi` tests, so the bridge-touching ones hold its reset lock.

use super::*;
use crate::ffi::tests::BRIDGE_RESET;

/// Copy a returned buffer out and free it, as a shell does.
fn take(bytes: LollopBytes) -> Vec<u8> {
    let out = if bytes.data.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(bytes.data, bytes.len) }.to_vec()
    };
    unsafe { lollop_bytes_free(bytes) };
    out
}

fn update(event: &str) -> Vec<u8> {
    take(unsafe { lollop_update(event.as_ptr(), event.len()) })
}

const PROFILE: &str = r#"{"SetProfile":{"progression_cadence":"EverySession","lift_goal":"MaxStrength","goal_distance":"FiveK","concurrent_goal":"Strength","weekly_sets":12,"running_days_per_week":3,"running_km_per_week":30.0,"advanced":false}}"#;

#[test]
fn update_and_view_answer_over_the_c_abi() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let requests = update(PROFILE);
    assert_eq!(requests.first(), Some(&b'['), "an effect-request array");
    let view: serde_json::Value = serde_json::from_slice(&take(lollop_view())).unwrap();
    assert!(!view["guidance"].as_array().unwrap().is_empty());
}

#[test]
fn a_dropped_or_null_event_is_the_empty_answer() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let dropped = unsafe { lollop_update(b"{\"NoSuchEvent\":1}".as_ptr(), 17) };
    assert!(dropped.data.is_null() && dropped.len == 0);
    let null = unsafe { lollop_update(std::ptr::null(), 0) };
    assert!(null.data.is_null() && null.len == 0);
    // Freeing the empty answer is a no-op.
    unsafe { lollop_bytes_free(null) };
}

#[test]
fn compact_log_reports_a_torn_tail_by_line_index() {
    let lines = ["\"ClearSets\"", "{\"SetPro"];
    let slices: Vec<LollopSlice> = lines
        .iter()
        .map(|l| LollopSlice {
            data: l.as_ptr(),
            len: l.len(),
        })
        .collect();
    let verdict = take(unsafe { lollop_compact_log(slices.as_ptr(), slices.len()) });
    let verdict: serde_json::Value = serde_json::from_slice(&verdict).unwrap();
    assert_eq!(verdict["torn"], serde_json::json!([1]));
    // An empty log still gets a verdict (nothing dropped).
    let empty = take(unsafe { lollop_compact_log(std::ptr::null(), 0) });
    let empty: serde_json::Value = serde_json::from_slice(&empty).unwrap();
    assert_eq!(empty["dropped"], serde_json::json!([]));
}

#[test]
fn pure_helpers_answer_errors_and_empties() {
    let fit = take(unsafe { lollop_parse_fit(b"not a fit file".as_ptr(), 14) });
    assert!(
        fit.starts_with(b"{\"error\""),
        "{}",
        String::from_utf8_lossy(&fit)
    );
    let figures = take(unsafe { lollop_track_figures(b"\"ClearSets\"".as_ptr(), 11) });
    assert!(figures.starts_with(b"{\"error\""));
    assert!(take(lollop_export_gpx(42, 1_700_000_000)).is_empty());
}

#[test]
fn reset_empties_the_core_as_a_fresh_process_has_it() {
    let _bridge = BRIDGE_RESET.write().unwrap_or_else(|e| e.into_inner());
    // Other tests leave their events in the shared core: start from a reset one.
    lollop_reset();
    let empty = take(lollop_view());
    update(PROFILE);
    assert_ne!(take(lollop_view()), empty, "the profile reached the core");
    lollop_reset();
    assert_eq!(take(lollop_view()), empty, "reset leaves the empty core");
}
