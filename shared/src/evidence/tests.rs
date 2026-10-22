use super::*;
use std::collections::HashSet;

#[test]
fn claim_ids_are_unique() {
    let mut seen = HashSet::new();
    for c in all_claims() {
        assert!(
            seen.insert(c.claim_id),
            "duplicate claim_id: {}",
            c.claim_id
        );
    }
}

#[test]
fn contested_question_ids_are_unique() {
    let mut seen = HashSet::new();
    for q in CONTESTED_QUESTIONS {
        assert!(
            seen.insert(q.id),
            "duplicate contested-question id: {}",
            q.id
        );
    }
}

#[test]
fn citations_name_real_published_work_not_internal_refs() {
    // Citations must name real, internet-findable
    // published work: never internal file/KB-index references. The same
    // guard covers `contradicting` notes, which may cross-reference a
    // contested question (CQ-...) or another registered claim id, but must
    // not cite internal KB rule numbers or file indices as if they were
    // evidence.
    const MARKERS: &[&str] = &["(CAP-", "File 0", "§"];
    // Lowercase KB rule-id prefixes (e.g. "running-016", "strength-012")
    // are internal references, never published work. Contested-question
    // (CQ-...) and uppercase claim-id cross-refs match none of these.
    const RULE_PREFIXES: &[&str] = &[
        "running-0",
        "strength-0",
        "feedback-0",
        "autoreg-0",
        "hybrid-0",
        "onboard-0",
        "safety-0",
    ];
    for c in all_claims() {
        for cite in c.primary_citations {
            for marker in MARKERS {
                assert!(
                    !cite.contains(marker),
                    "{} primary_citation contains internal-ref marker {:?}: {:?}",
                    c.claim_id,
                    marker,
                    cite
                );
            }
        }
        for note in c.contradicting {
            for marker in MARKERS {
                assert!(
                    !note.contains(marker),
                    "{} contradicting note contains internal-ref marker {:?}: {:?}",
                    c.claim_id,
                    marker,
                    note
                );
            }
            for prefix in RULE_PREFIXES {
                assert!(
                    !note.contains(prefix),
                    "{} contradicting note cites internal KB rule id {:?}: {:?}",
                    c.claim_id,
                    prefix,
                    note
                );
            }
        }
    }
}

#[test]
fn claim_ids_name_their_rows() {
    // `ClaimId::entry` indexes its group's table: every id must name its own
    // row, spelled like its `claim_id`.
    for c in all_claims() {
        assert!(
            std::ptr::eq(c.id.entry(), c),
            "{} is out of place",
            c.claim_id
        );
        assert_eq!(format!("{:?}", c.id), c.claim_id.replace('-', "_"));
    }
}

/// Every `claims::X` / `ClaimId::X` named by non-test engine code.
fn cited_claims() -> HashSet<String> {
    fn walk(dir: &std::path::Path, out: &mut HashSet<String>) {
        for entry in std::fs::read_dir(dir).expect("src dir").flatten() {
            let path = entry.path();
            let name = entry.file_name();
            // The registry itself and test code cite nothing.
            if path.is_dir() {
                if name != "evidence" && name != "tests" {
                    walk(&path, out);
                }
                continue;
            }
            if name == "tests.rs" || path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("readable source");
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap_or("");
            for line in code.lines() {
                let line = line.split("//").next().unwrap_or("");
                for prefix in ["claims::", "ClaimId::"] {
                    for (i, _) in line.match_indices(prefix) {
                        let id: String = line[i + prefix.len()..]
                            .chars()
                            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                            .collect();
                        out.insert(id);
                    }
                }
            }
        }
    }
    let mut out = HashSet::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut out,
    );
    out
}

#[test]
fn every_surfaced_claim_is_cited() {
    // A surfaced claim is one some engine rule grades with; a row the
    // engine never cites is marked `#[kb_only]`, and the marker comes off
    // once a rule starts citing it. Myths are blocked from both.
    let cited = cited_claims();
    for c in all_claims().filter(|c| !c.is_blocked()) {
        let name = format!("{:?}", c.id);
        if c.surfaced {
            assert!(
                cited.contains(&name),
                "{} is cited by no engine rule: mark it #[kb_only]",
                c.claim_id
            );
        } else {
            assert!(
                !cited.contains(&name),
                "{} is cited by an engine rule: drop its #[kb_only] marker",
                c.claim_id
            );
        }
    }
}

