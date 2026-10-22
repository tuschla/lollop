//! Compaction golden corpus, shared with the Kotlin shell. Each
//! `<case>.in.ndjson` is a raw event log in serde wire form; the matching
//! `<case>.expected.ndjson` is the replay-equivalent survivor set. The Android
//! `EventLog.compact` reads the identical directory (see
//! `EventLogTest.compactionGoldenFixtures`), so both implementations are graded
//! against one corpus and cannot drift apart silently.

use std::fs;
use std::path::Path;

use crux_core::App;
use shared::{Engine, Event, Model, compact_event_log};

/// The events of one fixture file; a failure names the file.
fn parse(path: &Path) -> Vec<Event> {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("unreadable fixture {}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("unparseable line in {}: {l:?}: {e}", path.display()))
        })
        .collect()
}

/// The view JSON a shell renders after replaying `events` from an empty model.
fn replayed_view(events: &[Event]) -> String {
    let app = Engine;
    let mut model = Model::default();
    for ev in events {
        let _ = app.update(ev.clone(), &mut model);
    }
    serde_json::to_string(&app.view(&model)).expect("view serializes")
}

#[test]
fn compaction_golden_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compaction");
    let mut cases: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".in.ndjson").map(str::to_owned))
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no compaction fixtures found in {}",
        dir.display()
    );
    for case in &cases {
        let in_path = dir.join(format!("{case}.in.ndjson"));
        let expected_path = dir.join(format!("{case}.expected.ndjson"));
        let input = parse(&in_path);
        let expected = parse(&expected_path);
        assert_eq!(
            compact_event_log(input.clone()),
            expected,
            "compacting {} diverged from {}",
            in_path.display(),
            expected_path.display(),
        );
        // The survivor set must render exactly what the full log renders.
        assert!(
            replayed_view(&input) == replayed_view(&expected),
            "{} does not replay to the same view as {}",
            expected_path.display(),
            in_path.display(),
        );
    }
}
