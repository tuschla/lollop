//! The of-record figures (`examples/track_figures/figures.rs`) of
//! `fixtures/device_tracks/soak-e37-20min.events.ndjson`, a run the soak
//! recorded on the API 37 emulator (fed route 2858.4 m, 946 s moving; screen
//! off, backgrounded, a light, an in-app and a notification pause, Doze).

use std::path::Path;

mod common;
#[path = "../examples/track_figures/figures.rs"]
mod figures;

#[test]
fn of_record_figures_of_a_tracked_run() {
    let f = figures::figures(
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
