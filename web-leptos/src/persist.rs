//! Shell-side persistence: the core stays pure, so the shell records the event
//! stream in `localStorage` and replays it on startup. Works identically under
//! the Android WebView (DOM storage enabled).

use shared::Event;

const KEY: &str = "fitness_anlage_events";

pub fn load() -> Vec<Event> {
    let Some(store) = storage() else {
        return Vec::new();
    };
    match store.get_item(KEY) {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn save(events: &[Event]) {
    let Some(store) = storage() else {
        return;
    };
    if let Ok(json) = serde_json::to_string(events) {
        let _ = store.set_item(KEY, &json);
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
