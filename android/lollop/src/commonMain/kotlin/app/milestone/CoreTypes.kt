package app.milestone

// Shell-side behaviour on the core's wire types. The types themselves are
// generated from the Rust serde types into WireTypes.kt (shared/wiregen).

/** Training is on hold (a Stop-level or rest-day call): [ViewModel.hold_reason] is set. */
val ViewModel.train_blocked: Boolean get() = hold_reason != null
