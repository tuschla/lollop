//! The C entry points pass the bridge's answers through unchanged: these drive
//! them exactly as a C caller would (raw pointers in, owned buffers out, one
//! free each). They share the process's core, so none of them reads state
//! another could change.

use super::*;
use crate::ffi::ops::Op;

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

fn call(op: Op, input: &[u8]) -> Vec<u8> {
    take(unsafe { lollop_call(op.code(), input.as_ptr(), input.len()) })
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn the_view_answers_over_the_c_abi() {
    let view = json(&call(Op::View, b""));
    assert!(view["guidance"].is_array(), "{view}");
}

#[test]
fn a_dropped_or_null_event_is_the_empty_answer() {
    let dropped = unsafe { lollop_call(Op::Update.code(), b"{\"NoSuchEvent\":1}".as_ptr(), 17) };
    assert!(dropped.data.is_null() && dropped.len == 0);
    let null = unsafe { lollop_call(Op::Update.code(), std::ptr::null(), 0) };
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
    let code = Op::CompactLog.code();
    let verdict = json(&take(unsafe {
        lollop_call_lines(code, slices.as_ptr(), slices.len())
    }));
    assert_eq!(verdict["torn"], serde_json::json!([1]));
    // An empty log still gets a verdict (nothing dropped).
    let empty = json(&take(unsafe {
        lollop_call_lines(code, std::ptr::null(), 0)
    }));
    assert_eq!(empty["dropped"], serde_json::json!([]));
    // A line that is not UTF-8 fails the op.
    let bad = [LollopSlice {
        data: [0xff_u8].as_ptr(),
        len: 1,
    }];
    let failed = json(&take(unsafe { lollop_call_lines(code, bad.as_ptr(), 1) }));
    assert_eq!(failed["error"]["kind"], "bad_input", "{failed}");
}

#[test]
fn ops_answer_errors_and_empties() {
    let verdict = json(&call(Op::ParseRunFile, b"not a run file"));
    assert_eq!(verdict["refusal"], "wrong_type", "{verdict}");
    let figures = call(Op::TrackFigures, b"\"ClearSets\"");
    assert!(figures.starts_with(b"{\"error\""));
    assert!(
        call(
            Op::ExportGpx,
            br#"{"entry_id":42,"observed_at":1700000000}"#
        )
        .is_empty()
    );
    let unknown = json(&take(unsafe { lollop_call(u32::MAX, std::ptr::null(), 0) }));
    assert_eq!(unknown["error"]["kind"], "bad_input", "{unknown}");
    let lines_as_bytes = json(&call(Op::CompactLog, b"\"ClearSets\""));
    assert_eq!(
        lines_as_bytes["error"]["context"], "compact_log",
        "{lines_as_bytes}"
    );
}
