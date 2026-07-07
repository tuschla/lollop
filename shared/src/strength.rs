//! Strength & power calculators (knowledge-base File 02, Strength & Power
//! Resistance-Training Programming Rules).
//!
//! Pure, deterministic math: e1RM estimators, RPE/RIR mapping, reps→%1RM
//! estimation, and Prilepin volume governance. No IO. Values that make a
//! prescriptive recommendation are wrapped in [`Recommended`] via
//! [`recommend`], which forces attached evidence + confidence.
//!
//! All numbers transcribed verbatim from File 02 (rules strength-005,
//! strength-007, strength-011 and the Prilepin's-chart verbatim table). Claim
//! ids come from the canonical registry in `crate::evidence`.

use crate::evidence::claim;
use crate::schema::{
    Citation, ConfidenceTag, Evidence, EvidenceGrade, Recommended,
};

// ---------------------------------------------------------------------------
// Recommendation helper
// ---------------------------------------------------------------------------

/// Wrap a prescriptive value with evidence + confidence pulled from the
/// registry (File 02 claim ids). Falls back to an `ExpertOpinion` synthesis
/// citation if the id is absent, so the helper never silently drops evidence.
pub fn recommend<T>(value: T, claim_id: &str) -> Recommended<T> {
    match claim(claim_id) {
        Some(entry) => Recommended {
            value,
            evidence: entry.to_evidence(),
            confidence: entry.to_confidence_tag(),
        },
        None => Recommended {
            value,
            evidence: Evidence {
                grade: EvidenceGrade::ExpertOpinion,
                citation: Citation {
                    claim_id: None,
                    reference: "File 02 synthesis (unregistered id)".to_string(),
                },
                contradicting: vec![],
            },
            confidence: ConfidenceTag {
                score: EvidenceGrade::ExpertOpinion.default_confidence(),
                contested: false,
                contested_question_ref: None,
                safety_critical: false,
            },
        },
    }
}

// ---------------------------------------------------------------------------
// 1. Estimated 1RM (File 02 strength-005)
// ---------------------------------------------------------------------------

/// Epley estimated 1RM: `weight * (1 + reps/30)` (File 02 strength-005).
pub fn e1rm_epley(weight: f64, reps: u32) -> f64 {
    weight * (1.0 + reps as f64 / 30.0)
}

/// Brzycki estimated 1RM: `weight * 36 / (37 - reps)` (File 02 strength-005).
/// Undefined at 37 reps (division by zero); callers should keep reps in the
/// 1–10 accuracy window noted in strength-005/strength-006.
pub fn e1rm_brzycki(weight: f64, reps: u32) -> f64 {
    weight * 36.0 / (37.0 - reps as f64)
}

// ---------------------------------------------------------------------------
// 2. RPE ↔ RIR mapping (File 02 strength-007; registry AUTOREG-RIR-001)
// ---------------------------------------------------------------------------

/// Reps in reserve from RPE via the Zourdos anchor: RPE 10 = 0 RIR, each RIR
/// = −1 RPE (File 02 strength-007; registry AUTOREG-RIR-001). Clamped at 0.
pub fn rpe_to_rir(rpe: f64) -> f64 {
    let rir = 10.0 - rpe;
    if rir < 0.0 {
        0.0
    } else {
        rir
    }
}

/// RPE from reps in reserve, inverse of [`rpe_to_rir`] (File 02 strength-007;
/// registry AUTOREG-RIR-001). Clamped at 10.
pub fn rir_to_rpe(rir: f64) -> f64 {
    let rpe = 10.0 - rir;
    if rpe > 10.0 {
        10.0
    } else {
        rpe
    }
}

// ---------------------------------------------------------------------------
// 3. Reps → %1RM estimate (File 02 strength-005, Epley inverse)
// ---------------------------------------------------------------------------

