//! Feedback and adherence claims: File 05 per-rule rows (`feedback.rs`) and
//! `12-adherence.md`.

use crate::evidence::cite::{
    CITE_BOSQUET_2013, CITE_GRGIC_2018, CITE_MEEUSEN_2013, CITE_MOUNTJOY_2023, CITE_NHS_C25K,
};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong, Weak};

claim_group! {
    FeedbackClaim => Feedback;
    // File 05 feedback-003: the rule behind the app's goal framing is graded
    // Moderate and contested in the KB, below File 09's GOAL-PROCESS-001.
    FB_PROCESS_GOAL_001 => claim(
        "FB-PROCESS-GOAL-001",
        "Next steps are framed as things you control (pacing, effort, technique) rather than results; whether these beat specific result goals in sport is still debated.",
        Moderate,
        &[
            "Williamson et al. 2022, Int Rev Sport Exerc Psychol (goal-setting meta-analysis)",
            "Kingston & Hardy 1997",
        ],
    )
    .contradicting(&[
        "Kyllo & Landers 1995, J Sport Exerc Psychol: specific, moderately difficult goals improve sport performance",
    ])
    .contested("CQ-F05-01"),
    FB_PACING_001 => claim(
        "FB-PACING-001",
        "If the second half of an even-effort run is more than about 3% slower than the first, the tip is to start easier and aim for an even or faster second half.",
        Moderate,
        &[
            "Hanley 2016, J Sports Sci 34(17):1637-1645",
            "Smyth 2018",
            "Abbiss & Laursen 2008",
        ],
    ),
    FB_BADDAY_001 => claim(
        "FB-BADDAY-001",
        "Missing a target pace at a very high effort is usually normal day-to-day variation, and the session still counted.",
        ExpertOpinion,
        &[
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668 (positive verbal feedback)",
            "Williamson et al. 2022, Int Rev Sport Exerc Psychol 17(2):1050-1078 (process focus)",
        ],
    ),
    #[kb_only]
    FB_TONE_001 => claim(
        "FB-TONE-001",
        "On planned-easy sessions reinforce restraint and praise not chasing pace as the win; on planned-hard praise completion/effort.",
        ExpertOpinion,
        &[
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668",
            "Carpentier & Mageau 2013, Psychol Sport Exerc 14(3):423-435 (change-oriented feedback)",
        ],
    ),
    FB_TREND_001 => claim(
        "FB-TREND-001",
        "When a trend improves, the feedback names your consistency and one next step. A plateau is normal; change one thing at a time. A drop explained by heavy training gets a recovery-first answer and a suggested lighter week.",
        ExpertOpinion,
        &[
            "Williamson et al. 2022, Int Rev Sport Exerc Psychol 17(2):1050-1078",
            "Bandura 1997, Self-Efficacy: The Exercise of Control (Freeman)",
        ],
    ),
    FB_RECOVERY_001 => claim(
        "FB-RECOVERY-001",
        "Two or three warning signs together for one to two weeks (worse performance, low mood, a higher resting heart rate or lower HRV, poor sleep) point to under-recovery; that calls for a calm recovery check rather than praise.",
        Moderate,
        &[
            CITE_MEEUSEN_2013,
            "POMS mood monitoring (McNair et al. 1971); Kajaia et al. 2017, Georgian Med News (264):97-103",
        ],
    )
    .safety(),
    FB_BEHAVIOR_001 => claim(
        "FB-BEHAVIOR-001",
        "Training through pain, distress over missed sessions, or rapidly escalating volume can signal an unhealthy relationship with exercise. Rest counts as part of training, and streaks or sudden volume jumps are never rewarded.",
        ExpertOpinion,
        &[
            CITE_MOUNTJOY_2023,
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668 (rewarding behavior vs outcomes)",
        ],
    )
    .safety(),
    FB_PROVISIONAL_001 => claim(
        "FB-PROVISIONAL-001",
        "Starting values come from people like you, not from you yet, so treat them as a first guess: they move toward your own numbers as you log.",
        ExpertOpinion,
        &[
            "Kraemer & Ratamess 2004, Med Sci Sports Exerc 36(4):674-688 (ACSM progression models; individualization principle)",
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668 (autonomy-supportive framing)",
        ],
    ),
    // File 05 feedback-019/020 (RIR-vs-target lifting feedback).
    FB_RIR_001 => claim(
        "FB-RIR-001",
        "Each set is compared with its target. Hitting the reps with at least the planned reps left means you've mastered the weight, and the planned progression goes ahead. Hitting them only by going to failure, when 2–3 reps left were planned, means holding or slightly lowering the weight. Anything in between is on track.",
        Moderate,
        &[
            "Zourdos et al. 2016, J Strength Cond Res 30:267",
            "Graham & Cleather 2021",
            "Halperin et al. 2022",
        ],
    ),
    FB_INTERVAL_MASTERY_001 => claim(
        "FB-INTERVAL-MASTERY-001",
        "Hitting your interval or threshold paces at or below the target effort means the session did what it was meant to.",
        Moderate,
        &[
            "Bandura 1997, Self-Efficacy: The Exercise of Control (Freeman) (mastery experiences)",
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668",
        ],
    ),
    FB_MASTERY_ANCHOR_001 => claim(
        "FB-MASTERY-ANCHOR-001",
        "Seeing what you've already achieved helps you stick with training, so wins are named specifically: a new best, a finished session, a hard week done.",
        Strong,
        &[
            "Bandura 1997",
            "McAuley & Blissmer 2000, Exerc Sport Sci Rev 28:85-88",
        ],
    ),
    FB_VERBOSITY_001 => claim(
        "FB-VERBOSITY-001",
        "Feedback is sized to your experience: beginners get one takeaway in plain words, always with the reason; experienced users may see 2–3 numbers, still led by the one that matters most.",
        ExpertOpinion,
        &[
            "Williamson et al. 2022, Int Rev Sport Exerc Psychol 17(2):1050-1078",
            "Deci, Koestner & Ryan 1999, Psychol Bull 125(6):627-668",
        ],
    ),
    FB_BSI_FEMALE_001 => claim(
        "FB-BSI-FEMALE-001",
        "If you're a woman with a possible bone stress injury, tell your doctor about your periods and how much you eat; missed periods and under-eating raise the risk. Don't try to diagnose it yourself.",
        ExpertOpinion,
        &[
            CITE_MOUNTJOY_2023,
            "Warden, Davis & Fredericson 2014, J Orthop Sports Phys Ther 44(10):749-765 (bone stress injuries)",
        ],
    )
    .safety(),
    ADH_MISSED_001 => claim(
        "ADH-MISSED-001",
        "Missing one session is normal and barely affects progress or habit. Carry on with the plan, and don't double up to make it back.",
        Weak,
        &[
            "Lally et al. 2010, Eur J Soc Psychol 40(6):998-1009 (habit formation in the real world)",
            "Ogasawara et al. 2013, Eur J Appl Physiol 113(4):975-985 (periodic training breaks vs continuous training)",
            CITE_BOSQUET_2013,
        ],
    ),
    ADH_RESCHED_001 => claim(
        "ADH-RESCHED-001",
        "A missed or clashing session can move to another free day that week if at least 48 hours remain before the next hard session, and long or hard runs stay at least 24 hours from heavy leg days. Otherwise skip it. Weekly volume drives strength gains, not the day, and beginners who picked their own session order gained at least as much.",
        Weak,
        &[
            "McNamara & Stearne 2010, J Strength Cond Res 24(1):17-22 (flexible nonlinear periodization)",
            CITE_GRGIC_2018,
        ],
    )
    .contradicting(&[
        "The same-week and 48-hour limits are an app default; the flexible-order trial was small (16 beginners)",
    ]),
    ADH_PROGRESS_001 => claim(
        "ADH-PROGRESS-001",
        "The plan moves forward by completed work, not the calendar: a week is repeated if fewer than about two-thirds of its sessions were done (Couch to 5K: fewer than 2 of 3 runs), with no penalty.",
        ExpertOpinion,
        &[CITE_NHS_C25K],
    )
    .contradicting(&["The two-thirds threshold is an app default"]),
    ADH_SELFMON_001 => claim(
        "ADH-SELFMON-001",
        "Tracking what you do and comparing it with your plan (sessions done vs planned each week) is among the most effective ways to help people stay active.",
        Moderate,
        &[
            "Michie et al. 2009, Health Psychol 28(6):690-701 (behaviour-change techniques meta-regression)",
        ],
    ),
    #[kb_only]
    ADH_DAYS_001 => claim(
        "ADH-DAYS-001",
        "Picking your own training days (and times) helps you follow through: concrete plans like \"if it's Tuesday at 7, I lift\" improve goal attainment.",
        Moderate,
        &[
            "Gollwitzer & Sheeran 2006, Adv Exp Soc Psychol 38:69-119 (implementation intentions meta-analysis)",
        ],
    ),
    #[kb_only]
    ADH_GAMIFY_001 => claim(
        "ADH-GAMIFY-001",
        "Points, badges and challenges give a small-to-moderate short-term boost in activity that mostly fades afterwards, so neutral consistency stats are fine, but streak counters and pressure that punishes rest are not used.",
        Moderate,
        &[
            "Mazeas et al. 2022, J Med Internet Res 24(1):e26779 (gamification and physical activity meta-analysis)",
        ],
    ),
    #[kb_only]
    ADH_PRAISE_001 => claim(
        "ADH-PRAISE-001",
        "Feedback after good performances, such as a personal best or a completed session, improves learning more than feedback after poor ones, through motivation and confidence.",
        Moderate,
        &[
            "Chiviacowsky & Wulf 2007, Res Q Exerc Sport 78(2):40-47 (feedback after good trials)",
            "Wulf & Lewthwaite 2016, Psychon Bull Rev 23(5):1382-1414 (OPTIMAL theory of motor learning)",
        ],
    ),
    #[kb_only]
    ADH_RATIONALE_001 => claim(
        "ADH-RATIONALE-001",
        "Explaining the reason behind each change to a plan supports people's sense of choice and wellbeing; programs that give a meaningful rationale are associated with larger benefits.",
        Moderate,
        &[
            "Ntoumanis et al. 2021, Health Psychol Rev 15(2):214-244 (self-determination theory interventions meta-analysis)",
        ],
    ),
}