#[test]
fn every_contested_ref_resolves() {
    for c in all_claims() {
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
    let expected: HashSet<ClaimId> = [
        claims::SAFE_REDS_001,
        claims::SAFE_OTS_001,
        claims::SAFE_BSI_001,
        claims::SAFE_PREG_001,
        claims::SAFE_CVD_001,
        claims::SAFE_PAIN_001,
        claims::ILLNESS_NECK_001,
        claims::ENV_001,
        claims::PLYO_001,
        claims::PLYO_PREREQ_001,
        claims::STR_2FOR2_001,
        claims::STR_DLPEAK_001,
        claims::STR_CONJ_001,
        claims::STR_PLYO_SCHED_001,
        claims::STR_1RMTEST_001,
        claims::HYP_PAIN_SHIFT_001,
        claims::HYP_VOLRAMP_SAFE_001,
        claims::HYP_SKILL_RIR_001,
        claims::HYP_FAIL_SAFE_001,
        claims::HYP_PAIN_SWAP_001,
        claims::RUN_HRRECALC_001,
        claims::RUN_SPIKE_BLOCK_001,
        claims::RUN_TAPER_001,
        claims::RUN_QUALITY_001,
        claims::RUN_C25K_001,
        claims::RUN_DOWNWEEK_001,
        claims::RUN_RUNWALK_EXT_001,
        claims::FB_RECOVERY_001,
        claims::FB_BEHAVIOR_001,
        claims::AUTOREG_FALLBACK_001,
        claims::HYB_BSI_001,
        claims::HYB_PROG_001,
        claims::HYB_DELOAD_001,
        claims::MYTH_NO_PAIN_JOINT,
        // File 08 safety-038/039 (graded pain rules).
        claims::SAFE_PAIN_STRUCT_001,
        claims::SAFE_TENDON_001,
        // File 08 safety-011 (pediatric), safety-046
        // (pregnancy warning signs), safety-047 (pregnancy avoid-list),
        // safety-048 (injury/rehab deferral: ExpertOpinion yet
        // safety-critical, conservative by design).
        claims::SAFE_PEDS_001,
        claims::SAFE_PREG_WARN_001,
        claims::SAFE_PREG_AVOID_001,
        claims::SAFE_INJURY_001,
        // File 06 autoreg-042 (NFOR cluster), File 05
        // feedback-035 (female BSI clinician prompt), File 10 hybrid-024
        // (energy-availability guard) + hybrid-025 (tendon-stiffness
        // conservative dual progression).
        claims::AUTOREG_NFOR_001,
        claims::FB_BSI_FEMALE_001,
        claims::HYB_EA_001,
        claims::HYB_TENDON_001,
        // File 11 (calisthenics) safety_critical set (extracted/11-calisthenics.md
        // §4 disallowed-failure set + tendon/static/wrist gates + the Valsalva BP row).
        claims::CAL_EFAIL_SAFE_001,
        claims::CAL_TENDON_001,
        claims::CAL_INJURY_STATIC_001,
        claims::CAL_INJURY_WRIST_001,
        claims::CAL_BP_VALSALVA_001,
        // 2026-10 update (extracted/12-*.md, 13-*.md): C25K week table
        // (mirrors running-025), noisy-RHR corroboration, illness return
        // check, return after a hold, recurring RED-S screen, PAR-Q+ items,
        // ACSM symptom screen, and the onboarding 30-day-longest-run anchor.
        claims::RUN_C25K_002,
        claims::AUTOREG_RHR_NOISE_001,
        claims::ILLNESS_RTT_001,
        claims::RETURN_HOLD_001,
        claims::REDS_SCREEN_001,
        claims::SCREEN_PARQ_001,
        claims::SCREEN_ACSM_001,
        claims::RUN_START_001,
        claims::ILLNESS_COLD_RUN_001,
    ]
    .into_iter()
    .collect();
    let actual: HashSet<ClaimId> = all_claims()
        .filter(|c| c.safety_critical)
        .map(|c| c.id)
        .collect();
    assert_eq!(actual, expected, "safety_critical set drifted from the KB");
}

#[test]
fn file09_canonical_claims_all_present() {
    // Criterion: the File 09 canonical claims (and 21 myths) must never be
    // dropped when per-rule entries are added.
    let canonical = [
        claims::HYP_VOL_001,
        claims::HYP_LOAD_001,
        claims::PERIOD_001,
        claims::STR_INTENT_001,
        claims::CONC_RE_001,
        claims::CONC_INTERF_001,
        claims::AUTOREG_RIR_001,
        claims::STR_TRAGE_001,
        claims::GOAL_PROCESS_001,
        claims::FEEDBACK_001,
        claims::TAPER_001,
        claims::HYP_FAIL_001,
        claims::HYP_LENGTH_001,
        claims::AUTOREG_PCT_001,
        claims::AUTOREG_VL_001,
        claims::TAPER_STR_001,
        claims::RUN_DIST_001,
        claims::RUN_SPIKE_001,
        claims::CONC_ORDER_001,
        claims::CONC_SEP_001,
        claims::CONC_MODE_001,
        claims::HRV_001,
        claims::WELLNESS_001,
        claims::RUN_VDOT_001,
        claims::LOAD_TRIMP_001,
        claims::HYP_LANDMARKS_001,
        claims::RUN_GAP_001,
        claims::RUN_FORM_001,
        claims::RUN_DECOUPLE_001,
        claims::RUN_HRMAX_001,
        claims::RUN_10PCT_001,
        claims::FEM_MENSTRUAL_001,
        claims::READY_CONSUMER_001,
        claims::SAFE_REDS_001,
        claims::SAFE_OTS_001,
        claims::SAFE_BSI_001,
        claims::SAFE_PREG_001,
        claims::SAFE_CVD_001,
    ];
    // The list compiles only while every canonical id is registered.
    for id in canonical {
        assert!(!id.entry().is_blocked(), "{id:?} is a myth");
    }
    let myths = all_claims().filter(|c| c.is_blocked()).count();
    assert_eq!(myths, 21, "File 09 lists 21 hard-blocked myths");
}

#[test]
fn lookup_and_confidence() {
    let c = claims::TAPER_001.entry();
    assert_eq!(c.grade, EvidenceGrade::Strong);
    assert!((c.confidence_score() - 0.90).abs() < f32::EPSILON);
}

#[test]
fn myths_are_blocked() {
    let acwr = claims::LOAD_ACWR_001.entry();
    assert!(acwr.is_blocked());
    assert!(
        claims::MYTH_POSITIVITY.entry().is_blocked(),
        "the retracted 2.9:1 positivity ratio stays hard-blocked"
    );
    assert!(!claims::TAPER_001.entry().is_blocked());
}

#[test]
#[should_panic(expected = "must never back a recommendation")]
fn surfacing_a_myth_trips_the_choke_point_guard() {
    // HARD RULE 2: grading a value with a myth claim panics. The guard is
    // UNCONDITIONAL - this test is deliberately not gated on
    // `debug_assertions` and must pass under `cargo test --release` too.
    let _ = graded((), claims::LOAD_ACWR_001);
}

#[test]
fn recommended_reads_evidence_off_the_claim() {
    let r = graded((), claims::RUN_DIST_001);
    assert_eq!(r.claim_id(), claims::RUN_DIST_001);
    assert_eq!(r.grade(), EvidenceGrade::Moderate);
    assert_eq!(r.contested(), Some("CQ-07"));
    assert!((r.confidence() - 0.65).abs() < f32::EPSILON);
}

#[test]
fn fast_moving_topics_review_semiannually() {
    for id in [claims::HYP_VOL_001, claims::HRV_001, claims::LOAD_ACWR_001] {
        assert_eq!(id.entry().review_months, 6, "{id:?} should review at 6mo");
    }
}

#[test]
fn every_non_myth_primary_citation_names_a_year() {
    // The "?" panel shows `primary_citations[0]`; for any claim we surface
    // as advice (everything except a hard-blocked myth) that lead citation
    // must name a real, findable work, i.e. carry a four-digit year. Manual
    // scan for "19"/"20" followed by two digits - no regex crate needed.
    fn names_a_year(cite: &str) -> bool {
        let b = cite.as_bytes();
        b.windows(4).any(|w| {
            (&w[..2] == b"19" || &w[..2] == b"20") && w[2].is_ascii_digit() && w[3].is_ascii_digit()
        })
    }
    for c in all_claims() {
        if c.grade == EvidenceGrade::MarketingMyth {
            continue;
        }
        // A claim with no primary citation has no lead citation to check.
        let Some(lead) = c.primary_citations.first() else {
            continue;
        };
        assert!(
            names_a_year(lead),
            "{} lead primary_citation names no year: {:?}",
            c.claim_id,
            lead
        );
    }
}

/// Byte-level scan for registry syntax that must never reach the user-facing
/// "?" panel (no regex crate in the core).
fn registry_syntax_in(text: &str) -> Option<&'static str> {
    for marker in [
        "->",
        "<-",
        "=>",
        "HARD RULE",
        "File 0",
        "File 1",
        "§",
        "(CAP-",
        "CQ-",
    ] {
        if text.contains(marker) {
            return Some("arrow / internal marker");
        }
    }
    let b = text.as_bytes();
    for (i, w) in b.windows(5).enumerate() {
        let digits = w[2..].iter().all(u8::is_ascii_digit);
        let next_is_digit = b.get(i + 5).is_some_and(u8::is_ascii_digit);
        if w[1] == b'-' && digits && !next_is_digit {
            if w[0].is_ascii_lowercase() {
                return Some("KB rule id (e.g. feedback-019)");
            }
            if w[0].is_ascii_uppercase() {
                return Some("claim id (e.g. AUTOREG-RIR-001)");
            }
        }
    }
    for w in b.windows(4) {
        if w[0].is_ascii_uppercase()
            && w[1].is_ascii_uppercase()
            && w[2] == b'_'
            && w[3].is_ascii_uppercase()
        {
            return Some("ALL_CAPS category name");
        }
    }
    for (i, w) in b.windows(3).enumerate() {
        let starts_word = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if starts_word
            && w[0] == b'G'
            && (b'1'..=b'8').contains(&w[1])
            && matches!(w[2], b':' | b' ' | b'=')
        {
            return Some("internal gate code (e.g. G5:)");
        }
    }
    None
}

