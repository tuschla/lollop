//! The structured text parts the core sends so the shells never read its
//! English: glossary spans, spoken subjects, the prose mark, the non-breaking
//! figure glue, basis sentences, the effort-free face line and the win's
//! advice link.

use crate::app::reviews::link_win_advice;
use crate::app::text_parts::{glossary_spans, is_prose, keep_units, spoken_subject};
use crate::app::view_types::{AdjustmentView, GlossaryTerm, basis_sentences};

#[test]
fn a_win_links_its_lifts_next_time_advice_only() {
    let row = |exercise: &str, load: Option<f64>, end: bool| AdjustmentView {
        exercise: Some(exercise.into()),
        suggested_load_kg: load,
        suggests_end_exercise: end,
        ..Default::default()
    };
    let mut rows = vec![
        // An in-session cue for another lift.
        row("Deadlift", Some(140.0), false),
        // The won lift's next-time note (its name in another case).
        row("back Squat", None, false),
        // An in-session cue for the same lift is not next-time advice.
        row("Back squat", Some(120.0), false),
        row("Back squat", None, true),
        // Advice for another lift stays its own card.
        row("Bench press", None, false),
    ];
    link_win_advice(Some("Back squat"), &mut rows);
    let linked: Vec<bool> = rows.iter().map(|a| a.win_advice).collect();
    assert_eq!(linked, [false, true, false, false, false]);
    // A win naming no lift (a run, a whole week) links nothing.
    let mut fresh = vec![row("Back squat", None, false)];
    link_win_advice(None, &mut fresh);
    assert!(!fresh[0].win_advice);
}

/// The terms of `text`'s spans, each once, in glossary order: what a card's
/// "?" panel links.
fn terms(text: &str) -> Vec<GlossaryTerm> {
    let mut t: Vec<_> = glossary_spans(text).into_iter().map(|s| s.term).collect();
    t.sort();
    t.dedup();
    t
}

/// `text` sliced by a span's UTF-16 offsets, the way a Kotlin or Swift
/// string is indexed.
fn utf16_slice(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[start as usize..end as usize]).expect("span on char boundaries")
}

#[test]
fn prescription_jargon_gets_glossary_terms() {
    assert_eq!(terms("Romanian deadlift: 3×5 · RIR 3"), [GlossaryTerm::Rir]);
    assert_eq!(
        terms("Back Squat: 4×3 @ 117.5 kg · 85% e1RM"),
        [GlossaryTerm::E1rm]
    );
    assert_eq!(terms("Bench Press: 3×5 @ RPE 8"), [GlossaryTerm::Rpe]);
    assert_eq!(terms("Easy run: 40 min · 70% HRmax"), [GlossaryTerm::HrMax]);
    assert_eq!(
        terms("Deloading: CTL and TRIMP over 2 mesocycles, HR zones, RED-S, PAPE, MAF"),
        [
            GlossaryTerm::Deload,
            GlossaryTerm::Mesocycle,
            GlossaryTerm::Ctl,
            GlossaryTerm::Trimp,
            GlossaryTerm::HrZones,
            GlossaryTerm::Maf,
            GlossaryTerm::Pap,
            GlossaryTerm::RedS,
        ]
    );
    assert_eq!(
        terms("A z-score of 1.2, the variability index, VDOT 50, tonnage, decoupling"),
        [
            GlossaryTerm::ZScore,
            GlossaryTerm::Vdot,
            GlossaryTerm::Decoupling,
            GlossaryTerm::VariabilityIndex,
            GlossaryTerm::Tonnage,
        ]
    );
}

#[test]
fn one_rm_does_not_fire_inside_e1rm() {
    assert_eq!(terms("85% e1RM"), [GlossaryTerm::E1rm]);
    assert_eq!(terms("80% 1RM"), [GlossaryTerm::OneRm]);
    assert_eq!(terms("85% E1RM"), [GlossaryTerm::E1rm], "e1RM ignores case");
}