/// Estimated %1RM for a maximal set of `reps`, as the Epley inverse
/// `100 / (1 + reps/30)` (File 02 strength-005). ESTIMATE ONLY: accurate at
/// 2–10 reps (±5%), diverges ±15–20% above 10 reps (strength-005/006).
pub fn est_pct_1rm_from_reps(reps: u32) -> f64 {
    100.0 / (1.0 + reps as f64 / 30.0)
}

// ---------------------------------------------------------------------------
// 4. Prilepin's chart (File 02 strength-011, verbatim table)
// ---------------------------------------------------------------------------

/// One row of Prilepin's chart: an intensity band with its rep-per-set range,
/// optimal total reps, and total-rep range (File 02 verbatim table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrilepinRow {
    /// Inclusive lower %1RM bound for this band.
    pub pct_min: u8,
    /// Inclusive upper %1RM bound for this band.
    pub pct_max: u8,
    /// Reps per set (min, max).
    pub reps_per_set: (u8, u8),
    /// Optimal total reps for the session at this intensity.
    pub optimal_total: u16,
    /// Acceptable total-rep range (min, max).
    pub total_range: (u16, u16),
}

/// Prilepin's chart, transcribed verbatim from File 02 (strength-011).
///
/// | %1RM   | Reps/set | Optimal total | Total range |
/// |--------|----------|---------------|-------------|
/// | <70%   | 3–6      | 24            | 18–30       |
/// | 70–80% | 3–6      | 18            | 12–24       |
/// | 80–90% | 2–4      | 15            | 10–20       |
/// | >90%   | 1–2      | 7             | 4–10        |
///
/// Band bounds are encoded as half-open on the verbatim breakpoints (69, 79,
/// 89, 100) so every intensity 0–100% resolves to exactly one row.
pub static PRILEPIN: &[PrilepinRow] = &[
    PrilepinRow {
        pct_min: 0,
        pct_max: 69,
        reps_per_set: (3, 6),
        optimal_total: 24,
        total_range: (18, 30),
    },
    PrilepinRow {
        pct_min: 70,
        pct_max: 79,
        reps_per_set: (3, 6),
        optimal_total: 18,
        total_range: (12, 24),
    },
    PrilepinRow {
        pct_min: 80,
        pct_max: 89,
        reps_per_set: (2, 4),
        optimal_total: 15,
        total_range: (10, 20),
    },
    PrilepinRow {
        pct_min: 90,
        pct_max: 100,
        reps_per_set: (1, 2),
        optimal_total: 7,
        total_range: (4, 10),
    },
];

/// Look up the Prilepin row governing a given %1RM (File 02 strength-011).
/// Returns `None` for out-of-range (<0 or >100) intensities. The <70% band is
/// treated as a floor, matching the verbatim "<70%" row.
pub fn prilepin_for(pct_1rm: f64) -> Option<&'static PrilepinRow> {
    if !(0.0..=100.0).contains(&pct_1rm) {
        return None;
    }
    // Round to nearest whole percent for band membership.
    let pct = pct_1rm.round() as i64;
    PRILEPIN
        .iter()
        .find(|row| pct >= row.pct_min as i64 && pct <= row.pct_max as i64)
}

