use serde::{Deserialize, Serialize};

/// Canonical evidence grading scale (File 09).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceGrade {
    /// Contradicted or retracted claims; must be hard-blocked, never programmed.
    MarketingMyth,
    /// Practice heuristic with no direct trial evidence.
    ExpertOpinion,
    /// Mechanistic or observational evidence only.
    Weak,
    /// Mixed or limited RCTs; promising but unsettled.
    Moderate,
    /// Well-replicated meta-analyses / RCTs.
    Strong,
}

impl EvidenceGrade {
    /// Default confidence score per grade (File 09 mapping).
    pub const fn default_confidence(self) -> f32 {
        match self {
            EvidenceGrade::Strong => 0.90,
            EvidenceGrade::Moderate => 0.65,
            EvidenceGrade::Weak => 0.40,
            EvidenceGrade::ExpertOpinion => 0.30,
            EvidenceGrade::MarketingMyth => 0.05,
        }
    }
}
