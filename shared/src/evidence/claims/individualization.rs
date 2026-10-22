//! Individualization claims: File 08 per-rule rows (`individualization.rs`):
//! training age, scaling, re-entry, masters, time and equipment limits, the
//! onboarding lift-day and re-entry ramp rules, and the WHO 5-17 activity
//! reference tracked for under-18 profiles.

use crate::evidence::cite::{CITE_BOSQUET_2013, CITE_GRGIC_2018};
use crate::evidence::claim;
use crate::schema::EvidenceGrade::{ExpertOpinion, Moderate, Strong};

claim_group! {
    IndividualizationClaim => Individualization;
    DETRAIN_001 => claim(
        "DETRAIN-001",
        "Fitness fades at different rates when you stop: power within about a week, aerobic max (VO2max) fastest, strength slowest (about 7–12% over 8–12 weeks). You keep most of it if you hold intensity and cut volume. To scale down, drop extra exercises first and main lifts and intensity last, training each muscle at least twice a week; after a break, ramp back based on how long you were off rather than starting over.",
        Moderate,
        &[
            "Mujika & Padilla 2000, Sports Med 30(2):79-87",
            "Bosquet et al. 2013",
        ],
    )
    .contradicting(&["Re-entry brackets (Table 3.4b) are ExpertOpinion extrapolation"]),
    MASTERS_001 => claim(
        "MASTERS-001",
        "For adults 65 and over, aim for at least 1.2–1.6 g of protein per kg of bodyweight a day (about 0.4 g/kg per meal), because muscles respond less to protein with age. Include fast power work for everyday function, and allow more recovery when performance hasn't bounced back.",
        Moderate,
        &[
            "Fragala et al. 2019, JSCR 33(8):2019-2052 (NSCA position)",
            "Breen & Phillips 2011, Nutr Metab (Lond) 8:68 (anabolic resistance in the elderly)",
        ],
    ),
    TIMECAP_001 => claim(
        "TIMECAP-001",
        "When short on time, keep how often and how hard you train, and cut extra exercises first. A muscle can be maintained on about one session a week, because keeping muscle takes far less volume than building it.",
        Moderate,
        &[
            "Bickel, Cross & Bamman 2011, Med Sci Sports Exerc 43(7):1177-1187 (adaptations maintained on reduced volume)",
            "Mujika & Padilla 2000, Sports Med 30(2):79-87 & 30(3):145-154 (detraining)",
        ],
    ),
    DBLPROG_001 => claim(
        "DBLPROG-001",
        "Double progression: work within a rep range and add weight (dropping to the bottom of the range) once you hit the top of the range on all sets. By default, beginners add about 2.5 kg (5 lb) on upper-body lifts and 5 kg (10 lb) on lower-body lifts each session while they complete their reps.",
        ExpertOpinion,
        &[
            "Rippetoe & Kilgore 2011, Starting Strength: Basic Barbell Training (3rd ed., Aasgaard) (novice linear increments)",
            "Rippetoe & Baker 2013, Practical Programming for Strength Training (3rd ed., Aasgaard) (double progression)",
        ],
    ),
    SUBST_001 => claim(
        "SUBST-001",
        "With home or minimal equipment, swap in a lift with the same movement pattern (not the exact lift) and make up for lighter weights with more reps taken closer to failure, which largely preserves muscle growth.",
        ExpertOpinion,
        &[
            "Rippetoe & Kilgore 2011, Starting Strength (3rd ed., Aasgaard) (movement-pattern substitution)",
            "Schoenfeld, Grgic, Ogborn & Krieger 2017, J Strength Cond Res 31(12):3508-3523 (effort-matched loads preserve hypertrophy)",
        ],
    ),
    INDIV_TRAGE_001 => claim(
        "INDIV-TRAGE-001",
        "Your experience level comes from how often you can add weight (beginner: every session; intermediate: week to week; advanced: month to month), not from how you'd label yourself.",
        ExpertOpinion,
        &[
            "Rippetoe & Baker 2013, Practical Programming for Strength Training (3rd ed., Aasgaard)",
        ],
    ),
    SCALE_DOWN_001 => claim(
        "SCALE-DOWN-001",
        "When cutting back, drop extra and single-joint exercises first, then sets toward the minimum that still makes progress, then sessions (keeping each muscle trained at least twice a week), then your second goal. Keep intensity, weights and main lifts until last.",
        Strong,
        &[
            "Mujika & Padilla 2000 (volume cut 60-90% with adaptations retained if intensity held)",
        ],
    ),
    SCALE_UP_001 => claim(
        "SCALE-UP-001",
        "When adding training, first add volume to your main goal (from the minimum toward the productive range), then sessions per week, then extra exercises or variety, then weight; take on a second goal only once the first is progressing.",
        ExpertOpinion,
        &[
            "Kraemer & Ratamess 2004, Med Sci Sports Exerc 36(4):674-688 (ACSM progression models)",
            "Mujika & Padilla 2000, Sports Med 30(2):79-87 & 30(3):145-154",
        ],
    ),
    REENTRY_001 => claim(
        "REENTRY-001",
        "After a break, the restart depends on time off: 1–2 weeks off, about 90% of your weights; 2–4 weeks, about 80–85%; 4–8 weeks, about 70%; over 8 weeks, start again as a beginner. Intensity holds while sets build back up.",
        ExpertOpinion,
        &[
            "Mujika & Padilla 2000, Sports Med 30(2):79-87 & 30(3):145-154 (detraining time course)",
            "Bosquet et al. 2013, Scand J Med Sci Sports 23(3):e140-e149 (training-cessation meta-analysis)",
        ],
    ),
    LIFT_DAYS_001 => claim(
        "LIFT-DAYS-001",
        "Lifting is planned on the number of days you actually have (2–4 or more) by spreading the same weekly volume across them: when volume is matched, how often you train has no significant effect on strength.",
        Moderate,
        &[CITE_GRGIC_2018],
    )
    .contradicting(&["Which session split fits which number of days is coaching convention"]),
    REENTRY_RAMP_001 => claim(
        "REENTRY-RAMP-001",
        "After a break, lighter starting weights climb back to full over the return period. They apply only to numbers set before the break and replace, rather than add to, the general adjustment for time off. Strength holds up best during a break.",
        ExpertOpinion,
        &[CITE_BOSQUET_2013],
    )
    .contradicting(&["The ramp shape is an app default"]),
    // File 08 youth-pa-001: information and tracking for under-18 profiles
    // (the plan hold stays); never a program.
    YOUTH_PA_001 => claim(
        "YOUTH-PA-001",
        "Children and teenagers aged 5 to 17 should average at least 60 minutes a day of moderate-to-vigorous activity across the week, mostly aerobic, with vigorous activity and muscle- and bone-strengthening activity on at least 3 days a week. Moderate means about 5–6 out of 10 effort, vigorous about 7–8 out of 10; running and jumping strengthen bone.",
        Strong,
        &[
            "Bull et al. 2020, Br J Sports Med 54(24):1451-1462, doi:10.1136/bjsports-2020-102955 (WHO 2020 guidelines on physical activity and sedentary behaviour)",
            "World Health Organization 2020, WHO guidelines on physical activity and sedentary behaviour (Geneva: WHO)",
        ],
    ),
}