#[test]
fn plain_copy_gets_no_glossary_terms() {
    assert!(terms("Rest day. Recovery is part of the plan.").is_empty());
    // Substrings of ordinary words never match an acronym.
    assert!(terms("Stir the porridge, then ripen").is_empty());
    assert!(terms("RPEs and xRIR").is_empty());
    // The generic words are never spotted in text.
    assert!(terms("A spike in volume").is_empty());
}

#[test]
fn glossary_spans_are_utf16_offsets() {
    // "×", "–" and "·" are one UTF-16 unit but two or three UTF-8 bytes; "💪"
    // is two units: every span still slices to its term.
    let text = "4×3 @ 117.5 kg · 1–2 reps left · 💪 85% e1RM · RPE 8";
    let spans = glossary_spans(text);
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert_eq!(utf16_slice(text, spans[0].start, spans[0].end), "e1RM");
    assert_eq!(utf16_slice(text, spans[1].start, spans[1].end), "RPE");
    assert!(spans[0].start < spans[1].start, "spans run in text order");
    // Every occurrence is a span.
    assert_eq!(glossary_spans("RPE 7, then RPE 8").len(), 2);
}

#[test]
fn spoken_subject_is_the_first_sentence_up_to_60_units() {
    assert_eq!(
        spoken_subject("New best on Back squat: 120 kg × 5. Your estimated max is up."),
        "New best on Back squat: 120 kg × 5"
    );
    assert_eq!(spoken_subject("Easy run"), "Easy run");
    let long = "a".repeat(70);
    assert_eq!(spoken_subject(&long).len(), 60);
    // Never cut inside a character: 59 units then a two-unit emoji.
    let astral = format!("{}💪 more", "b".repeat(59));
    assert_eq!(spoken_subject(&astral), "b".repeat(59));
    // A decimal point is no sentence end.
    assert_eq!(spoken_subject("Run 3.5 km. Then rest."), "Run 3.5 km");
}

#[test]
fn a_summary_over_90_utf16_units_is_prose() {
    assert!(!is_prose(&"a".repeat(90)));
    assert!(is_prose(&"a".repeat(91)));
    // Counted in UTF-16 units, as a shell measures its strings: "×" is one.
    assert!(!is_prose(&format!("{}×", "a".repeat(89))));
    assert!(is_prose(&format!("{}💪", "a".repeat(89))));
}

#[test]
fn keep_units_glues_figures_to_units_and_at() {
    let nb = '\u{a0}';
    assert_eq!(
        keep_units("Back squat · 5×3 @ 117.5 kg"),
        format!("Back squat · 5×3 @{nb}117.5{nb}kg")
    );
    assert_eq!(keep_units("5:40–6:00 /km"), format!("5:40–6:00{nb}/km"));
    assert_eq!(keep_units("8 km"), format!("8{nb}km"));
    assert_eq!(keep_units("3.4 mi"), format!("3.4{nb}mi"));
    assert_eq!(keep_units("45 min"), format!("45{nb}min"));
    assert_eq!(keep_units("4×30 s hold"), format!("4×30{nb}s hold"));
    assert_eq!(keep_units("60 lb"), format!("60{nb}lb"));
    // A unit is a whole word, and only a figure keeps one.
    assert_eq!(keep_units("5 sets"), "5 sets");
    assert_eq!(keep_units("3 minutes"), "3 minutes");
    assert_eq!(keep_units("6 × 800 m"), "6 × 800 m");
    assert_eq!(keep_units("easy km"), "easy km");
}

#[test]
fn basis_sentences_split_at_sentence_ends_only() {
    let basis = "6×5 at 83% of your estimated max (120 kg): 264.5 lb. Aim to stop with ~2 reps left! Why? Rest.";
    let s = basis_sentences::split(basis);
    assert_eq!(
        s,
        [
            "6×5 at 83% of your estimated max (120 kg): 264.5 lb.",
            "Aim to stop with ~2 reps left!",
            "Why?",
            "Rest.",
        ]
    );
    assert_eq!(
        s.join(" "),
        basis,
        "joined by a space, the basis reads as built"
    );
    assert!(basis_sentences::split("").is_empty());
}
