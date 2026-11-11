//! The of-record figures of a saved GPS run, from the core itself: distance,
//! moving time and the time its km and mile splits account for (the splits of
//! a profile set to each unit). Shared by the `track_figures` example and the
//! `track_figures_of_record` test (both declare `mod common` from
//! `tests/common`).

use std::path::Path;

use crux_core::App;
use serde_json::{Value, json};
use shared::profile::DistanceUnit;
use shared::running::MILE_M;
use shared::{Engine, Event};

use crate::common;

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

/// The newest tracked run's view after replaying `events`.
fn newest_tracked_run(events: Vec<Event>) -> Value {
    let model = common::replay(events);
    let vm = serde_json::to_value(Engine.view(&model)).unwrap();
    vm["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["has_gpx"].as_bool() == Some(true))
        .max_by_key(|r| r["observed_at"].as_i64())
        .cloned()
        .expect("no tracked run")
}

/// `events` with the profile set to `unit`: the log's last profile edit
/// repeated with the unit changed, else a profile that sets only the unit.
fn in_unit(mut events: Vec<Event>, unit: DistanceUnit) -> Vec<Event> {
    let mut profile = events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::SetProfile(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            serde_json::from_value(json!({
                "progression_cadence": "EverySession",
                "lift_goal": "MaxStrength",
                "goal_distance": "General",
                "concurrent_goal": "Strength",
                "weekly_sets": 0,
                "running_days_per_week": 0,
                "running_km_per_week": 0.0,
                "advanced": false,
                "high_load_block": false,
            }))
            .expect("a profile that sets only the unit")
        });
    profile.distance_unit = unit;
    events.push(Event::SetProfile(profile));
    events
}

/// `{"entry_id", "distance_m", "moving_s", "split_clock_s", "split_clock_mi_s"}`
/// of the newest tracked run after replaying the log at `path`.
pub fn figures(path: &Path) -> Value {
    let events = common::events(path);
    let run = newest_tracked_run(events.clone());
    let run_km = newest_tracked_run(in_unit(events.clone(), DistanceUnit::Km));
    let run_mi = newest_tracked_run(in_unit(events, DistanceUnit::Mi));
    json!({
        "entry_id": run["entry_id"],
        "distance_m": run["distance_km"].as_f64().unwrap() * 1000.0,
        "moving_s": run["duration_min"].as_f64().unwrap() * 60.0,
        "split_clock_s": split_clock_s(run_km["splits"].as_array().unwrap(), 1.0),
        "split_clock_mi_s": split_clock_s(run_mi["splits"].as_array().unwrap(), MILE_M / 1000.0),
    })
}
