//! Helpers shared by the integration test crates (`mod common;`).

use std::path::Path;

use crux_core::App;
use serde_json::Value;
use shared::{Engine, Event, Model};

/// The model after replaying `events` into an empty one, as a shell's boot does.
pub fn replay(events: impl IntoIterator<Item = Event>) -> Model {
    let app = Engine;
    let mut model = Model::default();
    for e in events {
        let _ = app.update(e, &mut model);
    }
    model
}

/// The non-blank lines of `path`; a failure names the file.
#[allow(dead_code)]
pub fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("unreadable {}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// The events of an NDJSON event log ([`lines`]); a failure names the file
/// and the line.
#[allow(dead_code)]
pub fn events(path: &Path) -> Vec<Event> {
    lines(path)
        .iter()
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("unparseable line in {}: {l:.120}: {e}", path.display()))
        })
        .collect()
}

/// An event line's variant tag: a unit variant's string, a struct variant's
/// one key.
#[allow(dead_code)]
pub fn tag(event: &Value) -> String {
    match event {
        Value::String(s) => s.clone(),
        Value::Object(map) => map.keys().next().expect("a tagged event").clone(),
        other => panic!("not an event: {other}"),
    }
}

/// The case names in `dir`: each file name ending in `suffix`, without it,
/// sorted; at least one.
#[allow(dead_code)]
pub fn cases(dir: &Path, suffix: &str) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("missing fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(suffix).map(str::to_owned))
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "no *{suffix} fixtures in {}",
        dir.display()
    );
    out
}

/// The view a shell renders after replaying `events` from an empty model, as
/// JSON.
#[allow(dead_code)]
pub fn view_json(events: impl IntoIterator<Item = Event>) -> Value {
    serde_json::to_value(Engine.view(&replay(events))).expect("view serializes")
}
