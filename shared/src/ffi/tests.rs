use super::{current_view, process_event};

/// The ffi tests share ONE global bridge, so the two tests that submit a
/// `Pain` readiness input race: if the graded-tendon report lands between
/// the undo test's submit and its RemoveReadiness, the remove drops the
/// tendon input (most recent) and the bare stop stays, a false failure.
/// Serialize just those two; the rest keep running in parallel.
static PAIN_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Same shared-bridge hazard for the calculator-echo tests: both submit a
/// `ComputeHrZones` with a different age, so their `hr_zone_input` reads
/// race. Serialize just these two.
static CALC_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The panic-reset test replaces the entire shared bridge with a fresh
/// core. Ordinary bridge tests tolerate a concurrent peer clobbering a
/// field, but not a wholesale wipe landing mid-[update → view]. An RwLock
/// lets the ordinary tests keep running in parallel (read) while the reset
/// test runs alone (write).
static BRIDGE_RESET: std::sync::RwLock<()> = std::sync::RwLock::new(());

#[test]
fn json_event_updates_json_view() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
            "advanced": false
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
            "advanced": false
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    // exact decoupling→CorrectiveProcess mapping is pinned in app/mod.rs against an
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // Mirrors Core.kt SubmitReview.toJson week-fatigue fields, guarding the
    // snake_case wire names against a rename silently dropping the trigger on
    // the JNI boundary. A missing/renamed field would deserialize to None and
    // the event would still be accepted, so proving acceptance is not enough -
    // but these ffi tests share one global bridge, so a concurrent SubmitReview
    // could clobber model.review between this update and the view read. The
    // exact two-failed-session→Deload mapping is therefore pinned in app/mod.rs
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // The additive graded-pain fields (streak + pain{kind,severity,trend,
    // persists}) must survive the JNI-boundary Bridge exactly as a future
    // Core.kt will emit them: a field-name drift would make process_event
    // silently drop the report (deserialize error → cleared output), which
    // for a pain signal is a safety regression. The exact Table 4.1
    // adjustment mapping is pinned in the autoreg and app tests against isolated
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
                "persists": false,
                "location": "Left knee"
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

    // Clean up the Pain we submitted into the shared global bridge before
    // releasing PAIN_TESTS: otherwise, depending on parallel scheduling order,
    // a leftover Pain keeps `train_blocked` set and flakes the sibling
    // `remove_readiness_wire_shape_round_trips_the_bridge` test (which removes
    // only its OWN Pain and then asserts the hold is lifted).
    let remove = serde_json::json!({ "RemoveReadiness": { "signal": "Pain" } });
    process_event(remove.to_string().as_bytes());
}

