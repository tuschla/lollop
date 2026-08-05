package app.milestone

import android.content.Context
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.draw.clip
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.io.Serializable

/**
 * The three training modalities the Focus row (and the guided setup) map onto the
 * volume fields: Lift = weekly_sets>0 & no running; Run = running>0 & no lifting;
 * Both = both sides carry volume. A short enum → rendered as segmented buttons
 * (owner rule).
 */
enum class Focus { Lift, Run, Both }

internal val Focus.label: String
    get() = when (this) {
        Focus.Lift -> "Lift"
        Focus.Run -> "Run"
        Focus.Both -> "Both"
    }

/**
 * Durable stash of the volume numbers for whichever modality side is switched OFF,
 * so toggling Focus away and back restores the user's prior sets/days/km instead of
 * a generic seed. Shell chrome (not coaching state) → SharedPreferences, not the
 * crux event log.
 */
object ModalityStash {
    private const val PREFS = "milestone_modality"
    private const val K_SETS = "stash_weekly_sets"
    private const val K_DAYS = "stash_running_days"
    private const val K_KM = "stash_running_km"

    private fun p(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun stashLift(ctx: Context, weeklySets: Int) {
        // A side already at 0 has nothing worth keeping: writing it would
        // clobber the last real numbers and degrade restore to the seed.
        if (weeklySets <= 0) return
        p(ctx).edit().putInt(K_SETS, weeklySets).apply()
    }

    fun stashRun(ctx: Context, days: Int, km: Double) {
        if (days <= 0) return
        p(ctx).edit().putInt(K_DAYS, days).putFloat(K_KM, km.toFloat()).apply()
    }

    /** Stashed weekly sets, or null when never stashed. */
    fun weeklySets(ctx: Context): Int? = p(ctx).let { if (it.contains(K_SETS)) it.getInt(K_SETS, 0) else null }
    fun runningDays(ctx: Context): Int? = p(ctx).let { if (it.contains(K_DAYS)) it.getInt(K_DAYS, 0) else null }
    fun runningKm(ctx: Context): Double? = p(ctx).let { if (it.contains(K_KM)) it.getFloat(K_KM, 0f).toDouble() else null }
}

/**
 * Plain data holder for the nine [Event.SetProfile] fields. Kept separate from the
 * event so the editor can mutate a draft locally and emit a `SetProfile` per
 * committed change (the event log compacts SetProfile last-write-wins, so
 * apply-on-change costs nothing at replay).
 */
data class ProfileDraft(
    val progressionCadence: ProgressionCadence,
    val liftGoal: LiftGoal,
    val goalDistance: GoalDistance,
    val concurrentGoal: ConcurrentGoal,
    val weeklySets: Int,
    val runningDaysPerWeek: Int,
    val runningKmPerWeek: Double,
    val advanced: Boolean,
    val enduranceIntensityPctVo2max: Double,
    // Consolidated person data (Phase 5 / M5). Carried on the draft so every
    // SetProfile the editor emits re-sends it; otherwise a training-field edit
    // would last-write-wins away the person data set in the guided setup.
    val female: Boolean = false,
    val bodyweightKg: Double? = null,
    val ageYears: Double? = null,
    val restingHrBpm: Double? = null,
    val measuredHrMax: Double? = null,
    // Stage-0 onboarding health screen (A1). Carried on the draft so every
    // SetProfile re-sends it (else a training-field edit would last-write-wins
    // away the health gates). youth is core-derived from age, never set here.
    val health: HealthScreen = HealthScreen(),
) : Serializable {
    fun toEvent() = Event.SetProfile(
        progressionCadence = progressionCadence,
        liftGoal = liftGoal,
        goalDistance = goalDistance,
        concurrentGoal = concurrentGoal,
        weeklySets = weeklySets,
        runningDaysPerWeek = runningDaysPerWeek,
        runningKmPerWeek = runningKmPerWeek,
        advanced = advanced,
        enduranceIntensityPctVo2max = enduranceIntensityPctVo2max,
        female = female,
        bodyweightKg = bodyweightKg,
        ageYears = ageYears,
        restingHrBpm = restingHrBpm,
        measuredHrMax = measuredHrMax,
        health = health,
    )

    companion object {
        /**
         * Rebuild a draft from the core-echoed profile (post log-replay hydration).
         * Runs on the first frame, so an unknown enum name (a forward-incompatible
         * log line, or a future core/shell variant divergence) must fall back to a
         * SEED default rather than throwing `valueOf` into a launch crash: the same
         * drop-don't-crash stance the FFI layer takes on replay.
         */
        fun from(p: ProfileView) = ProfileDraft(
            progressionCadence = enumOr(p.progression_cadence, SEED.progressionCadence),
            liftGoal = enumOr(p.lift_goal, SEED.liftGoal),
            goalDistance = enumOr(p.goal_distance, SEED.goalDistance),
            concurrentGoal = enumOr(p.concurrent_goal, SEED.concurrentGoal),
            weeklySets = p.weekly_sets,
            runningDaysPerWeek = p.running_days_per_week,
            runningKmPerWeek = p.running_km_per_week,
            advanced = p.advanced,
            enduranceIntensityPctVo2max = p.endurance_intensity_pct_vo2max,
            female = p.female,
            bodyweightKg = p.bodyweight_kg,
            ageYears = p.age_years,
            restingHrBpm = p.resting_hr_bpm,
            measuredHrMax = p.measured_hr_max,
            // Drop the core-DERIVED youth flag from the editable draft so a later
            // edit never re-asserts it (youth is owned by the core, keyed off age).
            health = p.health.copy(youth = false),
        )

        private inline fun <reified T : Enum<T>> enumOr(name: String, default: T): T =
            runCatching { enumValueOf<T>(name) }.getOrDefault(default)

        /**
         * Fallback starting point for the full editor only (rehydration when no
         * profile is echoed). NOT auto-seeded on fresh install anymore: first run
         * goes through the guided setup (M5), which writes user-asserted values
         * rather than these opinionated defaults.
         */
        val SEED = ProfileDraft(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 14,
            runningDaysPerWeek = 4,
            runningKmPerWeek = 45.0,
            advanced = false,
            enduranceIntensityPctVo2max = 75.0,
        )
    }
}

// Human-readable labels for the profile pickers. Display-only, the wire value is
// still each variant's `.name` (see Event.SetProfile), so these strings can change
// freely. Exhaustive `when` (no `else`) so a new variant fails to compile until it
// is given a label rather than silently showing a cryptic raw name. Internal, not
// private: the Coach PROFILE summary card reuses the same strings.

internal val ProgressionCadence.label: String
    get() = when (this) {
        ProgressionCadence.EverySession -> "Every session"
        ProgressionCadence.WeekToWeek -> "Week to week"
        ProgressionCadence.MonthToMonth -> "Month to month"
    }

internal val LiftGoal.label: String
    get() = when (this) {
        LiftGoal.MaxStrength -> "Max strength"
        LiftGoal.Power -> "Power"
        LiftGoal.Hypertrophy -> "Hypertrophy"
    }

internal val GoalDistance.label: String
    get() = when (this) {
        GoalDistance.General -> "General fitness"
        GoalDistance.C25k -> "Couch to 5K"
        GoalDistance.FiveK -> "5K"
        GoalDistance.TenK -> "10K"
        GoalDistance.HalfMarathon -> "Half marathon"
        GoalDistance.Marathon -> "Marathon"
    }

internal val ConcurrentGoal.label: String
    get() = when (this) {
        ConcurrentGoal.Strength -> "Strength"
        ConcurrentGoal.Power -> "Power"
        ConcurrentGoal.Hypertrophy -> "Hypertrophy"
        ConcurrentGoal.EndurancePriority -> "Endurance priority"
    }

/**
 * User-editable training profile. Drives the same `SetProfile` event the seed used
 * to hardcode, so all evidence-cited guidance becomes user-driven.
 *
 * Applies on every change: each committed edit immediately calls [onApply] with
 * the new draft, so a picker tap or the Advanced switch takes effect the moment
 * it is touched, no separate "Apply" step to forget (the old Apply button left
 * toggles looking dead until it was found and tapped). The event log compacts
 * `SetProfile` last-write-wins, so per-tap events never bloat the replay.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProfileEditor(initial: ProfileDraft, onApply: (ProfileDraft) -> Unit) {
    val ctx = LocalContext.current
    // rememberSaveable, not remember: this editor lives in a LazyColumn item, so it
    // is disposed when scrolled out of view. Plain remember would discard the draft
    // on scroll-away; the saveable draft survives that disposal (and process death /
    // rotation). ProfileDraft is Serializable so the autoSaver can bundle it.
    //
    // KEYED on `initial` (SAFETY / HARD RULE 3): an unkeyed saveable outlives the
    // incoming profile, so after "Re-run guided setup", which rewrites the core
    // profile, including the health gates, the stale pre-setup draft would survive
    // and, on the next field edit (apply-on-change re-sends every field via
    // last-write-wins SetProfile), silently revert the just-set values. That could
    // re-raise a just-cleared medical deferral or drop a just-raised gate. Keying on
    // the incoming profile identity invalidates the draft the moment the core echoes
    // a different profile, so a fresh setup always wins. Normal apply-on-change edits
    // round-trip (`from(toEvent())` is stable), so the key is unchanged there and the
    // draft is NOT reset mid-edit.
    var draft by rememberSaveable(initial) { mutableStateOf(initial) }
    // Commit-and-apply: the single funnel every edit goes through.
    val commit: (ProfileDraft) -> Unit = {
        draft = it
        onApply(it)
    }

    // Which row's inline picker is open. One at a time; -1 = all collapsed.
    // Saveable so an open picker survives scroll-away / rotation like the draft.
    var open by rememberSaveable { mutableStateOf(-1) }

    // Modality flip (Focus row). Switching a side OFF stashes its volume numbers so
    // switching BACK restores them; switching a side ON restores its stash (or a
    // sensible seed when the stash is empty). Every path funnels through `commit`
    // → a single immediate-apply SetProfile.
    val applyFocus: (Focus) -> Unit = applyFocus@{ new ->
        val current = when {
            draft.weeklySets > 0 && draft.runningDaysPerWeek > 0 -> Focus.Both
            draft.runningDaysPerWeek > 0 -> Focus.Run
            else -> Focus.Lift
        }
        // Re-tapping the active segment must be a no-op: otherwise it would
        // re-stash and re-commit for nothing.
        if (new == current) return@applyFocus
        val next = when (new) {
            Focus.Lift -> {
                ModalityStash.stashRun(ctx, draft.runningDaysPerWeek, draft.runningKmPerWeek)
                draft.copy(
                    runningDaysPerWeek = 0,
                    runningKmPerWeek = 0.0,
                    weeklySets = if (draft.weeklySets > 0) draft.weeklySets
                    else (ModalityStash.weeklySets(ctx)?.takeIf { it > 0 } ?: ProfileDraft.SEED.weeklySets),
                )
            }
            Focus.Run -> {
                ModalityStash.stashLift(ctx, draft.weeklySets)
                val d = draft.copy(weeklySets = 0)
                if (draft.runningDaysPerWeek > 0) {
                    d
                } else {
                    d.copy(
                        runningDaysPerWeek = ModalityStash.runningDays(ctx)?.takeIf { it > 0 } ?: 3,
                        runningKmPerWeek = ModalityStash.runningKm(ctx)?.takeIf { it > 0.0 } ?: 24.0,
                    )
                }
            }
            Focus.Both -> {
                var d = draft
                if (d.weeklySets == 0) {
                    d = d.copy(weeklySets = ModalityStash.weeklySets(ctx)?.takeIf { it > 0 } ?: ProfileDraft.SEED.weeklySets)
                }
                if (d.runningDaysPerWeek == 0) {
                    d = d.copy(
                        runningDaysPerWeek = ModalityStash.runningDays(ctx)?.takeIf { it > 0 } ?: 3,
                        runningKmPerWeek = ModalityStash.runningKm(ctx)?.takeIf { it > 0.0 } ?: 24.0,
                    )
                }
                d
            }
        }
        commit(next)
        open = -1
    }

    Card(
        colors = CardDefaults.cardColors(containerColor = BgElevated),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            // Modality derivation: which sides carry volume drives which rows show.
            val lift = draft.weeklySets > 0
            val run = draft.runningDaysPerWeek > 0
            val focus = when {
                lift && run -> Focus.Both
                run -> Focus.Run
                else -> Focus.Lift
            }
            // Focus, the top row. A short enum → segmented buttons (owner rule);
            // switching modality reshapes which rows below are visible. Concurrent
            // goal only exists for Both, which is why it sits behind this gate.
            SegmentedEnumRow(
                label = "Focus",
                values = listOf(Focus.Lift, Focus.Run, Focus.Both),
                current = focus,
                display = { it.label },
                onSelect = applyFocus,
            )

            // Lift rows: only when lifting is in the plan.
            if (lift) {
                RowHairline()
                ProfileRow(open == 0, { open = if (open == 0) -1 else 0 }, "Training goal", draft.liftGoal.label) {
                    OptionList(LiftGoal.entries.toList(), draft.liftGoal, { it.label }) {
                        commit(draft.copy(liftGoal = it))
                        open = -1
                    }
                }
            }
            // Progression, always relevant.
            RowHairline()
            ProfileRow(open == 1, { open = if (open == 1) -1 else 1 }, "Progression", draft.progressionCadence.label) {
                OptionList(ProgressionCadence.entries.toList(), draft.progressionCadence, { it.label }) {
                    commit(draft.copy(progressionCadence = it))
                    open = -1
                }
            }
            // Run rows: goal distance only when running is in the plan.
            if (run) {
                RowHairline()
                ProfileRow(open == 2, { open = if (open == 2) -1 else 2 }, "Goal distance", draft.goalDistance.label) {
                    OptionList(GoalDistance.entries.toList(), draft.goalDistance, { it.label }) {
                        commit(draft.copy(goalDistance = it))
                        open = -1
                    }
                }
            }
            // Concurrent goal: only meaningful when BOTH modalities run at once
            // (it decides which one leads). Hidden for single-modality profiles,
            // fixing the owner-flagged "concurrent goal on a run-only profile"
            // contradiction.
            if (lift && run) {
                RowHairline()
                ProfileRow(open == 3, { open = if (open == 3) -1 else 3 }, "Concurrent goal", draft.concurrentGoal.label) {
                    OptionList(ConcurrentGoal.entries.toList(), draft.concurrentGoal, { it.label }) {
                        commit(draft.copy(concurrentGoal = it))
                        open = -1
                    }
                }
            }
            if (lift) {
                RowHairline()
                ProfileRow(open == 4, { open = if (open == 4) -1 else 4 }, "Weekly sets / muscle", "${draft.weeklySets}") {
                    // Grid starts at 1, not 0: a modality's on/off is owned by the
                    // Focus row above (which STASHES the volume before zeroing it, so
                    // toggling back restores the user's numbers). A 0 here would zero
                    // weekly_sets → the lift rows vanish, but the value was discarded
                    // WITHOUT a stash, so a later Focus→Both restores only a SEED, not
                    // the pre-zero number. Removing 0 keeps the picker to "adjust
                    // volume within an active modality" and routes removal through the
                    // stashing Focus control.
                    ScrollableScaleRow((1..30).toList(), draft.weeklySets, { "$it" }) {
                        commit(draft.copy(weeklySets = it))
                    }
                }
            }
            if (run) {
                RowHairline()
                ProfileRow(open == 5, { open = if (open == 5) -1 else 5 }, "Running days/wk", "${draft.runningDaysPerWeek}") {
                    // Starts at 1, not 0 (same rationale as Weekly sets): 0 days would
                    // vanish the run rows while discarding the volume un-stashed. To
                    // drop running entirely, use the Focus row (Run→Lift), which
                    // stashes running days/km first so switching back restores them.
                    ChoiceScaleRow((1..7).toList(), draft.runningDaysPerWeek, { "$it" }) {
                        commit(draft.copy(runningDaysPerWeek = it))
                    }
                }
                RowHairline()
                ProfileRow(open == 6, { open = if (open == 6) -1 else 6 }, "Running km/wk", "${draft.runningKmPerWeek.toInt()}") {
                    // step 1, not step 5: guided setup writes runningDaysPerWeek ×
                    // kmPerRunDay (e.g. 3×8 = 24, 4×11 = 44), which a coarse
                    // 5-multiple grid can neither highlight nor preserve: a tap would
                    // silently snap the user's setup volume to the nearest 5.
                    ScrollableScaleRow((0..150).toList(), draft.runningKmPerWeek.toInt(), { "$it" }) {
                        commit(draft.copy(runningKmPerWeek = it.toDouble()))
                    }
                }
                RowHairline()
                ProfileRow(open == 7, { open = if (open == 7) -1 else 7 }, "Endurance %VO2max", "${draft.enduranceIntensityPctVo2max.toInt()}%") {
                    ChoiceScaleRow((50..95 step 5).toList(), draft.enduranceIntensityPctVo2max.toInt(), { "$it" }) {
                        commit(draft.copy(enduranceIntensityPctVo2max = it.toDouble()))
                    }
                }
                RowHairline()
                // Advanced: a toggle row with the whole row tappable and a subtitle
                // saying what the flag actually changes (its only consumer in the core
                // is the running goal-week plan: coaching depth/verbosity follows the
                // Progression cadence above, per INDIV-TRAGE-001, not this switch).
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .clickable { commit(draft.copy(advanced = !draft.advanced)) },
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(
                        modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                    ) {
                        Text("Advanced running base", color = OnBgBody, style = Type.Body)
                        Text(
                            "Plans 5–7 run sessions/wk instead of 4–6. Coaching depth follows Progression above.",
                            color = OnBgMuted,
                            style = Type.Caption,
                        )
                    }
                    Switch(
                        checked = draft.advanced,
                        onCheckedChange = { commit(draft.copy(advanced = it)) },
                    )
                }
            }

            // You, consolidated person data (Phase 5 / M5). Entered once here;
            // the Coach protein / HR-zone calculators prefill from it instead of
            // asking again. All optional: "Not set" until you pick a value.
            RowHairline()
            Text(
                "You",
                color = OnBgFaint,
                style = Type.Caption.copy(fontWeight = FontWeight.Bold),
                modifier = Modifier.padding(top = Space.Sm.dp),
            )
            ProfileRow(open == 8, { open = if (open == 8) -1 else 8 }, "Sex", if (draft.female) "Female" else "Male") {
                ChoiceScaleRow(listOf(false, true), draft.female, { if (it) "Female" else "Male" }) {
                    commit(draft.copy(female = it))
                    open = -1
                }
            }
            RowHairline()
            ProfileRow(open == 9, { open = if (open == 9) -1 else 9 }, "Age", draft.ageYears?.let { "${it.toInt()} yr" } ?: "Not set") {
                ScrollableScaleRow((14..90).toList(), draft.ageYears?.toInt() ?: 30, { "$it" }) {
                    commit(draft.copy(ageYears = it.toDouble()))
                }
            }
            RowHairline()
            ProfileRow(open == 10, { open = if (open == 10) -1 else 10 }, "Bodyweight", draft.bodyweightKg?.let { "${it.toInt()} kg" } ?: "Not set") {
                ScrollableScaleRow((40..150 step 1).toList(), draft.bodyweightKg?.toInt() ?: 75, { "$it" }) {
                    commit(draft.copy(bodyweightKg = it.toDouble()))
                }
            }
            RowHairline()
            ProfileRow(open == 11, { open = if (open == 11) -1 else 11 }, "Resting HR", draft.restingHrBpm?.let { "${it.toInt()} bpm" } ?: "Not set") {
                ScrollableScaleRow((35..90).toList(), draft.restingHrBpm?.toInt() ?: 60, { "$it" }) {
                    commit(draft.copy(restingHrBpm = it.toDouble()))
                }
            }
            RowHairline()
            ProfileRow(open == 12, { open = if (open == 12) -1 else 12 }, "Max HR (measured)", draft.measuredHrMax?.let { "${it.toInt()} bpm" } ?: "Not set") {
                ScrollableScaleRow((140..210).toList(), draft.measuredHrMax?.toInt() ?: 185, { "$it" }) {
                    commit(draft.copy(measuredHrMax = it.toDouble()))
                }
            }

            // Health & safety screen (A1). These arm the core's medical-deferral
            // gates (File 08: PAR-Q+, pregnancy, injury/rehab, RED-S). A raised
            // gate makes the engine defer to a professional rather than program a
            // plan: SAFETY overrides goals (HARD RULE 3). Age drives the youth
            // gate core-side, so it isn't repeated here.
            RowHairline()
            Text(
                "Health & safety",
                color = OnBgFaint,
                style = Type.Caption.copy(fontWeight = FontWeight.Bold),
                modifier = Modifier.padding(top = Space.Sm.dp),
            )
            HealthSwitchRow(
                "Positive health screen (PAR-Q+)",
                "Known heart, metabolic or kidney condition, uncontrolled blood pressure, recent surgery, or a doctor told you to check before vigorous exercise.",
                draft.health.parq_positive,
            ) {
                // Turning the parent OFF must also clear its dependent child -
                // otherwise a stale `medically_cleared` survives (its row is
                // hidden) and, when a NEW positive screen is later declared, the
                // `parq_positive && !medically_cleared` deferral never fires:
                // the medical-referral gate would be silently defeated (HARD
                // RULE 3). Keep the child only while the parent stays on.
                commit(
                    draft.copy(
                        health = draft.health.copy(
                            parq_positive = it,
                            medically_cleared = if (it) draft.health.medically_cleared else false,
                        ),
                    ),
                )
            }
            if (draft.health.parq_positive) {
                RowHairline()
                HealthSwitchRow(
                    "Cleared by a doctor",
                    "A clinician has cleared you to train since that positive screen.",
                    draft.health.medically_cleared,
                ) { commit(draft.copy(health = draft.health.copy(medically_cleared = it))) }
            }
            RowHairline()
            HealthSwitchRow(
                "Currently pregnant",
                "The engine defers autonomous prescription during pregnancy and individualises with your provider.",
                draft.health.pregnant,
            ) {
                // Same stale-child hazard as PAR-Q+ above, but this one pins an
                // un-clearable STOP hold: turning pregnant OFF must clear
                // `pregnancy_warning_sign`, whose editing row would otherwise
                // vanish while the flag stays true (a hidden, un-liftable hold).
                commit(
                    draft.copy(
                        health = draft.health.copy(
                            pregnant = it,
                            pregnancy_warning_sign = if (it) draft.health.pregnancy_warning_sign else false,
                        ),
                    ),
                )
            }
            if (draft.health.pregnant) {
                RowHairline()
                HealthSwitchRow(
                    "Pregnancy warning sign present",
                    "Bleeding, breathlessness before exertion, chest pain, or reduced fetal movement - stop and seek care.",
                    draft.health.pregnancy_warning_sign,
                ) { commit(draft.copy(health = draft.health.copy(pregnancy_warning_sign = it))) }
            }
            RowHairline()
            HealthSwitchRow(
                "Injury, recent surgery, or in rehab",
                "The engine never prescribes rehabilitation - resume general programming only once cleared.",
                draft.health.injury_or_rehab,
            ) { commit(draft.copy(health = draft.health.copy(injury_or_rehab = it))) }
            RowHairline()
            HealthSwitchRow(
                "Under-fuelling / disordered-eating signal",
                "Missed periods, rapid weight loss, compulsive exercise, or persistent unexplained fatigue - routes to a professional (RED-S).",
                draft.health.reds_signal,
            ) { commit(draft.copy(health = draft.health.copy(reds_signal = it))) }

            Text(
                "Changes apply immediately. Age and bodyweight prefill the Coach calculators.",
                color = OnBgFaint,
                style = Type.Caption,
                modifier = Modifier.padding(top = Space.Sm.dp),
            )
        }
    }
}

/**
 * A whole-row toggle for one health-screen gate: bold label + a plain-language
 * consequence line, a [Switch] on the right. Tapping anywhere on the row flips
 * it. Uses [DangerOn]-neutral styling: these aren't errors, just screens.
 */
@Composable
internal fun HealthSwitchRow(
    label: String,
    subtitle: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable { onChange(!checked) },
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        ) {
            Text(label, color = OnBgBody, style = Type.Body)
            Text(subtitle, color = OnBgMuted, style = Type.Caption)
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

/** The 1dp hairline between grouped profile rows (04-profile §1). */
@Composable
private fun RowHairline() {
    HorizontalDivider(thickness = 1.dp, color = OnBgBody.copy(alpha = 0.05f))
}

/**
 * A profile field as a tap-row (04-profile §1): label `OnBgMuted` left, value
 * Bold + `ui-chevron-right` right; tapping toggles an inline [content] picker
 * below it (full-width OptionList / scale rows, no anchored dropdowns, no
 * steppers as primary). ≥48dp whole-row tap target; edits apply immediately.
 */
@Composable
private fun ProfileRow(
    expanded: Boolean,
    onToggle: () -> Unit,
    label: String,
    value: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                // Clip before clickable so the ripple stays rounded (no square
                // flash around the row).
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable { onToggle() },
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(label, color = OnBgMuted, style = Type.Body)
            Row(
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    value,
                    color = OnBgBody,
                    style = Type.Body.copy(fontWeight = FontWeight.Bold).merge(TabularFigures),
                )
                RowChevron(expanded)
            }
        }
        if (expanded) content()
    }
}
