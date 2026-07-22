//! JSON FFI bridge for native shells.
//!
//! Exposes the crux [`Engine`] over a JSON-serialised [`Bridge`] so a native
//! shell (Android/Kotlin via JNI) can drive the same side-effect-free core.
//! The wire format is JSON: events in, effect-requests +
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

/// Lock the bridge, recovering the guard even if the mutex was poisoned.
///
/// The whole point of the FFI layer (see [`process_event`]) is that no shell
/// input turns into an unrecoverable crash-loop. A plain `.expect()` on the lock
/// would betray that: if any prior call ever panicked while holding the lock, the
/// mutex is poisoned and *every* subsequent `update`/`view` would panic across the
/// JNI `extern "system"` boundary (UB), a permanent brick, not a dropped event.
/// The bridge holds plain serde model data with no cross-field lock invariant, so
/// reusing the inner value after a poison is safe; recover it rather than crash.
#[allow(dead_code)]
fn lock_bridge() -> std::sync::MutexGuard<'static, JsonBridge> {
    bridge()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Minimal JSON string escaping for the panic-error payload: backslash, quote,
/// and control characters. Hand-rolled because `serde_json` is a dev-dependency
/// only, the error path must not add a runtime dependency to the core crate.
#[allow(dead_code)]
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Run a core call with a panic firewall: any panic is caught and converted to
/// a structured error JSON object instead of unwinding across the JNI
/// `extern "system"` boundary (which since Rust 1.81 aborts the whole process -
/// a hard app crash the shell can never intercept).
///
/// Shape: `{"error":{"kind":"panic","context":"update","message":"..."}}`, an
/// object, never an array, so a shell distinguishes it from the effect-request
/// array / `ViewModel` object by its top-level `"error"` key.
///
/// `AssertUnwindSafe` is sound here: the only state crossing the boundary is
/// the global bridge `Mutex`, whose poison [`lock_bridge`] deliberately
/// recovers (the model is plain serde data with no cross-field lock
/// invariant), so a caught panic leaves no poisoned state and the next call
/// proceeds on the last consistent-enough model rather than crash-looping.
#[allow(dead_code)]
fn catch_panic(context: &str, f: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(out) => out,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string());
            format!(
                r#"{{"error":{{"kind":"panic","context":"{}","message":"{}"}}}}"#,
                escape_json(context),
                escape_json(&msg)
            )
            .into_bytes()
        }
    }
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
///
/// A *panic* inside the core (as opposed to a serde error) is caught by
/// [`catch_panic`] and returned as a structured error object.
#[allow(dead_code)]
fn process_event(event: &[u8]) -> Vec<u8> {
    catch_panic("update", || {
        let mut out = Vec::new();
        if lock_bridge().update(event, &mut out).is_err() {
            out.clear();
        }
        out
    })
}

