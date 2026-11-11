//! Writes the of-record figures of the newest tracked run in an event log, for
//! `scripts/soak-emulator.sh` and `scripts/emulator-scenarios.sh` to compare
//! the app's Finish sheet and History row with what the core derives from the
//! saved fixes:
//!
//! ```sh
//! cargo run -q -p shared --example track_figures -- event-log.ndjson core.json
//! ```

use std::path::Path;

#[path = "../../tests/common/mod.rs"]
mod common;
mod figures;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [log, out] = args.as_slice() else {
        eprintln!("usage: track_figures EVENT_LOG OUT_JSON");
        std::process::exit(2);
    };
    let f = figures::figures(Path::new(log));
    std::fs::write(out, serde_json::to_string_pretty(&f).unwrap()).unwrap();
}
