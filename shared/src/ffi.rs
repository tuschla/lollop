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

type JsonBridge = Bridge<Engine, JsonFfiFormat>;

fn bridge() -> &'static Mutex<JsonBridge> {
    static BRIDGE: OnceLock<Mutex<JsonBridge>> = OnceLock::new();
    BRIDGE.get_or_init(|| Mutex::new(Bridge::new(Core::default())))
}

/// Process a JSON-encoded event, returning the JSON effect-request array.
fn process_event(event: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    bridge()
        .lock()
        .expect("ffi bridge poisoned")
        .update(event, &mut out)
        .expect("event should deserialize + process");
    out
}

/// Serialize the current view model as JSON.
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
        assert!(requests.as_array().is_some(), "expected effect request array");

        let view = current_view();
        let view: serde_json::Value = serde_json::from_slice(&view).unwrap();
        let guidance = view["guidance"].as_array().expect("guidance array");
        assert!(!guidance.is_empty(), "profile should yield guidance rows");
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
