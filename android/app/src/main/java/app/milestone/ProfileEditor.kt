package app.milestone

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
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.ui.draw.clip
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.io.Serializable

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
        )

        private inline fun <reified T : Enum<T>> enumOr(name: String, default: T): T =
            runCatching { enumValueOf<T>(name) }.getOrDefault(default)

        /** The representative starting point used on a fresh install. */
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
@Composable
fun ProfileEditor(initial: ProfileDraft, onApply: (ProfileDraft) -> Unit) {
    // rememberSaveable, not remember: this editor lives in a LazyColumn item, so it
    // is disposed when scrolled out of view. Plain remember would discard the draft
    // on scroll-away; the saveable draft survives that disposal (and process death /
    // rotation). ProfileDraft is Serializable so the autoSaver can bundle it.
    var draft by rememberSaveable { mutableStateOf(initial) }
    // Commit-and-apply: the single funnel every edit goes through.
    val commit: (ProfileDraft) -> Unit = {
        draft = it
        onApply(it)
    }

    // Which row's inline picker is open. One at a time; -1 = all collapsed.
    // Saveable so an open picker survives scroll-away / rotation like the draft.
    var open by rememberSaveable { mutableStateOf(-1) }

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
            ProfileRow(open == 0, { open = if (open == 0) -1 else 0 }, "Progression", draft.progressionCadence.label) {
                OptionList(ProgressionCadence.entries.toList(), draft.progressionCadence, { it.label }) {
                    commit(draft.copy(progressionCadence = it))
                    open = -1
                }
            }
            ProfileRow(open == 1, { open = if (open == 1) -1 else 1 }, "Lift goal", draft.liftGoal.label) {
                OptionList(LiftGoal.entries.toList(), draft.liftGoal, { it.label }) {
                    commit(draft.copy(liftGoal = it))
                    open = -1
                }
            }
            ProfileRow(open == 2, { open = if (open == 2) -1 else 2 }, "Goal distance", draft.goalDistance.label) {
                OptionList(GoalDistance.entries.toList(), draft.goalDistance, { it.label }) {
                    commit(draft.copy(goalDistance = it))
                    open = -1
                }
            }
            ProfileRow(open == 3, { open = if (open == 3) -1 else 3 }, "Concurrent goal", draft.concurrentGoal.label) {
                OptionList(ConcurrentGoal.entries.toList(), draft.concurrentGoal, { it.label }) {
                    commit(draft.copy(concurrentGoal = it))
                    open = -1
                }
            }
            ProfileRow(open == 4, { open = if (open == 4) -1 else 4 }, "Weekly sets", "${draft.weeklySets}") {
                ScrollableScaleRow((0..30).toList(), draft.weeklySets, { "$it" }) {
                    commit(draft.copy(weeklySets = it))
                }
            }
            ProfileRow(open == 5, { open = if (open == 5) -1 else 5 }, "Running days/wk", "${draft.runningDaysPerWeek}") {
                ChoiceScaleRow((0..7).toList(), draft.runningDaysPerWeek, { "$it" }) {
                    commit(draft.copy(runningDaysPerWeek = it))
                }
            }
            ProfileRow(open == 6, { open = if (open == 6) -1 else 6 }, "Running km/wk", "${draft.runningKmPerWeek.toInt()}") {
                ScrollableScaleRow((0..150 step 5).toList(), draft.runningKmPerWeek.toInt(), { "$it" }) {
                    commit(draft.copy(runningKmPerWeek = it.toDouble()))
                }
            }
            ProfileRow(open == 7, { open = if (open == 7) -1 else 7 }, "Endurance %VO2max", "${draft.enduranceIntensityPctVo2max.toInt()}%") {
                ChoiceScaleRow((50..95 step 5).toList(), draft.enduranceIntensityPctVo2max.toInt(), { "$it" }) {
                    commit(draft.copy(enduranceIntensityPctVo2max = it.toDouble()))
                }
            }
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
            Text(
                "Changes apply immediately.",
                color = OnBgFaint,
                style = Type.Caption,
                modifier = Modifier.padding(top = Space.Sm.dp),
            )
        }
    }
}

/**
 * A profile field as a tap-row: label + current value + chevron; tapping toggles
 * an inline [content] picker below it. The wire contract is untouched, the picker
 * still mutates the same draft field the old dropdown/stepper did. The header row
 * is a ≥48dp whole-row tap target.
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
            Text(label, color = OnBgBody, style = Type.Body)
            Row(
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(value, color = Accent, style = Type.Body.merge(TabularFigures))
                Text(if (expanded) "⌄" else "›", color = OnBgFaint, style = Type.Body)
            }
        }
        if (expanded) content()
    }
}
