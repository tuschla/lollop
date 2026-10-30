//! Helpers shared by the integration test crates (`mod common;`).

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
