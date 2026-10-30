//! Compaction golden corpus. Each `<case>.in.ndjson` is a raw event log in
//! serde wire form; the matching `<case>.expected.ndjson` is the
//! replay-equivalent survivor set. Both entry points are graded: the decoded
//! [`compact_event_log`] and the raw-line [`compact_lines`] the Android shell
//! calls over JNI.

use std::fs;
use std::path::Path;

use crux_core::App;
use shared::log::{LineFacts, compact_lines};
use shared::{Engine, Event, compact_event_log};

mod common;

/// The view JSON a shell renders after replaying `events` from an empty model.
fn replayed_view(events: &[Event]) -> String {
    serde_json::to_string(&Engine.view(&common::replay(events.iter().cloned())))
        .expect("view serializes")
}

#[test]
fn compaction_golden_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compaction");
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let mut cases: Vec<String> = names
        .iter()
        .filter_map(|n| n.strip_suffix(".in.ndjson").map(str::to_owned))
        .collect();
    cases.sort();
    // Every other file must be the expected half of a case; an orphan
    // `.expected.ndjson` (or stray file) would otherwise grade nothing.
    let orphans: Vec<&String> = names
        .iter()
        .filter(|n| {
            !n.ends_with(".in.ndjson")
                && n.strip_suffix(".expected.ndjson")
                    .is_none_or(|case| !cases.iter().any(|c| c == case))
        })
        .collect();
    assert!(
        orphans.is_empty(),
        "files without a matching .in.ndjson in {}: {orphans:?}",
        dir.display()
    );
    assert!(
        !cases.is_empty(),
        "no compaction fixtures found in {}",
        dir.display()
    );
    for case in &cases {
        let in_path = dir.join(format!("{case}.in.ndjson"));
        let expected_path = dir.join(format!("{case}.expected.ndjson"));
        let input = common::events(&in_path);
        let expected = common::events(&expected_path);
        assert_eq!(
            compact_event_log(input.clone()),
            expected,
            "compacting {} diverged from {}",
            in_path.display(),
            expected_path.display(),
        );
        let raw = fs::read_to_string(&in_path).expect("fixture reads");
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        let verdict = compact_lines(
            &lines
                .iter()
                .map(|l| LineFacts::parse(l))
                .collect::<Vec<_>>(),
        );
        let kept: Vec<Event> = (0..lines.len())
            .filter(|i| !verdict.dropped.contains(i))
            .map(|i| serde_json::from_str(lines[i]).expect("fixture line decodes"))
            .collect();
        assert_eq!(
            kept,
            expected,
            "raw-line compaction of {} diverged",
            in_path.display()
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