#[test]
fn statements_are_plain_language() {
    // A claim statement is the "?" panel basis line and the contested
    // explainer shows `question` / `engine_default` /
    // `other_side` verbatim, so none may leak arrows, rule numbers, claim
    // ids, contested-question ids, ALL_CAPS category names or gate codes.
    // Ids live only in `claim_id` / `id`.
    for c in all_claims() {
        if let Some(why) = registry_syntax_in(c.statement) {
            panic!("{} statement leaks {why}: {:?}", c.claim_id, c.statement);
        }
    }
    for q in CONTESTED_QUESTIONS {
        for text in [Some(q.question), Some(q.engine_default), q.other_side]
            .into_iter()
            .flatten()
        {
            if let Some(why) = registry_syntax_in(text) {
                panic!("{} contested text leaks {why}: {text:?}", q.id);
            }
        }
    }
}

#[test]
fn prilepin_points_at_its_own_contested_question() {
    // File 02's local CQ-03 (Prilepin transfer) is not the global CQ-03
    // (periodization model), so it carries its own namespaced row.
    let c = claims::STR_PRILEPIN_001.entry();
    assert_eq!(c.contested, Some("CQ-F02-03"));
    let q = contested_question("CQ-F02-03").expect("row");
    assert!(q.question.contains("Prilepin"), "{}", q.question);
}

