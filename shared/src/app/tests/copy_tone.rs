//! Tone lint over every string the core renders (the `copy_invariants` view
//! sweep; references verbatim and dormant fields skipped), enforcing
//! `planning/design/writing.md`: neutral and factual, like a tool. The rules
//! are objective markers of that voice: exclamation
//! marks, emojis, "we/us/our", a hype/AI word list, rhetorical question
//! openers ("First time? Pick…"), stock phrases ("Let's", "Ready to", "It's
//! important to note") and em dashes in sentences (a lone `—` no-value
//! placeholder is fine). The Android twin is `CopyToneTest.kt`; keep the two
//! rule sets in step.
//!
//! A real exception goes in `ALLOW` with its reason; an entry nothing matches
//! any more fails `tone_allowlist_is_reasoned_and_live`.

use super::copy_invariants::{all_hits, assert_family, has_word, is_word_at};

/// A sanctioned exception: `rule` (a `tone:` rule prefix) on a string at
/// `path` (`""`: any path) containing `text`.
struct Allow {
    rule: &'static str,
    path: &'static str,
    text: &'static str,
    reason: &'static str,
}

const ALLOW: &[Allow] = &[];

/// Hype and AI-voice words: (label, the forms that count). "elevated" is not
/// listed: "elevated resting heart rate" is the clinical term.
const HYPE: &[(&str, &[&str])] = &[
    ("unlock", &["unlock", "unlocks", "unlocked", "unlocking"]),
    ("crush", &["crush", "crushes", "crushed", "crushing"]),
    ("journey", &["journey", "journeys"]),
    (
        "level up",
        &[
            "level up",
            "levels up",
            "leveled up",
            "leveling up",
            "level-up",
        ],
    ),
    ("boost", &["boost", "boosts", "boosted", "boosting"]),
    (
        "supercharge",
        &[
            "supercharge",
            "supercharges",
            "supercharged",
            "supercharging",
        ],
    ),
    ("seamless", &["seamless", "seamlessly"]),
    ("effortless", &["effortless", "effortlessly"]),
    (
        "empower",
        &[
            "empower",
            "empowers",
            "empowered",
            "empowering",
            "empowerment",
        ],
    ),
    ("elevate", &["elevate", "elevates", "elevating"]),
    ("delve", &["delve", "delves", "delved", "delving"]),
    ("robust", &["robust", "robustly"]),
    ("vibrant", &["vibrant"]),
    ("tapestry", &["tapestry"]),
    ("testament", &["testament"]),
    (
        "game-changer",
        &["game-changer", "game changer", "game-changing"],
    ),
    ("insights", &["insight", "insights"]),
    (
        "personalized",
        &[
            "personalized",
            "personalised",
            "personalize",
            "personalization",
        ],
    ),
    ("tailored", &["tailor", "tailors", "tailored", "tailoring"]),
    ("Welcome back", &["welcome back"]),
    ("Great job", &["great job"]),
    ("Nice work", &["nice work"]),
    ("Well done", &["well done"]),
    ("Well paced", &["well paced", "well-paced"]),
    ("You've got this", &["you've got this"]),
    ("Keep it up", &["keep it up"]),
    ("Don't worry", &["don't worry"]),
    ("No pressure", &["no pressure"]),
    ("simply", &["simply"]),
    ("when you're ready", &["when you're ready"]),
    (
        "studies show",
        &["studies show", "studies suggest", "studies have shown"],
    ),
    (
        "research shows",
        &[
            "research shows",
            "research backs",
            "research suggests",
            "research has shown",
        ],
    ),
    (
        "experts say",
        &["experts say", "experts agree", "experts recommend"],
    ),
];

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
    for (label, forms) in HYPE {
        if forms.iter().any(|f| has_phrase(&n, f)) {
            v.push(format!("tone: hype `{label}`"));
        }
    }
    if question_opener(s) {
        v.push("tone: rhetorical question opener".into());
    }
    if has_phrase(&n, "let's") {
        v.push("tone: stock phrase `Let's`".into());
    }
    if has_word(&straight, "Ready to") {
        v.push("tone: stock phrase `Ready to`".into());
    }
    if has_phrase(&n, "it's important to note") || has_phrase(&n, "it is important to note") {
        v.push("tone: stock phrase `It's important to note`".into());
    }
    if s.contains('—') && s.trim() != "—" {
        v.push("tone: em dash in a sentence".into());
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

/// The sweep's tone hits for the string `s` at `path`: each broken rule with
/// the string's template (so every distinct offending line is listed), or
/// `tone-allowed: <ALLOW index>` when an allowlist entry covers it.
pub(super) fn tone_hits(path: &str, s: &str) -> Vec<String> {
    tone_violations(s)
        .into_iter()
        .map(|rule| {
            let allowed = ALLOW.iter().position(|a| {
                rule.starts_with(a.rule)
                    && (a.path.is_empty() || a.path == path)
                    && s.contains(a.text)
            });
            match allowed {
                Some(i) => format!("tone-allowed: {i}"),
                None => format!("{rule} · `{}`", template(s)),
            }
        })
        .collect()
}

// ── Self-check: each rule fires on a sample and passes clean copy ───────────

#[test]
fn the_tone_lint_flags_each_rule_and_passes_clean_copy() {
    for clean in [
        "Squat 5×3 @ 85 kg · ~2 reps left",
        "Held: resting heart rate 8 bpm above your usual. Easy run instead.",
        "—",
        "Feeling ill?",
        "Pace 4:50–5:05 /km → 150–162 bpm",
        "US spelling; elevated resting heart rate; Expert opinion",
        "Lighter week: the plan keeps 3 sessions. Log them by hand if needed.",
    ] {
        let v = tone_violations(clean);
        assert!(v.is_empty(), "`{clean}` must pass: {v:?}");
    }
    for (s, rule) in [
        ("Nice run!", "exclamation"),
        ("Done 💪", "emoji"),
        ("Rest ☀", "emoji"),
        ("We hold this day", "`we`"),
        ("We’ll lower the weight", "`we`"),
        ("Tell us how it felt", "`us`"),
        ("Our plan adapts", "`our`"),
        ("This unlocks the next block", "`unlock`"),
        ("Level up your squat", "`level up`"),
        ("Personalised targets", "`personalized`"),
        ("A plan tailored to you", "`tailored`"),
        ("Welcome back, Sam", "`Welcome back`"),
        ("Well-paced run", "`Well paced`"),
        ("Don’t worry about a missed day", "`Don't worry`"),
        ("Simply log a set", "`simply`"),
        ("Research backs this", "`research shows`"),
        ("Studies show it works", "`studies show`"),
        ("Experts say rest", "`experts say`"),
        ("Start when you're ready", "`when you're ready`"),
        ("First time? Pick a plan", "question opener"),
        ("Done. Feeling ill? Hold today", "question opener"),
        ("Let's go", "`Let's`"),
        ("Ready to train", "`Ready to`"),
        ("It's important to note the cap", "`It's important to note`"),
        ("Easy run — keep it short", "em dash"),
    ] {
        let v = tone_violations(s);
        assert!(
            v.iter().any(|r| r.contains(rule)),
            "`{s}` must flag {rule}: {v:?}"
        );
    }
    // Every hit names its distinct line, numbers folded.
    assert_eq!(
        tone_hits("x", "Ran 5 km — 25:10!"),
        [
            "tone: exclamation mark · `Ran # km — #:#!`",
            "tone: em dash in a sentence · `Ran # km — #:#!`",
        ]
    );
}

#[test]
fn tone_allowlist_is_reasoned_and_live() {
    let hits = all_hits();
    for (i, a) in ALLOW.iter().enumerate() {
        assert!(a.rule.starts_with("tone: ") && !a.reason.trim().is_empty() && !a.text.is_empty());
        let key = format!("tone-allowed: {i}");
        assert!(
            hits.keys().any(|(_, r)| *r == key),
            "allowlist entry {i} ({} on `{}`) matches nothing any more; remove it",
            a.rule,
            a.text
        );
    }
}

// ── Over every swept view ───────────────────────────────────────────────────

#[test]
fn copy_reads_neutral() {
    assert_family(&["tone:"]);
}
