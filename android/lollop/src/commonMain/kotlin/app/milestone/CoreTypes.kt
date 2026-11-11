package app.milestone

// Shell-side behaviour on the core's wire types. The types themselves are
// generated from the Rust serde types into WireTypes.kt (shared/wiregen).

/** Training is on hold (a Stop-level or rest-day call): [ViewModel.hold_reason] is set. */
val ViewModel.train_blocked: Boolean get() = hold_reason != null

/** The display text of a run-intent label. */
val WorkoutType.label: String
    get() = when (this) {
        WorkoutType.LongRun -> "Long run"
        else -> wire
    }

/** The seven PAR-Q+ general health questions (onboard-051), in form order. Each
 *  "Yes" maps onto one core gate flag by the core's masks ([CoreConstants]):
 *  Q2/Q3 are symptoms (onboard-052, not clearable), Q6 is a bone/joint problem
 *  (injury/rehab), the rest are doctor-follow-up items (parq_positive). [bit] is
 *  the item's bit in [HealthScreen.parq_yes]. */
enum class ParqItem(val bit: Int) {
    HeartOrBloodPressure(0),
    ChestPain(1),
    DizzyOrFainting(2),
    OtherCondition(3),
    Medication(4),
    BoneOrJoint(5),
    SupervisedOnly(6),
    ;

    private val mask: Int get() = 1 shl bit
    val symptom: Boolean get() = CoreConstants.PARQ_SYMPTOM_BITS and mask != 0
    val injury: Boolean get() = CoreConstants.PARQ_INJURY_BIT == mask
    val followUp: Boolean get() = CoreConstants.PARQ_FOLLOWUP_BITS and mask != 0
}

/** Any SHELL-answerable flag raised. `youth` is deliberately EXCLUDED: it is
 *  core-derived from age, never asserted by the shell. The gate-relevant
 *  `parq_positive` counts even when cleared. */
fun HealthScreen.anyRaised(): Boolean =
    parq_positive || medically_cleared || pregnant ||
        pregnancy_warning_sign || injury_or_rehab || reds_signal ||
        symptomatic || acute_illness || parq_yes != 0

/** A "Doctor-flagged" answer from the old single toggle, before the seven
 *  questions existed: raised, but no follow-up answer recorded. */
val HealthScreen.legacyParq: Boolean
    get() = parq_positive && parq_yes and CoreConstants.PARQ_FOLLOWUP_BITS == 0

/** This PAR-Q+ question answered Yes. Q6 also reads the injury flag, which
 *  predates the questions and means the same thing. */
fun HealthScreen.parqYes(item: ParqItem): Boolean =
    parq_yes and (1 shl item.bit) != 0 || (item.injury && injury_or_rehab)

/** How many of the seven questions are answered Yes. */
val HealthScreen.parqYesCount: Int get() = ParqItem.entries.count { parqYes(it) }

/** Answer one PAR-Q+ question and re-derive the gate flags from all seven.
 *  A legacy flag stays until a question of its own kind is answered, so an
 *  unrelated answer never drops a hold: a legacy doctor flag ([legacyParq])
 *  until a follow-up question (Q1/Q4/Q5/Q7) is answered, a legacy
 *  `symptomatic` flag (no symptom bit) until a symptom question is
 *  (symptoms are never clearable, onboard-052). A clearance survives only
 *  while every follow-up Yes was already covered by it: a NEW follow-up Yes
 *  drops `medically_cleared` (safety-044). */
fun HealthScreen.withParqAnswer(item: ParqItem, yes: Boolean): HealthScreen {
    val bit = 1 shl item.bit
    // Fold a legacy injury flag into its question first, so answering another
    // question can't silently clear it.
    val base = if (injury_or_rehab) parq_yes or (1 shl ParqItem.BoneOrJoint.bit) else parq_yes
    val mask = if (yes) base or bit else base and bit.inv()
    fun has(i: ParqItem) = mask and (1 shl i.bit) != 0
    val followUpBits = CoreConstants.PARQ_FOLLOWUP_BITS
    val oldFollowUps = parq_yes and followUpBits
    val newFollowUps = mask and followUpBits
    val followUp = newFollowUps != 0 || (legacyParq && !item.followUp)
    // What the existing clearance covers: the recorded mask, or (a clearance
    // saved before the mask existed / a legacy toggle) the answers at the time.
    val covered = when {
        !medically_cleared -> 0
        cleared_parq_mask != 0 -> cleared_parq_mask
        else -> oldFollowUps
    }
    val stillCleared = medically_cleared && followUp &&
        (newFollowUps and covered.inv()) == 0 &&
        (newFollowUps != 0 || covered == 0)
    val symptomBits = CoreConstants.PARQ_SYMPTOM_BITS
    return copy(
        parq_yes = mask,
        parq_positive = followUp,
        medically_cleared = stillCleared,
        cleared_parq_mask = if (stillCleared) newFollowUps else 0,
        symptomatic = mask and symptomBits != 0 ||
            (symptomatic && parq_yes and symptomBits == 0 && !item.symptom),
        injury_or_rehab = has(ParqItem.BoneOrJoint),
    )
}

/** Set the "Cleared by a doctor" answer. The clearance covers exactly the
 *  follow-up answers given now, so a later new Yes is not covered. */
fun HealthScreen.withClearance(cleared: Boolean): HealthScreen =
    copy(
        medically_cleared = cleared,
        cleared_parq_mask = if (cleared) parq_yes and CoreConstants.PARQ_FOLLOWUP_BITS else 0,
    )
