//! Safety claims: stop signals, referrals and screening beyond File 09's five
//! referral deferrals (those live in [`super::canonical`]).
//!
//! The pain and illness stop rules come from File 06 (autoreg-043/045/046),
//! which grades them ExpertOpinion (0.30): safety-critical on consensus
//! authority, not trial evidence. Grades are never overstated to make a stop
//! signal look better-evidenced than it is.
//!
//! File 08 safety rules whose grade/citation match a File 09 referral claim
//! reuse it instead of getting a row: safety-044/onboard-050 → SAFE-CVD-001;
//! safety-045 → SAFE-PREG-001; safety-017/022/049 → SAFE-REDS-001 (the KB's
//! "safety-035" cross-refs in safety-017/-022 are a KB numbering bug; the
//! RED-S absolute rule is safety-049); safety-041/042 → SAFE-OTS-001;
//! safety-024/indiv-025 → ENV-001.

use crate::evidence::cite::{CITE_MEEUSEN_2013, CITE_MOUNTJOY_2023, CITE_SCHWELLNUS_2022};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong};

claim_group! {
    SafetyClaim => Safety;
    SAFE_PAIN_001 => claim(
        "SAFE-PAIN-001",
        "Sharp, pinpoint, joint or tendon pain during exercise is a reason to stop. Stop loading that area and check it before you carry on.",
        ExpertOpinion,
        &[
            "Cook & Purdam 2009, Br J Sports Med 43:409-416 (load-induced tendon pathology continuum)",
            "Silbernagel et al. 2007, Am J Sports Med 35(6):897-906 (pain-monitoring model)",
        ],
    )
    .safety(),
    ILLNESS_NECK_001 => claim(
        "ILLNESS-NECK-001",
        "An illness with a fever rules out hard exercise because of the risk of heart inflammation (myocarditis) and sudden cardiac death. Any fever, or symptoms below the neck such as chest congestion or body aches, means no training. Symptoms only above the neck, with no fever, allow lighter training.",
        ExpertOpinion,
        &[
            "Eichner 1993, Phys Sportsmed 21(1):125-135 (neck-check rule)",
            CITE_MEEUSEN_2013,
        ],
    )
    .safety(),
    ENV_001 => claim(
        "ENV-001",
        "In heat, ease off your pace and effort, allow about 10–14 days to adapt, drink enough, and stop at once on signs of heat illness: confusion, dizziness or no longer sweating. Above about 2,500 m, ease off until you adapt. In cold, warm up longer.",
        ExpertOpinion,
        &[
            "Armstrong et al. 2007, Med Sci Sports Exerc 39(3):556-572 (ACSM position stand on exertional heat illness; covers heat only - the altitude and cold parameters are practitioner-set)",
        ],
    )
    // File 08 safety-024: heat-illness signs (confusion, no sweating,
    // dizziness) are a hard STOP.
    .safety(),
    // File 08 safety-038/039 (graded pain, Table 4.1): this row and
    // SAFE-TENDON-001.
    SAFE_PAIN_STRUCT_001 => claim(
        "SAFE-PAIN-STRUCT-001",
        "Stop that exercise if the pain is sharp, in one spot or along the joint line, changes how you move, or comes with swelling. See a professional if it lasts; it may be an injury.",
        Moderate,
        &[
            "Silbernagel et al. 2007, Am J Sports Med 35(6):897-906 (pain-monitoring model)",
            "Cook & Purdam 2009, Br J Sports Med 43:409-416 (tendon pathology continuum)",
        ],
    )
    .safety(),
    SAFE_TENDON_001 => claim(
        "SAFE-TENDON-001",
        "If tendon pain stays at or below 3–5 out of 10, holds steady during training and for 24 hours after, and isn't getting worse week to week, keep training and keep an eye on it rather than resting completely. If it goes above 5 out of 10, is worse afterwards or rises week to week, train lighter, avoid positions that press on the tendon, and see a professional if it lasts.",
        Moderate,
        &[
            "Silbernagel et al. 2007 pain-monitoring model",
            "Cook & Purdam continuum",
        ],
    )
    .safety(),
    SAFE_PEDS_001 => claim(
        "SAFE-PEDS-001",
        "Children and teenagers should train under a qualified coach, with technique and skill ahead of added weight. lollop doesn't plan near-max weights or 1-rep-max tests for them. There is no agreed age cutoff for this.",
        Strong,
        &[
            "Lloyd, Faigenbaum et al. 2014 Intl Consensus, BJSM 48:498-505",
            "Lloyd & Oliver 2012 YPD",
        ],
    )
    .safety(),
    SAFE_PREG_WARN_001 => claim(
        "SAFE-PREG-WARN-001",
        "During pregnancy, stop and contact your provider if you notice vaginal bleeding, fluid leaking, regular painful contractions, breathlessness before exercise, dizziness or feeling faint, headache, chest pain, calf pain or swelling, muscle weakness affecting your balance, or the baby moving less. Conditions such as placenta previa after 26 weeks, pre-eclampsia or high blood pressure caused by pregnancy, a weak cervix or severe anemia need a plan from your provider.",
        Strong,
        &["ACOG Committee Opinion 804, 2020"],
    )
    .safety(),
    SAFE_PREG_AVOID_001 => claim(
        "SAFE-PREG-AVOID-001",
        "In pregnancy, avoid lying flat on your back for long, overheating, sports with contact or fall risk, scuba diving, exercise at altitudes above 2,500 m, and holding your breath while lifting.",
        Strong,
        &["ACOG Committee Opinion 804, 2020"],
    )
    .safety(),
    SAFE_INJURY_001 => claim(
        "SAFE-INJURY-001",
        "lollop doesn't plan rehab. With a current injury under care, recent surgery or ongoing rehab, follow your doctor or physio, and go back to general training only once they clear you. No fixed wait after surgery is established.",
        // The LOWEST-graded of the File 08 safety rules, yet still
        // safety-critical, conservative by design (File 08 §5.4).
        ExpertOpinion,
        &[
            "Ardern et al. 2016, Br J Sports Med 50(14):853-864 (consensus on return to sport)",
            "ACSM's Guidelines for Exercise Testing and Prescription (10th ed., 2018, Wolters Kluwer)",
        ],
    )
    .safety(),
    ILLNESS_RTT_001 => claim(
        "ILLNESS-RTT-001",
        "After a cold or chest infection, come back step by step. Check your symptoms each day. Make the first session a 10–20 minute test at 60–70% of your normal effort, then recheck after 24 hours. Stop and see a doctor if you get chest pain, breathlessness, dizziness, a racing or pounding heartbeat, unusual tiredness, or muscle or joint pain. The same goes if the work feels harder than usual. After a moderate or severe infection, let a doctor guide your return.",
        ExpertOpinion,
        &[CITE_SCHWELLNUS_2022],
    )
    .safety(),
    RETURN_HOLD_001 => claim(
        "RETURN-HOLD-001",
        "After a pain or illness hold ends, return gradually: the first session back is easy (lifts with 3 or more reps left, runs at easy effort), then the plan resumes. Return after pain only once the movement is pain-free through its full range, and after illness only once symptoms are gone and there is no fever.",
        ExpertOpinion,
        &[
            CITE_SCHWELLNUS_2022,
            "Ardern et al. 2016, Br J Sports Med 50(14):853-864 (consensus on return to sport)",
        ],
    )
    .contradicting(&[
        "The single easy session is an app default; the sources describe a gradual return without fixing its length",
    ])
    .safety(),
    ILLNESS_COLD_RUN_001 => claim(
        "ILLNESS-COLD-RUN-001",
        "A head cold means a runny or blocked nose, sneezing or a sore throat, with no fever, chills, body aches, chest symptoms or unusual tiredness. With one, skip long runs and hard sessions. If you feel up to it, run at most 10–20 minutes at 60–70% of your normal effort; otherwise rest. Stop on any warning sign. Fever, chills, body aches or chest symptoms mean no training.",
        ExpertOpinion,
        &[
            CITE_SCHWELLNUS_2022,
            "Eichner 1993, Phys Sportsmed 21(1):125-135 (neck-check rule)",
        ],
    )
    .contradicting(&[
        "The older neck-check rule allows reduced-intensity training with a head cold without a time limit; the 10-20 minute cap is the consensus exercise-test dose, applied here to every run while symptoms last",
    ])
    .safety(),
    REDS_SCREEN_001 => claim(
        "REDS-SCREEN-001",
        "Check for signs of under-eating (RED-S) regularly, not just once. For women, missed or irregular periods are a main warning sign; for men, low sex drive or fewer morning erections. Any yes leads to the same advice as other signs of under-eating: see a doctor, a registered dietitian or a mental-health professional.",
        Strong,
        &[CITE_MOUNTJOY_2023],
    )
    .safety(),
    #[kb_only]
    SCREEN_PARQ_001 => claim(
        "SCREEN-PARQ-001",
        "Ask the seven PAR-Q+ health questions one at a time: a heart condition or high blood pressure; chest pain; dizziness or fainting in the last year; another chronic condition; medication for a chronic condition; a bone, joint or soft-tissue problem that more activity could worsen; a doctor's advice to exercise only under supervision. All \"no\" clears you for activity; any \"yes\" needs follow-up or a doctor's clearance.",
        Strong,
        &[
            "Warburton, Bredin, Jamnik & Gledhill 2011, Health Fit J Can 4(2):38-46 (validation of the PAR-Q+)",
        ],
    )
    .safety(),
    SCREEN_ACSM_001 => claim(
        "SCREEN-ACSM-001",
        "Whether you need a doctor's OK before exercising depends on whether you already exercise regularly, any signs or symptoms of heart, metabolic or kidney disease, and how hard you plan to train. Anyone with symptoms stops and gets a doctor's OK first, however active they are.",
        Strong,
        &[
            "Riebe et al. 2015, Med Sci Sports Exerc 47(11):2473-2479 (ACSM preparticipation screening update)",
        ],
    )
    .safety(),
}
