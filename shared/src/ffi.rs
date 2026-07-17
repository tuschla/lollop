//! JSON FFI bridge for native shells.
//!
//! Exposes the crux [`Engine`] over a JSON-serialised [`Bridge`] so a native
//! shell (Android/Kotlin via JNI) can drive the same side-effect-free core the
//! web shell uses. The wire format is JSON: events in, effect-requests +
//! view-model out. Reuses the existing `serde` derives, no Facet typegen
//! required.
//!
//! Flow for the shell:
//!   1. `update(eventJson)` → JSON array of effect requests (only `Render`).
//!   2. `view()` → JSON `ViewModel` reflecting the new state.

use std::sync::{Mutex, OnceLock};

use crux_core::{
    Core,
    bridge::{Bridge, JsonFfiFormat},
};

use crate::app::Engine;

// These helpers are consumed only by the `#[cfg(target_os = "android")]` JNI
// exports below and by the `#[cfg(test)]` module. On a plain host build neither
// is compiled, so the lint fires as a false positive, silence it there.
#[allow(dead_code)]
type JsonBridge = Bridge<Engine, JsonFfiFormat>;

#[allow(dead_code)]
fn bridge() -> &'static Mutex<JsonBridge> {
    static BRIDGE: OnceLock<Mutex<JsonBridge>> = OnceLock::new();
    BRIDGE.get_or_init(|| Mutex::new(Bridge::new(Core::default())))
}

/// Process a JSON-encoded event, returning the JSON effect-request array.
///
/// A malformed or unknown-variant event is dropped rather than propagated as a
/// panic: these functions are called across a JNI `extern "system"` boundary,
/// where unwinding is undefined behaviour. The realistic trigger is log replay -
/// the shell re-feeds every persisted event on launch, so a line written by a
/// future app version (a renamed/removed `Event` variant) must be skipped, not
/// turned into an unrecoverable startup crash-loop. Core state is simply left
/// unchanged and the shell renders the prior view.
#[allow(dead_code)]
fn process_event(event: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if bridge()
        .lock()
        .expect("ffi bridge poisoned")
        .update(event, &mut out)
        .is_err()
    {
        out.clear();
    }
    out
}

/// Serialize the current view model as JSON.
#[allow(dead_code)]
fn current_view() -> Vec<u8> {
    let mut out = Vec::new();
    bridge()
        .lock()
        .expect("ffi bridge poisoned")
        .view(&mut out)
        .expect("view should serialize");
    out
}

#[cfg(test)]
mod tests {
    use super::{current_view, process_event};

