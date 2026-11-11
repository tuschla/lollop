//! Structured parts of the core's text, so a shell renders them without
//! reading the English: the glossary terms a summary uses (as spans), a
//! card's spoken subject, and the non-breaking space that keeps a figure with
//! its unit on a compact face line.

use crate::app::view_types::{
    AdjustmentView, FeedbackView, GlossarySpan, GlossaryTerm, GuidanceView, PrescriptionView,
    RacePredictionView, SessionPlanView, SrpeLoadView, TodayHeadlineView, TrainingLoadView,
    ViewModel,
};

// ── Glossary terms ───────────────────────────────────────────────────────────

/// How a term is spotted in text: its spellings (longest first), whether case
/// is ignored, and which ends must sit on a word boundary.
struct Pattern {
    spellings: &'static [&'static str],
    ignore_case: bool,
    left: Left,
    /// The next character is no word character (`[A-Za-z0-9_]`).
    right: bool,
}

/// What may precede a term.
#[derive(Clone, Copy)]
enum Left {
    /// Anything.
    Any,
    /// No word character (`[A-Za-z0-9_]`).
    Word,
    /// No ASCII letter or digit ("1RM" never fires inside "e1RM").
    AlphaNum,
}

impl GlossaryTerm {
    /// Every term, in the glossary's order (the order a card lists its terms).
    pub const ALL: [GlossaryTerm; 20] = [
        GlossaryTerm::E1rm,
        GlossaryTerm::OneRm,
        GlossaryTerm::Rpe,
        GlossaryTerm::Rir,
        GlossaryTerm::ZScore,
        GlossaryTerm::Vdot,
        GlossaryTerm::Decoupling,
        GlossaryTerm::VariabilityIndex,
        GlossaryTerm::Tonnage,
        GlossaryTerm::Spike,
        GlossaryTerm::HrMax,
        GlossaryTerm::Deload,
        GlossaryTerm::Mesocycle,
        GlossaryTerm::Ctl,
        GlossaryTerm::Trimp,
        GlossaryTerm::HrZones,
        GlossaryTerm::Volume,
        GlossaryTerm::Maf,
        GlossaryTerm::Pap,
        GlossaryTerm::RedS,
    ];

