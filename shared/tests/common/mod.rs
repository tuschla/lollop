//! Helpers shared by the integration test crates (`mod common;`).

use std::path::Path;

use crux_core::App;
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

/// The events of an NDJSON event log, blank lines skipped; a failure names
/// the file and the line.
// `upgrade_logs` parses its own lines: it counts the ones this core rejects.
#[allow(dead_code)]
pub fn events(path: &Path) -> Vec<Event> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("unreadable event log {}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("unparseable line in {}: {l:.120}: {e}", path.display()))
        })
        .collect()
}
