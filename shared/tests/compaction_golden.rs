//! Compaction golden corpus. Each `<case>.in.ndjson` is a raw event log in
//! serde wire form; the matching `<case>.expected.ndjson` is the
//! replay-equivalent survivor set of [`compact_lines`], the compaction the
//! shells run over the bridge.

use std::fs;
use std::path::Path;

use shared::Event;
use shared::log::{LineFacts, compact_lines};

mod common;

#[test]
fn compaction_golden_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compaction");
    let cases = common::cases(&dir, ".in.ndjson");
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
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
    for case in &cases {
        let in_path = dir.join(format!("{case}.in.ndjson"));
        let expected_path = dir.join(format!("{case}.expected.ndjson"));
        let input = common::events(&in_path);
        let expected = common::events(&expected_path);
        let lines = common::lines(&in_path);
        let verdict = compact_lines(
            &lines
                .iter()
                .map(|l| LineFacts::parse(l))
                .collect::<Vec<_>>(),
        );
        let kept: Vec<Event> = (0..lines.len())
            .filter(|i| !verdict.dropped.contains(i))
            .map(|i| serde_json::from_str(&lines[i]).expect("fixture line decodes"))
            .collect();
        assert_eq!(
            kept,
            expected,
            "raw-line compaction of {} diverged",
            in_path.display()
        );
        assert!(
            common::view_json(input) == common::view_json(expected.iter().cloned()),
            "{} does not replay to the same view as {}",
            expected_path.display(),
            in_path.display(),
        );
    }
}