#[test]
fn remove_readiness_wire_shape_round_trips_the_bridge() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    // A health-gated profile changes every later view on the shared bridge
    // (calculators show the hold row, guidance drops prescriptive rows), so
    // this test runs alone and leaves a fresh, ungated core behind, even if
    // an assertion fails.
    let _bridge = BRIDGE_RESET.write().unwrap_or_else(|e| e.into_inner());
    struct FreshBridgeOnDrop;
    impl Drop for FreshBridgeOnDrop {
        fn drop(&mut self) {
            super::reset_bridge();
        }
    }
    let _fresh = FreshBridgeOnDrop;
    // The additive Stage-0 health screen: a future Core.kt profile with
    // the nested `health` object plus the `female`/`high_load_block` flags
    // must survive the JNI-boundary Bridge: a field drift would make
    // process_event silently drop the profile, losing the safety gates on
    // replay (a safety regression). The exact gate→tier mapping is pinned
    // in app/mod.rs against an isolated model; the shared global bridge here
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
fn core_panic_resets_the_bridge_and_the_next_event_is_processed() {
    let _bridge = BRIDGE_RESET.write().unwrap_or_else(|e| e.into_inner());
    // A panic inside a core call must never unwind toward the JNI
    // `extern "system"` boundary (process abort since Rust 1.81). It is
    // caught and shaped as {"error":{"kind":"panic_reset",...}}, an object
    // (never the request array / view object), and, crucially, the bridge is
    // REPLACED with a fresh core: a panic under crux's inner
    // model RwLock would otherwise re-panic every later update/view for the
    // process lifetime. Panicking here while HOLDING the lock exercises the
    // poison path; the reset must leave a working bridge behind.
    let out = super::catch_panic("update", true, || {
        let _guard = super::lock_bridge();
        panic!("boom: \"quoted\" payload");
    });
    let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(v["error"]["kind"], "panic_reset");
    assert_eq!(v["error"]["context"], "update");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message string")
            .contains("boom"),
        "panic payload should be carried: {v}"
    );

    // Not bricked: the NEXT event is accepted (an effect-request array, not
    // an error envelope) on the fresh bridge, and its view renders — the exact
    // replay-after-reset the shell performs on `panic_reset`.
    let profile = serde_json::json!({
        "SetProfile": {
            "progression_cadence": "EverySession",
            "lift_goal": "MaxStrength",
            "goal_distance": "FiveK",
            "concurrent_goal": "Strength",
            "weekly_sets": 12,
            "running_days_per_week": 3,
            "running_km_per_week": 30.0,
            "advanced": false
        }
    });
    let requests = process_event(profile.to_string().as_bytes());
    let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
    assert!(
        requests.as_array().is_some(),
        "the event after a reset is processed, not an error envelope: {requests}"
    );
    let view = current_view();
    let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
    assert!(
        view["guidance"].is_array(),
        "view must render on the fresh bridge after a caught panic"
    );
}

#[test]
fn a_view_path_panic_reports_panic_without_resetting_the_bridge() {
    // `view` only takes crux's model READ guard, which a panic cannot poison,
    // so a caught view panic must return the NON-resetting `panic` kind and
    // leave the model intact (the shell keeps its last good view). A read
    // guard suffices: this call never resets the shared bridge.
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let out = super::catch_panic("view", false, || {
        panic!("view boom");
    });
    let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(v["error"]["kind"], "panic");
    assert_eq!(v["error"]["context"], "view");
}

