//! Canonical evidence registry (knowledge-base File 09).
//!
//! Compile-time table of every graded claim + contested question the engine may
//! cite. Data only, no coaching logic. Every recommendation the core emits must
//! reference a `claim_id` that exists here (see [`claim`]); `MarketingMyth`
//! claims are hard-blocked and must never be surfaced as advice.
//!
//! Short-form citations only; full statistics + DOIs live in
//! `knowledge-base/extracted/09-evidence-map.md`. Verify DOIs against primary
//! sources before production use.

use crate::schema::{Citation, ConfidenceTag, Evidence, EvidenceGrade};

/// One graded claim in the registry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceEntry {
    /// Stable key, e.g. `"HYP-VOL-001"`. Referenced by every recommendation.
    pub claim_id: &'static str,
    pub statement: &'static str,
    pub grade: EvidenceGrade,
    pub primary_citations: &'static [&'static str],
    pub contradicting: &'static [&'static str],
    /// Non-negotiable deferral / stop signal when true.
    pub safety_critical: bool,
    /// Contested-question id (`"CQ-01"`) if the claim is under active debate.
    pub contested: Option<&'static str>,
    /// Re-grade cadence in months (12 default; 6 for fast-moving topics).
    pub review_months: u8,
}

impl EvidenceEntry {
    /// Default confidence from grade (File 09 mapping).
    pub const fn confidence_score(&self) -> f32 {
        self.grade.default_confidence()
    }

    /// True for `MarketingMyth`: must never be surfaced as advice.
    pub const fn is_blocked(&self) -> bool {
        matches!(self.grade, EvidenceGrade::MarketingMyth)
    }

    /// Build the schema `Evidence` that a `Recommended<T>` carries.
    pub fn to_evidence(&self) -> Evidence {
        Evidence {
            grade: self.grade,
            citation: Citation {
                claim_id: Some(self.claim_id.to_string()),
                reference: self.primary_citations.first().copied().unwrap_or("unstated").to_string(),
            },
            contradicting: self
                .contradicting
                .iter()
                .map(|r| Citation { claim_id: Some(self.claim_id.to_string()), reference: r.to_string() })
                .collect(),
        }
    }

    /// Build the schema `ConfidenceTag` that a `Recommended<T>` carries.
    pub fn to_confidence_tag(&self) -> ConfidenceTag {
        ConfidenceTag {
            score: self.confidence_score(),
            contested: self.contested.is_some(),
            contested_question_ref: self.contested.map(str::to_string),
            safety_critical: self.safety_critical,
        }
    }
}

/// An open question where evidence conflicts; the engine holds a default lean.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContestedQuestion {
    pub id: &'static str,
    pub question: &'static str,
    /// Current engine default / lean while the question stays open.
    pub engine_default: &'static str,
}

/// Look up a claim by id.
pub fn claim(claim_id: &str) -> Option<&'static EvidenceEntry> {
    CLAIMS.iter().find(|c| c.claim_id == claim_id)
}

/// Look up a contested question by id.
pub fn contested_question(id: &str) -> Option<&'static ContestedQuestion> {
    CONTESTED_QUESTIONS.iter().find(|q| q.id == id)
}

use EvidenceGrade::{ExpertOpinion, MarketingMyth, Moderate, Strong, Weak};

