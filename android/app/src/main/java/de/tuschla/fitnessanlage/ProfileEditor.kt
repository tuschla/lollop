package de.tuschla.fitnessanlage

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
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
 * event so the editor can mutate a draft locally and only emit a `SetProfile`
 * (which appends to the persisted log) when the user taps Apply.
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
// is given a label rather than silently showing a cryptic raw name.

private val ProgressionCadence.label: String
    get() = when (this) {
        ProgressionCadence.EverySession -> "Every session"
        ProgressionCadence.WeekToWeek -> "Week to week"
        ProgressionCadence.MonthToMonth -> "Month to month"
    }

private val LiftGoal.label: String
    get() = when (this) {
        LiftGoal.MaxStrength -> "Max strength"
        LiftGoal.Power -> "Power"
        LiftGoal.Hypertrophy -> "Hypertrophy"
    }

private val GoalDistance.label: String
    get() = when (this) {
        GoalDistance.General -> "General fitness"
        GoalDistance.C25k -> "Couch to 5K"
        GoalDistance.FiveK -> "5K"
        GoalDistance.TenK -> "10K"
        GoalDistance.HalfMarathon -> "Half marathon"
        GoalDistance.Marathon -> "Marathon"
    }

private val ConcurrentGoal.label: String
    get() = when (this) {
        ConcurrentGoal.Strength -> "Strength"
        ConcurrentGoal.Power -> "Power"
        ConcurrentGoal.Hypertrophy -> "Hypertrophy"
        ConcurrentGoal.EndurancePriority -> "Endurance priority"
    }

/**
 * User-editable training profile. Drives the same `SetProfile` event the seed used
 * to hardcode, so all evidence-cited guidance becomes user-driven. Emits only on
 * Apply; the draft lives in local state until then.
 */
@Composable
fun ProfileEditor(initial: ProfileDraft, onApply: (ProfileDraft) -> Unit) {
    // rememberSaveable, not remember: this editor lives in a LazyColumn item, so it
    // is disposed when scrolled out of view. Plain remember would discard an
    // in-progress (un-applied) draft on scroll-away; the saveable draft survives
    // that disposal (and process death / rotation). ProfileDraft is Serializable so
    // the autoSaver can bundle it.
    var draft by rememberSaveable { mutableStateOf(initial) }

    Card(
        colors = CardDefaults.cardColors(containerColor = BgElevated),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            SegmentedEnumRow("Progression", ProgressionCadence.entries, draft.progressionCadence, display = { it.label }) {
                draft = draft.copy(progressionCadence = it)
            }
            SegmentedEnumRow("Lift goal", LiftGoal.entries, draft.liftGoal, display = { it.label }) {
                draft = draft.copy(liftGoal = it)
            }
            // Goal distance keeps the dropdown: 6 options exceed the segmented-button
            // ceiling and its labels ("Half marathon") are too long to fit a segment.
            EnumRow("Goal distance", GoalDistance.entries, draft.goalDistance, display = { it.label }) {
                draft = draft.copy(goalDistance = it)
            }
            // Dropdown, not segmented: "Endurance priority" is too long to fit a
            // quarter-width segment without truncating.
            EnumRow("Concurrent goal", ConcurrentGoal.entries, draft.concurrentGoal, display = { it.label }) {
                draft = draft.copy(concurrentGoal = it)
            }
            IntStepperRow("Weekly sets", draft.weeklySets, 0, 30, 1) {
                draft = draft.copy(weeklySets = it)
            }
            IntStepperRow("Running days/wk", draft.runningDaysPerWeek, 0, 7, 1) {
                draft = draft.copy(runningDaysPerWeek = it)
            }
            IntStepperRow("Running km/wk", draft.runningKmPerWeek.toInt(), 0, 150, 5) {
                draft = draft.copy(runningKmPerWeek = it.toDouble())
            }
            IntStepperRow("Endurance %VO2max", draft.enduranceIntensityPctVo2max.toInt(), 50, 95, 5) {
                draft = draft.copy(enduranceIntensityPctVo2max = it.toDouble())
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Advanced", color = OnBgBody, style = Type.Body)
                Switch(
                    checked = draft.advanced,
                    onCheckedChange = { draft = draft.copy(advanced = it) },
                )
            }
            Button(
                onClick = { onApply(draft) },
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Apply profile") }
        }
    }
}
