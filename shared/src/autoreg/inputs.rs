//! Latest-observation lookups over the readiness inputs every rule reads.

use crate::schema::{ReadinessInput, ReadinessSignal};

/// Latest observation for a signal, if present (deterministic: max `observed_at`).
pub(super) fn latest_input(
    inputs: &[ReadinessInput],
    signal: ReadinessSignal,
) -> Option<&ReadinessInput> {
    inputs
        .iter()
        .filter(|i| i.signal == signal)
        .max_by_key(|i| i.observed_at)
}

/// Latest observed value for a signal, if present.
pub(super) fn latest(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<f64> {
    latest_input(inputs, signal).map(|i| i.value)
}
