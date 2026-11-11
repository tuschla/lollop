use crate::schema::{ReadinessInput, ReadinessSignal};

pub(super) fn latest_input(
    inputs: &[ReadinessInput],
    signal: ReadinessSignal,
) -> Option<&ReadinessInput> {
    inputs
        .iter()
        .filter(|i| i.signal == signal)
        .max_by_key(|i| i.observed_at)
}

pub(super) fn latest(inputs: &[ReadinessInput], signal: ReadinessSignal) -> Option<f64> {
    latest_input(inputs, signal).map(|i| i.value)
}
