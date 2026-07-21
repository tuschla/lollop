//! Canonical evidence registry (knowledge-base File 09 + per-rule claims from
//! Files 02–08 and 10).
//!
//! Compile-time table of every graded claim + contested question the engine may
//! cite. Data only, no coaching logic. Every recommendation the core emits must
//! reference a `claim_id` that exists here (see [`claim`]); `MarketingMyth`
//! claims are hard-blocked and must never be surfaced as advice.
//!
//! The File 09 canonical claims keep their original statements untouched. Where
//! an engine rule comes from a specific KB rule whose grade/citation/safety
//! metadata differs from the nearest File 09 claim, a *per-rule* entry exists
//! (e.g. `STR-PAP-001` for File 02 strength-034) so call sites never over- or
//! under-state the source rule's grade.
//!
//! Contested-question convention: bare `CQ-##` ids always mean File 09's
//! GLOBAL contested-question index (CQ-01…CQ-15). Files 02–08/10 number their
//! local contested lists independently; when a local question corresponds to a
//! global one, the global id is used (File 02 block-periodization "CQ-02" →
//! global CQ-03; File 04 interval-length "CQ-06" → global CQ-10; File 10 ACWR
//! "CQ-04" → global CQ-05, HRV "CQ-05" → global CQ-04, interference "CQ-02/03"
//! → global CQ-06). A file-local question with no global counterpart gets a
//! namespaced id `CQ-F<file>-<local>` (e.g. `CQ-F03-04`) and its own
//! [`ContestedQuestion`] row, a global number is never reused for a different
//! question.
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
        // HARD RULE 2: a `MarketingMyth` claim must never reach a surfaced
        // recommendation. Every `recommend`/`graded` wrapper funnels through
        // here, so an UNCONDITIONAL guard at this choke point (release builds
        // included) turns a myth-id leak into a loud panic instead of shipped
        // advice. `Recommended::new` enforces the same invariant structurally.
        assert!(
            !self.is_blocked(),
            "MarketingMyth claim {} must never be surfaced as a recommendation",
            self.claim_id,
        );
        Evidence {
            grade: self.grade,
            citation: Citation {
                claim_id: Some(self.claim_id.to_string()),
                reference: self
                    .primary_citations
                    .first()
                    .copied()
                    .unwrap_or("unstated")
                    .to_string(),
            },
            contradicting: self
                .contradicting
                .iter()
                .map(|r| Citation {
                    claim_id: Some(self.claim_id.to_string()),
                    reference: r.to_string(),
                })
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
        primary_citations: &[
            "Schoenfeld, Ogborn & Krieger 2017, J Sports Sci",
            "Pelland et al. 2025, Sports Med",
        ],
        contradicting: &[
            "Undetectable superiority beyond ~31 fractional weekly sets (Pelland 2025)",
        ],
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
        claim_id: "STR-PRILEPIN-001",
        statement: "Cap per-session rep totals per %1RM zone using Prilepin's chart as a volume ceiling, verifying intensity via bar speed/technique.",
        grade: Moderate,
        primary_citations: &[
            "A.S. Prilepin (Soviet weightlifting, >1,000 elite lifters, 1960s–70s)",
        ],
        contradicting: &[
            "Derived from Olympic lifts/elite lifters; apply to powerlifts/novices with caution (CQ-03)",
        ],
        safety_critical: false,
        contested: Some("CQ-03"),
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
        primary_citations: &[
            "Deci, Koestner & Ryan 1999",
            "Carpentier & Mageau 2013",
            "Mouratidis 2010",
        ],
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
    // ---- Safety (non-negotiable deferrals / stop signals) ----
    // File 09 assigns Strong only to the five referral deferrals
    // (REDS/OTS/BSI/PREG/CVD). The pain and illness stop rules come from File 06
    // (autoreg-043/045/046), which grades them ExpertOpinion (0.30), safety-
    // critical on consensus authority, not trial evidence. Grades are never
    // overstated to make a stop signal look better-evidenced than it is.
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
        grade: ExpertOpinion,
        primary_citations: &[
            "unstated (File 06 §6C safety guardrail, autoreg-043 - never ramp load into pain)",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "ILLNESS-NECK-001",
        statement: "Febrile illness contraindicates strenuous exercise (myocarditis / sudden-cardiac-death risk): any fever or below-neck symptoms = no training. Above-neck-only symptoms without fever permit reduced-intensity training (neck-check operationalization).",
        grade: ExpertOpinion,
        primary_citations: &[
            "Neck-check rule / sports-medicine standard (File 06 §6E, autoreg-045/046)",
            "Meeusen et al. 2013 ECSS/ACSM",
        ],
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
        primary_citations: &[
            "Grgic 2022",
            "Refalo 2023",
            "Robinson et al. 2024, Sports Med",
        ],
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
        primary_citations: &[
            "Schoenfeld/Ogborn/Krieger 2016",
            "Schoenfeld/Grgic/Krieger 2019",
            "Pelland 2024/2025",
        ],
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
        contradicting: &[
            "Singer 2024 preprint: ~90s often sufficient, 60s still substantial growth",
        ],
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
        contradicting: &[
            "File 02 graded both Moderate and Moderate-Strong; reconciled to Moderate",
        ],
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
        primary_citations: &[
            "Stoggl & Sperlich 2014, Front Physiol",
            "Rosenblat 2019",
            "Filipas 2022",
        ],
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
        primary_citations: &[
            "Jones et al. 2013",
            "Wilson et al. 2012",
            "Doma, Deakin & Bentley 2017",
            "Baar 2014 synthesis",
        ],
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
        primary_citations: &["File 04 periodization synthesis (running-033)"],
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
        // File 07 "Banister/Calvert impulse-response": CTL = EWMA of daily
        // TSS/TRIMP tau=42d, ATL tau=7d, TSB = CTL - ATL (yesterday's).
        // [Moderate] for CTL/ATL bookkeeping, [Weak] for prediction: graded
        // at the bookkeeping level here; the statement carries the caveat so
        // no caller can present TSB as a validated performance predictor.
        claim_id: "LOAD-PMC-001",
        statement: "CTL/ATL/TSB fitness-fatigue bookkeeping: CTL = EWMA of daily load (tau 42 d), ATL = EWMA (tau 7 d), TSB = CTL - ATL (yesterday's). Moderate as bookkeeping only; Weak for predicting performance.",
        grade: Moderate,
        primary_citations: &["Banister/Calvert impulse-response (File 07)", "Coggan PMC"],
        contradicting: &["[Weak] for prediction (File 07)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        // File 07 "Cooper test": VO2max ~= (d_meters - 504.9)/44.73 from a
        // 12-min max run. [Moderate]
        claim_id: "LOAD-COOPER-001",
        statement: "Cooper 12-min test estimates VO2max as (distance_m - 504.9)/44.73.",
        grade: Moderate,
        primary_citations: &["Cooper 1968 (File 07 formulas)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "DETRAIN-001",
        statement: "Detraining is quality-specific (power fades ~1wk, VO2max fastest, strength most protected ~7-12%/8-12wk); adaptations are largely retained when intensity is held and volume cut. Scale down by shedding accessory volume first and removing intensity/main compounds last (keep >=2 exposures/muscle/wk); ramp re-entry by layoff length rather than restarting.",
        grade: Moderate,
        primary_citations: &[
            "Mujika & Padilla 2000, Sports Med 30(2):79-87",
            "Bosquet et al. 2013",
        ],
        contradicting: &["Re-entry brackets (Table 3.4b) are ExpertOpinion extrapolation"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "DEFICIT-001",
        statement: "In a caloric deficit preserving lean mass, set protein 1.8-2.7 g/kg bodyweight/day, hold training intensity high, and reduce volume toward MEV rather than cutting intensity.",
        grade: Strong,
        primary_citations: &[
            "Helms et al. 2014 systematic review",
            "Longland et al. 2016, Am J Clin Nutr",
        ],
        contradicting: &["Volume-sparing ordering itself is Moderate"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "MASTERS-001",
        statement: "For masters (65+), target protein >=1.2-1.6 g/kg/day (~0.4 g/kg per meal) for anabolic resistance, include power/velocity work for function, and extend recovery when performance is unrestored.",
        grade: Moderate,
        primary_citations: &[
            "Fragala et al. 2019, JSCR 33(8):2019-2052 (NSCA position)",
            "Phillips",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "PLYO-001",
        statement: "Cap plyometric volume by training level: beginner 80-100, intermediate 100-120, advanced 120-140 foot contacts/session; progress volume OR intensity, not both. Require landing competence and ~1.5x bodyweight back-squat before high-intensity depth jumps.",
        grade: Moderate,
        primary_citations: &["Potash & Chu 2008 (NSCA/UKSCA)", "Verkhoshansky"],
        contradicting: &["Low-dose 40-60 contacts also effective"],
        // File 02 strength-032 marks the contact caps safety_critical.
        safety_critical: true,
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
        primary_citations: &["File 08 §1.5 (safety-024 heat STOP; indiv-025 altitude/cold)"],
        contradicting: &[],
        // File 08 safety-024: heat-illness signs (confusion, no sweating,
        // dizziness) are a hard STOP.
        safety_critical: true,
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
        contradicting: &[
            "Errs up to ~3x on steep downhill; Strava switched to HR-equivalency 2017",
        ],
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
        contradicting: &[
            "Marathon Handbook; Strength Running (specific 180-age not individually validated)",
        ],
        safety_critical: false,
        // Global CQ-12 (MAF 180-formula vs measured LT1), not File 04's local
        // numbering.
        contested: Some("CQ-12"),
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
    // ---- Per-rule claims (Files 02-08, 10) ----
    // Precise entries for engine rules whose source grade/citation/safety
    // metadata differs from the nearest File 09 claim. File 09 canonical
    // entries above stay untouched for their original statements.
    //
    // -- File 02 (strength & power) --
    EvidenceEntry {
        claim_id: "STR-PWR-001",
        statement: "Power/explosive work at maximal concentric velocity across a load spectrum (0-60% ballistic/jump, 30-70% loaded power, 70-95% weightlifting pulls), 1-5 reps stopped before velocity decay, 3-6 sets, never to failure.",
        grade: Moderate,
        primary_citations: &["Cormie et al. 2011", "Suchomel", "Kawamori & Haff 2004"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-2FOR2-001",
        statement: "2-for-2 rule: increase load (within capped increments) once the athlete completes >=2 reps over goal on the last set in 2 consecutive sessions.",
        grade: ExpertOpinion,
        primary_citations: &[
            "Graves & Baechle; NSCA Essentials of Strength Training and Conditioning",
        ],
        contradicting: &[
            "File 02 strength-012 grades ExpertOpinion/Moderate (0.40); registered conservatively as ExpertOpinion",
        ],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-STALL-001",
        statement: "Switch periodization model or insert a deload once (estimated) 1RM stalls 2-3 weeks despite good recovery.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 02 strength-039)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-MODEL-001",
        statement: "Select periodization model by athlete level: novice linear; intermediate DUP or block; advanced block or conjugate.",
        grade: Moderate,
        primary_citations: &["unstated (File 02 strength-010 synthesis)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-LINEAR-001",
        statement: "Linear periodization: high-volume/low-intensity -> low-volume/high-intensity across mesocycles for novices to early intermediates.",
        grade: Moderate,
        primary_citations: &["Matveyev", "Bompa, Periodization", "NSCA"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-BLOCK-001",
        statement: "Block periodization: accumulation (~65-80%, 3-5x6-10) -> transmutation (~80-90%, 3-6x3-6) -> realization (peak/taper, 1-3 reps) in concentrated unidirectional blocks.",
        grade: Moderate,
        primary_citations: &[
            "Verkhoshansky",
            "Issurin 2008/2016",
            "Painter et al. 2012 IJSPP 7(2):161-169",
            "Bartolomei et al. 2014 JSCR 28(4):990-997",
        ],
        contradicting: &["No meta-analysis confirms block > traditional for max strength"],
        safety_critical: false,
        contested: Some("CQ-03"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-PAP-001",
        statement: "PAP/PAPE contrast pairing: 5-7 min rest after heavy conditioning activities (optimal window ~3-7 min); abort if the explosive set is slower than baseline.",
        grade: Moderate,
        primary_citations: &[
            "Seitz & Haff 2016, Sports Med 46(2):231-240",
            "Wilson et al. 2013 JSCR",
            "Docherty et al.",
        ],
        contradicting: &["Too-short rest hurts performance (fatigue > potentiation)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-OLY-001",
        statement: "Olympic-lift pulling derivatives: 3-5 sets x 1-3 reps at 85-100%+ of full-lift 1RM (lighter for velocity-biased variants), placed early in the session.",
        grade: Moderate,
        primary_citations: &["Suchomel et al."],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-DLPEAK-001",
        statement: "Schedule the last true near-max deadlift 10-14 days out from a test/meet (high systemic fatigue and recovery cost).",
        grade: Moderate,
        primary_citations: &["Travis et al. 2020/2021"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-BACKOFF-001",
        statement: "RPE-anchored top set with back-off sets at a fixed % drop (top set ~RPE 8, back-offs -10 to -15%).",
        grade: Moderate,
        primary_citations: &["unstated (File 02 strength-015)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // -- File 03 (hypertrophy) --
    EvidenceEntry {
        claim_id: "HYP-MEV-AGE-001",
        statement: "MEV scales with training age: beginners ~6-10, intermediates ~10-18, advanced ~12-20+ sets/muscle/week, individualized.",
        grade: Weak,
        primary_citations: &["RP framework (Israetel & Hoffmann), unstated primary"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-MESO-ADD-001",
        statement: "If a muscle is not growing while recovery is easy, add +2 sets/week next mesocycle (currently below MEV).",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-008)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-MRV-DELOAD-001",
        statement: "Weekly sets > ~20/muscle with regressing performance or aching joints = at/over MRV; deload.",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-009)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-RECOVOL-001",
        statement: "In a caloric deficit or under poor sleep/high stress, cut weekly volume 20-30% and reduce failure frequency; high recovery tolerates higher MRV.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-010/045)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SETRAMP-001",
        statement: "RP set-progression: start at MEV and add sets weekly toward MRV (e.g. 10->13->16->20) while ramping RIR 4->1.",
        grade: ExpertOpinion,
        primary_citations: &["Renaissance Periodization worked example"],
        contradicting: &[
            "Enes, De Souza & Souza-Junior 2024, MSSE 56(3):553-563 (no significant hypertrophy difference vs constant volume)",
        ],
        safety_critical: false,
        contested: Some("CQ-F03-04"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-DELOAD-TRIG-001",
        statement: "Deload when >=2 triggers co-occur (performance decrement, unintended RIR drift to 0, persistent joint/tendon aches, disrupted sleep, elevated RHR, mood/motivation drop) or on the preplanned 4-8-week schedule.",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-035)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-DELOAD-RX-001",
        statement: "Deload = one week at ~MV (roughly half the sets), 2-4+ RIR, loads ~60-70% of working weight, movement patterns kept.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-036)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SPLIT-001",
        statement: "Split any weekly target above ~12 sets/muscle across >=2 sessions.",
        grade: Moderate,
        primary_citations: &["Remmert 2025", "Krieger/Weightology analyses"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SESSCAP-001",
        statement: "Cap per-session volume at ~11 fractional sets/muscle; beyond that redistribute to another session rather than adding sets.",
        grade: Moderate,
        primary_citations: &[
            "Remmert, Pelland, Robinson, Hinson & Zourdos 2025, SportRxiv (PUOS analysis)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-PAIN-SHIFT-001",
        statement: "On joint pain at heavy loads, shift that muscle's work to higher reps (12-25) at lighter load; hypertrophy is preserved (load interchangeability).",
        grade: Strong,
        primary_citations: &["Schoenfeld/Grgic/Ogborn/Krieger 2017 (load-interchangeability basis)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // File 03 rule claims, task 17
    EvidenceEntry {
        claim_id: "HYP-LOADRANGE-001",
        statement: "Hypertrophy is equivalent across ~30-85% 1RM (~5-30+ reps) when sets are taken close to failure; load is interchangeable for growth (heavy favors strength).",
        grade: Strong,
        primary_citations: &[
            "Schoenfeld/Grgic/Ogborn/Krieger 2017 load meta",
            "Morton 2016",
            "Lasevicius 2018",
        ],
        contradicting: &[
            "Older ACSM 6-12 rep / 70-85% 1RM guidance",
            "Very light loads (<~30% 1RM) underperform",
        ],
        safety_critical: false,
        contested: Some("CQ-F03-02"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-VOLRAMP-SAFE-001",
        statement: "Ramp weekly volume gradually from a low base and never jump straight to MRV; rapid week-over-week volume jumps raise injury risk.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-011, SAFETY item)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SKILL-RIR-001",
        statement: "For high-skill/high-stability exercises, keep reps >=5 and stop at >=1-2 RIR to protect technique.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-017, SAFETY item)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-RIR-DEFAULT-001",
        statement: "Train most sets at 1-3 RIR; growth rises continuously as sets approach failure, but true failure is neither required nor superior enough to justify its fatigue.",
        grade: Moderate,
        primary_citations: &[
            "Robinson et al. 2024, Sports Medicine 54(9):2209-31",
            "Grgic 2022",
            "Refalo et al. 2023, Sports Medicine 53(3):649-65",
        ],
        contradicting: &[
            "Refalo 2023 trivial failure advantage ES=0.19 (CI 0.00,0.37)",
            "Consistent-failure advocates",
        ],
        safety_critical: false,
        contested: Some("CQ-02"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-RIR-ACC-001",
        statement: "Treat RIR as accurate to ~+/-1 rep only when close to failure (0-5 RIR); error exceeds 2 reps far from failure, so novices should start at 3-4 RIR and calibrate against actual failure.",
        grade: Moderate,
        primary_citations: &["Hackett 2017", "Zourdos 2016/2021", "Refalo/Remmert 2023"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-FAIL-SAFE-001",
        statement: "Reserve 0 RIR / true failure for machines and isolation where failure is safe; avoid failure on heavy free-weight compounds (e.g., unspotted squat/bench) due to injury risk.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-021, SAFETY item)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-CUT-OBJ-001",
        statement: "On a cut, trust rep count and bar speed over perceived effort because RPE inflates in a deficit.",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-022)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-VEL-CHECK-001",
        statement: "Use last-rep bar-speed slowdown as an objective cross-check on proximity to failure.",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-023)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SFR-001",
        statement: "Select exercises maximizing stimulus-to-fatigue ratio (SFR): favor machines, cables, and stable isolation; reserve lower-SFR heavy deadlifts and high-skill free weights for compound stimulus/strength.",
        grade: ExpertOpinion,
        primary_citations: &["Israetel (Renaissance Periodization)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-LENGTHSEL-001",
        statement: "Bias exercise selection toward long muscle lengths (lengthened positions/partials), e.g., seated over prone leg curl (+14% vs +9% hamstrings) and overhead over pushdown triceps.",
        grade: Moderate,
        primary_citations: &[
            "Maeo 2021 MSSE 53(4):825-837",
            "Maeo 2023",
            "Pedrosa 2022",
            "Kassiano 2023",
        ],
        contradicting: &[
            "Effect shrinks in trained-subject replications (Wolf 2025); most studies untrained",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SUBST-001",
        statement: "Substitute exercises by filtering to same primary muscle and available equipment, then ranking by long-length bias, SFR, and stability; fall back to bodyweight variants if no equipment match.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-029, substitution algorithm)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-PAIN-SWAP-001",
        statement: "On movement-specific joint pain, substitute a same-muscle exercise with higher stability or a different resistance profile.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-030, SAFETY item)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-MESO-STRUCT-001",
        statement: "Structure mesocycles as 4-6 weeks accumulation plus 1 deload week (3:1 to 6:1), deloading every 4-8 weeks.",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-031, Expert-opinion/Moderate)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-DOUBLEPROG-001",
        statement: "Drive overload via double progression: fix a rep range + RIR (e.g., 10-15 @ 2-0 RIR), add reps weekly, then increase load by the smallest increment (~2.5 kg/5 lb) and restart at the range bottom, holding volume constant.",
        grade: ExpertOpinion,
        primary_citations: &["Eric Helms / 3DMJ, Muscle & Strength Pyramid"],
        contradicting: &[],
        safety_critical: false,
        contested: Some("CQ-F03-04"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-LAYOFF-001",
        statement: "After a layoff, restart at lower MEV because landmarks are temporarily reduced.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-037)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-TEMPO-001",
        statement: "Use controlled rep tempos of ~0.5-8 s (e.g., 1-2 s concentric, 2-3 s eccentric); tempo has minimal effect on hypertrophy. Avoid superslow >10 s/rep because it forces load reduction.",
        grade: Moderate,
        primary_citations: &[
            "Schoenfeld/Ogborn/Krieger 2015 tempo meta",
            "2025 meta-analyses",
        ],
        contradicting: &["Superslow/HIT time-under-tension proponents (unsupported)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SUPERSET-001",
        statement: "When time-limited, use antagonist/non-competing supersets to save time without cutting rest for the working muscle (accept small volume loss at >=90 s).",
        grade: Moderate,
        primary_citations: &["unstated (File 03 hypertrophy-041)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-DEFAULT-PROG-001",
        statement: "Intermediate default program: each muscle 2x/week at MEV (~8-10 sets/week, <=~8/session), compounds 5-10 reps and isolation 10-20 reps, Week1 at 3 RIR descending ~1 RIR/week to 1 RIR, rest 2-3 min compounds / 1-2 min isolation, controlled tempo, 1-2 high-SFR/long-length exercises per muscle, deload week 5-6.",
        grade: Moderate,
        primary_citations: &["File 03 synthesis (hypertrophy-043, mixed grades)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYP-SPEC-BLOCK-001",
        statement: "For advanced trainees not responding to ~15 sets, run a specialization block: raise one muscle's volume and drop others to MV.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 03 hypertrophy-044)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // end File 03 rule claims, task 17
    // -- File 04 (running) --
    EvidenceEntry {
        claim_id: "RUN-HRRECALC-001",
        statement: "Prefer a measured max HR (all-out field test) over age formulas; recalculate HR zones every 4-6 weeks.",
        grade: Strong,
        primary_citations: &[
            "Sarzynski et al. HERITAGE",
            "Tanaka, Monahan & Seals 2001 (351 studies/18,712 subjects)",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-SPIKE-BLOCK-001",
        statement: "HARD RULE: block/flag any single run >10% longer than the longest run of the prior 30 days - single-session distance spikes drive overuse injury.",
        grade: Moderate,
        primary_citations: &[
            "Frandsen et al. 2025, BJSM 59(17):1203-1210 (n=5,205; 588,071 sessions)",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-TAPER-001",
        statement: "Race taper: 2 weeks, exponential volume cut 41-60%, intensity and frequency held, never add new stimulus; distance-specific defaults (5K/10K 7-10 d ~40-50%, HM 10-14 d ~50%, marathon 2-3 wk 40-60%) keep the same shape.",
        grade: Strong,
        primary_citations: &[
            "Bosquet et al. 2007, MSSE 39(8):1358-65 (27-study meta-analysis)",
            "Mujika & Padilla",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-QUALITY-001",
        statement: "Cap quality sessions at 2-3/week spaced >=48 h; never two Z3 sessions on consecutive days for non-elites.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 04 running-023)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-C25K-001",
        statement: "Couch-to-5K beginners: 3 run/walk sessions/week over 9 (extendable to 10-12) weeks at conversational effort with rest days between; repeat a too-hard week without penalty.",
        grade: ExpertOpinion,
        primary_citations: &["Josh Clark 1996; NHS version"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-DOWNWEEK-001",
        statement: "Insert an unscheduled down week when >=2 overtraining signals fire (RHR +5-7 bpm >=3 days, HRV downtrend, rising RPE, disrupted sleep/soreness/mood, standard-workout performance down >3-5%).",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 04 running-034)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-VOLCAP-001",
        statement: "Daniels weekly-share volume caps: long run <=25-30% of weekly volume (time-cap ~2:00-2:30), threshold <=10%, intervals <=8%, repetitions <=5%.",
        grade: ExpertOpinion,
        primary_citations: &["Daniels' Running Formula (E/M/T/I/R weekly caps)", "Pfitzinger & Douglas"],
        contradicting: &[
            "T/I band caps are Moderate via running-018/019; the long-run share is ExpertOpinion (running-016) - registered at the ExpertOpinion floor",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-TEMPO-001",
        statement: "Tempo/threshold: T pace (~15K-HM, ~90% MP), 88-92% HRmax, RPE 6-7, 20-40 min continuous or cruise intervals; threshold total <=10% of weekly volume.",
        grade: Moderate,
        primary_citations: &["Daniels", "Pfitzinger"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-INTERVAL-001",
        statement: "VO2max intervals: I pace (~3K-5K), 95-100% HRmax, RPE 8-9, 3-5 min reps (800-1600 m) with recovery ~= rep time; interval total <=8% of weekly volume.",
        grade: Moderate,
        primary_citations: &["Daniels"],
        contradicting: &["Ronnestad: short intervals may be superior for VO2max"],
        safety_critical: false,
        contested: Some("CQ-10"),
        review_months: 12,
    },
    // -- File 05 (feedback & communication) --
    EvidenceEntry {
        claim_id: "FB-PACING-001",
        statement: "A positive split >~3% on an even-effort run earns a pacing-discipline cue: start easier toward an even-to-negative split.",
        grade: Moderate,
        primary_citations: &[
            "Hanley 2016, J Sports Sci 34(17):1637-1645",
            "Smyth 2018",
            "Abbiss & Laursen 2008",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-BADDAY-001",
        statement: "Target pace missed at very high RPE -> attribute to normal off-day variation and affirm the stimulus still counted.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-018)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-TONE-001",
        statement: "On planned-easy sessions reinforce restraint and praise not chasing pace as the win; on planned-hard praise completion/effort.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-026)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-TREND-001",
        statement: "Trend messaging: celebrate improving trends via consistency + next process goal; reframe plateaus as normal consolidation changing ONE variable; answer load-explained declines recovery-first with a deload suggestion.",
        grade: ExpertOpinion,
        primary_citations: &[
            "File 05 feedback-027/028/029 (plateau framing grounded in Williamson 2022; Bandura 1997)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-RECOVERY-001",
        statement: ">=2-3 overtraining/NFOR signals co-occurring over >=1-2 weeks -> CONCERN_RECOVERY with a supportive non-alarmist message; suppresses praise.",
        grade: Moderate,
        primary_citations: &[
            "Meeusen et al. 2013 (ECSS/ACSM consensus)",
            "POMS monitoring; Kajaia et al.",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-BEHAVIOR-001",
        statement: "Compulsive/unhealthy patterns (training through pain, distress at missed sessions, rapidly escalating volume) -> CONCERN_BEHAVIOR; frame rest as a performance behaviour and never reward streaks or volume jumps.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-039)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-PROVISIONAL-001",
        statement: "Never present population defaults as ground truth: state the default, mark it provisional, and converge using the per-user personalization signal.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-040 honest-unknowns list)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // -- File 06 (autoregulation) --
    EvidenceEntry {
        claim_id: "AUTOREG-VBT-001",
        statement: "Daily VBT readiness: reference-load MCV beyond +/-0.06 m/s of baseline moves working loads up/down; within the reliability band hold.",
        grade: Strong,
        primary_citations: &[
            "Banyard 2017",
            "Weakley/Pearson et al. 2020 (0.06 m/s reliability band)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-RHR-DOWN-001",
        statement: "Morning RHR > baseline +5-7 bpm (or +1 SD) for >=2 days -> downgrade intensity; check illness/sleep.",
        grade: Weak,
        primary_citations: &["TrainingPeaks (practitioner)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-WELLNESS-RHR-001",
        statement: "Multi-day suppressed wellness + rising RHR -> 1-3 easy days or cross-train.",
        grade: Moderate,
        primary_citations: &["File 06 §3C"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-FALLBACK-001",
        statement: "Graceful signal fallback: no HRV today -> 7-day rolling HRV (>=4 recent readings), else subjective wellness + last-session performance; neither -> performance-only autoregulation and HOLD load (no progression beyond plan).",
        grade: ExpertOpinion,
        primary_citations: &["File 06 §6A graceful fallback (autoreg-047/048)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // -- File 08 (individualization & safety) --
    EvidenceEntry {
        claim_id: "INDIV-TRAGE-001",
        statement: "Classify training age by progression cadence (novice: every workout; intermediate: week-to-week; advanced: month-to-month), not self-label.",
        grade: ExpertOpinion,
        primary_citations: &["Rippetoe & Kilgore, Practical Programming"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SCALE-DOWN-001",
        statement: "Scale-down order: cut accessory/isolation volume first, then sets toward MEV, then frequency (keep >=2 exposures/muscle/wk), then secondary quality; preserve intensity/load and main compounds last.",
        grade: Strong,
        primary_citations: &[
            "Mujika & Padilla 2000 (volume cut 60-90% with adaptations retained if intensity held)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SCALE-UP-001",
        statement: "Scale-up order: add volume to the priority quality MEV->MAV, then frequency, then accessory/variation, then intensity/load; add a secondary quality only once the primary progresses.",
        grade: ExpertOpinion,
        primary_citations: &[
            "File 08 §2.2 synthesis (ordering ExpertOpinion, consistent with Mujika)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "REENTRY-001",
        statement: "Re-entry ramp after a layoff by time-off bracket (1-2 wk: ~90% loads; 2-4 wk: ~80-85%; 4-8 wk: ~70%; >8 wk: novice re-entry), holding intensity and rebuilding volume.",
        grade: ExpertOpinion,
        primary_citations: &[
            "File 08 Table 3.4b, extrapolated from Mujika & Padilla 2000",
            "Bosquet et al. 2013",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // -- File 10 (hybrid / concurrent) --
    EvidenceEntry {
        claim_id: "HYB-TRAINED-001",
        statement: "Lower-body strength interference applies to trained lifters (~>1-2 yr) only; expect none in moderately-trained or untrained individuals.",
        grade: Moderate,
        primary_citations: &["Petre et al. 2021, Sports Med 51:991-1010"],
        contradicting: &["Schumann 2022 (training-status moderator null)"],
        safety_critical: false,
        contested: Some("CQ-06"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-THRESH-001",
        statement: "Expect strength/hypertrophy attenuation when endurance frequency exceeds 3-4 d/wk or intensity exceeds 80% VO2max; cap endurance or lower lifting-gain expectations.",
        grade: Moderate,
        primary_citations: &["Baar 2014 synthesis", "Jones 2013"],
        contradicting: &[],
        safety_critical: false,
        contested: Some("CQ-06"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-MAINT-001",
        statement: "A quality being maintained (not improved) needs ~1/3 of the improvement dose (~2 low-volume sessions/wk) to free recovery for the priority.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 10 CAP-7 / hybrid-017)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-BSI-001",
        statement: "Raise bone-stress-injury surveillance when running exceeds ~64 km/wk; resistance training is protective given adequate energy availability.",
        grade: Moderate,
        primary_citations: &["Warden 2021, Curr Osteoporos Rep"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-PROG-001",
        statement: "Combined-load running progression guard: keep weekly volume growth <=~10%/wk and avoid acute spikes; ACWR only as a tracking heuristic, never an injury predictor.",
        grade: Moderate,
        primary_citations: &[
            "Gabbett 2016; ACWR literature (criticized - Impellizzeri 2020: tracking heuristic, not predictor)",
        ],
        contradicting: &[
            "Large runner RCT: 10% rule no better than a standard program",
            "Impellizzeri 2020 (conceptual flaws)",
        ],
        safety_critical: true,
        contested: Some("CQ-05"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-DELOAD-001",
        statement: "Deload when >=2-3 overreaching red flags persist >1-2 weeks (RHR >=5-7 bpm over baseline, HRV 7-day trend down >~15% for 3-5 days, sleep/mood/performance decline).",
        grade: Moderate,
        primary_citations: &[
            "File 10 Section E monitoring",
            "Foster 2001 (sRPE)",
            "HRV RCT synthesis",
        ],
        contradicting: &[
            "Bellenger 2016 (resting HRV may not reliably detect overreaching)",
            "Plews (both high and low HRV can signal problems)",
        ],
        safety_critical: true,
        contested: Some("CQ-04"),
        review_months: 12,
    },
    // ---- Marketing myths (hard-blocked) ----
    EvidenceEntry {
        claim_id: "LOAD-ACWR-001",
        statement: "Acute:chronic workload ratio predicts injury risk with a 0.8-1.3 'sweet spot'.",
        grade: MarketingMyth,
        primary_citations: &["Gabbett 2016, BJSM"],
        contradicting: &[
            "Impellizzeri et al. 2019 (retraction request)",
            "Lolli et al. 2019 (mathematical coupling)",
        ],
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
    // File 02 rule claims, task 16
    // Per-rule entries for File 02 rules newly wired into `strength.rs`
    // (strength-006/008/016/017/019/023/024/025/027/030/032-schedule/035/036/
    // 037/038/040). Grades/citations transcribed from the KB rule blocks;
    // contested ids follow the module-doc convention (File 02 local CQ-02 →
    // global CQ-03; local CQ-01/CQ-04 have no global counterpart → CQ-F02-01 /
    // CQ-F02-04).
    EvidenceEntry {
        claim_id: "STR-E1RM-CHECK-001",
        statement: "Above 10 reps or on isolation lifts, do not treat estimated 1RM as reliable; cross-check >=2 formulas and prefer 3-6 rep test sets.",
        grade: Moderate,
        primary_citations: &["DiStasio 2014"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-LOADSEL-001",
        statement: "Prefer fixed % loading for novices, teaching, or when no monitoring is available; prefer RPE/RIR autoregulation for intermediate/advanced and fatigue-sensitive phases.",
        grade: Moderate,
        primary_citations: &[
            "Helms et al. 2018 Front Physiol",
            "Graham & Cleather 2019/2021 JSCR",
        ],
        contradicting: &[
            "RPE vs %1RM matched shows both effective, RPE small non-significant advantage",
        ],
        safety_critical: false,
        contested: Some("CQ-F02-01"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-VZONE-001",
        statement: "Set velocity zones from the individual load-velocity relationship; mean concentric velocity maps inversely and near-perfectly to %1RM (R^2 ~0.98 bench; MVT ~0.15 m/s bench, ~0.30 m/s squat).",
        grade: Moderate,
        primary_citations: &["Gonzalez-Badillo & Sanchez-Medina 2010 Int J Sports Med"],
        contradicting: &["Zone boundaries approximate and individual"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-LVP-001",
        statement: "Estimate 1RM via a 4-7 (recommend 5-7) incremental-load velocity profile extrapolated to MVT; monitoring only (SEE ~9.8% of 1RM); deadlift LVP must not predict 1RM.",
        grade: Moderate,
        primary_citations: &[
            "Jovanovic & Flanagan",
            "Greig et al. 2023 (434 participants, 20 studies)",
            "Lake et al. (deadlift caveat)",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-VBT-DAILY-001",
        statement: "Autoregulate daily load by first-rep bar speed: add load if first-rep velocity exceeds the target-zone upper bound, reduce load if below the lower bound (true 1RM varies +/-18% day-to-day).",
        grade: Moderate,
        primary_citations: &["Jovanovic & Flanagan 2014 J Aust Strength Cond 22(2):58-69"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-DUP-001",
        statement: "Structure DUP by varying intensity/rep focus session-to-session within the week (heavy 3-5x3-5 @85-90%; power 3-6x3-5 @50-70% fast; hypertrophy 3-4x8-12 @70-75%); best supported for intermediate/advanced training a lift >=2-3x/wk.",
        grade: Moderate,
        primary_citations: &["Fleck & Kraemer", "Rhea et al. 2002 JSCR 16(2):250-255"],
        contradicting: &["Harries/Grgic meta-analyses find no consistent DUP advantage"],
        safety_critical: false,
        contested: Some("CQ-03"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-CONJ-001",
        statement: "Reserve conjugate/Westside for advanced lifters (>=2-3 yr barbell training): rotate Max Effort variations to 1-3RM (90%+), run Dynamic Effort speed work with accommodating resistance, and use Repetition-method accessories for weak points.",
        grade: ExpertOpinion,
        primary_citations: &["Louie Simmons (Westside Barbell)", "Prilepin"],
        contradicting: &["Little direct RCT support (Weak experimental)"],
        safety_critical: true,
        contested: Some("CQ-F02-04"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-WAVE-001",
        statement: "Use wave loading (ascending/descending load waves across sets, e.g. 3-2-1/3-2-1 rising) to exploit acute potentiation.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 02 strength-025; graded ExpertOpinion/Weak)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-PEAK-001",
        statement: "For strength peaking, reduce volume 30-70%, maintain or slightly increase intensity >=85% 1RM, taper (step or exponential) over 1-2 wk, then 2-7 days cessation.",
        grade: Moderate,
        primary_citations: &[
            "Pritchard et al. 2015 Strength Cond J 37(2):72-83",
            "Travis et al. 2020 Sports 8(9):125",
            "Travis et al. 2021",
            "Pritchard et al. 2019 IJSPP",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-PWRSPEC-001",
        statement: "Train power across a load spectrum matched to force-velocity needs, not a single optimal load: jump squat ~0% 1RM, loaded squat power >30-<70% 1RM, power clean ~40-80% (>=70% for clean variations), weightlifting pulls 90-95%.",
        grade: Moderate,
        primary_citations: &[
            "Cormie et al. 2007/2011",
            "Soriano, Jimenez-Reyes, Rhea & Marin 2015 Sports Med 45:1191-1205 (27 studies)",
            "Suchomel",
            "Kawamori & Haff",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-PLYO-SCHED-001",
        statement: "Schedule plyometrics 1-3x/wk with 48-72 h between sessions; rest 2-3 min/set (depth jumps up to ~1:10 work:rest, 5-10 s between reps).",
        grade: Moderate,
        primary_citations: &["Potash & Chu 2008 (NSCA/UKSCA)", "Verkhoshansky"],
        contradicting: &[],
        // Same File 02 rule (strength-032) as PLYO-001, which the KB marks
        // safety_critical.
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-COMP-ANCHOR-001",
        statement: "Anchor strength-goal training with the competition lift (highest specificity); adaptations are specific to trained movement, velocity, and ROM; over-specialized variations improve the variation, not the comp lift.",
        grade: Strong,
        primary_citations: &["unstated (File 02 strength-035 specificity principle)", "Westside caution"],
        contradicting: &["Effect magnitudes only Moderate"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-VARIATION-001",
        statement: "Select variations to bias weak points (ROM, stance/grip, tempo, bar) with best carryover when stance/grip/ROM match the target movement; use accessories for lagging muscles (single-joint, muscle-focused).",
        grade: Moderate,
        primary_citations: &["unstated (File 02 strength-036)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-WEAKPOINT-001",
        statement: "Apply weak-point IF/THEN exercise rules keyed to sticking point (bench off-chest -> paused/Spoto/incline + pec/front-delt; bench lockout -> close-grip/floor + triceps; squat hole -> pause/front + quad; squat mid/high -> low-bar + posterior; DL floor -> deficit + quad/upper-back; DL lockout -> rack/block + glute/ham/upper-back).",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 02 strength-037, Westside-influenced)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-SUBST-EQUIP-001",
        statement: "When substituting equipment, preserve the movement pattern and velocity/load intent, ordering substitutions by specificity and preferring free-weight over machine for transfer to free-weight tests.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 02 strength-038)"],
        contradicting: &[
            "Per-pattern substitution table referenced but not reproduced in the KB extract",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "STR-1RMTEST-001",
        statement: "Test 1RM only when technically proficient, adequately recovered, and warmed up (novices supervised; spinal loading needs bracing competence); cap novice load jumps upper 2.5-5%, lower 5-10%, and prefer estimated 1RM early on.",
        grade: ExpertOpinion,
        primary_citations: &["NSCA"],
        contradicting: &[
            "File 02 strength-040 grades ExpertOpinion/Moderate (0.40); registered conservatively as ExpertOpinion",
        ],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // end File 02 rule claims, task 16
    // Task 2-4 additions
    // Per-rule claims for the autoreg/feedback/pain fixes: File 06 autoreg-041
    // (RHR +10 stop), File 08 safety-038/039 (graded pain, Table 4.1), and
    // File 05 feedback-019/020 (RIR-vs-target lifting feedback). Grades and
    // citations transcribed from the KB rule blocks, never overstated.
    EvidenceEntry {
        claim_id: "AUTOREG-RHR-STOP-001",
        statement: "Morning RHR > baseline +10 bpm (or elevated with symptoms) -> rest day; run the illness neck-check before training.",
        grade: Weak,
        primary_citations: &["TrainingPeaks (practitioner convention; File 06 autoreg-041)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-PAIN-STRUCT-001",
        statement: "Sharp, localized, or joint-line pain; pain that alters movement/gait; or pain with swelling -> STOP that exercise; DEFER if it persists (possible structural injury).",
        grade: Moderate,
        primary_citations: &[
            "File 08 §4.1 pain-pattern table (safety-038; Silbernagel model for tolerable tendon pain)",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-TENDON-001",
        statement: "Tendon pain <=3-5/10 that stays stable during & 24 h after and is not worsening week-to-week -> MODIFY/continue with monitoring, avoid complete rest; >5/10, worsening after, or rising week-to-week -> REDUCE load & compressive positions, DEFER if it persists.",
        grade: Moderate,
        primary_citations: &[
            "Silbernagel et al. 2007 pain-monitoring model",
            "Cook & Purdam continuum",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-RIR-001",
        statement: "Lifting execution by RIR vs target: reps met with RIR >= target -> mastery + cue planned progression (feedback-019); reps met at RIR ~0 vs a 2-3 target -> corrective hold/slightly-drop-load caution (feedback-020); in between -> neutral process tone.",
        grade: Moderate,
        primary_citations: &[
            "Zourdos et al. 2016, J Strength Cond Res 30:267",
            "Graham & Cleather 2021",
            "Halperin et al. 2022",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // end Task 2-4 additions
    // ------------------------------------------------------------------
    // File 04 rule claims, task 18
    // Per-rule entries for File 04 rules newly wired into `running.rs` /
    // `load.rs` (running-005/009/010/014/015/016/017/020/021/026/032/033/041
    // plus the VDOT R row). Grades/citations transcribed verbatim from the KB
    // rule blocks; KB-internal discrepancies are documented in `contradicting`
    // rather than silently resolved.
    // ------------------------------------------------------------------
    EvidenceEntry {
        claim_id: "RUN-KARVONEN-001",
        statement: "Prefer Karvonen (%HRR) over %HRmax when resting HR is low (RHR<55; the methods diverge substantially there); at RHR>=70 the methods converge and either is acceptable. %HRmax target = HRmax*%; Karvonen target = ((HRmax-RHR)*%)+RHR. Behavior for RHR 55-69 is unstated in the KB.",
        grade: Moderate,
        primary_citations: &["Wood, Topend Sports (Karvonen ~ %VO2max), unstated year"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-CS-PROTOCOL-001",
        statement: "Model Critical Speed as D = CS*t + D' from 2-5 maximal efforts of different durations (headline window 2-20 min; explicitly invalid <2 min or >30 min; ideal pair one 3-8 min + one 12-30 min); CS ~= LT2 sustainable pace; negative D' indicates non-maximal trials; avoid too-similar (1500m+mile) or too-spread (800m+HM) pairings.",
        grade: Moderate,
        primary_citations: &["Galbraith/Nicolo et al. 2018 (linearity r=0.979-0.999)"],
        contradicting: &[
            "KB gives both a 2-20 min headline window and a 12-30 min ideal upper duration without reconciling them; the validity gate rejects only the explicitly-invalid bounds (<2 min, >30 min)",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-POWER-001",
        statement: "Running power zones (Coggan %FTP 7-zone: Z1<55, Z2 56-75, Z3 76-90, Z4 91-105, Z5 106-120, Z6 121-150, Z7>150, SweetSpot 88-94; Stryd %CP 5-zone: Z1 65-80, Z2 80-90, Z3 90-100, Z4 100-115, Z5 115-130; Stryd CP ~= 40-min power) are a consistent proxy, not a criterion metabolic measure; the two frameworks are NOT interchangeable.",
        grade: Weak,
        primary_citations: &[
            "Coggan/Allen, Training and Racing with a Power Meter",
            "Stryd docs; Palladino Power Project, unstated year",
        ],
        contradicting: &["Weak that power = metabolic cost (Moderate as a framework only)"],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-RECOVERY-001",
        statement: "Recovery runs: >=20% slower than marathon pace (no upper slowness bound stated), HR ceilings <76%HRmax and <70%HRR (no lower HR bound stated), RPE 2-3, 20-40 min.",
        grade: ExpertOpinion,
        primary_citations: &["Pfitzinger & Douglas, Advanced Marathoning, unstated year"],
        contradicting: &[
            "KB internal inconsistency: the statement gives <76%HRmax AND <70%HRR (different reference scales) while the parameters line collapses them to <70-76%HRmax; the engine carries both ceilings and lets the stricter govern (conservative)",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-EASY-001",
        statement: "Easy/General-Aerobic runs: E pace 15-25% slower than marathon pace, 65-79%HRmax, RPE 3-4, 30-90 min.",
        grade: ExpertOpinion,
        primary_citations: &["Daniels; Pfitzinger, unstated year"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-LONGRUN-001",
        statement: "Long runs: E to E+ pace (MP minus 10-20%), 65-80%HRmax, RPE 3-5; LR share 25-30% of weekly volume (Daniels: single run <=25%, time-cap ~2:00-2:30, no duration floor stated); low-mileage guardrail: LR <= 2x average daily run.",
        grade: ExpertOpinion,
        primary_citations: &["Daniels; Pfitzinger, unstated year"],
        contradicting: &[
            "running-024 gives LR share 20-30% vs running-016's (and the volume-caps table's) 25-30%; the KB does not reconcile - each rule is implemented with its own figure",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-RACEPACE-001",
        statement: "Marathon-pace segments: M pace, 80-85%HRmax, RPE 5-6, in 8-26 km blocks (no weekly M-pace volume cap stated; the VDOT table M-row cap column is '-').",
        grade: ExpertOpinion,
        primary_citations: &["Pfitzinger; Canova, unstated year"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-REPETITION-001",
        statement: "Repetition (R) work: >100% VO2max, prescribed by pace not HR, reps <=2 min, total R <=5% of weekly volume. The KB has NO dedicated R-session rule; rep distance, rep count, recovery, and RPE are unstated.",
        grade: ExpertOpinion,
        primary_citations: &["Daniels' Running Formula (VDOT R row + volume-caps section)"],
        contradicting: &[
            "Band values (>100% VO2max, pace-not-HR) are Moderate via the running-007 VDOT table; registered at the ExpertOpinion floor of the caps column (see RUN-VOLCAP-001)",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-STRIDES-001",
        statement: "Strides: 15-30 s x 4-8 controlled-fast (not sprint) efforts, RPE 6-7, near-full recovery 45 s-2 min, 1-3x/week, introduced a few weeks into base (no numeric week count stated).",
        grade: ExpertOpinion,
        primary_citations: &["Daniels, unstated year"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-HILLSPRINT-001",
        statement: "Hill sprints: 8-20 s x 4-10 near-max (90-95%) efforts on 6-10% grade, RPE 9, full recovery (walk down / ~2 min), treated as strength work on easy days; weekly session frequency unstated.",
        grade: ExpertOpinion,
        primary_citations: &["Magness; Lydiard; Hudson, unstated year"],
        contradicting: &[
            "Lydiard discrete 4-6-wk hill phase vs Hudson year-round (disagreement over placement, not parameters)",
        ],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-RUNWALK-EXT-001",
        statement: "For obese/very deconditioned runners, extend run/walk intervals longer before continuous running to manage impact-injury risk (no numeric extension length or defining threshold stated).",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 04 running-026)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-ONEVAR-001",
        statement: "Progress only ONE variable at a time - volume OR intensity, not both in the same week.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 04 running-032)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-DELOAD-DEPTH-001",
        statement: "Recovery-week magnitude: reduce volume 20-40% (higher-mileage 10-30%; lower-mileage up to 50%), reduce both volume and intensity (intensity magnitude unstated), and drop a quality session; numeric higher/lower-mileage thresholds unstated.",
        grade: ExpertOpinion,
        primary_citations: &["practice standard; RCT protocol NCT06111144"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "RUN-RETEST-001",
        statement: "Set training paces from CURRENT fitness (not goal) and re-test every 4-6 weeks, recomputing paces as VDOT/CS improves; race input must be recent (<=6-8 wk), honest, flat/cool; apply corrections for heat >~15 C and altitude >~900 m (trigger thresholds only - correction magnitudes unstated).",
        grade: Moderate,
        primary_citations: &["Daniels, unstated year"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    // end File 04 rule claims, task 18
    // ------------------------------------------------------------------
    // Task 5 additions
    // File 08 safety-gate per-rule claims (safety-011/046/047/048). Rules whose
    // grade/citation match an existing File 09 referral claim REUSE it instead:
    // safety-044/onboard-050 → SAFE-CVD-001; safety-045 → SAFE-PREG-001;
    // safety-017/022/049 → SAFE-REDS-001 (NB the KB's "safety-035" cross-refs
    // in safety-017/-022 are a KB numbering bug; the RED-S absolute rule is
    // safety-049); safety-041/042 → SAFE-OTS-001; safety-024/indiv-025 →
    // ENV-001. Grades/citations transcribed verbatim, never overstated.
    // ------------------------------------------------------------------
    EvidenceEntry {
        claim_id: "SAFE-PEDS-001",
        statement: "Child/adolescent users require qualified supervision and technique/skill emphasis over external load; the engine must NOT autonomously prescribe maximal loading or 1RM testing (no numeric age cutoff stated in the KB).",
        grade: Strong,
        primary_citations: &[
            "Lloyd, Faigenbaum et al. 2014 Intl Consensus, BJSM 48:498-505",
            "Lloyd & Oliver 2012 YPD",
        ],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-PREG-WARN-001",
        statement: "Pregnancy warning signs (vaginal bleeding, amniotic fluid leakage, regular painful contractions, dyspnea before exertion, dizziness/faintness, headache, chest pain, calf pain/swelling, muscle weakness affecting balance, decreased fetal movement) -> STOP and DEFER; contraindication conditions (placenta previa after 26 wk, preeclampsia/gestational hypertension, incompetent cervix, severe anemia) -> DEFER.",
        grade: Strong,
        primary_citations: &["ACOG Committee Opinion 804, 2020"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-PREG-AVOID-001",
        statement: "In pregnancy avoid prolonged supine positioning, overheating, contact/fall-risk activities, scuba diving, exercise at high altitude (>2,500 m), and breath-holding (Valsalva) during strength work.",
        grade: Strong,
        primary_citations: &["ACOG Committee Opinion 804, 2020"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "SAFE-INJURY-001",
        statement: "The engine must NOT prescribe rehabilitation; a current injury under care, recent surgery, or active rehab defers to a physician/physiotherapist, resuming general programming only upon clearance (no numeric 'recent surgery' window stated).",
        // The LOWEST-graded of the File 08 safety rules, yet still
        // safety-critical, conservative by design (File 08 §5.4).
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 08 §5.4, conservative ExpertOpinion)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // end Task 5 additions
    // ------------------------------------------------------------------
    // Task 19 additions
    // Per-rule claims for Files 05/06/10 rules newly wired (autoreg-006/025/
    // 029/032/042; feedback-005/015/023/024/035; hybrid-004/011/019/020/024/
    // 025). Contested ids follow the module-doc convention (File 10 local
    // CQ-02 → global CQ-06). Grades/citations transcribed verbatim.
    // ------------------------------------------------------------------
    EvidenceEntry {
        claim_id: "AUTOREG-E1RM-GATE-001",
        statement: "Today's e1RM (from the first top set) below baseline -5% -> cap the session at planned RPE -1 and reduce top-set load ~5%.",
        grade: Strong,
        primary_citations: &["Helms et al. 2018, Front Physiol 9:247 (e1RM session gate)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-MRV-001",
        statement: "At/above-MRV sign cluster (joint aches, performance stall, sleep disruption, motivation drop) -> deload (no numeric sign count stated).",
        grade: ExpertOpinion,
        primary_citations: &["RP framework (Israetel et al. 2021)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-HRV-SAT-001",
        statement: "lnRMSSD7d above the SWC upper limit during a high-load block (possible parasympathetic saturation) -> do NOT auto-add load; hold and weigh with wellness.",
        grade: Moderate,
        primary_citations: &["Plews et al. (parasympathetic saturation)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-PACE-RETEST-001",
        statement: "Pace at target HR improved by at least a smallest-worthwhile amount sustained over 2-3 weeks -> re-test / raise threshold pace (SWC magnitude unstated).",
        grade: Moderate,
        primary_citations: &["unstated (File 06 §3B running autoregulation)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "AUTOREG-NFOR-001",
        statement: "Unexplained performance decrement >=2 weeks with >=2 wellness domains suppressed (NFOR cluster) -> mandatory recovery block; if it persists, escalate to 'consult a professional'.",
        // File 06 autoreg-042 grades this ExpertOpinion (0.30) despite the
        // Meeusen citation: never overstated to match the Strong File 08
        // safety-042 rule (which reuses SAFE-OTS-001).
        grade: ExpertOpinion,
        primary_citations: &["Meeusen et al. 2013, Med Sci Sports Exerc 45:186-205"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-INTERVAL-MASTERY-001",
        statement: "Interval/threshold reps hitting target paces at or below target RPE -> POSITIVE_MASTERY confirming the intended adaptation.",
        grade: Moderate,
        primary_citations: &["unstated (File 05 feedback-015)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-MASTERY-ANCHOR-001",
        statement: "Build self-efficacy by highlighting concrete mastery experiences (recent PRs, completed sessions, barriers overcome) in praise copy - name specific controllable achievements.",
        grade: Strong,
        primary_citations: &[
            "Bandura 1997",
            "McAuley & Blissmer 2000, Exerc Sport Sci Rev 28:85-88",
        ],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-VERBOSITY-001",
        statement: "Personalize feedback density by experience: beginners get one takeaway, minimal jargon, and a mandatory 'why'; advanced users may get 2-3 metrics but still led by the single biggest lever.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-023/024, §5.1 defaults)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "FB-BSI-FEMALE-001",
        statement: "For a female user routed to a bone-stress-injury referral, gently prompt discussing menstrual/nutrition status with the clinician (amenorrhea/under-fueling raise BSI risk); no self-diagnosis.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 05 feedback-035; RED-S linkage, detection out of scope)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-DURATION-001",
        statement: "Interference magnitude scales with endurance frequency (r -0.26..-0.35) and per-session duration (r -0.29..-0.75, up to -0.75 for hypertrophy); continuous duration is the single strongest moderator (no numeric onset threshold defined).",
        grade: Moderate,
        primary_citations: &["Wilson 2012 moderator analysis"],
        contradicting: &[],
        safety_critical: false,
        // File 10 local CQ-02 (onset threshold undefined) → global CQ-06.
        contested: Some("CQ-06"),
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-SCHED-001",
        statement: "Schedule the highest-priority quality when freshest - start of the week or after a rest day.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 10 hybrid-011)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-CHO-001",
        statement: "On double (AM/PM) days, fully refuel carbohydrate between endurance and lifting - low glycogen amplifies AMPK activation and interference.",
        // File 10 rule entry grades Weak (0.40); the CAP-8 table row says
        // Weak-Moderate, registered at the rule entry's Weak floor.
        grade: Weak,
        primary_citations: &["Baar 2014 (CAP-8)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-PHASE-001",
        statement: "Periodize interference by phase: a general phase separates qualities to minimize interference; a specific/event phase deliberately combines them (strength-endurance hybrids, moderate load/high rep/minimal rest), accepting some interference for sport-specific transfer; hybrid-race split 2-3 strength + 3-4 endurance sessions/wk.",
        grade: ExpertOpinion,
        primary_citations: &["standard periodization practice (File 10 template d)"],
        contradicting: &[],
        safety_critical: false,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-EA-001",
        statement: "Ensure adequate energy availability (guard against RED-S/LEA), especially in high-volume endurance, leaner, and female athletes, to avoid compounding bone-stress-injury risk.",
        grade: ExpertOpinion,
        primary_citations: &["unstated (File 10 Section E)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    EvidenceEntry {
        claim_id: "HYB-TENDON-001",
        statement: "The concurrent-training effect on tendon stiffness is unknown (no direct study); err conservative when simultaneously progressing high running volume and heavy lifting.",
        grade: Weak,
        primary_citations: &["Baar 2014 (evidence gap noted)"],
        contradicting: &[],
        safety_critical: true,
        contested: None,
        review_months: 12,
    },
    // end Task 19 additions
];

/// Contested questions (File 09). Engine holds a default lean while open.
pub static CONTESTED_QUESTIONS: &[ContestedQuestion] = &[
    ContestedQuestion {
        id: "CQ-01",
        question: "Hypertrophy volume ceiling",
        engine_default: "10-20 sets/muscle/wk",
    },
    ContestedQuestion {
        id: "CQ-02",
        question: "Train to failure?",
        engine_default: "0-3 RIR",
    },
    ContestedQuestion {
        id: "CQ-03",
        question: "Periodization model superiority",
        engine_default: "Auto-DUP, any structured plan accepted",
    },
    ContestedQuestion {
        id: "CQ-04",
        question: "HRV-guided training value",
        engine_default: "Gate hard/easy only",
    },
    ContestedQuestion {
        id: "CQ-05",
        question: "ACWR validity",
        engine_default: "DO NOT use ACWR as injury predictor",
    },
    ContestedQuestion {
        id: "CQ-06",
        question: "Interference real-world magnitude",
        engine_default: "Protect power/explosive only",
    },
    ContestedQuestion {
        id: "CQ-07",
        question: "Runner intensity distribution",
        engine_default: "Pyramidal base -> polarized peak",
    },
    ContestedQuestion {
        id: "CQ-08",
        question: "Grade-adjusted-pace downhill validity",
        engine_default: "Trust uphill; soften/flag downhill",
    },
    ContestedQuestion {
        id: "CQ-09",
        question: "Menstrual-cycle periodization",
        engine_default: "Symptom-based optional adjustment",
    },
    ContestedQuestion {
        id: "CQ-10",
        question: "Optimal interval length",
        engine_default: "Menu selected by goal/event",
    },
    ContestedQuestion {
        id: "CQ-11",
        question: "Marathon philosophy",
        engine_default: "Aerobic base -> specific block",
    },
    ContestedQuestion {
        id: "CQ-12",
        question: "MAF 180-formula vs measured LT1",
        engine_default: "Measured LT1 when available, else MAF fallback",
    },
    ContestedQuestion {
        id: "CQ-13",
        question: "Running power / form-metric value",
        engine_default: "Display only, never prescribe",
    },
    ContestedQuestion {
        id: "CQ-14",
        question: "Consumer readiness-score trust",
        engine_default: "3-band GO/CAUTION/REST",
    },
    ContestedQuestion {
        id: "CQ-15",
        question: "Concurrent session order & separation",
        engine_default: "RT-first for strength; >=3h (ideally 6-24h) gap",
    },
    // File-local contested questions with no File 09 global counterpart
    // (namespaced CQ-F<file>-<local>; see module docs).
    ContestedQuestion {
        id: "CQ-F03-04",
        question: "Set-addition vs double progression within a hypertrophy block (File 03 local CQ-04)",
        engine_default: "Either accepted; set-addition optional (Enes 2024: no difference vs constant sets)",
    },
    // File 03 rule claims, task 17
    ContestedQuestion {
        id: "CQ-F03-02",
        question: "Fixed \"hypertrophy rep range\" vs 5-30 wide spectrum (File 03 local CQ-02)",
        engine_default: "Wide spectrum: ~5-30+ reps at ~30-85% 1RM near failure; <~30% 1RM avoided",
    },
    // File 02 rule claims, task 16
    ContestedQuestion {
        id: "CQ-F02-01",
        question: "Fixed % vs RPE/RIR autoregulation superiority (File 02 local CQ-01) - both effective; RPE small non-significant edge",
        engine_default: "Fixed % for novices/teaching/no monitoring; RPE/RIR for intermediate/advanced and fatigue-sensitive phases",
    },
    ContestedQuestion {
        id: "CQ-F02-04",
        question: "Conjugate/Westside efficacy (File 02 local CQ-04) - strong practice record, weak controlled evidence",
        engine_default: "Advanced-only option (>=2-3 yr barbell training); never a default model",
    },
    // end File 02 rule claims, task 16
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn claim_ids_are_unique() {
        let mut seen = HashSet::new();
        for c in CLAIMS {
            assert!(
                seen.insert(c.claim_id),
                "duplicate claim_id: {}",
                c.claim_id
            );
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
    fn safety_critical_set_matches_knowledge_base() {
        // safety_critical mirrors the KB rule's own marking, never invented,
        // never dropped. This is the exact set the KB flags (File 09 referral
        // deferrals; File 02 strength-012/024/029/032/040; File 03 hyp-011/016/017/021/030; File 04
        // running-006/023/025/026/029/034/037/038; File 05 feedback-036/039;
        // File 06 autoreg-043/045/046/047/048; File 08 safety-024;
        // File 10 hybrid-021/023/026; plus the joint-pain myth hard-block).
        let expected: HashSet<&str> = [
            "SAFE-REDS-001",
            "SAFE-OTS-001",
            "SAFE-BSI-001",
            "SAFE-PREG-001",
            "SAFE-CVD-001",
            "SAFE-PAIN-001",
            "ILLNESS-NECK-001",
            "ENV-001",
            "PLYO-001",
            "STR-2FOR2-001",
            "STR-DLPEAK-001",
            "STR-CONJ-001",
            "STR-PLYO-SCHED-001",
            "STR-1RMTEST-001",
            "HYP-PAIN-SHIFT-001",
            "HYP-VOLRAMP-SAFE-001",
            "HYP-SKILL-RIR-001",
            "HYP-FAIL-SAFE-001",
            "HYP-PAIN-SWAP-001",
            "RUN-HRRECALC-001",
            "RUN-SPIKE-BLOCK-001",
            "RUN-TAPER-001",
            "RUN-QUALITY-001",
            "RUN-C25K-001",
            "RUN-DOWNWEEK-001",
            "RUN-RUNWALK-EXT-001",
            "FB-RECOVERY-001",
            "FB-BEHAVIOR-001",
            "AUTOREG-FALLBACK-001",
            "HYB-BSI-001",
            "HYB-PROG-001",
            "HYB-DELOAD-001",
            "MYTH-NO-PAIN-JOINT",
            // Task 2-4 additions: File 06 autoreg-041 (RHR +10 stop, KB marks
            // safety_critical) + File 08 safety-038/039 (graded pain rules).
            "AUTOREG-RHR-STOP-001",
            "SAFE-PAIN-STRUCT-001",
            "SAFE-TENDON-001",
            // Task 5 additions: File 08 safety-011 (pediatric), safety-046
            // (pregnancy warning signs), safety-047 (pregnancy avoid-list),
            // safety-048 (injury/rehab deferral, ExpertOpinion yet
            // safety-critical, conservative by design).
            "SAFE-PEDS-001",
            "SAFE-PREG-WARN-001",
            "SAFE-PREG-AVOID-001",
            "SAFE-INJURY-001",
            // Task 19 additions: File 06 autoreg-042 (NFOR cluster), File 05
            // feedback-035 (female BSI clinician prompt), File 10 hybrid-024
            // (energy-availability guard) + hybrid-025 (tendon-stiffness
            // conservative dual progression).
            "AUTOREG-NFOR-001",
            "FB-BSI-FEMALE-001",
            "HYB-EA-001",
            "HYB-TENDON-001",
        ]
        .into_iter()
        .collect();
        let actual: HashSet<&str> = CLAIMS
            .iter()
            .filter(|c| c.safety_critical)
            .map(|c| c.claim_id)
            .collect();
        assert_eq!(actual, expected, "safety_critical set drifted from the KB");
    }

    #[test]
    fn file09_canonical_claims_all_present() {
        // Criterion: the File 09 canonical claims (and 21 myths) must never be
        // dropped when per-rule entries are added.
        let canonical = [
            "HYP-VOL-001", "HYP-LOAD-001", "PERIOD-001", "STR-INTENT-001",
            "CONC-RE-001", "CONC-INTERF-001", "AUTOREG-RIR-001", "STR-TRAGE-001",
            "GOAL-PROCESS-001", "FEEDBACK-001", "TAPER-001", "HYP-FAIL-001",
            "HYP-LENGTH-001", "AUTOREG-PCT-001", "AUTOREG-VL-001", "TAPER-STR-001",
            "RUN-DIST-001", "RUN-SPIKE-001", "CONC-ORDER-001", "CONC-SEP-001",
            "CONC-MODE-001", "HRV-001", "WELLNESS-001", "RUN-VDOT-001",
            "LOAD-TRIMP-001", "HYP-LANDMARKS-001", "RUN-GAP-001", "RUN-FORM-001",
            "RUN-DECOUPLE-001", "RUN-HRMAX-001", "RUN-10PCT-001",
            "FEM-MENSTRUAL-001", "READY-CONSUMER-001", "SAFE-REDS-001",
            "SAFE-OTS-001", "SAFE-BSI-001", "SAFE-PREG-001", "SAFE-CVD-001",
        ];
        for id in canonical {
            assert!(claim(id).is_some(), "File 09 canonical claim {id} missing");
        }
        let myths = CLAIMS.iter().filter(|c| c.is_blocked()).count();
        assert_eq!(myths, 21, "File 09 lists 21 hard-blocked myths");
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
    #[should_panic(expected = "must never be surfaced")]
    fn surfacing_a_myth_trips_the_choke_point_guard() {
        // HARD RULE 2: no `recommend`/`graded` wrapper may surface a myth. They
        // all funnel through `to_evidence()`, whose guard is UNCONDITIONAL -
        // this test is deliberately not gated on `debug_assertions` and must
        // pass under `cargo test --release` too.
        let _ = claim("LOAD-ACWR-001").expect("present").to_evidence();
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
            assert_eq!(
                claim(id).unwrap().review_months,
                6,
                "{id} should review at 6mo"
            );
        }
    }
}
