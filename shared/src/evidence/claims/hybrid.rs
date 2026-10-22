//! Hybrid (concurrent training) claims: File 10 per-rule rows (`hybrid.rs`) and
//! `13-hybrid-update.md`. File 10 local CQ-02 maps to global CQ-06; local CQ-01
//! has no global counterpart and is CQ-F10-01.

use crate::evidence::cite::{
    CITE_HUIBERTS_2024, CITE_MEEUSEN_2013, CITE_MOUNTJOY_2023, CITE_SCHUMANN_2022,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Weak};

claim_group! {
    HybridClaim => Hybrid;
    HYB_CAP_001 => claim(
        "HYB-CAP-001",
        "When lifting and running share a week, keep long or hard runs at least 24 hours away from heavy leg sessions, in both directions. When you run 4 or more days or 40+ km (25+ miles) a week, cap lower-body lifting at 2 sessions a week and trim lower-body muscle-building volume by about 20–33%.",
        Moderate,
        &[
            "Jones et al. 2013 (endurance-frequency dose-response for interference)",
            "Wilson et al. 2012, J Strength Cond Res 26(8):2293-2307 (concurrent-training meta-analysis)",
            "Doma, Deakin & Bentley 2017, Sports Med 47:2187-2200 (residual fatigue)",
            "Baar 2014, Sports Med 44(Suppl 2):S117-S125",
            CITE_SCHUMANN_2022,
        ],
    )
    .contradicting(&[
        "Schumann et al. 2022 (43-trial meta): interference on strength/hypertrophy in healthy adults is small; the caps are a conservative convention",
    ])
    .contested("CQ-15"),
    #[kb_only]
    HYB_TRAINED_001 => claim(
        "HYB-TRAINED-001",
        "Training background does not reliably change how much running blunts lifting gains: one meta-analysis found a small lower-body strength cost only in trained lifters, while two others found no difference by training status.",
        Weak,
        &[CITE_HUIBERTS_2024, CITE_SCHUMANN_2022],
    )
    .contradicting(&[
        "Petre et al. 2021, Sports Med 51(5):991-1010: lower-body 1RM was blunted in trained individuals only (ES -0.35)",
    ])
    .contested("CQ-06"),
    HYB_THRESH_001 => claim(
        "HYB-THRESH-001",
        "Expect smaller strength and muscle gains when you do endurance work more than 3–4 days a week or harder than 80% of your aerobic max (VO2max); cap endurance or expect slower lifting progress.",
        Moderate,
        &[
            "Wilson et al. 2012, J Strength Cond Res 26(8):2293-2307",
            "Hickson 1980, Eur J Appl Physiol 45:255-263 (original interference study)",
            "Jones et al. 2013; Baar 2014, Sports Med 44(Suppl 2):S117-S125",
        ],
    )
    .contested("CQ-06"),
    HYB_MAINT_001 => claim(
        "HYB-MAINT-001",
        "Maintaining a quality you're not focusing on takes about a third of the training that builds it (about 2 short sessions a week), which frees recovery for your main goal.",
        ExpertOpinion,
        &[
            "Bickel, Cross & Bamman 2011, Med Sci Sports Exerc 43(7):1177-1187 (adaptations retained on ~1/3 volume)",
            "Blagrove, Howatson & Hayes 2018, Sports Med 48(5):1117-1149 (running economy maintenance)",
        ],
    ),
    HYB_BSI_001 => claim(
        "HYB-BSI-001",
        "Above about 64 km (40 miles) of running a week, watch more closely for bone stress injury; strength training helps protect bone if you eat enough.",
        Moderate,
        &["Warden 2021, Curr Osteoporos Rep"],
    )
    .safety(),
    HYB_PROG_001 => claim(
        "HYB-PROG-001",
        "When you both lift and run, grow weekly running by about 10% at most and avoid sudden jumps. The ratio of this week's load to your usual load is only a tracking aid, never an injury predictor.",
        Moderate,
        &[
            "Gabbett 2016; ACWR literature (criticized - Impellizzeri 2020: tracking heuristic, not predictor)",
        ],
    )
    .contradicting(&[
        "Large runner RCT: 10% rule no better than a standard program",
        "Impellizzeri 2020 (conceptual flaws)",
    ])
    .safety()
    .contested("CQ-05"),
    HYB_DELOAD_001 => claim(
        "HYB-DELOAD-001",
        "Take a lighter week when at least 2–3 signs of overreaching last more than 1–2 weeks, such as resting heart rate 5–7 bpm or more above your normal, a 7-day heart-rate variability (HRV) trend down more than about 15% for 3–5 days, or falling sleep, mood or performance.",
        Moderate,
        &[
            "Foster 2001, J Strength Cond Res 15(1):109-115 (session-RPE monitoring)",
            CITE_MEEUSEN_2013,
            "Plews et al. 2013, Sports Med 43(9):773-781 (HRV trend)",
        ],
    )
    .contradicting(&[
        "Bellenger 2016 (resting HRV may not reliably detect overreaching)",
        "Plews (both high and low HRV can signal problems)",
    ])
    .safety()
    .contested("CQ-04")
    .review(6),
    HYB_DURATION_001 => claim(
        "HYB-DURATION-001",
        "Endurance work holds back lifting gains more the more often you do it (correlation −0.26 to −0.35) and the longer each session lasts (−0.29 to −0.75, reaching −0.75 for muscle growth); how long you go continuously matters most. No clear point at which this starts is known.",
        Moderate,
        &["Wilson 2012 moderator analysis"],
    )
    // File 10 local CQ-02 (onset threshold undefined) → global CQ-06.
    .contested("CQ-06"),
    HYB_SCHED_001 => claim(
        "HYB-SCHED-001",
        "Schedule your top-priority training when you're freshest: early in the week or after a rest day.",
        ExpertOpinion,
        &[
            "Baar 2014, Sports Med 44(Suppl 2):S117-S125 (schedule priority quality when fresh)",
            "Robineau et al. 2016, J Strength Cond Res 30(3):672-683",
        ],
    ),
    HYB_CHO_001 => claim(
        "HYB-CHO-001",
        "On days with two sessions, fully refuel with carbohydrate between the endurance session and the lift; low carbohydrate stores make endurance work hold back lifting gains more.",
        // File 10 rule entry grades Weak (0.40); the CAP-8 table row says
        // Weak-Moderate, registered at the rule entry's Weak floor.
        Weak,
        &["Baar 2014, Sports Med 44(Suppl 2):S117-S125"],
    ),
    HYB_PHASE_001 => claim(
        "HYB-PHASE-001",
        "Early in a training cycle, keep strength and endurance work apart to limit interference. Closer to an event, combine them on purpose (moderate weights, high reps, short rests), accepting some interference for event-specific fitness. For hybrid races, aim for 2–3 strength and 3–4 endurance sessions a week.",
        ExpertOpinion,
        &[
            "Issurin 2010, Sports Med 40(3):189-206 (block periodization of qualities)",
            "Baar 2014, Sports Med 44(Suppl 2):S117-S125",
        ],
    ),
    HYB_EA_001 => claim(
        "HYB-EA-001",
        "Eat enough to cover your training and avoid under-eating (RED-S), especially if you do a lot of endurance work, are lean or are female, so you don't add to the risk of a bone stress injury.",
        ExpertOpinion,
        &[
            CITE_MOUNTJOY_2023,
            "Warden, Davis & Fredericson 2014, J Orthop Sports Phys Ther 44(10):749-765",
        ],
    )
    .safety(),
    HYB_TENDON_001 => claim(
        "HYB-TENDON-001",
        "No study has tested how combined training affects tendon stiffness, so go carefully when raising high running volume and heavy lifting at the same time.",
        Weak,
        &["Baar 2014 (evidence gap noted)"],
    )
    .safety(),
    HYB_SEX_001 => claim(
        "HYB-SEX-001",
        "Combining running with lifting can slightly blunt lower-body strength gains in men; this has not been seen in women (few studies). Upper-body strength, power and aerobic max (VO2max) show no difference between the sexes.",
        Moderate,
        &[CITE_HUIBERTS_2024],
    )
    .contradicting(&["Female-specific data are sparse"]),
    HYB_RUNCAP_001 => claim(
        "HYB-RUNCAP-001",
        "Keeping running to 3 days a week while strength or muscle is also a main goal is a cautious default, not a limit backed by evidence. Recent meta-analyses found that how often you combine lifting and running didn't change strength or muscle results; spacing sessions 3+ hours apart and keeping runs shorter matter more.",
        ExpertOpinion,
        &[
            CITE_SCHUMANN_2022,
            "Held et al. 2026, Sports Med 56(6):1489-1512 (concurrent-training umbrella review, 17 meta-analyses)",
        ],
    )
    .contradicting(&[
        "Wilson et al. 2012, J Strength Cond Res 26(8):2293-2307: more endurance days per week correlated with smaller strength and muscle gains",
    ])
    .contested("CQ-06"),
    HYB_MAINT_002 => claim(
        "HYB-MAINT-002",
        "To maintain a quality you're not focusing on, hold its intensity and cut its volume. Endurance held for up to 15 weeks on 2 sessions a week, or with a third to two-thirds less volume at the same intensity. Strength and muscle held for up to 32 weeks on one session a week, one set per exercise at the same relative weight; older adults may need 2 sessions and 2–3 sets.",
        Moderate,
        &[
            "Spiering, Mujika, Sharp & Foulis 2021, J Strength Cond Res 35(5):1449-1458 (minimal maintenance dose)",
        ],
    )
    .contradicting(&["Limited athlete-specific data"]),
    HYB_RE_DOSE_001 => claim(
        "HYB-RE-DOSE-001",
        "For running economy, choose heavy strength work (80% of max or more) and/or jump training; moderate-weight (40–79%) and isometric programs did not improve running economy.",
        Moderate,
        &[
            "Llanos-Lagos et al. 2024, Sports Med 54(4):895-932 (strength training and running economy)",
            "Ronnestad & Mujika 2014, Scand J Med Sci Sports 24(4):603-612",
        ],
    )
    .contradicting(&["Evidence for combined heavy-plus-jump programs is weaker"]),
}