#[test]
fn run_view_json_carries_split_verdict_and_full_spike_evidence() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // Pins the additive wire shape on RunResultView: the spike
    // gate's full evidence tag (grade/confidence/safety_critical/contested,
    // not just citation) and the core-owned split-verdict chip. Android's
    // ignoreUnknownKeys tolerates the new keys until the shell consumes them.
    // Equatorial fixes ~1.1 km apart (two full km splits, the floor for a
    // pacing verdict): fast front half, back half ~2.5x slower → "fade".
    let event = serde_json::json!({
        "LogRunTrack": {
            "points": [
                { "lat": 0.0, "lon": 0.00, "observed_at": 0,    "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.01, "observed_at": 200,  "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.02, "observed_at": 400,  "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.03, "observed_at": 900,  "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.04, "observed_at": 1400, "accuracy_m": 5.0 }
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
            .contains("as fast or faster")
    );
    assert_eq!(split["grade"], "Moderate");
    assert!(split["citation"].as_str().unwrap().contains("Hanley"));
    assert!(split["confidence"].as_f64().unwrap() > 0.0);
    assert_eq!(split["safety_critical"], false);
    assert_eq!(split["contested"], false);
}

#[test]
fn lift_view_json_carries_e1rm_trend_fields() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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

    // Second set: 102.5x5 vs 100x5, RPE 8 → 7 effective reps → +3.1 kg, "up".
    assert!((lifts[1]["e1rm_delta_kg"].as_f64().unwrap() - 3.1).abs() < 0.05);
    assert_eq!(lifts[1]["e1rm_direction"], "up");
}

#[test]
fn dormant_engine_view_sections_present_with_serde_defaults() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // Pins the additive ViewModel sections (weekly report, lift audit, the
    // dormant-engine calculators): they must serialize under these exact
    // keys, which Android's ignoreUnknownKeys tolerates until a shell
    // renders them. Shared global bridge → shape checks only.
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let cooper = serde_json::json!({ "ComputeCooper": { "distance_m_12min": 2600.0 } });
    let requests = process_event(cooper.to_string().as_bytes());
    let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
    assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

    let apre = serde_json::json!({
        "ComputeApre": { "scheme": "Apre6", "reps": 9, "current_load_kg": 100.0 }
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let _guard = CALC_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    // Old persisted form (no optional fields) must stay replayable…
    let old = serde_json::json!({ "ComputeHrZones": { "age_years": 30.0 } });
    let requests = process_event(old.to_string().as_bytes());
    let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "old form"
    );

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
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "new form"
    );

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
fn calculator_views_echo_their_inputs_in_json_for_form_rehydration() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    let _guard = CALC_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    // The shell rehydrates each calculator form from these echoed input
    // fields after process death: pin their JSON wire shape.
    let race = serde_json::json!({
        "PredictRace": {
            "recent_distance_m": 10000.0,
            "recent_time_sec": 2520.0,
            "goal_distance_m": 21097.5,
            "weekly_km": 55.0,
            "weeks_since_race": 3
        }
    });
    process_event(race.to_string().as_bytes());
    let hr = serde_json::json!({
        "ComputeHrZones": { "age_years": 47.0, "resting_hr_bpm": 52.0 }
    });
    process_event(hr.to_string().as_bytes());
    let protein = serde_json::json!({
        "ComputeProtein": { "bodyweight_kg": 82.5, "masters": true, "deficit": false }
    });
    process_event(protein.to_string().as_bytes());
    let hyper = serde_json::json!({
        "PlanHypertrophyMeso": { "muscle": "back", "weeks": 6 }
    });
    process_event(hyper.to_string().as_bytes());

    let view = current_view();
    let view: serde_json::Value = serde_json::from_slice(&view).unwrap();

    let rp = &view["race_prediction"];
    assert_eq!(rp["recent_distance_m"], 10000.0);
    assert_eq!(rp["recent_time_sec"], 2520.0);
    assert_eq!(rp["goal_distance_m"], 21097.5);
    assert_eq!(rp["weekly_km"], 55.0);
    assert_eq!(rp["weeks_since_race"], 3);

    let hz = &view["hr_zone_input"];
    assert_eq!(hz["age_years"], 47.0);
    assert_eq!(hz["resting_hr_bpm"], 52.0);

    let pr = &view["protein_input"];
    assert_eq!(pr["bodyweight_kg"], 82.5);
    assert_eq!(pr["masters"], true);
    assert_eq!(pr["deficit"], false);

    let hy = &view["hypertrophy_input"];
    assert_eq!(hy["muscle"], "back");
    assert_eq!(hy["weeks"], 6);
}

#[test]
fn run_view_json_carries_the_qc_dropped_count() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
fn android_shaped_extended_review_fields_round_trip_the_bridge() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // The additive review fields must survive the JNI-boundary
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
        "an accepted review-shape event emits a Render request"
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
fn android_shaped_extended_profile_fields_round_trip_the_bridge() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
        "an accepted profile-shape event emits a Render request"
    );

    // NB: the ffi tests share one global bridge, so another test's
    // SetProfile can clobber model.profile between this update and the
    // view read, the exact env→guidance mapping is pinned in app/mod.rs
    // against an isolated model. Here we prove the wire shape is accepted
    // and that the echoed profile always carries the new keys.
    let view = current_view();
    let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
    let profile = view["profile"].as_object().expect("profile echoed");
    for key in [
        "environment",
        "env_temp_c",
        "env_altitude_m",
        "weeks_off",
        "bodyweight_kg",
    ] {
        assert!(profile.contains_key(key), "profile key {key} missing");
    }
}

