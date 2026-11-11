//! The shell's event lines (`fixtures/events/shell_encoder.ndjson`, the bytes
//! the Kotlin encoder persists) against the core's `Event`: every line parses,
//! every variant is covered, and no key the shell writes is one the core
//! drops.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use shared::Event;
use shared::wire::{Body, Registry, Wire};

fn corpus() -> Vec<String> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/events/shell_encoder.ndjson");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("unreadable {}: {e}", path.display()))
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Every key path in `value` (`a.b` for nested objects; list items share
/// their list's path).
fn key_paths(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.insert(path.clone());
                key_paths(v, &path, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| key_paths(v, prefix, out)),
        _ => {}
    }
}

fn tag(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(map) => map.keys().next().expect("a tagged event").clone(),
        other => panic!("not an event: {other}"),
    }
}

#[test]
fn every_shell_line_parses_without_dropping_a_key() {
    for line in corpus() {
        let event: Event = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("core rejects shell line {line:.160}: {e}"));
        let again: Event = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(again, event, "{line:.160}");
        let written: Value = serde_json::from_str(&line).unwrap();
        let mut shell_keys = BTreeSet::new();
        key_paths(&written, "", &mut shell_keys);
        let mut core_keys = BTreeSet::new();
        key_paths(&serde_json::to_value(&event).unwrap(), "", &mut core_keys);
        let dropped: Vec<_> = shell_keys.difference(&core_keys).collect();
        assert!(dropped.is_empty(), "core drops {dropped:?} of {line:.160}");
    }
}

#[test]
fn the_corpus_covers_every_event_variant() {
    let mut reg = Registry::default();
    Event::shape(&mut reg);
    let Some(Body::Enum { variants, .. }) = reg.get("Event").map(|t| &t.body) else {
        panic!("Event is an enum");
    };
    let all: BTreeSet<String> = variants.iter().map(|v| v.name.to_owned()).collect();
    let seen: BTreeSet<String> = corpus()
        .iter()
        .map(|l| tag(&serde_json::from_str(l).unwrap()))
        .collect();
    let missing: Vec<_> = all.difference(&seen).collect();
    assert!(missing.is_empty(), "no shell line for {missing:?}");
}