/// Serialize the current view model as JSON. A serialization panic surfaces as
/// a structured error object (see [`catch_panic`]), never a process abort.
#[allow(dead_code)]
fn current_view() -> Vec<u8> {
    catch_panic("view", || {
        let mut out = Vec::new();
        lock_bridge().view(&mut out).expect("view should serialize");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::{current_view, process_event};

    /// The ffi tests share ONE global bridge, so the two tests that submit a
    /// `Pain` readiness input race: if the graded-tendon report lands between
    /// the undo test's submit and its RemoveReadiness, the remove drops the
    /// tendon input (most recent) and the bare stop stays, a false failure.
    /// Serialize just those two; the rest keep running in parallel.
    static PAIN_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn android_shaped_graded_pain_readiness_round_trips_the_bridge() {
        // The additive graded-pain fields (streak + pain{kind,severity,trend,
        // persists}) must survive the JNI-boundary Bridge exactly as a future
        // Core.kt will emit them: a field-name drift would make process_event
        // silently drop the report (deserialize error → cleared output), which
        // for a pain signal is a safety regression. The exact Table 4.1
        // adjustment mapping is pinned in autoreg.rs/app.rs against isolated
        // models; the shared global bridge here only proves acceptance.
        let _pain_lock = PAIN_TESTS.lock().unwrap_or_else(|p| p.into_inner());
        let event = serde_json::json!({
            "SubmitReadiness": {
                "signal": "Pain",
                "value": 1.0,
                "observed_at": 0,
                "streak": 1,
                "pain": {
                    "kind": "TendonLoadRelated",
                    "severity": 3,
                    "trend": "Stable",
                    "persists": false
                }
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "a graded pain report must emit a Render request, not be dropped"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["adjustments"].is_array(),
            "view still renders an adjustments array after a graded pain report"
        );
    }

    #[test]
    fn remove_readiness_wire_shape_round_trips_the_bridge() {
        // Pins the undo event's JSON shape ({"RemoveReadiness":{"signal":...}})
        // end-to-end: a bare pain report hard-blocks, its removal lifts the
        // hold: the shell's mis-tap undo depends on exactly this sequence.
        let _pain_lock = PAIN_TESTS.lock().unwrap_or_else(|p| p.into_inner());
        let submit = serde_json::json!({
            "SubmitReadiness": {
                "signal": "Pain", "value": 1.0, "observed_at": 424_242
            }
        });
        process_event(submit.to_string().as_bytes());
        let view: serde_json::Value = serde_json::from_slice(&current_view()).unwrap();
        assert!(view["train_blocked"].as_bool().unwrap());

        let remove = serde_json::json!({ "RemoveReadiness": { "signal": "Pain" } });
        let requests = process_event(remove.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "RemoveReadiness must parse and emit a Render request"
        );
        let view: serde_json::Value = serde_json::from_slice(&current_view()).unwrap();
        assert!(
            !view["train_blocked"].as_bool().unwrap(),
            "removing the mis-tapped pain report must lift the hold"
        );
    }

    #[test]
    fn android_shaped_health_screen_profile_round_trips_the_bridge() {
        // The additive Stage-0 screen (Task 5): a future Core.kt profile with
        // the nested `health` object plus the `female`/`high_load_block` flags
        // must survive the JNI-boundary Bridge: a field drift would make
        // process_event silently drop the profile, losing the safety gates on
        // replay (a safety regression). The exact gate→tier mapping is pinned
        // in app.rs against an isolated model; the shared global bridge here
        // proves acceptance. Old profiles WITHOUT these fields are covered by
        // the pre-existing profile tests above (serde defaults).
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
                "endurance_intensity_pct_vo2max": 70.0,
                "female": true,
                "high_load_block": false,
                "health": {
                    "youth": false,
                    "parq_positive": true,
                    "medically_cleared": false,
                    "pregnant": false,
                    "pregnancy_warning_sign": false,
                    "injury_or_rehab": false,
                    "reds_signal": false
                }
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "a health-screen profile must emit a Render request, not be dropped"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["guidance"].is_array(),
            "view still renders guidance after a health-screen profile"
        );
    }

    #[test]
    fn core_panic_becomes_structured_error_json_and_state_survives() {
        // A panic inside a core call must never unwind toward the JNI
        // `extern "system"` boundary (process abort since Rust 1.81). It is
        // caught and shaped as {"error":{...}}, an object, so a shell can
        // tell it apart from the request array / view object, and, because
        // the panic here fires while HOLDING the bridge lock, this also
        // proves the poison-recovery path: the very next view call must work.
        let out = super::catch_panic("update", || {
            let _guard = super::lock_bridge();
            panic!("boom: \"quoted\" payload");
        });
        let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
        assert_eq!(v["error"]["kind"], "panic");
        assert_eq!(v["error"]["context"], "update");
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message string")
                .contains("boom"),
            "panic payload should be carried: {v}"
        );

        // Bridge not bricked: the lock poison is recovered and view serializes.
        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(
            view["guidance"].is_array(),
            "view must still render after a caught panic"
        );
    }

    #[test]
    fn run_view_json_carries_split_verdict_and_full_spike_evidence() {
        // Pins the additive Task-7/8 wire shape on RunResultView: the spike
        // gate's full evidence tag (grade/confidence/safety_critical/contested,
        // not just citation) and the core-owned split-verdict chip. Android's
        // ignoreUnknownKeys tolerates the new keys until task 14 consumes them.
        // Equatorial fixes: fast front half, back half ~2x slower → "fade".
        let event = serde_json::json!({
            "LogRunTrack": {
                "points": [
                    { "lat": 0.0, "lon": 0.000, "observed_at": 0,  "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.001, "observed_at": 20, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.002, "observed_at": 40, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.003, "observed_at": 90, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.004, "observed_at": 140, "accuracy_m": 5.0 }
                ],
                "hr_pct_max": 78.0,
                "longest_recent_km": 12.0,
                "observed_at": 777_000
            }
        });
        process_event(event.to_string().as_bytes());

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        // The ffi tests share one global bridge, so other tests' runs (some
        // with their own split objects) may be present too; select ours by its
        // unique final-fix timestamp instead of by shape.
        let run = view["runs"]
            .as_array()
            .expect("runs array")
            .iter()
            .find(|r| r["split"].is_object() && r["observed_at"] == 777_000)
            .expect("the split-carrying run row logged by this test");

        // Full spike-gate evidence tag (fields exist with real values).
        assert!(!run["grade"].as_str().unwrap().is_empty(), "grade: {run}");
        assert!(run["confidence"].as_f64().unwrap() > 0.0);
        assert!(run["safety_critical"].is_boolean());
        assert!(run["contested"].is_boolean());

        // Core-owned split verdict chip: threshold applied in core, shell
        // renders verdict/label/message/evidence verbatim.
        let split = &run["split"];
        assert_eq!(split["verdict"], "fade");
        assert!(split["label"].as_str().unwrap().starts_with("FADE +"));
        assert!(
            split["message"]
                .as_str()
                .unwrap()
                .contains("even-to-negative split")
        );
        assert_eq!(split["grade"], "Moderate");
        assert!(split["citation"].as_str().unwrap().contains("Hanley"));
        assert!(split["confidence"].as_f64().unwrap() > 0.0);
        assert_eq!(split["safety_critical"], false);
        assert_eq!(split["contested"], false);
    }

    #[test]
    fn lift_view_json_carries_e1rm_trend_fields() {
        // Pins the additive per-lift e1RM trend wire shape: delta + direction
        // vs the previous logged set of the same exercise, computed in core so
        // the shell renders without arithmetic. Distinctive exercise name so
        // the shared global bridge cannot collide with other tests' sets.
        let ex = "FFI Trend Pin Squat";
        for (weight, reps) in [(100.0, 5), (102.5, 5)] {
            let event = serde_json::json!({
                "LogSet": {
                    "exercise": ex, "weight_kg": weight, "reps": reps,
                    "rpe": 8.0, "observed_at": 0
                }
            });
            process_event(event.to_string().as_bytes());
        }

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let lifts: Vec<&serde_json::Value> = view["lifts"]
            .as_array()
            .expect("lifts array")
            .iter()
            .filter(|l| l["exercise"] == ex)
            .collect();
        assert_eq!(lifts.len(), 2);

        // First set of the exercise: nothing to compare against.
        assert!(lifts[0]["e1rm_delta_kg"].is_null());
        assert!(lifts[0]["e1rm_direction"].is_null());

        // Second set: 102.5x5 vs 100x5 Epley → +2.9 kg, direction "up".
        assert!((lifts[1]["e1rm_delta_kg"].as_f64().unwrap() - 2.9).abs() < 0.05);
        assert_eq!(lifts[1]["e1rm_direction"], "up");
    }

    #[test]
    fn task20_view_sections_present_with_serde_defaults() {
        // Pins the additive Task-20 ViewModel fields: they must serialize under
        // these exact keys (Android's ignoreUnknownKeys tolerates them until a
        // shell task consumes them). Shared global bridge → shape checks only.
        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(view["weekly_report"].is_array(), "weekly_report key");
        assert!(view["lift_audit"].is_array(), "lift_audit key");
        assert!(view["cooper"].is_array(), "cooper key");
        assert!(view["critical_speed"].is_array(), "critical_speed key");
        assert!(view["apre"].is_array(), "apre key");
        // Option fields serialize as null-or-object, never missing.
        let keys = view.as_object().expect("view object");
        for key in ["training_load", "provisional", "trend", "autoreg_source"] {
            assert!(keys.contains_key(key), "missing view key {key}");
        }
    }

    #[test]
    fn cooper_and_apre_calculator_events_round_trip_the_bridge() {
        let cooper = serde_json::json!({ "ComputeCooper": { "distance_m_12min": 2600.0 } });
        let requests = process_event(cooper.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

        let apre = serde_json::json!({
            "ComputeApre": { "scheme": "Apre6", "reps": 9, "current_load_lb": 100.0 }
        });
        let requests = process_event(apre.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let cooper_rows = view["cooper"].as_array().expect("cooper rows");
        assert!(
            cooper_rows
                .iter()
                .any(|r| r["summary"].as_str().unwrap().contains("VO2max")),
            "cooper estimate row present: {cooper_rows:?}"
        );
        let apre_rows = view["apre"].as_array().expect("apre rows");
        assert!(
            apre_rows
                .iter()
                .any(|r| r["summary"].as_str().unwrap().contains("APRE-6")),
            "apre row present: {apre_rows:?}"
        );
    }

    #[test]
    fn critical_speed_event_with_nested_efforts_round_trips_the_bridge() {
        let event = serde_json::json!({
            "ComputeCriticalSpeed": {
                "efforts": [
                    { "distance_m": 1200.0, "time_sec": 180.0 },
                    { "distance_m": 5000.0, "time_sec": 1200.0 }
                ]
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let rows = view["critical_speed"].as_array().expect("cs rows");
        assert!(
            rows.iter()
                .any(|r| r["summary"].as_str().unwrap().contains("Critical Speed")),
            "cs fit row present: {rows:?}"
        );
    }

    #[test]
    fn hr_zone_event_accepts_both_old_and_extended_wire_forms() {
        // Old persisted form (no optional fields) must stay replayable…
        let old = serde_json::json!({ "ComputeHrZones": { "age_years": 30.0 } });
        let requests = process_event(old.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()), "old form");

        // …and the extended form drives the Karvonen preference rows.
        let new = serde_json::json!({
            "ComputeHrZones": {
                "age_years": 30.0,
                "resting_hr_bpm": 50.0,
                "weeks_since_recalc": 5
            }
        });
        let requests = process_event(new.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()), "new form");

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let zones = view["hr_zones"].as_array().expect("zones");
        assert!(
            zones
                .iter()
                .any(|z| z["summary"].as_str().unwrap().contains("Karvonen")),
            "karvonen rows: {zones:?}"
        );
    }

    #[test]
    fn run_view_json_carries_the_qc_dropped_count() {
        let event = serde_json::json!({
            "LogRunTrack": {
                "points": [
                    { "lat": 0.0, "lon": 0.000, "observed_at": 1000, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.001, "observed_at": 1020, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.100, "observed_at": 1021, "accuracy_m": 5.0 },
                    { "lat": 0.0, "lon": 0.002, "observed_at": 1040, "accuracy_m": 5.0 }
                ],
                "hr_pct_max": 78.0,
                "longest_recent_km": 12.0,
                "observed_at": 1000
            }
        });
        process_event(event.to_string().as_bytes());

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let run = view["runs"]
            .as_array()
            .expect("runs")
            .iter()
            .find(|r| r["qc_dropped"].as_u64() == Some(1))
            .expect("the teleport-carrying run reports one dropped fix");
        // The teleport never inflates distance: well under 1 km.
        assert!(run["distance_km"].as_f64().unwrap() < 1.0);
    }

    #[test]
    fn lift_view_json_carries_the_e1rm_cross_check_object() {
        let ex = "FFI Cross Check Deadlift";
        let event = serde_json::json!({
            "LogSet": {
                "exercise": ex, "weight_kg": 180.0, "reps": 3,
                "rpe": 8.0, "observed_at": 0
            }
        });
        process_event(event.to_string().as_bytes());

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let lift = view["lifts"]
            .as_array()
            .expect("lifts")
            .iter()
            .find(|l| l["exercise"] == ex)
            .expect("our lift");
        let check = &lift["cross_check"];
        assert!(check.is_object(), "cross_check object: {lift}");
        assert_eq!(check["formulas"], 3);
        assert!(check["low_kg"].as_f64().unwrap() <= check["high_kg"].as_f64().unwrap());
        assert!(!check["citation"].as_str().unwrap().is_empty());
    }

    #[test]
    fn android_shaped_task20_review_fields_round_trip_the_bridge() {
        // The additive Task-20 review fields must survive the JNI-boundary
        // Bridge under these snake_case names: a drift would silently drop
        // the whole review on replay.
        let event = serde_json::json!({
            "SubmitReview": {
                "bone_pain_red_flag": false,
                "compulsive_flag": false,
                "overtraining_signal_count": 0,
                "mcv_delta_m_s": -0.1,
                "hrv_suppressed_days": 3,
                "hypertrophy_deload_triggers": 2,
                "trend_direction": "down",
                "performance_down": true,
                "low_recovery": true,
                "planned_hard": true,
                "bad_day": false
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "task-20 review shape accepted"
        );

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(view["review_adjustments"].is_array());
        assert!(
            view.as_object().expect("view object").contains_key("trend"),
            "trend key rendered after a trend-carrying review"
        );
    }

    #[test]
    fn android_shaped_task20_profile_fields_round_trip_the_bridge() {
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
                "endurance_intensity_pct_vo2max": 70.0,
                "environment": "Heat",
                "env_temp_c": 30.0,
                "weeks_off": 6.0,
                "bodyweight_kg": 80.0
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(
            requests.as_array().is_some_and(|a| !a.is_empty()),
            "task-20 profile shape accepted"
        );

        // NB: the ffi tests share one global bridge, so another test's
        // SetProfile can clobber model.profile between this update and the
        // view read, the exact env→guidance mapping is pinned in app.rs
        // against an isolated model. Here we prove the wire shape is accepted
        // and that the echoed profile always carries the new keys.
        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let profile = view["profile"].as_object().expect("profile echoed");
        for key in ["environment", "env_temp_c", "env_altitude_m", "weeks_off", "bodyweight_kg"] {
            assert!(profile.contains_key(key), "profile key {key} missing");
        }
    }

    #[test]
    fn readiness_summary_headline_and_signal_groups_serialize() {
        // Pins the additive KB-honest readiness wire shape: the per-signal
        // summary array, the core-owned today headline (always an object with
        // a kind), and the static signal→group metadata the shell's red-flag
        // picker fence consumes. Shared global bridge → shape checks only.
        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        assert!(view["readiness_summary"].is_array(), "readiness_summary key");
        let headline = view["today_headline"]
            .as_object()
            .expect("today_headline object");
        assert!(
            !headline["kind"].as_str().unwrap().is_empty(),
            "headline always carries a kind"
        );
        for key in ["summary", "grade", "citation", "confidence"] {
            assert!(headline.contains_key(key), "headline key {key}");
        }
        let groups = view["signal_groups"].as_array().expect("signal_groups");
        assert_eq!(groups.len(), 15, "one row per readiness signal");
        assert!(
            groups
                .iter()
                .any(|g| g["signal"] == "Pain" && g["group"] == "red_flag"),
            "pain fenced as red_flag: {groups:?}"
        );
        assert!(
            groups
                .iter()
                .any(|g| g["signal"] == "HrvLnRmssd" && g["group"] == "metric"),
            "hrv grouped as metric: {groups:?}"
        );
    }

    #[test]
    fn android_shaped_backdated_readiness_surfaces_in_the_summary() {
        // A readiness submission with an explicit past stamp (the shell's new
        // backdating control) must round-trip into a per-signal summary row
        // with the judged state + evidence tag. Distinctive signal (Soreness -
        // no other ffi test submits it) so the shared bridge cannot collide.
        let event = serde_json::json!({
            "SubmitReadiness": {
                "signal": "Soreness", "value": 6.0, "observed_at": 1_600_000_000
            }
        });
        let requests = process_event(event.to_string().as_bytes());
        let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
        assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let row = view["readiness_summary"]
            .as_array()
            .expect("summary array")
            .iter()
            .find(|s| s["signal"] == "Soreness")
            .expect("soreness row present");
        assert_eq!(row["state"], "high");
        assert_eq!(row["group"], "metric");
        assert!(!row["grade"].as_str().unwrap().is_empty());
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
    pub extern "system" fn Java_app_milestone_Core_update(
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
    pub extern "system" fn Java_app_milestone_Core_view(
        env: JNIEnv,
        _class: JClass,
    ) -> jbyteArray {
        let out = current_view();
        env.byte_array_from_slice(&out)
            .expect("view byte array should build")
            .into_raw()
    }
}