    #[test]
    fn json_event_updates_json_view() {
        // A profile event drives evidence-cited guidance into the view model.
        let event = serde_json::json!({
            "SetProfile": {
                "progression_cadence": "EverySession",
                "lift_goal": "MaxStrength",
                "goal_distance": "FiveK",
                "concurrent_goal": "Strength",
                "weekly_sets": 12,
                "running_days_per_week": 3,
                "running_km_per_week": 30.0,
                "advanced": false,
                "endurance_intensity_pct_vo2max": 70.0
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some(),
            "expected effect request array"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let guidance = view["guidance"].as_array().expect("guidance array");
        assert!(!guidance.is_empty(), "profile should yield guidance rows");
    }

    #[test]
    fn profile_enums_serialise_to_the_names_the_shell_parses() {
        // The view echoes each profile enum as a bare variant-name string
        // (serde unit-variant), and Kotlin's `ProfileDraft.from` feeds those
        // straight into `enumValueOf`. A `#[serde(rename)]` or a variant rename
        // would make that call throw on log replay, a launch crash. Lock the
        // exact wire strings so such a change fails here instead.
        let event = serde_json::json!({
            "SetProfile": {
                "progression_cadence": "EverySession",
                "lift_goal": "MaxStrength",
                "goal_distance": "FiveK",
                "concurrent_goal": "Strength",
                "weekly_sets": 12,
                "running_days_per_week": 3,
                "running_km_per_week": 30.0,
                "advanced": false,
                "endurance_intensity_pct_vo2max": 70.0
            }
        });
        process_event(event.to_string().as_bytes());

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let p = &view["profile"];
        assert_eq!(p["progression_cadence"], "EverySession");
        assert_eq!(p["lift_goal"], "MaxStrength");
        assert_eq!(p["goal_distance"], "FiveK");
        assert_eq!(p["concurrent_goal"], "Strength");
    }

    #[test]
    fn android_shaped_review_with_omitted_option_fields_is_accepted() {
        // Mirrors Core.kt SubmitReview.toJson: the optional distance-spike,
        // decoupling, and pacing fields are omitted entirely when unset. serde
        // maps a missing Option field to None, so this wire shape must round-trip
        // rather than being dropped, a guard against a future required (non-Option)
        // field silently breaking every shell-sent review.
        let event = serde_json::json!({
            "SubmitReview": {
                "bone_pain_red_flag": false,
                "compulsive_flag": false,
                "overtraining_signal_count": 0,
                "lift": { "reps_met": true, "rir_actual": 2, "rir_target": 2 },
                "bad_day": false
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "an accepted event must emit a Render request, not be dropped"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["feedback"].is_object(),
            "a review carrying a completed lift should surface session feedback"
        );
    }

    #[test]
    fn android_shaped_run_review_context_reaches_the_bridge() {
        // Mirrors Core.kt SubmitReview.toJson for a *run* review (no lift): the
        // decoupling object and the easy-run fraction are the fields the shell
        // added last, driving the decoupling / intensity-discipline feedback that
        // is otherwise unreachable from the device. Prove the exact wire shape
        // survives the JNI-boundary Bridge, not just a direct serde parse.
        let event = serde_json::json!({
            "SubmitReview": {
                "bone_pain_red_flag": false,
                "compulsive_flag": false,
                "overtraining_signal_count": 0,
                "decoupling": { "drift_pct": 12.0, "cool_steady_context": true },
                "bad_day": false
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "a run-review context must emit a Render request, not be dropped"
        );

        // NB: these ffi tests share one global bridge, so a tight assertion on the
        // feedback *category* would race a concurrent test's SubmitReview. The
        // exact decoupling→CorrectiveProcess mapping is pinned in app.rs against an
        // isolated model; here we only prove the wire shape is accepted, not dropped.
        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["feedback"].is_object(),
            "a run-review carrying decoupling context should surface session feedback"
        );
    }

    #[test]
    fn android_shaped_week_fatigue_review_round_trips_the_bridge() {
        // Mirrors Core.kt SubmitReview.toJson week-fatigue fields, guarding the
        // snake_case wire names against a rename silently dropping the trigger on
        // the JNI boundary. A missing/renamed field would deserialize to None and
        // the event would still be accepted, so proving acceptance is not enough -
        // but these ffi tests share one global bridge, so a concurrent SubmitReview
        // could clobber model.review between this update and the view read. The
        // exact two-failed-session→Deload mapping is therefore pinned in app.rs
        // against an isolated model; here we only prove the wire shape round-trips
        // the Bridge (Render emitted, view still serializes).
        let event = serde_json::json!({
            "SubmitReview": {
                "bone_pain_red_flag": false,
                "compulsive_flag": false,
                "overtraining_signal_count": 0,
                "failed_key_sessions": 2,
                "weekly_velocity_drop_m_s": 0.08,
                "rpe_load_gap_sessions": 2,
                "bad_day": false
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "an accepted week-fatigue review must emit a Render request"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["adjustments"].is_array(),
            "view still renders an adjustments array after a week-fatigue review"
        );
    }

    #[test]
    fn android_shaped_gps_track_round_trips_to_a_run_view() {
        // Mirrors Core.kt LogRunTrack.toJson: a nested `points` array of
        // {lat, lon, observed_at, accuracy_m} objects. This is the most complex
        // shell-sent shape (nested array + f32 accuracy), so lock it against a
        // field rename silently dropping every GPS-tracked run on the boundary.
        let event = serde_json::json!({
            "LogRunTrack": {
                "points": [
                    { "lat": 52.5200, "lon": 13.4050, "observed_at": 0, "accuracy_m": 5.0 },
                    { "lat": 52.5210, "lon": 13.4050, "observed_at": 30, "accuracy_m": 5.0 }
                ],
                "hr_pct_max": 78.0,
                "longest_recent_km": 12.0
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "an accepted GPS track must emit a Render request, not be dropped"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let runs = view["runs"].as_array().expect("runs array");
        assert!(
            !runs.is_empty(),
            "a logged GPS track should surface a run row"
        );
    }

    #[test]
    fn malformed_event_is_dropped_without_panicking() {
        // Simulates a stale/forward-incompatible log line hitting replay: an
        // unknown variant must not unwind across the FFI boundary, and the view
        // must still serialize cleanly afterwards.
        let out = process_event(br#"{"NoSuchEventVariant":{}}"#);
        assert!(out.is_empty(), "a rejected event yields no effect requests");

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["guidance"].is_array(),
            "view still renders after a bad event"
        );
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::{current_view, process_event};
    use jni::JNIEnv;
    use jni::objects::{JByteArray, JClass};
    use jni::sys::jbyteArray;

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_de_tuschla_fitnessanlage_Core_update(
        env: JNIEnv,
        _class: JClass,
        event: JByteArray,
    ) -> jbyteArray {
        let bytes = env
            .convert_byte_array(&event)
            .expect("event byte array should convert");
        let out = process_event(&bytes);
        env.byte_array_from_slice(&out)
            .expect("requests byte array should build")
            .into_raw()
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_de_tuschla_fitnessanlage_Core_view(
        env: JNIEnv,
        _class: JClass,
    ) -> jbyteArray {
        let out = current_view();
        env.byte_array_from_slice(&out)
            .expect("view byte array should build")
            .into_raw()
    }
}