#[test]
fn goal_framing_rule_is_moderate_and_contested() {
    // File 05 feedback-003: Moderate, contested (CQ-goal-specificity).
    let c = claims::FB_PROCESS_GOAL_001.entry();
    assert_eq!(c.grade, EvidenceGrade::Moderate);
    assert_eq!(c.contested, Some("CQ-F05-01"));
    assert!(contested_question("CQ-F05-01").is_some_and(|q| q.other_side.is_some()));
}

#[test]
fn preprint_led_claims_never_exceed_expert_opinion() {
    // A claim whose lead citation is a not-yet-peer-reviewed
    // preprint cannot carry a Moderate or Strong badge.
    for c in all_claims() {
        let Some(lead) = c.primary_citations.first() else {
            continue;
        };
        let lower = lead.to_ascii_lowercase();
        if lower.contains("preprint") || lower.contains("sportrxiv") {
            assert!(
                matches!(c.grade, EvidenceGrade::ExpertOpinion | EvidenceGrade::Weak),
                "{} is led by a preprint yet graded {:?}",
                c.claim_id,
                c.grade
            );
        }
    }
}

#[test]
fn evidence_update_2026_10_regrades_hold() {
    let grade = |id: ClaimId| id.entry().grade;
    // Preprint-based per-session cap / split claims.
    assert_eq!(grade(claims::HYP_SPLIT_001), EvidenceGrade::ExpertOpinion);
    assert_eq!(grade(claims::HYP_SESSCAP_001), EvidenceGrade::ExpertOpinion);
    // hyp-upd-005: rep progression has an RCT now.
    assert_eq!(grade(claims::HYP_DOUBLEPROG_001), EvidenceGrade::Weak);
    // Training status is not a reliable moderator.
    assert_eq!(grade(claims::HYB_TRAINED_001), EvidenceGrade::Weak);
    // The <=3 run-day cap is its own, lower-graded claim and no
    // longer rides in HYB-CAP-001's statement.
    assert_eq!(grade(claims::HYB_RUNCAP_001), EvidenceGrade::ExpertOpinion);
    assert!(!claims::HYB_CAP_001.entry().statement.contains("3 d"));
    // CONC-MODE-001 lists the KB's contradictions and is contested.
    let mode = claims::CONC_MODE_001.entry();
    assert_eq!(mode.grade, EvidenceGrade::Weak);
    assert!(mode.contradicting.len() >= 2);
    assert_eq!(mode.contested, Some("CQ-F10-01"));
    // No "polarized beats threshold" overclaim.
    assert!(!claims::RUN_DIST_001.entry().statement.contains("beats"));
}

