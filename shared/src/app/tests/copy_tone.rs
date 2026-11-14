//! Tone lint over every string the core renders (the `copy_invariants` view
//! sweep; references verbatim and dormant fields skipped), enforcing
//! `planning/design/writing.md`: neutral and factual, like a tool. The rules
//! are objective markers of that voice: exclamation marks, emojis,
//! "we/us/our", a hype and AI-vocabulary word list, rhetorical question
//! openers ("First time? Pick…"), stock phrases ("Let's", "Ready to", "It's
//! important to note", "serves as", "plays a role", "I hope this helps"),
//! placeholder text ("[insert", "TODO"), "not just X but Y" parallels and em
//! dashes in sentences (a lone `—` no-value placeholder is fine). The word
//! lists and the lint's own cases are `tests/fixtures/copy_tone.json`, which
//! the Android twin `CopyToneTest.kt` reads too.

use std::sync::LazyLock;

use serde::Deserialize;

use super::copy_invariants::{assert_family, has_word, is_word_at};

/// A phrase rule: its label and the lowercase forms that count.
#[derive(Deserialize)]
struct Phrase {
    label: String,
    forms: Vec<String>,
}

/// writing.md Terms: the word not to use and the app's word.
#[derive(Deserialize)]
struct Term {
    term: String,
    say: String,
}

#[derive(Deserialize)]
struct Flagged {
    text: String,
    rule: String,
}

/// A negative parallel: a `first` phrase with a `then` word later in the same
/// sentence.
#[derive(Deserialize)]
struct Parallel {
    label: String,
    first: Vec<String>,
    then: Vec<String>,
}

/// `tests/fixtures/copy_tone.json`.
#[derive(Deserialize)]
struct ToneRules {
    hype: Vec<Phrase>,
    stock_phrases: Vec<Phrase>,
    placeholders: Vec<String>,
    parallels: Vec<Parallel>,
    terms: Vec<Term>,
    clean: Vec<String>,
    flagged: Vec<Flagged>,
}

static RULES: LazyLock<ToneRules> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../tests/fixtures/copy_tone.json"))
        .expect("copy_tone.json parses")
});

/// Pictographic emoji blocks plus the emoji joiner and presentation selector.
/// Arrows (`→`, `↔`) and math symbols are typography, not emoji.
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF
            | 0x2600..=0x27BF
            | 0x2B05..=0x2B55
            | 0x231A..=0x231B
            | 0x23E9..=0x23FA
            | 0xFE0F
            | 0x200D
    )
}

/// A whole-word phrase in already-normalized (lowercase, straight-apostrophe)
/// text.
fn has_phrase(n: &str, p: &str) -> bool {
    n.match_indices(p).any(|(i, _)| is_word_at(n, i, p))
}

/// Whether normalized `n` has a `first` phrase of `p` followed by one of its
/// `then` words before the sentence ends (`. `, a newline or the end).
fn has_parallel(n: &str, p: &Parallel) -> bool {
    p.first.iter().any(|f| {
        n.match_indices(f.as_str()).any(|(i, _)| {
            if !is_word_at(n, i, f) {
                return false;
            }
            let rest = &n[i + f.len()..];
            let end = [rest.find(". "), rest.find('\n')]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(rest.len());
            p.then.iter().any(|t| has_phrase(&rest[..end], t))
        })
    })
}

/// A sentence (string start, after `. ` or a newline) that opens with a short
/// question and goes on: `^[A-Z][^?]{0,40}\? ` plus more text. A lone
/// question (a field label) is fine.
fn question_opener(s: &str) -> bool {
    let mut starts = std::iter::once(0)
        .chain(s.match_indices(". ").map(|(i, _)| i + 2))
        .chain(s.match_indices('\n').map(|(i, _)| i + 1));
    starts.any(|st| {
        let rest = &s[st..];
        if !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
            return false;
        }
        let Some(q) = rest.find('?') else {
            return false;
        };
        let after = &rest[q + 1..];
        rest[..q].chars().count() <= 41 && after.starts_with(' ') && !after.trim().is_empty()
    })
}

/// Every tone rule `s` breaks, as `tone: <rule>` names.
fn tone_violations(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let straight = s.replace('\u{2019}', "'");
    let n = straight.to_lowercase();
    if s.contains('!') {
        v.push("tone: exclamation mark".into());
    }
    if s.chars().any(is_emoji) {
        v.push("tone: emoji".into());
    }
    for w in ["we", "our"] {
        if has_phrase(&n, w) {
            v.push(format!("tone: first-person plural `{w}`"));
        }
    }
    // Lowercase or sentence-case only: `US` is the country.
    if has_word(s, "us") || has_word(s, "Us") {
        v.push("tone: first-person plural `us`".into());
    }
    for p in &RULES.hype {
        if p.forms.iter().any(|f| has_phrase(&n, f)) {
            v.push(format!("tone: hype `{}`", p.label));
        }
    }
    if question_opener(s) {
        v.push("tone: rhetorical question opener".into());
    }
    for p in &RULES.stock_phrases {
        if p.forms.iter().any(|f| has_phrase(&n, f)) {
            v.push(format!("tone: stock phrase `{}`", p.label));
        }
    }
    for p in &RULES.placeholders {
        if has_phrase(&n, p) {
            v.push(format!("tone: placeholder `{p}`"));
        }
    }
    for p in &RULES.parallels {
        if has_parallel(&n, p) {
            v.push(format!("tone: parallel `{}`", p.label));
        }
    }
    if has_word(&straight, "Ready to") {
        v.push("tone: stock phrase `Ready to`".into());
    }
    if s.contains('—') && s.trim() != "—" {
        v.push("tone: em dash in a sentence".into());
    }
    for t in &RULES.terms {
        if has_phrase(&n, &t.term) {
            v.push(format!("tone: term `{}` (say {})", t.term, t.say));
        }
    }
    v
}

/// `s` with digit runs as `#`, so one template rendered with different
/// numbers reports once.
fn template(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The sweep's tone hits for the string `s`: each broken rule with the
/// string's template (so every distinct offending line is listed).
pub(super) fn tone_hits(s: &str) -> Vec<String> {
    tone_violations(s)
        .into_iter()
        .map(|rule| format!("{rule} · `{}`", template(s)))
        .collect()
}

#[test]
fn the_tone_lint_flags_each_rule_and_passes_clean_copy() {
    for clean in &RULES.clean {
        let v = tone_violations(clean);
        assert!(v.is_empty(), "`{clean}` must pass: {v:?}");
    }
    for Flagged { text, rule } in &RULES.flagged {
        let v = tone_violations(text);
        assert!(
            v.iter().any(|r| r.contains(rule.as_str())),
            "`{text}` must flag {rule}: {v:?}"
        );
    }
    // Every hit names its distinct line, numbers folded.
    assert_eq!(
        tone_hits("Ran 5 km — 25:10!"),
        [
            "tone: exclamation mark · `Ran # km — #:#!`",
            "tone: em dash in a sentence · `Ran # km — #:#!`",
        ]
    );
}

#[test]
fn copy_reads_neutral() {
    assert_family(&["tone:"]);
}
