//! Flattening a graded plan prescription into the shell-facing item.

use super::*;

/// Flatten one graded `Prescription` into a shell-facing `PrescriptionView`,
/// resolving the load from the anchor when the intensity is %1RM.
pub(in crate::app) fn flatten_prescription(
    rx: &Recommended<Prescription>,
    anchors: &crate::plan::Anchors,
    bodyweight_kg: Option<f64>,
    weight_lb: bool,
) -> PrescriptionView {
    match &rx.value {
        Prescription::Lift(l) => flatten_lift_prescription(l, anchors, rx, weight_lb),
        Prescription::Run(r) => flatten_run_prescription(r, rx),
        Prescription::Calisthenics(c) => {
            flatten_calisthenics_prescription(c, rx, anchors, bodyweight_kg, weight_lb)
        }
    }
}

/// The session's human title, marking a lift deload day as part of a lighter
/// week ("Heavy day (lighter week)") so a deload is never titled like the
/// full session. A lift session is a deload day when every lift/calisthenics
/// item carries a deload rule (strength STR-DELOAD-VOL-001, hypertrophy
/// HYP-DELOAD-RX-001), the claims `apply_deload_week` grades them with.
pub(in crate::app) fn plan_session_title(s: &Session) -> String {
    // Two heavy DUP days of one week read apart: the second, 5–6-rep exposure
    // (strength-022 block band) is the "heavy volume" day.
    let title = match (s.session_type, s.dup_day) {
        (
            SessionType::Lift(LiftSessionType::MaxEffort),
            Some(crate::strength::DupDay::HeavyVolume),
        ) => "Heavy volume day".to_string(),
        _ => session_title(s.session_type),
    };
    let mut lifts = s
        .prescriptions
        .iter()
        .filter(|rx| !matches!(rx.value, Prescription::Run(_)))
        .peekable();
    let deload = matches!(s.session_type, SessionType::Lift(_))
        && lifts.peek().is_some()
        && lifts.all(|rx| {
            matches!(
                rx.claim_id(),
                claims::STR_DELOAD_VOL_001 | claims::HYP_DELOAD_RX_001
            )
        });
    if deload {
        format!("{title} (lighter week)")
    } else {
        title
    }
}
