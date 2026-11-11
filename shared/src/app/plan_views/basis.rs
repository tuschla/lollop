//! A lift's why? basis, rendered once from the pieces its prescription arm
//! wrote ([`BasisRx`]) against the face the day ends up showing: after every
//! fold, hold and in-session cue moved the numbers.

use crate::app::evidence_views::reps_word;
use crate::app::format::fmt_weight;
use crate::app::plan_views::item_render::{sets_reps_lead, shown_rir, with_reps_left};
use crate::app::plan_views::plan_item::{BasisPiece, BasisRx, PlanItem};

/// `"at least two left"`.
fn at_least(n: i64) -> String {
    format!("at least {} left", reps_word(n.clamp(0, 255) as u8))
}

fn could_lift(times: i64) -> String {
    format!("could lift about {times} times")
}

/// The basis as the arm wrote it.
pub(super) fn planned_text(pieces: &[BasisPiece], weight_lb: bool) -> String {
    pieces
        .iter()
        .map(|p| match p {
            BasisPiece::Text(t) | BasisPiece::Lead(t) => t.clone(),
            BasisPiece::RepsLeft(n) => with_reps_left(*n),
            BasisPiece::AtLeast(n) => at_least(*n),
            BasisPiece::CouldLift { reps, rir } => could_lift(reps + rir),
            BasisPiece::Load(kg) => fmt_weight(*kg, weight_lb),
        })
        .collect()
}

/// The basis states the prescription the plan derived ("3×5 at …: 100 kg.
/// Stop each set with ~2 reps left."). Once a fold, a hold or an in-session
/// cue moved the face, it states the load as planned against today's, and
/// the sets×reps and the reps left as today's (a pick-a-weight face's reserve
/// included), so the session panel reads the prescription the face shows.
/// Sentences added after the arm's (a velocity cue) follow it unchanged; a
/// basis another rule replaced is left alone.
pub(in crate::app) fn render_basis(it: &mut PlanItem, weight_lb: bool) {
    let Some(rx) = it.basis_rx.take() else {
        return;
    };
    let written = planned_text(&rx.pieces, weight_lb);
    let Some(after) = it.why.basis.strip_prefix(written.as_str()) else {
        return;
    };
    let after = after.to_string();
    it.why.basis = render_pieces(&rx, it, weight_lb) + &after;
}

/// The pieces as `face` shows the day.
fn render_pieces(rx: &BasisRx, face: &PlanItem, weight_lb: bool) -> String {
    let lead = Some(sets_reps_lead(face)).filter(|l| *l != rx.lead);
    let rir = match (rx.rir, shown_rir(face)) {
        (Some(from), Some(to)) if from != to => Some((from, to)),
        _ => None,
    };
    // The reserve the face moved is stated in its "with ~N reps left" piece,
    // else in its "at least N left" piece.
    let restated_rir = rir.and_then(|(from, _)| {
        let with = with_reps_left(from);
        let least = at_least(from);
        rx.pieces
            .iter()
            .position(|p| matches!(p, BasisPiece::RepsLeft(n) if with_reps_left(*n) == with))
            .or_else(|| {
                rx.pieces
                    .iter()
                    .position(|p| matches!(p, BasisPiece::AtLeast(n) if at_least(*n) == least))
            })
    });
    let mut out = String::new();
    for (i, p) in rx.pieces.iter().enumerate() {
        match p {
            BasisPiece::Text(t) => out.push_str(t),
            BasisPiece::Lead(t) => match &lead {
                Some(now) if *t == rx.lead => out.push_str(now),
                _ => out.push_str(t),
            },
            BasisPiece::RepsLeft(n) => match rir {
                Some((_, to)) if restated_rir == Some(i) => out.push_str(&with_reps_left(to)),
                _ => out.push_str(&with_reps_left(*n)),
            },
            BasisPiece::AtLeast(n) => match rir {
                Some((_, to)) if restated_rir == Some(i) => out.push_str(&at_least(to)),
                _ => out.push_str(&at_least(*n)),
            },
            BasisPiece::CouldLift { reps, rir: n } => match rir {
                Some((from, to)) if reps + n == i64::from(face.reps_low) + from.max(0) => {
                    out.push_str(&could_lift(i64::from(face.reps_low) + to.max(0)));
                }
                _ => out.push_str(&could_lift(reps + n)),
            },
            BasisPiece::Load(kg) => {
                out.push_str(&fmt_weight(*kg, weight_lb));
                if rx.load_kg == Some(*kg) {
                    out.push_str(&load_today(face, *kg, weight_lb));
                }
            }
        }
    }
    out
}

/// What follows the planned load once the day moved it: `" planned, 95 kg
/// today"`; empty when the face shows the planned load.
fn load_today(it: &PlanItem, planned_kg: f64, weight_lb: bool) -> String {
    let planned = fmt_weight(planned_kg, weight_lb);
    let today = match it.load_kg {
        Some(to) if fmt_weight(to, weight_lb) == planned => return String::new(),
        Some(to) if it.load_held => format!(
            "held at {} today (the most you've already lifted for these reps)",
            fmt_weight(to, weight_lb)
        ),
        Some(to) => format!("{} today", fmt_weight(to, weight_lb)),
        None => "pick the weight today".to_string(),
    };
    format!(" planned, {today}")
}
