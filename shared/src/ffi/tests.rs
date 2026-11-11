//! Each test drives its own [`CoreBridge`] (its own core on its own thread),
//! so tests never see each other's events.

use std::borrow::Cow;

use serde_json::{Value, json};

use super::bridge::{CoreBridge, Input, answer};
use super::ops::Op;
use crate::envelope::ErrorKind;

/// Op `op`'s answer to `input`.
fn call(core: &CoreBridge, op: Op, input: &[u8]) -> Vec<u8> {
    core.call(op.code(), Input::Bytes(Cow::Borrowed(input)))
}

/// An op that reads no core state, answered by a fresh core.
fn pure(op: Op, input: &[u8]) -> Value {
    serde_json::from_slice(&call(&CoreBridge::new(), op, input)).unwrap()
}

/// Send one event's wire JSON; the answer, parsed.
fn send(core: &CoreBridge, event: &Value) -> Value {
    serde_json::from_slice(&call(core, Op::Update, event.to_string().as_bytes())).unwrap()
}

/// The current view, parsed.
fn view(core: &CoreBridge) -> Value {
    serde_json::from_slice(&call(core, Op::View, b"")).unwrap()
}

fn profile() -> Value {
    json!({
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
    })
}

#[test]
fn error_kinds_serialise_to_the_names_the_shell_reads() {
    // `CoreWire.kt` matches `panic_reset`.
    let kinds = [
        (ErrorKind::PanicReset, "panic_reset"),
        (ErrorKind::Panic, "panic"),
        (ErrorKind::Unserializable, "unserializable"),
        (ErrorKind::BadInput, "bad_input"),
    ];
    for (kind, name) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap(), name);
        assert_eq!(kind.to_string(), name);
    }
}

#[test]
fn json_event_updates_json_view() {
    let core = CoreBridge::new();
    // A profile event drives evidence-cited guidance into the view model.
    let requests = send(&core, &profile());
    assert!(
        requests.as_array().is_some(),
        "expected effect request array"
    );

    let view = view(&core);
    let guidance = view["guidance"].as_array().expect("guidance array");
    assert!(!guidance.is_empty(), "profile should yield guidance rows");
}

#[test]
fn profile_enums_serialise_to_the_names_the_shell_parses() {
    let core = CoreBridge::new();
    // The view echoes each profile enum as a bare variant-name string
    // (serde unit-variant), and Kotlin's `ProfileDraft.from` feeds those
    // straight into `enumValueOf`. A `#[serde(rename)]` or a variant rename
    // would make that call throw on log replay, a launch crash. Lock the
    // exact wire strings so such a change fails here instead.
    send(&core, &profile());

    let view = view(&core);
    let p = &view["profile"];
    assert_eq!(p["progression_cadence"], "EverySession");
    assert_eq!(p["lift_goal"], "MaxStrength");
    assert_eq!(p["goal_distance"], "FiveK");
    assert_eq!(p["concurrent_goal"], "Strength");
}

#[test]
fn android_shaped_review_with_omitted_option_fields_is_accepted() {
    let core = CoreBridge::new();
    // Mirrors Core.kt SubmitReview.toJson: the optional distance-spike,
    // decoupling, and pacing fields are omitted entirely when unset. serde
    // maps a missing Option field to None, so this wire shape must round-trip
    // rather than being dropped, a guard against a future required (non-Option)
    // field silently breaking every shell-sent review.
    let event = json!({
        "SubmitReview": {
            "bone_pain_red_flag": false,
            "compulsive_flag": false,
            "overtraining_signal_count": 0,
            "lift": { "reps_met": true, "rir_actual": 2, "rir_target": 2 },
            "bad_day": false
        }
    });
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "an accepted event must emit a Render request, not be dropped"
    );

    let view = view(&core);
    assert!(
        view["feedback"].is_object(),
        "a review carrying a completed lift should surface session feedback"
    );
}

