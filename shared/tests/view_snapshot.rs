//! ViewModel wire-identity corpus. Each `<case>.events.ndjson` is an event log
//! in serde wire form; replaying it through the engine must serialize to the
//! byte-identical `<case>.view.json`. Pins the JSON the Kotlin shell decodes
//! across internal refactors of the view path. Re-bless deliberately with
//! `BLESS_VIEW_SNAPSHOT=1 cargo test -p shared --test view_snapshot`: it
//! rewrites diverged snapshots, deletes orphan files, and still fails if it
//! changed anything, so rerun without it to confirm.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crux_core::App;
use shared::{Engine, Event, Model};

fn replay(ndjson: &str) -> Model {
    let app = Engine;
    let mut model = Model::default();
    for line in ndjson.lines().filter(|l| !l.trim().is_empty()) {
        let event: Event = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("unparseable fixture line {line:?}: {e}"));
        let _ = app.update(event, &mut model);
    }
    model
}

#[test]
fn view_json_matches_snapshot() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/view_snapshot");
    let mut cases: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".events.ndjson").map(str::to_owned))
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no view fixtures in {}", dir.display());
    let bless = std::env::var_os("BLESS_VIEW_SNAPSHOT").is_some();
    let mut stale = Vec::new();
    let mut wanted = BTreeSet::new();
    for case in &cases {
        let events_name = format!("{case}.events.ndjson");
        let events = fs::read_to_string(dir.join(&events_name)).unwrap();
        let model = replay(&events);
        let mut json = serde_json::to_string_pretty(&Engine.view(&model)).unwrap();
        json.push('\n');
        let name = format!("{case}.view.json");
        let path = dir.join(&name);
        match fs::read_to_string(&path) {
            Ok(expected) if expected == json => {}
            _ if bless => {
                fs::write(&path, &json).unwrap();
                stale.push(format!("{name} (rewritten)"));
            }
            Ok(expected) => {
                let line = json
                    .lines()
                    .zip(expected.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| json.lines().count().min(expected.lines().count()));
                stale.push(format!(
                    "{name} diverged at line {}:\n  got:      {:?}\n  expected: {:?}",
                    line + 1,
                    json.lines().nth(line),
                    expected.lines().nth(line),
                ));
            }
            Err(e) => stale.push(format!("{name} missing: {e}")),
        }
        wanted.insert(events_name);
        wanted.insert(name);
    }
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !wanted.contains(&name) {
            if bless {
                fs::remove_file(&path).unwrap();
                stale.push(format!("{name} (orphan, deleted)"));
            } else {
                stale.push(format!("{name} (orphan: no matching .events.ndjson)"));
            }
        }
    }
    let hint = if bless {
        "blessed; rerun without BLESS_VIEW_SNAPSHOT to confirm"
    } else {
        "stale; re-bless with BLESS_VIEW_SNAPSHOT=1"
    };
    assert!(stale.is_empty(), "view snapshot corpus {hint}: {stale:#?}");
}