    /// How the term is spotted in card copy; `None` for a word too generic to
    /// spot ("spike", "volume"), reachable only from an explicit glossary link.
    fn pattern(self) -> Option<Pattern> {
        let p = |spellings, ignore_case, left, right| {
            Some(Pattern {
                spellings,
                ignore_case,
                left,
                right,
            })
        };
        match self {
            GlossaryTerm::E1rm => p(&["e1RM"], true, Left::Word, true),
            GlossaryTerm::OneRm => p(&["1RM"], false, Left::AlphaNum, true),
            GlossaryTerm::Rpe => p(&["RPE"], false, Left::Word, true),
            GlossaryTerm::Rir => p(&["RIR"], false, Left::Word, true),
            GlossaryTerm::ZScore => p(&["z-scores", "z-score"], true, Left::Word, true),
            GlossaryTerm::Vdot => p(&["VDOT"], false, Left::Word, true),
            GlossaryTerm::Decoupling => p(&["decoupling"], true, Left::Word, true),
            GlossaryTerm::VariabilityIndex => p(&["variability index"], true, Left::Word, true),
            GlossaryTerm::Tonnage => p(&["tonnage"], true, Left::Word, true),
            GlossaryTerm::HrMax => p(&["HRmax"], false, Left::Any, false),
            GlossaryTerm::Deload => p(&["deload"], true, Left::Word, false),
            GlossaryTerm::Mesocycle => p(&["mesocycles", "mesocycle"], true, Left::Word, true),
            GlossaryTerm::Ctl => p(&["CTL"], false, Left::Word, true),
            GlossaryTerm::Trimp => p(&["TRIMP"], false, Left::Word, true),
            GlossaryTerm::HrZones => p(&["HR zones", "HR zone"], false, Left::Word, true),
            GlossaryTerm::Maf => p(&["MAF"], false, Left::Word, true),
            GlossaryTerm::Pap => p(&["PAPE", "PAP"], false, Left::Word, true),
            GlossaryTerm::RedS => p(&["RED-S"], false, Left::Word, true),
            GlossaryTerm::Spike | GlossaryTerm::Volume => None,
        }
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every glossary term occurrence in `text`, by start (then glossary order).
/// Offsets are UTF-16 code units, the indices of a Kotlin/Swift string.
pub(crate) fn glossary_spans(text: &str) -> Vec<GlossarySpan> {
    let b = text.as_bytes();
    let mut spans = Vec::new();
    for term in GlossaryTerm::ALL {
        let Some(p) = term.pattern() else { continue };
        let mut i = 0;
        while i < b.len() {
            let left_ok = match p.left {
                Left::Any => true,
                Left::Word => i == 0 || !is_word_byte(b[i - 1]),
                Left::AlphaNum => i == 0 || !b[i - 1].is_ascii_alphanumeric(),
            };
            let hit = left_ok
                .then(|| {
                    p.spellings.iter().find_map(|s| {
                        let end = i + s.len();
                        let window = b.get(i..end)?;
                        let same = if p.ignore_case {
                            window.eq_ignore_ascii_case(s.as_bytes())
                        } else {
                            window == s.as_bytes()
                        };
                        let right_ok = !p.right || b.get(end).is_none_or(|&n| !is_word_byte(n));
                        (same && right_ok).then_some(end)
                    })
                })
                .flatten();
            match hit {
                Some(end) => {
                    spans.push(GlossarySpan {
                        term,
                        start: utf16_len(&text[..i]),
                        end: utf16_len(&text[..end]),
                    });
                    i = end;
                }
                None => i += 1,
            }
        }
    }
    spans.sort_by_key(|s| (s.start, s.term));
    spans
}

fn utf16_len(s: &str) -> u32 {
    u32::try_from(s.encode_utf16().count()).unwrap_or(u32::MAX)
}

// ── Spoken subject ───────────────────────────────────────────────────────────

/// Longest spoken subject, UTF-16 code units.
const SUBJECT_MAX_UNITS: usize = 60;

/// What a card is about, spoken on its "?" ("Details: <subject>"): the
/// summary up to its first ". ", cut to [`SUBJECT_MAX_UNITS`] UTF-16 code
/// units (never inside a character).
pub(crate) fn spoken_subject(summary: &str) -> String {
    let first = summary.split(". ").next().unwrap_or_default();
    let mut units = 0;
    first
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= SUBJECT_MAX_UNITS
        })
        .collect()
}

// ── Prose ────────────────────────────────────────────────────────────────────

/// Longest card summary that still reads as a short claim, UTF-16 code units.
const CLAIM_MAX_UNITS: u32 = 90;

/// Whether a card summary reads as a paragraph rather than a short claim:
/// longer than [`CLAIM_MAX_UNITS`]. A paragraph is set in the body weight so it
/// never outweighs the card titles around it (visual-design-system.md, type).
pub(crate) fn is_prose(summary: &str) -> bool {
    utf16_len(summary) > CLAIM_MAX_UNITS
}

// ── Figures and units ────────────────────────────────────────────────────────

/// The units a figure keeps on its line.
const GLUED_UNITS: [&str; 8] = ["/km", "/mi", "min", "kg", "lb", "km", "mi", "s"];

/// `text` with the spaces a compact face line must not break at made
/// non-breaking: between a figure and its unit ("117.5 kg", "5:40 /km",
/// "30 s") and after "@", so a wrapped line keeps a number with its unit and
/// never ends on "@" ("Back squat · 5×3 @ 117.5 kg" may wrap before "@ 117.5
/// kg", the one break a narrow line at a large font needs inside a
/// prescription).
pub(crate) fn keep_units(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len() + 8);
    for (i, c) in text.char_indices() {
        let glue = c == ' '
            && i > 0
            && (b[i - 1] == b'@' || (b[i - 1].is_ascii_digit() && unit_at(text, i + 1)));
        out.push(if glue { '\u{a0}' } else { c });
    }
    out
}