/// True when `total_reps` falls within the Prilepin total-rep range for the
/// band at `pct_1rm` (File 02 strength-011 volume governor). False if the
/// intensity is out of range or the volume is outside the band's window.
pub fn prilepin_volume_ok(pct_1rm: f64, total_reps: u16) -> bool {
    match prilepin_for(pct_1rm) {
        Some(row) => total_reps >= row.total_range.0 && total_reps <= row.total_range.1,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epley_100kg_5reps_is_about_116_7() {
        let e = e1rm_epley(100.0, 5);
        assert!((e - 116.666_666).abs() < 1e-3, "got {e}");
    }

    #[test]
    fn brzycki_sanity() {
        // 100 kg x 5 -> 100 * 36 / 32 = 112.5
        let b = e1rm_brzycki(100.0, 5);
        assert!((b - 112.5).abs() < 1e-6, "got {b}");
        // A single rep must equal (near) the load itself: 100 * 36 / 36 = 100.
        assert!((e1rm_brzycki(100.0, 1) - 100.0).abs() < 1e-6);
        // Higher reps estimate a higher 1RM than fewer reps at equal load.
        assert!(e1rm_brzycki(100.0, 8) > e1rm_brzycki(100.0, 3));
    }

    #[test]
    fn rpe_to_rir_anchor() {
        assert_eq!(rpe_to_rir(8.0), 2.0);
        assert_eq!(rpe_to_rir(10.0), 0.0);
        // Clamp: overshoot stays at 0 RIR.
        assert_eq!(rpe_to_rir(11.0), 0.0);
        // Round trip.
        assert_eq!(rir_to_rpe(2.0), 8.0);
        assert_eq!(rir_to_rpe(0.0), 10.0);
    }

    #[test]
    fn est_pct_from_reps_is_estimate() {
        // 1 rep -> 100 / (1 + 1/30) ~= 96.77%
        let p = est_pct_1rm_from_reps(1);
        assert!((p - 96.774).abs() < 1e-2, "got {p}");
        // Monotonic decrease with more reps.
        assert!(est_pct_1rm_from_reps(10) < est_pct_1rm_from_reps(3));
    }

    #[test]
    fn prilepin_for_70pct_returns_correct_row() {
        let row = prilepin_for(70.0).expect("70% resolves");
        assert_eq!(row.pct_min, 70);
        assert_eq!(row.pct_max, 79);
        assert_eq!(row.reps_per_set, (3, 6));
        assert_eq!(row.optimal_total, 18);
        assert_eq!(row.total_range, (12, 24));
        // Boundaries and out-of-range.
        assert_eq!(prilepin_for(95.0).unwrap().optimal_total, 7);
        assert_eq!(prilepin_for(50.0).unwrap().optimal_total, 24);
        assert!(prilepin_for(-1.0).is_none());
        assert!(prilepin_for(101.0).is_none());
    }

    #[test]
    fn prilepin_volume_pass_and_fail() {
        // 85% band range is 10-20 total reps.
        assert!(prilepin_volume_ok(85.0, 15)); // optimal
        assert!(prilepin_volume_ok(85.0, 10)); // lower bound
        assert!(prilepin_volume_ok(85.0, 20)); // upper bound
        assert!(!prilepin_volume_ok(85.0, 9)); // under
        assert!(!prilepin_volume_ok(85.0, 25)); // over
        assert!(!prilepin_volume_ok(200.0, 15)); // out of intensity range
    }

    #[test]
    fn prilepin_table_nonempty_and_sorted() {
        assert!(!PRILEPIN.is_empty());
        assert_eq!(PRILEPIN.len(), 4);
        // Ascending, non-overlapping, gap-free bands.
        for w in PRILEPIN.windows(2) {
            assert!(w[0].pct_max < w[1].pct_min);
            assert_eq!(w[0].pct_max + 1, w[1].pct_min);
        }
        // Full 0-100 coverage.
        assert_eq!(PRILEPIN.first().unwrap().pct_min, 0);
        assert_eq!(PRILEPIN.last().unwrap().pct_max, 100);
    }

    #[test]
    fn recommend_attaches_registered_evidence() {
        let rx = recommend(LiftDummy, "AUTOREG-RIR-001");
        assert_eq!(rx.evidence.grade, EvidenceGrade::Strong);
        assert_eq!(
            rx.evidence.citation.claim_id.as_deref(),
            Some("AUTOREG-RIR-001")
        );
        // Unregistered id falls back to ExpertOpinion, never panics.
        let fallback = recommend(LiftDummy, "NOT-A-REAL-ID");
        assert_eq!(fallback.evidence.grade, EvidenceGrade::ExpertOpinion);
    }

    #[derive(Debug, PartialEq)]
    struct LiftDummy;
}
