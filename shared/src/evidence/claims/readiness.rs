//! Readiness and autoregulation claims: File 06 per-rule rows (`autoreg/`),
//! `13-readiness-update.md` and effort logging. The pain and illness stop rules
//! live in [`super::safety`]. File 06 autoreg-041's single-reading RHR rest
//! rule is superseded by AUTOREG-RHR-NOISE-001 and not registered.

use crate::evidence::cite::{
    CITE_DANIELS_2022, CITE_HALPERIN_2022, CITE_ISRAETEL_2021, CITE_MEEUSEN_2013,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    ReadinessClaim => Readiness;
    AUTOREG_SETVOL_001 => claim(
        "AUTOREG-SETVOL-001",
        "How your first work set goes (reps against the target, and how hard it felt) decides whether today's session adds a set, drops one or keeps the planned number.",
        Moderate,
        &[
            "Helms et al. 2018 (RPE stops)",
            "Israetel et al. 2021 (RP framework)",
        ],
    )
    .contradicting(&["Novices estimate RIR poorly"]),
    AUTOREG_DELOAD_001 => claim(
        "AUTOREG-DELOAD-001",
        "Signs of built-up fatigue (weights feeling harder than they should, effort creeping up while you feel run-down, or failed key sessions) call for one lighter week.",
        Moderate,
        &[
            "Israetel et al. 2021 (RP framework: deload ~50% volume and/or ~10% intensity, ~1 wk)",
        ],
    ),
    AUTOREG_DELOAD_VBT_001 => claim(
        "AUTOREG-DELOAD-VBT-001",
        "A drop in bar speed at the same weight over a week (more than about 0.06–0.10 m/s) calls for one lighter week. The size of that drop is a coaching convention, not a published value.",
        Moderate,
        &[
            "Pearson et al. 2020, IJERPH 17(18):6509 (between-day reliability of velocity-loss thresholds; the 0.06-0.10 m/s drop is practitioner-set)",
            "Israetel et al. 2021 (RP framework)",
        ],
    ),
    AUTOREG_APRE_001 => claim(
        "AUTOREG-APRE-001",
        "Your next weight comes from how many reps you managed on an as-many-reps-as-possible set, using standard adjustment tables (APRE).",
        Moderate,
        &["Mann et al. 2010, JSCR 24:1718-1723"],
    ),
    AUTOREG_VBT_001 => claim(
        "AUTOREG-VBT-001",
        "If your bar speed at a fixed warm-up weight is clearly above or below your normal (more than about 0.06 m/s), today's working weights move up or down; inside that band they stay. The 0.06 m/s band is a coaching convention.",
        Strong,
        &[
            "Gonzalez-Badillo & Sanchez-Medina 2010, Int J Sports Med 31:347-352 (load-velocity relationship)",
            "Pearson et al. 2020, IJERPH 17(18):6509 (between-day reliability of velocity-loss thresholds; the 0.06 m/s band is practitioner-set)",
        ],
    )
    .contradicting(&[
        "Banyard et al. 2017, J Strength Cond Res 31:1897-1904: predicting 1RM from bar velocity carried significant error",
    ]),
    AUTOREG_RHR_DOWN_001 => claim(
        "AUTOREG-RHR-DOWN-001",
        "A morning resting heart rate 5–7 bpm above your normal, or one typical day-to-day swing above it, for 2 or more days suggests an easier day and a check for illness or poor sleep.",
        Weak,
        &[
            "Bellenger et al. 2016, Sports Med 46(10):1461-1486 (resting-HR/HRV monitoring of fatigue)",
            "TrainingPeaks coaching guidance (trainingpeaks.com)",
        ],
    ),
    AUTOREG_WELLNESS_RHR_001 => claim(
        "AUTOREG-WELLNESS-RHR-001",
        "Feeling run-down for several days together with a rising resting heart rate calls for 1–3 easy days or low-impact cross-training.",
        Moderate,
        &[
            "Saw, Main & Gastin 2016, Br J Sports Med 50(5):281-291 (subjective wellness monitoring)",
            CITE_MEEUSEN_2013,
        ],
    ),
    AUTOREG_FALLBACK_001 => claim(
        "AUTOREG-FALLBACK-001",
        "With no heart-rate variability (HRV) reading today, your 7-day HRV average stands in (it needs 4 or more recent readings). Without HRV, how you feel and how your last session went decide. With neither, your weights stay steady and nothing progresses beyond the plan.",
        ExpertOpinion,
        &[
            "Saw, Main & Gastin 2016, Br J Sports Med 50(5):281-291",
            "Plews et al. 2013, Sports Med 43(9):773-781 (HRV averaging/interpretation)",
        ],
    )
    .safety()
    .review(6),
    AUTOREG_E1RM_GATE_001 => claim(
        "AUTOREG-E1RM-GATE-001",
        "If the max your first top set predicts is more than 5% below your normal, the whole session aims for one more rep left than planned, and the top-set weight drops by about 5%.",
        Strong,
        &[
            "Helms et al. 2018, Front Physiol 9:247 (RPE- vs %1RM-based loading; the 5% gate and ~5% load cut are practitioner-set)",
        ],
    ),
    AUTOREG_MRV_001 => claim(
        "AUTOREG-MRV-001",
        "A cluster of signs that you are at or past the most volume you can recover from (joint aches, stalled performance, disturbed sleep, low motivation) calls for a lighter week; no exact number of signs is established.",
        ExpertOpinion,
        &[CITE_ISRAETEL_2021],
    ),
    AUTOREG_HRV_SAT_001 => claim(
        "AUTOREG-HRV-SAT-001",
        "If your 7-day HRV average rises above its normal range during a hard training block (which can mean your body is overloaded, not fresh), don't step training up automatically; hold and weigh it with how you feel.",
        Moderate,
        &[
            "Plews et al. 2013, Sports Med 43(9):773-781 (parasympathetic saturation)",
        ],
    )
    .review(6),
    AUTOREG_PACE_RETEST_001 => claim(
        "AUTOREG-PACE-RETEST-001",
        "When your pace at the same heart rate improves by a meaningful amount for 2–3 weeks, re-test and raise your threshold pace; the exact size of a meaningful change is not established.",
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Hopkins 2004, Sportscience 8:1-7 (smallest worthwhile change)",
        ],
    ),
    AUTOREG_INTERVAL_PACE_001 => claim(
        "AUTOREG-INTERVAL-PACE-001",
        "If 2 or more interval repeats feel at least one step harder than planned on the 0–10 effort scale, or go over the heart-rate cap, slow the remaining repeats by about 2–4% (or do fewer).",
        // File 06 autoreg-031 (Moderate), cited per the GRADE "good practice
        // statement" model: the PRINCIPLE (govern interval quality by pace/HR;
        // scale the session down rather than push through) is backed by
        // Daniels' I-pace governance and Billat's intermittent-vVO2max work.
        // The specific ~2-4% MAGNITUDE has NO published source: it is a KB
        // practitioner heuristic / expert-opinion parameter, NOT claimed to
        // come from the cited studies. Grade stays the KB's Moderate (the
        // principle is Moderate-supported).
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Billat et al. 2000, Eur J Appl Physiol 81(3):188-196 (intermittent runs at vVO2max sustain VO2max longer)",
        ],
    )
    .contradicting(&[
        "The ~2-4% pace-cut magnitude is a practitioner heuristic with no direct study - an expert-opinion parameter, not from the cited works",
    ]),
    AUTOREG_EASY_CAP_001 => claim(
        "AUTOREG-EASY-CAP-001",
        "If you can't hold the planned easy pace under the heart-rate cap, slow down: heart rate, not pace, governs easy days.",
        // File 06 autoreg-033 (Moderate): the PRINCIPLE (easy days governed by
        // an intensity ceiling below threshold, so slow down to stay under it
        // rather than hold pace) is backed by Daniels' E-pace ceiling and
        // Seiler's polarized-distribution work (~75% of training below the
        // first threshold). Caveat: the literature caps easy intensity by a
        // PHYSIOLOGICAL threshold (VT1 / <2 mmol·L⁻¹); an HR ceiling as the
        // field proxy for that threshold is a practitioner operationalization,
        // not a directly-studied HR-percentage cutoff.
        Moderate,
        &[
            CITE_DANIELS_2022,
            "Seiler & Kjerland 2006, Scand J Med Sci Sports 16(1):49-56, PMID 16430681 (polarized distribution; ~75% below the first threshold)",
        ],
    )
    .contradicting(&[
        "Low-intensity is defined in the literature by a physiological threshold (VT1 / <2 mmol·L⁻¹), not a specific HR-% cap - the HR ceiling is a field proxy",
    ]),
    AUTOREG_NFOR_001 => claim(
        "AUTOREG-NFOR-001",
        "Performance dropping for 2 or more weeks with no clear reason, while 2 or more check-in scores stay low, calls for a dedicated recovery block; if it continues, see a professional.",
        // File 06 autoreg-042 grades this ExpertOpinion (0.30) despite the
        // Meeusen citation: never overstated to match the Strong File 08
        // safety-042 rule (which reuses SAFE-OTS-001).
        ExpertOpinion,
        &[CITE_MEEUSEN_2013],
    )
    .safety(),
    AUTOREG_HRV_LIFT_001 => claim(
        "AUTOREG-HRV-LIFT-001",
        "HRV-guided training is an endurance tool. For lifting, timing sessions by HRV did not improve strength or muscle gains in two small trials. So on lift days, HRV, resting heart rate or one low check-in only pause progress; a lift session gets lighter only after a hard first set or several run-down days.",
        Weak,
        &[
            "De Oliveira et al. 2019, Eur J Sport Sci 19(8):1092-1100 (HRV-gated resistance training RCT)",
            "Bittencourt et al. 2024, Front Physiol 15:1472702",
            "Manresa-Rocamora et al. 2021, Int J Environ Res Public Health 18(19):10299 (HRV-guided training meta-analysis)",
        ],
    )
    .contested("CQ-04")
    .review(6),
    AUTOREG_RHR_NOISE_001 => claim(
        "AUTOREG-RHR-NOISE-001",
        "Resting heart rate varies about 10% from day to day, and one evening of drinking can raise overnight heart rate by about 7 bpm. So one high morning means an illness check and an easier day; a full rest day needs 2 or more high days in a row, or symptoms.",
        Weak,
        &[
            "Buchheit 2014, Front Physiol 5:73 (heart-rate monitoring review)",
            "Pietila et al. 2018, JMIR Ment Health 5(1):e23 (alcohol and overnight heart rate)",
        ],
    )
    .contradicting(&[
        "Supersedes the single-reading rest rule (one morning 10+ bpm high meant a rest day)",
    ])
    .safety(),
    #[kb_only]
    HRV_SOURCE_001 => claim(
        "HRV-SOURCE-001",
        "Use one HRV source and timing consistently (on waking or overnight, never mixed): morning and overnight HRV agree at rest but respond differently to hard training, with overnight readings more sensitive to load.",
        Weak,
        &[
            "Nuuttila et al. 2024, Sports Med Open 10:120 (morning vs nocturnal HRV responses to intensified training)",
        ],
    )
    .contradicting(&["Single study of 24 recreational runners"])
    .review(6),
    #[kb_only]
    WELLNESS_Z_001 => claim(
        "WELLNESS-Z-001",
        "A single one-step change on one check-in question should never count as a big drop in how you feel: changes are judged against your own typical spread with a sensible minimum, and only today's check-in counts for today's call.",
        ExpertOpinion,
        &[
            "Saw, Main & Gastin 2016, Br J Sports Med 50(5):281-291 (subjective vs objective monitoring review)",
            "Guyatt et al. 2015, J Clin Epidemiol 68(5):597-600 (good practice statements)",
        ],
    )
    .contradicting(&["The minimum-spread value is an app parameter, not a published number"]),
    #[kb_only]
    LOG_EFFORT_001 => claim(
        "LOG-EFFORT-001",
        "Rate the effort of each set (RPE or reps in reserve) right after it. A pre-filled number can anchor the rating, so effort starts unrated and the target is shown only as a hint.",
        Moderate,
        &[
            "Zourdos et al. 2016, J Strength Cond Res 30(1):267-275 (reps-in-reserve RPE scale)",
            CITE_HALPERIN_2022,
            "Tversky & Kahneman 1974, Science 185(4157):1124-1131 (anchoring)",
        ],
    )
    .contradicting(&[
        "Anchoring by pre-filled effort ratings in lifting apps is inferred from general anchoring research; no lifting-specific trial",
    ]),
}