/// The full claim registry (File 09).
pub static CLAIMS: &[EvidenceEntry] = &[
    // ---- Strong ----
    EvidenceEntry {
        claim_id: "HYP-VOL-001",
        statement: "More weekly sets increase hypertrophy with diminishing returns (curvilinear dose-response).",
        grade: Strong,
        primary_citations: &["Schoenfeld, Ogborn & Krieger 2017, J Sports Sci", "Pelland et al. 2025, Sports Med"],
        contradicting: &["Undetectable superiority beyond ~31 fractional weekly sets (Pelland 2025)"],
        safety_critical: false,
        contested: Some("CQ-01"),
        review_months: 6,
    },
    EvidenceEntry {
        claim_id: "HYP-LOAD-001",
        statement: "Load interchangeable ~30-85% 1RM when effort matched (hypertrophy).",
        grade: Strong,
        primary_citations: &["Schoenfeld/Grgic 2017", "Morton 2016", "Lasevicius 2018"],
        contradicting: &["Max-strength gains still favor heavier loads"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "PERIOD-001",
        statement: "Periodization beats no plan.",
        grade: Strong,
        primary_citations: &["Williams et al. 2017"],
        contradicting: &["Model-vs-model comparisons equivocal (CQ-03)"],
        safety_critical: false,
        contested: Some("CQ-03"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-INTENT-001",
        statement: "Intensity is the primary driver of max strength; velocity/intent drives power.",
        grade: Strong,
        primary_citations: &["File 02 consensus"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "CONC-RE-001",
        statement: "Strength training improves running economy by ~2-8% (reciprocal positive).",
        grade: Strong,
        primary_citations: &["Blagrove, Howatson & Hayes 2018, Sports Med"],
        contradicting: &["Magnitude speed- and method-dependent (Llanos-Lagos 2024)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "CONC-INTERF-001",
        statement: "Interference is real but modest and quality-specific.",
        grade: Strong,
        primary_citations: &["Schumann et al. 2022, Sports Med"],
        contradicting: &["Wilson et al. 2012 (nuance)"],
        safety_critical: false,
        contested: Some("CQ-06"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-RIR-001",
        statement: "Performance-based autoregulation (RIR/RPE, velocity) is the most defensible readiness backbone.",
        grade: Strong,
        primary_citations: &["Zourdos et al. 2016, JSCR", "Helms 2018", "Banyard 2017"],
        contradicting: &["Novices estimate RIR poorly"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-TRAGE-001",
        statement: "Training age dominates strength dose-response.",
        grade: Strong,
        primary_citations: &["Rhea et al. 2003, MSSE", "Peterson, Rhea & Alvar 2004"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "GOAL-PROCESS-001",
        statement: "Process goals beat outcome goals.",
        grade: Strong,
        primary_citations: &["Williamson et al. 2022, Int Rev Sport Exerc Psychol"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FEEDBACK-001",
        statement: "How feedback is communicated matters (autonomy-supportive/informational/process-focused).",
        grade: Strong,
        primary_citations: &["Deci, Koestner & Ryan 1999", "Carpentier & Mageau 2013", "Mouratidis 2010"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "TAPER-001",
        statement: "Taper is the best-evidenced peaking lever (2-wk, exponential -41-60% volume, hold intensity & frequency).",
        grade: Strong,
        primary_citations: &["Bosquet 2007, MSSE"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // ---- Safety (Strong, non-negotiable) ----
    EvidenceEntry {
        claim_id: "SAFE-REDS-001",
        statement: "Suspected low energy availability / RED-S triggers a training deferral and referral.",
        grade: Strong,
        primary_citations: &["Mountjoy et al. 2023 IOC consensus, BJSM"],
        contradicting: &["Some critique of diagnostic specificity (does not affect deferral)"],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-OTS-001",
        statement: "Overtraining syndrome - enforce recovery; no load progression.",
        grade: Strong,
        primary_citations: &["Meeusen et al. 2013 ECSS/ACSM"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-BSI-001",
        statement: "Bone stress injuries - stop-loading rule; refer.",
        grade: Strong,
        primary_citations: &["Warden/Davis/Fredericson 2014", "Nattiv 2013"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-PREG-001",
        statement: "Pregnancy - apply pregnancy guardrails; clinician sign-off.",
        grade: Strong,
        primary_citations: &["ACOG Committee Opinion 804 (2020)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-CVD-001",
        statement: "Cardiovascular screening - gate onboarding for at-risk users.",
        grade: Strong,
        primary_citations: &["ACSM / PAR-Q+"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-PAIN-001",
        statement: "Sharp/localized/joint/tendon pain during exercise is a stop signal - cease loading and assess before continuing.",
        grade: Strong,
        primary_citations: &["Musculoskeletal safety consensus - joint pain is a stop signal (File 06 §6C)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "ILLNESS-NECK-001",
        statement: "Febrile illness contraindicates strenuous exercise (myocarditis / sudden-cardiac-death risk): any fever or below-neck symptoms = no training. Above-neck-only symptoms without fever permit reduced-intensity training (neck-check operationalization).",
        grade: Strong,
        primary_citations: &["Sports-medicine consensus: exercise contraindicated during febrile illness (myocarditis risk)", "Meeusen et al. 2013 ECSS/ACSM", "Neck-check rule (File 06 §6E)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // ---- Moderate ----
    EvidenceEntry {
        claim_id: "HYP-FAIL-001",
        statement: "Proximity to failure drives growth continuously; absolute failure not required.",
        grade: Moderate,
        primary_citations: &["Grgic 2022", "Refalo 2023", "Robinson et al. 2024, Sports Med"],
        contradicting: &["Marginal effect small near failure"],
        safety_critical: false,
        contested: Some("CQ-02"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-LENGTH-001",
        statement: "Long-muscle-length training is the strongest recent exercise-selection finding.",
        grade: Moderate,
        primary_citations: &["Maeo 2021/2023", "Pedrosa 2022", "Kassiano 2023"],
        contradicting: &["Some measures site-specific"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-REPLOAD-001",
        statement: "Rep/load by exercise type: heavy compounds 5-10 reps at 75-85% 1RM, moderate compounds/machines 8-15 at 65-75%, isolation 12-25 at 50-70%.",
        grade: Moderate,
        primary_citations: &["File 03 Table 2 (hypertrophy-013/014/015)"],
        contradicting: &["Load interchangeable when effort matched (HYP-LOAD-001)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-FREQ-001",
        statement: "Train each muscle 2-3x/week; >=2x beats 1x by enabling more weekly volume, but frequency's independent effect is negligible when volume is equated.",
        grade: Strong,
        primary_citations: &["Schoenfeld/Ogborn/Krieger 2016", "Schoenfeld/Grgic/Krieger 2019", "Pelland 2024/2025"],
        contradicting: &["3x specifically Moderate"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-REST-001",
        statement: "Rest 2-3 min on compounds, 1-2 min on isolation - long enough to keep >=90% of first-set reps on later sets.",
        grade: Moderate,
        primary_citations: &["Schoenfeld, Pope et al. 2016, JSCR 30(7):1805-1812"],
        contradicting: &["Singer 2024 preprint: ~90s often sufficient, 60s still substantial growth"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-PCT-001",
        statement: "Autoregulation matches or slightly beats fixed % load.",
        grade: Moderate,
        primary_citations: &["Gonzalez-Badillo & Sanchez-Medina 2010", "Banyard 2017"],
        contradicting: &["True 1RM fluctuates +/-18% day-to-day (Jovanovic & Flanagan)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-VL-001",
        statement: "Velocity-loss thresholds are the best-evidenced volume-autoregulation tool.",
        grade: Moderate,
        primary_citations: &["Gonzalez-Badillo/Pareja-Blanco lineage"],
        contradicting: &["File 02 graded both Moderate and Moderate-Strong; reconciled to Moderate"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-APRE-001",
        statement: "APRE next-load adjustment from AMRAP rep count (APRE-3/6/10 tables).",
        grade: Moderate,
        primary_citations: &["Mann et al. 2010, JSCR 24:1718-1723"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "TAPER-STR-001",
        statement: "Tapering yields ~2-6% strength gains.",
        grade: Moderate,
        primary_citations: &["File 02"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-DIST-001",
        statement: "Polarized beats threshold in trained runners; sequence matters.",
        grade: Moderate,
        primary_citations: &["Stoggl & Sperlich 2014, Front Physiol", "Rosenblat 2019", "Filipas 2022"],
        contradicting: &["Phase-dependent; recreational responses mixed"],
        safety_critical: false,
        contested: Some("CQ-07"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-SPIKE-001",
        statement: "Single-session distance spike is the strongest running injury signal (>10% over 30-day longest run).",
        grade: Moderate,
        primary_citations: &["Frandsen et al. 2025, BJSM"],
        contradicting: &["ACWR showed a negative dose-response; week-to-week ratio none"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "CONC-ORDER-001",
        statement: "Resistance-before-endurance modestly favors lower-body strength/power.",
        grade: Moderate,
        primary_citations: &["Eddens 2018"],
        contradicting: &["Effect small"],
        safety_critical: false,
        contested: Some("CQ-15"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "CONC-SEP-001",
        statement: "Session separation >=3h (ideally 6-24h) reduces acute interference.",
        grade: Moderate,
        primary_citations: &["File 07"],
        contradicting: &[],
        safety_critical: false,
        contested: Some("CQ-15"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-CAP-001",
        statement: "Concurrent override caps: keep hard/long runs >=24h from heavy leg days both directions (CAP-3); cap endurance <=3 d/wk when strength/hypertrophy is co-primary (CAP-5); when running >=4 d/wk or >=40 km/wk, cap lower-body lifting <=2/wk and cut lower hypertrophy volume ~20-33% (CAP-1).",
        grade: Moderate,
        primary_citations: &["Jones et al. 2013", "Wilson et al. 2012", "Doma, Deakin & Bentley 2017", "Baar 2014 synthesis"],
        contradicting: &[],
        safety_critical: false,
        contested: Some("CQ-15"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "CONC-MODE-001",
        statement: "Running interferes more than cycling for chronic strength/hypertrophy.",
        grade: Moderate,
        primary_citations: &["Wilson 2012", "Lundberg 2022"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HRV-001",
        statement: "HRV-guided training reduces negative responders / improves submaximal markers.",
        grade: Moderate,
        primary_citations: &["Vesterinen 2016", "Nuuttila 2017", "Granero-Gallegos 2020"],
        contradicting: &["Group-average performance NS (Duking 2021)"],
        safety_critical: false,
        contested: Some("CQ-04"),
        review_months: 6,
    },
    EvidenceEntry {
        claim_id: "WELLNESS-001",
        statement: "Subjective wellness is the best multi-day early-warning tool.",
        grade: Moderate,
        primary_citations: &["Saw et al. 2016"],
        contradicting: &["Never veto a single day on it"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-DELOAD-001",
        statement: "Structure loading in a 3:1 load:recovery cycle (2:1 for older / injury-prone / low training-age), cutting both volume and intensity 20-40% on the recovery week.",
        grade: Moderate,
        primary_citations: &["File 04 periodization synthesis (running-045)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-VDOT-001",
        statement: "VDOT / critical-speed fitness estimates.",
        grade: Moderate,
        primary_citations: &["Daniels & Gilbert", "Jones & Vanhatalo"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-WORKOUT-001",
        statement: "Run workout-type prescriptions (pace/%HRmax/RPE/duration per session type).",
        grade: ExpertOpinion,
        primary_citations: &["Daniels", "Pfitzinger & Douglas"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "LOAD-TRIMP-001",
        statement: "Training-load quantification (TRIMP, TSS/rTSS) is standard and implementable.",
        grade: Moderate,
        primary_citations: &["Banister TRIMP", "Coggan TSS/rTSS"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "DETRAIN-001",
        statement: "Detraining is quality-specific (power fades ~1wk, VO2max fastest, strength most protected ~7-12%/8-12wk); adaptations are largely retained when intensity is held and volume cut. Scale down by shedding accessory volume first and removing intensity/main compounds last (keep >=2 exposures/muscle/wk); ramp re-entry by layoff length rather than restarting.",
        grade: Moderate,
        primary_citations: &["Mujika & Padilla 2000, Sports Med 30(2):79-87", "Bosquet et al. 2013"],
        contradicting: &["Re-entry brackets (Table 3.4b) are ExpertOpinion extrapolation"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "DEFICIT-001",
        statement: "In a caloric deficit preserving lean mass, set protein 1.8-2.7 g/kg bodyweight/day, hold training intensity high, and reduce volume toward MEV rather than cutting intensity.",
        grade: Strong,
        primary_citations: &["Helms et al. 2014 systematic review", "Longland et al. 2016, Am J Clin Nutr"],
        contradicting: &["Volume-sparing ordering itself is Moderate"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MASTERS-001",
        statement: "For masters (65+), target protein >=1.2-1.6 g/kg/day (~0.4 g/kg per meal) for anabolic resistance, include power/velocity work for function, and extend recovery when performance is unrestored.",
        grade: Moderate,
        primary_citations: &["Fragala et al. 2019, JSCR 33(8):2019-2052 (NSCA position)", "Phillips"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "PLYO-001",
        statement: "Cap plyometric volume by training level: beginner 80-100, intermediate 100-120, advanced 120-140 foot contacts/session; progress volume OR intensity, not both. Require landing competence and ~1.5x bodyweight back-squat before high-intensity depth jumps.",
        grade: Moderate,
        primary_citations: &["File 02 strength-032/033 (plyometric programming synthesis)"],
        contradicting: &["Low-dose 40-60 contacts also effective"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "TIMECAP-001",
        statement: "Under time pressure protect frequency + intensity and cut accessory volume first; a muscle can be maintained on ~once-weekly training (maintenance needs far less volume than growth).",
        grade: Moderate,
        primary_citations: &["File 08 §1.6", "Mujika & Padilla 2000"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // ---- Expert opinion ----
    EvidenceEntry {
        claim_id: "DBLPROG-001",
        statement: "Double progression: work within a rep range and add load (dropping to the range bottom) once the top of the range is hit on all sets. Novice linear default adds ~+2.5 kg upper / +5 kg lower per session while reps are completed.",
        grade: ExpertOpinion,
        primary_citations: &["Rippetoe & Kilgore, Practical Programming", "File 08 §3.1"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "ENV-001",
        statement: "Environment modifiers: in heat reduce intensity/pace, acclimatize ~10-14 days, hydrate, and STOP on heat-illness signs (confusion, no sweating, dizziness); at altitude >~2,500 m reduce absolute intensity until acclimatized; in cold extend warm-up.",
        grade: ExpertOpinion,
        primary_citations: &["File 08 §1.5"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SUBST-001",
        statement: "For home/minimal equipment substitute the movement pattern (not the exact lift) and compensate lighter loads with higher reps taken closer to failure; hypertrophy is largely preserved near failure.",
        grade: ExpertOpinion,
        primary_citations: &["File 08 §1.5 (Starting Strength substitution convention)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-LANDMARKS-001",
        statement: "MEV/MAV/MRV volume landmarks.",
        grade: ExpertOpinion,
        primary_citations: &["Renaissance Periodization (Israetel/Hoffmann)"],
        contradicting: &["Not validated constants"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-RIR-RAMP-001",
        statement: "Ramp RIR down across a 4-week accumulation block (W1=4, W2=3, W3=2, W4=1), then deload.",
        grade: ExpertOpinion,
        primary_citations: &["Renaissance Periodization (rpstrength.com 2018)"],
        contradicting: &["Consistent with, not established by, Robinson 2024"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // ---- Weak ----
    EvidenceEntry {
        claim_id: "RUN-GAP-001",
        statement: "Grade-adjusted pace (downhill).",
        grade: Weak,
        primary_citations: &["Minetti et al. 2002, J Appl Physiol"],
        contradicting: &["Errs up to ~3x on steep downhill; Strava switched to HR-equivalency 2017"],
        safety_critical: false,
        contested: Some("CQ-08"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-FORM-001",
        statement: "Running power + form metrics (GCT, vertical oscillation, Stryd/Garmin).",
        grade: Weak,
        primary_citations: &["Proprietary / mechanistic"],
        contradicting: &["Not RCT-backed"],
        safety_critical: false,
        contested: Some("CQ-13"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-DECOUPLE-001",
        statement: "Aerobic decoupling (Pa:HR).",
        grade: Weak,
        primary_citations: &["Friel (<5% good, >10% flag)"],
        contradicting: &["Confounded by heat/dehydration"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-HRMAX-001",
        statement: "HRmax default (Tanaka formula 208 - 0.7*age).",
        grade: Weak,
        primary_citations: &["Tanaka formula"],
        contradicting: &["SEE ~ +/-10 bpm - individual"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-MAF-001",
        statement: "Maffetone aerobic-base HR cap = 180 - age with adjustments (+5 elite/2+yr injury-free improving; -5 returning from injury-illness or 2+ colds/yr; -10 chronically overtrained/sedentary/on meds). Base-phase OPTION, not default; personalize toward measured LT1 when data exist.",
        grade: Weak,
        primary_citations: &["Maffetone (unstated year)"],
        contradicting: &["Marathon Handbook; Strength Running (specific 180-age not individually validated)"],
        safety_critical: false,
        contested: Some("CQ-03"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-PROGRESS-001",
        statement: "Weekly-distance increase >30% over two weeks flags elevated injury risk (~1.6x vs <10%). Baseline safe ramp 5-10%/wk; novices <=10% (hold 2-3 wk between bumps), experienced ~5% or an absolute cap. Progress one variable at a time.",
        grade: Weak,
        primary_citations: &["Nielsen et al. 2014 (>30%/2wk ~1.6x risk)"],
        contradicting: &["10%/week rule failed its RCT (Buist 2008)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-10PCT-001",
        statement: "10%/week mileage rule.",
        grade: Weak,
        primary_citations: &["unstated"],
        contradicting: &["Failed RCT (Buist 2008); Nielsen"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FEM-MENSTRUAL-001",
        statement: "Menstrual-cycle-based periodization.",
        grade: Weak,
        primary_citations: &["unstated"],
        contradicting: &["McNulty et al. 2020 (78 studies; trivial, highly variable)"],
        safety_critical: false,
        contested: Some("CQ-09"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "READY-CONSUMER-001",
        statement: "Consumer readiness scores (Whoop / Oura / Garmin Body Battery).",
        grade: Weak,
        primary_citations: &["Proprietary"],
        contradicting: &["Black-box algorithms"],
        safety_critical: false,
        contested: Some("CQ-14"),
        review_months: 12,
    },
    // ---- Marketing myths (hard-blocked) ----
    EvidenceEntry {
        claim_id: "LOAD-ACWR-001",
        statement: "Acute:chronic workload ratio predicts injury risk with a 0.8-1.3 'sweet spot'.",
        grade: MarketingMyth,
        primary_citations: &["Gabbett 2016, BJSM"],
        contradicting: &["Impellizzeri et al. 2019 (retraction request)", "Lolli et al. 2019 (mathematical coupling)"],
        safety_critical: false,
        contested: Some("CQ-05"),
        review_months: 6,
    },
    EvidenceEntry {
        claim_id: "RUN-CAD-180",
        statement: "All runners should run at 180 steps per minute.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Daniels 1984 observation misread as universal target"],
        safety_critical: false,
        contested: None,
        review_months: 24,
    },
    EvidenceEntry {
        claim_id: "MYTH-POSITIVITY",
        statement: "The 2.9:1 'positivity ratio' governs feedback.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Brown, Sokal & Friedman 2013; American Psychologist 2013 correction"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-10PCT",
        statement: "10% weekly mileage rule as a hard injury predictor.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Buist 2008 (failed RCT); Nielsen"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-LIFTING-SLOW",
        statement: "Lifting makes runners slow / bulky.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Blagrove, Howatson & Hayes 2018"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-MUST-FAIL",
        statement: "You must train to failure for growth.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Grgic 2022; Refalo 2023; Robinson et al. 2024"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-MENSTRUAL-MANDATORY",
        statement: "Mandatory menstrual-cycle-phase periodization.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["McNulty et al. 2020"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-TONING-SPOT",
        statement: "'Toning' vs 'bulking' / spot reduction.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Ramirez-Campillo et al. 2022; Vispute et al. 2011"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-MUSCLE-CONFUSION",
        statement: "'Muscle confusion' drives adaptation.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Progressive-overload RCT literature"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-PROTEIN-WINDOW",
        statement: "Anabolic / 30-minute protein window.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Aragon & Schoenfeld 2013; Schoenfeld/Aragon/Krieger 2013 meta"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-LACTIC-DOMS",
        statement: "Lactic acid causes DOMS.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Cheung, Hume & Maxwell 2003"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-STATIC-STRETCH",
        statement: "Static stretching prevents injury / boosts performance.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Lauersen, Bertelsen & Andersen 2014 (25 RCTs)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-FAT-BURN-ZONE",
        statement: "The 'fat-burning zone'.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["ACSM position; Treuth et al."],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-FASTED-CARDIO",
        statement: "Fasted cardio is superior for fat loss.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Schoenfeld et al. 2014; Hackett & Hagstrom 2017 meta"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-BAREFOOT",
        statement: "Barefoot/minimalist running universally reduces injury.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Ryan et al. 2014; multiple systematic reviews"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-BMI-COMP",
        statement: "BMI = body-composition truth.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Itani et al. (DXA; 24.9% of athletes misclassified)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-NO-PAIN-JOINT",
        statement: "'No pain, no gain' applied to joint pain.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Musculoskeletal safety consensus - joint pain is a stop signal"],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-WOMEN-HEAVY",
        statement: "Women shouldn't lift heavy.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Rhea et al. 2003; Hagstrom 2020"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-SWEAT",
        statement: "More sweat = better workout.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Thermoregulation physiology"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-WAIST-TRAINER",
        statement: "Waist trainers / spot fat loss devices.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["Ramirez-Campillo 2022; Vispute 2011"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MYTH-HR-ZONE-DOGMA",
        statement: "Heart-rate-zone dogmatism.",
        grade: MarketingMyth,
        primary_citations: &[],
        contradicting: &["File 04; Tanaka (HRmax individual, SEE ~ +/-10 bpm)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
];

/// Contested questions (File 09). Engine holds a default lean while open.
pub static CONTESTED_QUESTIONS: &[ContestedQuestion] = &[
    ContestedQuestion { id: "CQ-01", question: "Hypertrophy volume ceiling", engine_default: "10-20 sets/muscle/wk" },
    ContestedQuestion { id: "CQ-02", question: "Train to failure?", engine_default: "0-3 RIR" },
    ContestedQuestion { id: "CQ-03", question: "Periodization model superiority", engine_default: "Auto-DUP, any structured plan accepted" },
    ContestedQuestion { id: "CQ-04", question: "HRV-guided training value", engine_default: "Gate hard/easy only" },
    ContestedQuestion { id: "CQ-05", question: "ACWR validity", engine_default: "DO NOT use ACWR as injury predictor" },
    ContestedQuestion { id: "CQ-06", question: "Interference real-world magnitude", engine_default: "Protect power/explosive only" },
    ContestedQuestion { id: "CQ-07", question: "Runner intensity distribution", engine_default: "Pyramidal base -> polarized peak" },
    ContestedQuestion { id: "CQ-08", question: "Grade-adjusted-pace downhill validity", engine_default: "Trust uphill; soften/flag downhill" },
    ContestedQuestion { id: "CQ-09", question: "Menstrual-cycle periodization", engine_default: "Symptom-based optional adjustment" },
    ContestedQuestion { id: "CQ-10", question: "Optimal interval length", engine_default: "Menu selected by goal/event" },
    ContestedQuestion { id: "CQ-11", question: "Marathon philosophy", engine_default: "Aerobic base -> specific block" },
    ContestedQuestion { id: "CQ-12", question: "MAF 180-formula vs measured LT1", engine_default: "Measured LT1 when available, else MAF fallback" },
    ContestedQuestion { id: "CQ-13", question: "Running power / form-metric value", engine_default: "Display only, never prescribe" },
    ContestedQuestion { id: "CQ-14", question: "Consumer readiness-score trust", engine_default: "3-band GO/CAUTION/REST" },
    ContestedQuestion { id: "CQ-15", question: "Concurrent session order & separation", engine_default: "RT-first for strength; >=3h (ideally 6-24h) gap" },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn claim_ids_are_unique() {
        let mut seen = HashSet::new();
        for c in CLAIMS {
            assert!(seen.insert(c.claim_id), "duplicate claim_id: {}", c.claim_id);
        }
    }

    #[test]
    fn every_contested_ref_resolves() {
        for c in CLAIMS {
            if let Some(cq) = c.contested {
                assert!(
                    contested_question(cq).is_some(),
                    "{} references missing {}",
                    c.claim_id,
                    cq
                );
            }
        }
    }

    #[test]
    fn safety_critical_claims_are_strong_or_blocked() {
        // Safety deferrals are Strong; the one safety-critical myth is the
        // joint-pain "no pain no gain" hard-block. Nothing else may claim
        // safety authority on weak evidence.
        for c in CLAIMS.iter().filter(|c| c.safety_critical) {
            assert!(
                matches!(c.grade, EvidenceGrade::Strong | EvidenceGrade::MarketingMyth),
                "{} is safety_critical but graded {:?}",
                c.claim_id,
                c.grade
            );
        }
    }

    #[test]
    fn lookup_and_confidence() {
        let c = claim("TAPER-001").expect("TAPER-001 present");
        assert_eq!(c.grade, EvidenceGrade::Strong);
        assert!((c.confidence_score() - 0.90).abs() < f32::EPSILON);
        assert!(claim("NOPE-999").is_none());
    }

    #[test]
    fn myths_are_blocked() {
        let acwr = claim("LOAD-ACWR-001").expect("present");
        assert!(acwr.is_blocked());
        assert!(!claim("TAPER-001").unwrap().is_blocked());
    }

    #[test]
    fn builds_schema_evidence_and_confidence() {
        let c = claim("RUN-DIST-001").expect("present");
        let ev = c.to_evidence();
        assert_eq!(ev.citation.claim_id.as_deref(), Some("RUN-DIST-001"));
        assert_eq!(ev.grade, EvidenceGrade::Moderate);

        let tag = c.to_confidence_tag();
        assert!(tag.contested);
        assert_eq!(tag.contested_question_ref.as_deref(), Some("CQ-07"));
        assert!((tag.score - 0.65).abs() < f32::EPSILON);
    }

    #[test]
    fn fast_moving_topics_review_semiannually() {
        for id in ["HYP-VOL-001", "HRV-001", "LOAD-ACWR-001"] {
            assert_eq!(claim(id).unwrap().review_months, 6, "{id} should review at 6mo");
        }
    }
}