#[test]
fn readiness_summary_headline_and_signal_groups_serialize() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // Pins the additive KB-honest readiness wire shape: the per-signal
    // summary array, the core-owned today headline (always an object with
    // a kind), and the static signal→group metadata the shell's red-flag
    // picker fence consumes. Shared global bridge → shape checks only.
    let view = current_view();
    let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
    assert!(
        view["readiness_summary"].is_array(),
        "readiness_summary key"
    );
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
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
fn android_shaped_checkin_round_trips_and_surfaces_baseline_status() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
    // The morning check-in wire shape (raw human items +
    // optional watch numbers) must survive the JNI Bridge exactly as Core.kt
    // emits it; a field drift would silently drop the check-in on replay.
    // One check-in is far below the baseline minimum, so the view must carry
    // an honest "collecting baseline" status, never a fabricated z.
    let event = serde_json::json!({
        "SubmitCheckin": {
            "observed_at": 1_700_000_000,
            "sleep_quality": 2,
            "soreness": 4,
            "mood": 3,
            "resting_hr_bpm": 52.0,
            "hrv_rmssd_ms": 41.0
        }
    });
    let requests = process_event(event.to_string().as_bytes());
    let requests: serde_json::Value = serde_json::from_slice(&requests).unwrap();
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "a morning check-in must emit a Render request, not be dropped"
    );

    let view = current_view();
    let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
    // The check-in is echoed for rehydration.
    let echo = view["checkin_today"]
        .as_object()
        .expect("checkin_today object");
    assert_eq!(echo["sleep_quality"], 2);
    assert_eq!(echo["soreness"], 4);
    // Honest collecting-baseline status is present (no z was fabricated).
    let status = view["baseline_status"]
        .as_array()
        .expect("baseline_status array");
    assert!(
        status.iter().any(|b| b["signal"] == "WellnessZ"
            && b["kind"] == "Collecting"
            && b.get("note").is_none()),
        "wellness baseline is collecting: {status:?}"
    );

    // Clean up the shared global bridge so this check-in doesn't perturb the
    // sibling readiness/summary tests.
    process_event(b"\"ClearCheckins\"");
}

#[test]
fn parse_fit_json_reports_a_plain_error_for_garbage_bytes() {
    // No FIT/model/event-log involvement at all: prove the panic-firewalled
    // JSON wrapper around `crate::fit::parse_fit` shapes an `{"error":{...}}`
    // object rather than propagating a Rust `Result` or panicking across the
    // future JNI boundary.
    let out = super::parse_fit_json(b"not a fit file");
    let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(
        v,
        serde_json::json!({"error": {
            "kind": "not_fit",
            "context": "",
            "message": "Not a FIT file",
        }})
    );
}

#[test]
fn parse_fit_json_reports_a_bike_ride_as_not_a_run() {
    // A real Garmin Fenix 5 cycling file with a full GPS track: the shell gets
    // the machine-readable kind plus the sport, not a run import.
    let path = format!(
        "{}/tests/fixtures/garmin-fenix-5-bike.fit",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).expect("fixture should read");
    let out = super::parse_fit_json(&bytes);
    let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(
        v,
        serde_json::json!({"error": {
            "kind": "not_a_run",
            "context": "cycling",
            "message": "This FIT file records cycling, not a run",
        }})
    );
}

#[test]
fn parse_fit_json_refuses_a_fragment_too_short_to_be_a_run() {
    // The real `Activity.fit` running file holds a 13 s, ~3 m fragment: it
    // parses, but the import gate reports it as `no_track` / `too_short`
    // instead of handing the shell a "run".
    let path = format!("{}/tests/fixtures/Activity.fit", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).expect("fixture should read");
    let track = crate::fit::parse_fit(&bytes).expect("running file parses");
    assert_eq!(track.segments.iter().map(Vec::len).sum::<usize>(), 14);
    let out = super::parse_fit_json(&bytes);
    let v: serde_json::Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(v["error"]["kind"], "no_track", "{v}");
    assert_eq!(v["error"]["context"], "too_short", "{v}");
}

#[test]
fn malformed_event_is_dropped_without_panicking() {
    let _bridge = BRIDGE_RESET.read().unwrap_or_else(|e| e.into_inner());
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