#[test]
fn evidence_update_2026_10_claims_are_registered() {
    // The 2026-10 update rows: each must resolve, be citable advice (never a
    // myth) and carry a citation.
    let ids = [
        claims::STR_HEAVYFREQ_001,
        claims::STR_BASE_POWER_001,
        claims::STR_POWER_SELECT_001,
        claims::STR_DELOAD_VOL_001,
        claims::STR_LOADPROG_001,
        claims::DETRAIN_STR_001,
        claims::HYP_REST_002,
        claims::HYP_RIR_DOSE_001,
        claims::HYP_VL_001,
        claims::HYP_PERIOD_001,
        claims::HYP_DELOAD_CADENCE_001,
        claims::RUN_PERIOD_001,
        claims::RUN_PHASE_TID_001,
        claims::RUN_VOLUME_001,
        claims::RUN_LONGRUN_TARGET_001,
        claims::RUN_TALKTEST_001,
        claims::RUN_STRENGTH_001,
        claims::RUN_C25K_002,
        claims::HYB_SEX_001,
        claims::HYB_RUNCAP_001,
        claims::HYB_MAINT_002,
        claims::HYB_RE_DOSE_001,
        claims::CAL_PLACE_001,
        claims::CAL_RIR_ACC_001,
        claims::CAL_GOAL_RIR_001,
        claims::CAL_FREQ_VOL_002,
        claims::CAL_POWER_001,
        claims::CAL_POWER_LADDER_001,
        claims::CAL_REENTRY_001,
        claims::CAL_EQUIP_001,
        claims::AUTOREG_HRV_LIFT_001,
        claims::AUTOREG_RHR_NOISE_001,
        claims::ILLNESS_RTT_001,
        claims::ILLNESS_COLD_RUN_001,
        claims::RETURN_HOLD_001,
        claims::REDS_SCREEN_001,
        claims::HRV_SOURCE_001,
        claims::WELLNESS_Z_001,
        claims::ADH_MISSED_001,
        claims::ADH_RESCHED_001,
        claims::ADH_PROGRESS_001,
        claims::ADH_SELFMON_001,
        claims::ADH_DAYS_001,
        claims::ADH_GAMIFY_001,
        claims::ADH_PRAISE_001,
        claims::ADH_RATIONALE_001,
        claims::SCREEN_PARQ_001,
        claims::SCREEN_ACSM_001,
        claims::RUN_START_001,
        claims::LIFT_DAYS_001,
        claims::REENTRY_RAMP_001,
        claims::STR_WARMUP_001,
        claims::LOG_EFFORT_001,
        claims::RUN_LIVECUE_001,
        claims::RUN_EASYDRIFT_001,
        claims::LOAD_SRPE_001,
    ];
    for id in ids {
        let c = id.entry();
        assert!(!c.is_blocked(), "{id:?} must be citable advice");
        assert!(!c.primary_citations.is_empty(), "{id:?} has no citation");
    }
}