#[test]
fn android_shaped_run_review_context_reaches_the_bridge() {
    let core = CoreBridge::new();
    // Mirrors Core.kt SubmitReview.toJson for a *run* review (no lift): the
    // decoupling object and the easy-run fraction are the fields the shell
    // added last, driving the decoupling / intensity-discipline feedback that
    // is otherwise unreachable from the device. Prove the exact wire shape
    // survives the Bridge, not just a direct serde parse; the
    // decoupling→CorrectiveProcess mapping is pinned in the app tests.
    let event = json!({
        "SubmitReview": {
            "bone_pain_red_flag": false,
            "compulsive_flag": false,
            "overtraining_signal_count": 0,
            "decoupling": { "drift_pct": 12.0, "cool_steady_context": true },
            "bad_day": false
        }
    });
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "a run-review context must emit a Render request, not be dropped"
    );

    let view = view(&core);
    assert!(
        view["feedback"].is_object(),
        "a run-review carrying decoupling context should surface session feedback"
    );
}

#[test]
fn android_shaped_week_fatigue_review_round_trips_the_bridge() {
    let core = CoreBridge::new();
    // Mirrors Core.kt SubmitReview.toJson week-fatigue fields, guarding the
    // snake_case wire names against a rename silently dropping the trigger on
    // the boundary. The two-failed-session→Deload mapping is pinned in the
    // app tests; here the wire shape round-trips the Bridge (Render emitted,
    // view still serializes).
    let event = json!({
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
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "an accepted week-fatigue review must emit a Render request"
    );

    let view = view(&core);
    assert!(
        view["adjustments"].is_array(),
        "view still renders an adjustments array after a week-fatigue review"
    );
}

#[test]
fn android_shaped_gps_track_round_trips_to_a_run_view() {
    let core = CoreBridge::new();
    // Mirrors Core.kt LogRunTrack.toJson: a nested `points` array of
    // {lat, lon, observed_at, accuracy_m} objects. This is the most complex
    // shell-sent shape (nested array + f32 accuracy), so lock it against a
    // field rename silently dropping every GPS-tracked run on the boundary.
    let event = json!({
        "LogRunTrack": {
            "points": [
                { "lat": 52.5200, "lon": 13.4050, "observed_at": 0, "accuracy_m": 5.0 },
                { "lat": 52.5210, "lon": 13.4050, "observed_at": 30, "accuracy_m": 5.0 }
            ],
            "hr_pct_max": 78.0,
            "longest_recent_km": 12.0
        }
    });
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "an accepted GPS track must emit a Render request, not be dropped"
    );

    let view = view(&core);
    let runs = view["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 1, "a logged GPS track should surface a run row");
}

#[test]
fn android_shaped_graded_pain_readiness_round_trips_the_bridge() {
    let core = CoreBridge::new();
    // The additive graded-pain fields (streak + pain{kind,severity,trend,
    // persists}) must survive the Bridge exactly as Core.kt emits them: a
    // field-name drift would make process_event silently drop the report
    // (deserialize error → empty answer), which for a pain signal is a safety
    // regression. The exact Table 4.1 adjustment mapping is pinned in the
    // autoreg and app tests.
    let event = json!({
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
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "a graded pain report must emit a Render request, not be dropped"
    );

    let view = view(&core);
    assert!(
        view["adjustments"].is_array(),
        "view still renders an adjustments array after a graded pain report"
    );
}

#[test]
fn remove_readiness_wire_shape_round_trips_the_bridge() {
    let core = CoreBridge::new();
    // Pins the undo event's JSON shape ({"RemoveReadiness":{"signal":...}})
    // end-to-end: a bare pain report hard-blocks, its removal lifts the
    // hold: the shell's mis-tap undo depends on exactly this sequence.
    let submit = json!({
        "SubmitReadiness": {
            "signal": "Pain", "value": 1.0, "observed_at": 424_242
        }
    });
    send(&core, &submit);
    assert!(!view(&core)["hold_reason"].is_null());

    let remove = json!({ "RemoveReadiness": { "signal": "Pain" } });
    let requests = send(&core, &remove);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "RemoveReadiness must parse and emit a Render request"
    );
    assert!(
        view(&core)["hold_reason"].is_null(),
        "removing the mis-tapped pain report must lift the hold"
    );
}

#[test]
fn android_shaped_health_screen_profile_round_trips_the_bridge() {
    let core = CoreBridge::new();
    // The additive Stage-0 health screen: a Core.kt profile with the nested
    // `health` object plus the `female`/`high_load_block` flags must survive
    // the Bridge: a field drift would make process_event silently drop the
    // profile, losing the safety gates on replay (a safety regression). The
    // exact gate→tier mapping is pinned in the app tests. Old profiles
    // WITHOUT these fields are covered by the profile tests above (serde
    // defaults).
    let event = json!({
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
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "a health-screen profile must emit a Render request, not be dropped"
    );

    let view = view(&core);
    assert!(
        view["guidance"].is_array(),
        "view still renders guidance after a health-screen profile"
    );
}

#[test]
fn core_panic_resets_the_bridge_and_the_next_event_is_processed() {
    let core = CoreBridge::new();
    // A panic inside a core call must never unwind toward the FFI boundary
    // (process abort since Rust 1.81). It is caught and shaped as
    // {"error":{"kind":"panic_reset",...}}, an object (never the request
    // array / view object), and the core is REPLACED: a panic under crux's
    // inner model RwLock would otherwise re-panic every later update/view for
    // the process lifetime. Panicking while HOLDING the bridge lock exercises
    // the poison path; the reset must leave a working bridge behind.
    let out = answer(core.on_core(Op::Update, |_| panic!("boom: \"quoted\" payload")));
    let v: Value = serde_json::from_slice(&out).expect("error JSON parses");
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
    // an error envelope) on the fresh core, and its view renders: the exact
    // replay-after-reset the shell performs on `panic_reset`.
    let requests = send(&core, &profile());
    assert!(
        requests.as_array().is_some(),
        "the event after a reset is processed, not an error envelope: {requests}"
    );
    assert!(
        view(&core)["guidance"].is_array(),
        "view must render on the fresh bridge after a caught panic"
    );
}

#[test]
fn a_view_path_panic_reports_panic_without_resetting_the_bridge() {
    let core = CoreBridge::new();
    send(&core, &profile());
    let before = call(&core, Op::View, b"");
    // `view` only takes crux's model READ guard, which a panic cannot poison,
    // so a caught view panic must return the NON-resetting `panic` kind and
    // leave the model intact (the shell keeps its last good view).
    let out = answer(core.on_core(Op::View, |_| panic!("view boom")));
    let v: Value = serde_json::from_slice(&out).expect("error JSON parses");
    assert_eq!(v["error"]["kind"], "panic");
    assert_eq!(v["error"]["context"], "view");
    assert_eq!(
        call(&core, Op::View, b""),
        before,
        "the core survives a view panic"
    );
}

#[test]
fn run_view_json_carries_split_verdict_and_full_spike_evidence() {
    let core = CoreBridge::new();
    // Pins the additive wire shape on RunResultView: the spike
    // gate's full evidence tag (grade/confidence/safety_critical/contested,
    // not just citation) and the core-owned split-verdict chip. Android's
    // ignoreUnknownKeys tolerates the new keys until the shell consumes them.
    // Equatorial fixes ~1.1 km apart (two full km splits, the floor for a
    // pacing verdict): fast front half, back half ~2.5x slower → "fade".
    let event = json!({
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
            "observed_at": 1400
        }
    });
    send(&core, &event);

    let view = view(&core);
    let runs = view["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];

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
fn lift_view_json_carries_the_pr_marker() {
    let core = CoreBridge::new();
    // Pins the per-lift PR marker on the wire.
    for (weight, reps) in [(100.0, 5), (102.5, 5)] {
        let event = json!({
            "LogSet": {
                "exercise": "Back Squat", "weight_kg": weight, "reps": reps,
                "rpe": 8.0, "observed_at": 0
            }
        });
        send(&core, &event);
    }

    let view = view(&core);
    let lifts = view["lifts"].as_array().expect("lifts array");
    assert_eq!(lifts.len(), 2);

    // First set of the exercise: nothing to beat; the heavier second set is a PR.
    assert_eq!(lifts[0]["e1rm_pr"], false);
    assert_eq!(lifts[1]["e1rm_pr"], true);
}

#[test]
fn additive_view_sections_present_with_serde_defaults() {
    let core = CoreBridge::new();
    // Pins additive ViewModel sections: they must serialize under these exact
    // keys.
    let view = view(&core);
    assert!(view["weekly_report"].is_array(), "weekly_report key");
    assert!(view["lift_audit"].is_array(), "lift_audit key");
    // Option fields serialize as null-or-object, never missing.
    let keys = view.as_object().expect("view object");
    for key in ["training_load", "provisional", "trend", "autoreg_source"] {
        assert!(keys.contains_key(key), "missing view key {key}");
    }
}

#[test]
fn hr_zone_event_accepts_both_old_and_extended_wire_forms() {
    let core = CoreBridge::new();
    // Old persisted form (no optional fields) must stay replayable…
    let old = json!({ "ComputeHrZones": { "age_years": 30.0 } });
    let requests = send(&core, &old);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "old form"
    );

    // …and the extended form drives the Karvonen preference rows.
    let new = json!({
        "ComputeHrZones": {
            "age_years": 30.0,
            "resting_hr_bpm": 50.0
        }
    });
    let requests = send(&core, &new);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "new form"
    );

    let view = view(&core);
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
    let core = CoreBridge::new();
    // The shell rehydrates each calculator form from these echoed input
    // fields after process death: pin their JSON wire shape.
    let race = json!({
        "PredictRace": {
            "recent_distance_m": 10000.0,
            "recent_time_sec": 2520.0,
            "goal_distance_m": 21097.5,
            "weekly_km": 55.0,
            "weeks_since_race": 3
        }
    });
    send(&core, &race);
    let hr = json!({
        "ComputeHrZones": { "age_years": 47.0, "resting_hr_bpm": 52.0 }
    });
    send(&core, &hr);
    let protein = json!({
        "ComputeProtein": { "bodyweight_kg": 82.5, "masters": true, "deficit": false }
    });
    send(&core, &protein);
    let hyper = json!({
        "PlanHypertrophyMeso": { "muscle": "back", "weeks": 6 }
    });
    send(&core, &hyper);

    let view = view(&core);

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
fn run_view_json_drops_a_teleport_fix_from_the_distance() {
    let core = CoreBridge::new();
    let event = json!({
        "LogRunTrack": {
            "points": [
                { "lat": 0.0, "lon": 0.000, "observed_at": 0,  "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.001, "observed_at": 20, "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.100, "observed_at": 21, "accuracy_m": 5.0 },
                { "lat": 0.0, "lon": 0.002, "observed_at": 40, "accuracy_m": 5.0 }
            ],
            "hr_pct_max": 78.0,
            "longest_recent_km": 12.0,
            "observed_at": 0
        }
    });
    send(&core, &event);

    let view = view(&core);
    let runs = view["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 1, "the teleport-carrying run");
    // The ~11 km teleport never inflates distance: well under 1 km.
    assert!(runs[0]["distance_km"].as_f64().unwrap() < 1.0);
}

#[test]
fn lift_view_json_carries_the_e1rm_cross_check_object() {
    let core = CoreBridge::new();
    let event = json!({
        "LogSet": {
            "exercise": "Deadlift", "weight_kg": 180.0, "reps": 3,
            "rpe": 8.0, "observed_at": 0
        }
    });
    send(&core, &event);

    let view = view(&core);
    let lifts = view["lifts"].as_array().expect("lifts");
    assert_eq!(lifts.len(), 1, "our lift");
    let lift = &lifts[0];
    let check = &lift["cross_check"];
    assert!(check.is_object(), "cross_check object: {lift}");
    assert_eq!(check["formulas"], 3);
    assert!(check["low_kg"].as_f64().unwrap() <= check["high_kg"].as_f64().unwrap());
    assert!(!check["citation"].as_str().unwrap().is_empty());
}

#[test]
fn android_shaped_extended_review_fields_round_trip_the_bridge() {
    let core = CoreBridge::new();
    // The additive review fields must survive the Bridge under these
    // snake_case names: a drift would silently drop the whole review on
    // replay.
    let event = json!({
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
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "an accepted review-shape event emits a Render request"
    );

    let view = view(&core);
    assert!(view["review_adjustments"].is_array());
    assert!(
        view.as_object().expect("view object").contains_key("trend"),
        "trend key rendered after a trend-carrying review"
    );
}

#[test]
fn android_shaped_extended_profile_fields_round_trip_the_bridge() {
    let core = CoreBridge::new();
    let event = json!({
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
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "an accepted profile-shape event emits a Render request"
    );

    // The env→guidance mapping is pinned in the app tests; here the wire
    // shape is accepted and the echoed profile carries the new keys.
    let view = view(&core);
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
    let core = CoreBridge::new();
    // Pins the additive KB-honest readiness wire shape: the per-signal
    // summary array, the core-owned today headline (always an object with
    // a kind), and the static signal→group metadata the shell's red-flag
    // picker fence consumes.
    let view = view(&core);
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
    let core = CoreBridge::new();
    // A readiness submission with an explicit past stamp (the shell's
    // backdating control) must round-trip into a per-signal summary row
    // with the judged state + evidence tag.
    let event = json!({
        "SubmitReadiness": {
            "signal": "Soreness", "value": 6.0, "observed_at": 1_600_000_000
        }
    });
    let requests = send(&core, &event);
    assert!(requests.as_array().is_some_and(|a| !a.is_empty()));

    let view = view(&core);
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
    let core = CoreBridge::new();
    // The morning check-in wire shape (raw human items + optional watch
    // numbers) must survive the Bridge exactly as Core.kt emits it; a field
    // drift would silently drop the check-in on replay. One check-in is far
    // below the baseline minimum, so the view must carry an honest
    // "collecting baseline" status, never a fabricated z.
    let event = json!({
        "SubmitCheckin": {
            "observed_at": 1_700_000_000,
            "sleep_quality": 2,
            "soreness": 4,
            "mood": 3,
            "resting_hr_bpm": 52.0,
            "hrv_rmssd_ms": 41.0
        }
    });
    let requests = send(&core, &event);
    assert!(
        requests.as_array().is_some_and(|a| !a.is_empty()),
        "a morning check-in must emit a Render request, not be dropped"
    );

    let view = view(&core);
    // The check-in is echoed for rehydration.
    let echo = view["latest_checkin"]
        .as_object()
        .expect("latest_checkin object");
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
}

#[test]
fn parse_run_file_answers_a_verdict_for_garbage_bytes() {
    // No model or event log, no error envelope: a file that is no run is a
    // verdict with its reason.
    let v = pure(Op::ParseRunFile, b"not a run file");
    assert_eq!(v, json!({ "runs": [], "refusal": "wrong_type" }), "{v}");
    let fit_magic = pure(Op::ParseRunFile, b"not a .FIT file");
    assert_eq!(
        fit_magic,
        json!({ "runs": [], "refusal": "wrong_type" }),
        "{fit_magic}"
    );
}

#[test]
fn parse_run_file_reports_a_bike_ride_as_not_a_run() {
    // A real Garmin Fenix 5 cycling file with a full GPS track: the shell gets
    // the reason plus the sport, not a run import.
    let path = format!(
        "{}/tests/fixtures/garmin-fenix-5-bike.fit",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).expect("fixture should read");
    let v = pure(Op::ParseRunFile, &bytes);
    assert_eq!(
        v,
        json!({ "runs": [], "refusal": "not_a_run", "sport": "cycling" }),
        "{v}"
    );
}

#[test]
fn parse_run_file_refuses_a_fragment_too_short_to_be_a_run() {
    // The real `Activity.fit` running file holds a 13 s, ~3 m fragment: it
    // parses, but the keep threshold refuses it as `too_short`.
    let path = format!("{}/tests/fixtures/Activity.fit", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).expect("fixture should read");
    let v = pure(Op::ParseRunFile, &bytes);
    assert_eq!(v, json!({ "runs": [], "refusal": "too_short" }), "{v}");
}

#[test]
fn parse_run_file_answers_the_runs_to_log() {
    let gpx = crate::gps::export_gpx(
        &(0..300)
            .map(|k| crate::gps::GpsPoint {
                lat: 47.0 + f64::from(k) * 3.0 / 111_195.0,
                lon: 8.0,
                observed_at: 1_750_000_000 + i64::from(k),
                accuracy_m: 5.0,
            })
            .collect::<Vec<_>>(),
        "Run",
        &[],
        Some(1_750_000_400),
    );
    let v = pure(Op::ParseRunFile, gpx.as_bytes());
    let runs = v["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 1, "{v}");
    assert_eq!(runs[0]["points"].as_array().map(Vec::len), Some(300));
    assert_eq!(runs[0]["observed_at"], 1_750_000_400);
    assert!(
        v.get("refusal").is_none() && runs[0].get("segment_starts").is_none(),
        "{v}"
    );
}

#[test]
fn run_track_answers_a_runs_route_fixes_by_segment() {
    let core = CoreBridge::new();
    let points: Vec<Value> = (0..40)
        .map(|k| json!({"lat": 47.0 + f64::from(k) * 0.0001, "lon": 8.0, "observed_at": 1_750_000_000 + k * 5, "accuracy_m": 5.0}))
        .collect();
    send(
        &core,
        &json!({"LogRunTrack": {"points": points, "hr_pct_max": 0.0, "longest_recent_km": 0.0,
            "observed_at": 1_750_000_195, "entry_id": 77, "segment_starts": [20]}}),
    );
    let run = br#"{"entry_id":77,"observed_at":1750000195}"#;
    let track: Value = serde_json::from_slice(&call(&core, Op::RunTrack, run)).unwrap();
    let segments = track["segments"].as_array().expect("segments");
    assert_eq!(
        segments
            .iter()
            .map(|s| s.as_array().map_or(0, Vec::len))
            .collect::<Vec<_>>(),
        [20, 20],
        "{track}"
    );
    assert_eq!(segments[1][0]["observed_at"], 1_750_000_100);
    let unknown: Value = serde_json::from_slice(&call(
        &core,
        Op::RunTrack,
        br#"{"entry_id":1,"observed_at":2}"#,
    ))
    .unwrap();
    assert_eq!(unknown, json!({"segments": []}));
    let bad: Value = serde_json::from_slice(&call(&core, Op::RunTrack, b"{")).unwrap();
    assert_eq!(bad["error"]["kind"], "bad_input");
    assert_eq!(bad["error"]["context"], "run_track");
}

#[test]
fn malformed_event_is_dropped_without_panicking() {
    let core = CoreBridge::new();
    // Simulates a stale/forward-incompatible log line hitting replay: an
    // unknown variant must not unwind across the FFI boundary, and the view
    // must still serialize cleanly afterwards.
    let out = call(&core, Op::Update, br#"{"NoSuchEventVariant":{}}"#);
    assert!(out.is_empty(), "a rejected event yields no effect requests");

    assert!(
        view(&core)["guidance"].is_array(),
        "view still renders after a bad event"
    );
}

#[test]
fn compact_log_answers_an_error_when_a_line_cannot_be_read() {
    let core = CoreBridge::new();
    let mut unreadable = std::iter::once(Err("unreadable".to_owned()));
    let failed: Value =
        serde_json::from_slice(&core.call(Op::CompactLog.code(), Input::Lines(&mut unreadable)))
            .unwrap();
    assert_eq!(failed["error"]["kind"], "bad_input", "{failed}");
    assert_eq!(failed["error"]["context"], "compact_log", "{failed}");
    let mut none = std::iter::empty::<Result<Cow<'_, str>, String>>();
    let empty: Value =
        serde_json::from_slice(&core.call(Op::CompactLog.code(), Input::Lines(&mut none))).unwrap();
    assert_eq!(empty["dropped"], json!([]));
}

/// A 3 m/s runner for 100 s, as the LogRunTrack wire JSON Save sends.
fn run_track(entry_id: u64) -> Value {
    let points: Vec<Value> = (0..=100)
        .map(|t| {
            json!({
                "lat": 0.0,
                "lon": f64::from(t) * 3.0 / 111_194.926_644_558_7,
                "observed_at": 1_000 + i64::from(t),
                "accuracy_m": 5.0
            })
        })
        .collect();
    json!({
        "LogRunTrack": {
            "points": points,
            "hr_pct_max": 0.0,
            "longest_recent_km": 0.0,
            "observed_at": 1_100,
            "entry_id": entry_id
        }
    })
}

#[test]
fn track_figures_previews_a_log_run_track_and_refuses_other_events() {
    let out = pure(Op::TrackFigures, run_track(1).to_string().as_bytes());
    let km = out["distance_km"].as_f64().unwrap();
    let min = out["duration_min"].as_f64().unwrap();
    assert!((km - 0.3).abs() < 1e-3, "{out}");
    assert!((min * 60.0 - 100.0).abs() < 1e-9, "{out}");

    let other = pure(Op::TrackFigures, br#""ClearRuns""#);
    assert_eq!(other["error"]["kind"], "bad_input", "{other}");
    assert_eq!(other["error"]["context"], "track_figures", "{other}");
}

#[test]
fn export_gpx_reads_the_logged_run_without_a_prior_view() {
    let core = CoreBridge::new();
    assert!(send(&core, &run_track(7)).is_array());
    let gpx = call(
        &core,
        Op::ExportGpx,
        br#"{"entry_id":7,"observed_at":1100}"#,
    );
    let gpx = String::from_utf8(gpx).unwrap();
    assert!(gpx.starts_with("<?xml") && gpx.contains("<trkpt"), "{gpx}");
    assert!(
        call(
            &core,
            Op::ExportGpx,
            br#"{"entry_id":8,"observed_at":1100}"#
        )
        .is_empty()
    );
    let bad: Value = serde_json::from_slice(&call(
        &core,
        Op::ExportGpx,
        br#"{"entry_id":-1,"observed_at":0}"#,
    ))
    .unwrap();
    assert_eq!(bad["error"]["kind"], "bad_input", "{bad}");
    assert_eq!(bad["error"]["context"], "export_gpx", "{bad}");
}

#[test]
fn reset_empties_the_core_as_a_fresh_process_has_it() {
    let core = CoreBridge::new();
    let empty = call(&core, Op::View, b"");
    send(&core, &profile());
    assert_ne!(
        call(&core, Op::View, b""),
        empty,
        "the profile reached the core"
    );
    assert!(call(&core, Op::Reset, b"").is_empty());
    assert_eq!(
        call(&core, Op::View, b""),
        empty,
        "reset leaves the empty core"
    );
}

#[test]
fn the_core_runs_on_the_bridges_thread_whichever_thread_calls() {
    let core = CoreBridge::new();
    let core_thread = || {
        core.on_core(Op::View, |_| {
            Ok(std::thread::current()
                .name()
                .unwrap_or("")
                .as_bytes()
                .to_vec())
        })
        .unwrap()
    };
    let here = core_thread();
    let there = std::thread::scope(|s| s.spawn(core_thread).join().unwrap());
    assert_eq!(here, b"lollop-core");
    assert_eq!(there, here);
}

#[test]
fn an_unknown_op_or_mismatched_input_is_bad_input() {
    let core = CoreBridge::new();
    let unknown: Value =
        serde_json::from_slice(&core.call(u32::MAX, Input::Bytes(Cow::Borrowed(b"")))).unwrap();
    assert_eq!(unknown["error"]["kind"], "bad_input", "{unknown}");
    assert_eq!(unknown["error"]["context"], "call", "{unknown}");
    let mut lines = std::iter::empty::<Result<Cow<'_, str>, String>>();
    let mismatched: Value =
        serde_json::from_slice(&core.call(Op::Update.code(), Input::Lines(&mut lines))).unwrap();
    assert_eq!(mismatched["error"]["context"], "update", "{mismatched}");
}

#[test]
fn op_contexts_are_the_snake_case_names_the_shell_reads() {
    let names: Vec<String> = Op::ALL.iter().map(|op| op.context()).collect();
    assert_eq!(
        names,
        [
            "update",
            "view",
            "parse_run_file",
            "track_figures",
            "export_gpx",
            "compact_log",
            "reset",
            "exercise_keys",
            "warm_up_ramp",
            "draft_facts",
            "run_track",
            "thin_track",
            "imported_hr_pct"
        ]
    );
}

#[test]
fn exercise_keys_answers_each_names_key_in_order() {
    let core = CoreBridge::new();
    let mut names = ["  İNCLİNE Bench ", "Straße", ""]
        .into_iter()
        .map(|n| Ok::<_, String>(Cow::Borrowed(n)));
    let keys: Value =
        serde_json::from_slice(&core.call(Op::ExerciseKeys.code(), Input::Lines(&mut names)))
            .unwrap();
    assert_eq!(keys, json!(["incline bench", "strasse", ""]));
    let mut unreadable = std::iter::once(Err("unreadable".to_owned()));
    let failed: Value =
        serde_json::from_slice(&core.call(Op::ExerciseKeys.code(), Input::Lines(&mut unreadable)))
            .unwrap();
    assert_eq!(failed["error"]["kind"], "bad_input", "{failed}");
    assert_eq!(failed["error"]["context"], "exercise_keys", "{failed}");
}

#[test]
fn warm_up_ramp_answers_the_steps_of_a_barbell_lift() {
    let squat = pure(
        Op::WarmUpRamp,
        br#"{"exercise":"Back squat","work_kg":100.0,"weight_lb":false}"#,
    );
    assert_eq!(
        squat,
        json!([
            {"kg": 20.0, "reps": 5, "bar": true},
            {"kg": 40.0, "reps": 5, "bar": false},
            {"kg": 60.0, "reps": 3, "bar": false},
            {"kg": 80.0, "reps": 2, "bar": false}
        ])
    );
    let curl = pure(
        Op::WarmUpRamp,
        br#"{"exercise":"Dumbbell curl","work_kg":20.0,"weight_lb":false}"#,
    );
    assert_eq!(curl, json!([]));
    let bad = pure(Op::WarmUpRamp, br#"{"exercise":"Back squat"}"#);
    assert_eq!(bad["error"]["kind"], "bad_input", "{bad}");
    assert_eq!(bad["error"]["context"], "warm_up_ramp", "{bad}");
}

#[test]
fn draft_facts_answers_a_drafts_gates_and_answers() {
    let chest = pure(
        Op::DraftFacts,
        br#"{"birth_epoch_day":13938,"today":20512,"answer":{"Parq":{"question":"ChestPain","yes":true}}}"#,
    );
    assert_eq!(chest["age_years"], 17.0, "{chest}");
    assert_eq!(chest["youth"], true, "{chest}");
    assert_eq!(chest["health_defers"], true, "{chest}");
    assert_eq!(chest["parq_yes"], json!(["ChestPain"]), "{chest}");
    assert_eq!(chest["first_symptom"], "ChestPain", "{chest}");
    assert_eq!(chest["health"]["symptomatic"], true, "{chest}");
    assert_eq!(chest["can_hang"], Value::Null, "{chest}");
    let empty = pure(Op::DraftFacts, b"{}");
    assert_eq!(empty["youth"], false, "{empty}");
    assert_eq!(empty["health_defers"], false, "{empty}");
    let bad = pure(Op::DraftFacts, br#"{"today":"monday"}"#);
    assert_eq!(bad["error"]["kind"], "bad_input", "{bad}");
    assert_eq!(bad["error"]["context"], "draft_facts", "{bad}");
}

#[test]
fn thin_track_answers_the_kept_fixes_and_their_segment_starts() {
    // Ten fixes to ~4: stride 3 (0, 3, 6, 9) plus the segment start 5, which
    // is position 2 of the kept fixes. A start at 0 or past the end opens
    // nothing; order and repeats don't matter.
    let thin = pure(
        Op::ThinTrack,
        br#"{"fixes":10,"max_points":4,"segment_starts":[12,5,0,5]}"#,
    );
    assert_eq!(
        thin,
        json!({"kept": [0, 3, 5, 6, 9], "segment_starts": [2]})
    );
    let whole = pure(Op::ThinTrack, br#"{"fixes":3,"max_points":5400}"#);
    assert_eq!(whole, json!({"kept": [0, 1, 2], "segment_starts": []}));
    let bad = pure(Op::ThinTrack, br#"{"fixes":-1,"max_points":4}"#);
    assert_eq!(bad["error"]["kind"], "bad_input", "{bad}");
    assert_eq!(bad["error"]["context"], "thin_track", "{bad}");
}

#[test]
fn imported_hr_pct_answers_the_share_of_a_measured_hr_max() {
    let hr = pure(
        Op::ImportedHrPct,
        br#"{"avg_hr_bpm":152.0,"measured_hr_max":190.0}"#,
    );
    assert!((hr.as_f64().unwrap() - 80.0).abs() < 1e-9, "{hr}");
    assert_eq!(
        pure(Op::ImportedHrPct, br#"{"avg_hr_bpm":152.0}"#),
        json!(0.0)
    );
    let bad = pure(Op::ImportedHrPct, br#"{"avg_hr_bpm":"fast"}"#);
    assert_eq!(bad["error"]["kind"], "bad_input", "{bad}");
    assert_eq!(bad["error"]["context"], "imported_hr_pct", "{bad}");
}