/// A [`GLUED_UNITS`] unit starts at byte `i` and no letter or digit follows it.
fn unit_at(text: &str, i: usize) -> bool {
    let rest = &text[i..];
    GLUED_UNITS.iter().any(|u| {
        rest.strip_prefix(u)
            .is_some_and(|after| !after.starts_with(char::is_alphabetic))
    })
}

/// Serde for a compact face field: written with [`keep_units`] applied, read
/// as is. The core keeps the plain text (it derives other lines from it).
pub(crate) mod units_kept {
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::ptr_arg)] // serde's `with` passes the field as `&String`
    pub fn serialize<S: Serializer>(text: &String, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::keep_units(text))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        String::deserialize(d)
    }

    pub fn wire_shape(_: &mut crate::wire::Registry) -> crate::wire::Shape {
        crate::wire::Shape::Str
    }
}

// ── Marking the view ─────────────────────────────────────────────────────────

/// A row whose text gets its parts once the view is built.
trait Marked {
    fn mark(&mut self);
}

/// Glossary spans, the spoken subject and the prose mark of an evidence
/// card's text.
fn card_marks(text: &str) -> (Vec<GlossarySpan>, String, bool) {
    (glossary_spans(text), spoken_subject(text), is_prose(text))
}

impl Marked for AdjustmentView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.summary);
    }
}

impl Marked for TodayHeadlineView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.summary);
    }
}

impl Marked for FeedbackView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.message);
    }
}

impl Marked for GuidanceView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.summary);
    }
}

impl Marked for TrainingLoadView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.summary);
    }
}

impl Marked for SrpeLoadView {
    fn mark(&mut self) {
        (self.glossary, self.subject, self.prose) = card_marks(&self.summary);
    }
}

impl Marked for RacePredictionView {
    fn mark(&mut self) {
        self.glossary = glossary_spans(&self.summary);
        self.notes.iter_mut().for_each(Marked::mark);
    }
}

impl Marked for PrescriptionView {
    fn mark(&mut self) {
        self.glossary = glossary_spans(&self.summary);
    }
}

impl Marked for SessionPlanView {
    fn mark(&mut self) {
        self.items.iter_mut().for_each(Marked::mark);
        if let Some(a) = &mut self.adjustment {
            a.mark();
        }
    }
}

impl<T: Marked> Marked for Option<T> {
    fn mark(&mut self) {
        if let Some(t) = self {
            t.mark();
        }
    }
}

impl<T: Marked> Marked for Vec<T> {
    fn mark(&mut self) {
        self.iter_mut().for_each(Marked::mark);
    }
}

/// Mark every row of `vm` from its final text. The reference rows are static
/// and marked once where they are built ([`mark_guidance`]).
pub(super) fn mark_view(vm: &mut ViewModel) {
    vm.adjustments.mark();
    vm.review_adjustments.mark();
    vm.trend.mark();
    vm.provisional.mark();
    vm.autoreg_source.mark();
    vm.today_headline.mark();
    vm.feedback.mark();
    vm.guidance.mark();
    vm.profile_guidance.mark();
    vm.hypertrophy_plan.mark();
    vm.protein_targets.mark();
    vm.hr_zones.mark();
    vm.weekly_report.mark();
    vm.lift_audit.mark();
    vm.training_load.mark();
    vm.srpe_load.mark();
    vm.race_prediction.mark();
    vm.next_session.mark();
    vm.week_plan.mark();
    vm.next_week_plan.mark();
    if let Some(c) = &mut vm.consistency {
        c.notes.mark();
    }
}

/// Mark guidance rows built outside [`mark_view`] (the memoized reference).
pub(super) fn mark_guidance(rows: &mut [GuidanceView]) {
    rows.iter_mut().for_each(Marked::mark);
}
