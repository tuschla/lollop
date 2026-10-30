//! The of-record figures of a saved GPS run, from the core itself: distance,
//! moving time and the time its km splits account for.
//!
//! `scripts/soak-emulator.sh` runs this on the event log it pulls from the
//! device, to compare the Finish sheet and the History row with what the core
//! derives from the saved fixes:
//!
//! ```sh
//! TRACK_FIGURES_LOG=event-log.ndjson TRACK_FIGURES_OUT=core.json \
//!   cargo test -p shared --test track_figures_of_record
//! ```
//!
//! writes `{"entry_id", "distance_m", "moving_s", "split_clock_s",
//! "split_clock_mi_s"}` of the newest run with a track. Without the variables
//! it checks the figures of
//! `fixtures/device_tracks/soak-e37-20min.events.ndjson`, a run the soak
//! recorded on the API 37 emulator (fed route 2858.4 m, 946 s moving; screen
//! off, backgrounded, a light, an in-app and a notification pause, Doze).

use std::path::Path;

use crux_core::App;
use serde_json::{Value, json};
use shared::Engine;

mod common;

/// Seconds the split list accounts for: each split's seconds per unit times
/// the share of a unit it covers (full splits 1, the partial one less).
fn split_clock_s(splits: &[Value], unit_km: f64) -> f64 {
    let mut prev_km = 0.0;
    let mut sum = 0.0;
    for s in splits {
        let km = s["distance_km"].as_f64().unwrap();
        sum += s["seconds_per_unit"].as_f64().unwrap() * (km - prev_km) / unit_km;
        prev_km = km;
    }
    sum
}

/// The newest tracked run's figures after replaying the log at `path`.
fn figures(path: &Path) -> Value {
    let model = common::replay(common::events(path));
    let vm = serde_json::to_value(Engine.view(&model)).unwrap();
    let run = vm["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["has_gpx"].as_bool() == Some(true))
        .max_by_key(|r| r["observed_at"].as_i64())
        .unwrap_or_else(|| panic!("no tracked run in {}", path.display()));
    json!({
        "entry_id": run["entry_id"],
        "distance_m": run["distance_km"].as_f64().unwrap() * 1000.0,
        "moving_s": run["duration_min"].as_f64().unwrap() * 60.0,
        "split_clock_s": split_clock_s(run["splits_km"].as_array().unwrap(), 1.0),
        "split_clock_mi_s": split_clock_s(run["splits_mi"].as_array().unwrap(), 1.609_344),
    })
}

#[test]
fn of_record_figures_of_a_tracked_run() {
    if let Some(log) = std::env::var_os("TRACK_FIGURES_LOG") {
        let f = figures(Path::new(&log));
        let out = std::env::var_os("TRACK_FIGURES_OUT")
            .expect("TRACK_FIGURES_OUT with TRACK_FIGURES_LOG");
        std::fs::write(&out, serde_json::to_string_pretty(&f).unwrap()).unwrap();
        return;
    }
    let f = figures(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/device_tracks/soak-e37-20min.events.ndjson"),
    );
    let (dist, moving) = (
        f["distance_m"].as_f64().unwrap(),
        f["moving_s"].as_f64().unwrap(),
    );
    for key in ["split_clock_s", "split_clock_mi_s"] {
        let clock = f[key].as_f64().unwrap();
        assert!(
            (clock - moving).abs() <= 2.0,
            "{key} {clock:.1} s vs moving time {moving:.1} s"
        );
    }
    assert!(
        (dist - 2858.4).abs() <= 0.03 * 2858.4,
        "distance {dist:.1} m vs fed route 2858.4 m"
    );
    assert!(
        (moving - 946.0).abs() <= 45.0,
        "moving time {moving:.1} s vs fed route 946 s"
    );
}
