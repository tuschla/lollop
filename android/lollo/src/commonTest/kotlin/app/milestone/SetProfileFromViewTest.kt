package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.json.jsonObject

/** [Event.SetProfile.from]: the editors rebuild their draft from the profile the core
 *  echoes, so a field the rebuild leaves out is reset by the next profile edit. */
class SetProfileFromViewTest {

    /** Every answer at a value other than its default. */
    private val answered = Profile(
        progression_cadence = ProgressionCadence.MonthToMonth,
        lift_goal = LiftGoal.Hypertrophy,
        goal_distance = GoalDistance.TenK,
        concurrent_goal = ConcurrentGoal.EndurancePriority,
        weekly_sets = 12,
        running_days_per_week = 3,
        running_km_per_week = 25.0,
        running_km_per_week_set = false,
        advanced = true,
        female = true,
        high_load_block = true,
        health = HealthScreen(acute_illness = true),
        environment = Environment.Heat,
        env_temp_c = 28.0,
        env_altitude_m = 1800.0,
        weeks_off = 6.0,
        bodyweight_kg = 62.5,
        age_years = 34.0,
        birth_epoch_day = 7_740L,
        resting_hr_bpm = 54.0,
        measured_hr_max = 188.0,
        weight_lb = true,
        distance_unit = DistanceUnit.Mi,
        resistance_modality = ResistanceModality.Calisthenics,
        lift_days_per_week = 4,
        training_weekdays = 0b1100000,
        goal_race_epoch_day = 20_400L,
        recent_race = RecentRace(distance_km = 5.0, time_sec = 1575),
        stated_longest_recent_km = 12.0,
        layoff_returned_epoch_day = 20_360L,
        meet_epoch_day = 20_420L,
        calis_equipment = CalisEquipment(pull_up_bar = true),
        specialization_muscle = "side delts",
        runner_strength = true,
    )

    /** The dispatch stamps [effectiveFrom] adds when a draft is sent; a rebuilt draft
     *  starts without them. */
    private val stamps = setOf("effective_from_epoch_day", "edited_at")

    private fun SerialDescriptor.names(): Set<String> = (0 until elementsCount).map { getElementName(it) }.toSet()

    @Test
    fun aDraftRebuiltFromTheEchoedProfileKeepsEveryAnswer() {
        val fields = Profile.serializer().descriptor.names() - stamps
        assertEquals(fields, Event.SetProfile.serializer().descriptor.names() - stamps, "SetProfile carries the profile")

        val view = coreJson.encodeToJsonElement(Profile.serializer(), answered).jsonObject
        val defaults = coreJson.encodeToJsonElement(Profile.serializer(), Profile()).jsonObject
        assertEquals(emptyList(), fields.filter { view[it] == defaults[it] }, "fields the example leaves at their default")

        val draft = coreJson.encodeToJsonElement(Event.SetProfile.serializer(), Event.SetProfile.from(answered)).jsonObject
        val lost = fields.filter { draft[it] != view[it] }.map { "$it: profile ${view[it]}, draft ${draft[it]}" }
        assertEquals(emptyList(), lost, "answers the rebuilt draft lost")
    }
}
