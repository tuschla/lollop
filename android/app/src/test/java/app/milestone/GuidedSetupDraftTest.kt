package app.milestone

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM unit tests (no device) for [deriveDraft], the guided-setup answer → profile
 * mapping, and the pure setup/profile helpers around it. Pins the re-run
 * PRESERVATION contract: re-running the wizard with the same driving answers must
 * not silently reset customised profile values (a data-loss class of bug), while
 * a genuinely CHANGED level must re-derive the level-driven fields. The keep-flags
 * come from [keepFlags], the same function [GuidedSetup] calls on Finish.
 */
class GuidedSetupDraftTest {

    // A Both profile whose training-field values were CUSTOMISED past the plain
    // level-derived seeds (weeklySets 21 ≠ the level's 12; km 37; advanced ON while
    // the level is Intermediate; a measured HRmax and a priority muscle the wizard
    // has no step for).
    private val customised = ProfileDraft(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.Hypertrophy,
        goalDistance = GoalDistance.HalfMarathon,
        concurrentGoal = ConcurrentGoal.Hypertrophy,
        weeklySets = 21,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 37.0,
        advanced = true,
        female = false,
        bodyweightKg = 80.0,
        ageYears = 30.0,
        restingHrBpm = 55.0,
        measuredHrMax = 189.0,
        health = HealthScreen(),
        specializationMuscle = "biceps",
    )

    /** deriveDraft as GuidedSetup calls it for a Both re-run of [customised] with
     *  the level the user (re-)picked and every other driving answer unchanged;
     *  the keep-flags come from the production [keepFlags]. */
    private fun rerun(level: SetupLevel): ProfileDraft {
        val seedLevel = SetupLevel.fromCadence(customised.progressionCadence)
        val seedDays = seedTotalDays(Modality.Both, customised)!!
        val keep = keepFlags(
            initial = customised,
            seedModality = Modality.Both,
            seedLevel = seedLevel,
            seedRunLevel = seedLevel,
            seedDays = seedDays,
            seedLiftSplit = HYBRID_DEFAULT_LIFT_DAYS,
            seedBothPriority = customised.concurrentGoal.lead(),
            modality = Modality.Both,
            level = level,
            runLevel = level,
            days = seedDays,
            liftSplit = HYBRID_DEFAULT_LIFT_DAYS,
            bothPriority = BothPriority.LiftLean,
        )
        return deriveDraft(
            modality = Modality.Both,
            liftFocus = LiftFocus.Muscle,
            goalDistance = GoalDistance.HalfMarathon,
            bothPriority = BothPriority.LiftLean,
            daysPerWeek = seedDays,
            level = level,
            ageYears = 30.0,
            bodyweightKg = 80.0,
            female = false,
            restingHrBpm = 55.0,
            weightLb = false,
            health = HealthScreen(),
            initial = customised,
            keepLiftVolume = keep.liftVolume,
            keepRunVolume = keep.runVolume,
            keepRunDays = keep.runDays,
            keepAdvanced = keep.advanced,
        )
    }

    @Test
    fun keepFlagsFollowTheDrivingAnswers() {
        fun flags(
            initial: ProfileDraft? = customised,
            modality: Modality = Modality.Both,
            level: SetupLevel = SetupLevel.Intermediate,
            days: Int = 5,
            liftSplit: Int? = 2,
        ) = keepFlags(
            initial, Modality.Both, SetupLevel.Intermediate, SetupLevel.Intermediate, 5, 2, BothPriority.LiftLean,
            modality, level, level, days, liftSplit, BothPriority.LiftLean,
        )
        assertEquals(KeepFlags(liftVolume = true, runVolume = true, runDays = true, advanced = true), flags())
        // A first run has nothing to keep.
        assertEquals(KeepFlags(liftVolume = false, runVolume = false, runDays = false, advanced = false), flags(initial = null))
        // A new modality (e.g. a Profile Focus switch) re-derives every volume.
        val switched = flags(modality = Modality.Run)
        assertFalse(switched.liftVolume || switched.runVolume || switched.runDays)
        // More days: the run volume and run days re-derive, lifting volume stays.
        val moreDays = flags(days = 6)
        assertTrue(moreDays.liftVolume)
        assertFalse(moreDays.runVolume || moreDays.runDays)
        // A new split only re-derives the run days.
        val split = flags(liftSplit = 3)
        assertTrue(split.runVolume)
        assertFalse(split.runDays)
    }

    @Test
    fun unchangedLevelPreservesCustomisedAndWizardlessFields() {
        val out = rerun(SetupLevel.Intermediate)
        // Driving answers unchanged → the customised volumes survive verbatim
        // (not reset to the level's 12 sets / days×kmPerRunDay).
        assertEquals(21, out.weeklySets)
        assertEquals(37.0, out.runningKmPerWeek, 1e-9)
        // Editor-set Advanced survives an untouched re-run (level is its only driver).
        assertTrue(out.advanced)
        // Fields the wizard has no step for ride through untouched, never reset to
        // their defaults (null measured HRmax / no priority muscle).
        assertEquals(189.0, out.measuredHrMax!!, 1e-9)
        assertEquals("biceps", out.specializationMuscle)
    }

    @Test
    fun changedLevelRederivesAdvanced() {
        // A NEW level re-derives Advanced from the level alone: Advanced → true,
        // Novice → false, regardless of the profile's prior (true) flag.
        assertTrue(rerun(SetupLevel.Advanced).advanced)
        assertFalse(rerun(SetupLevel.Novice).advanced)
    }

    @Test
    fun untouchedPersonFieldsAndRunDaysSurviveVerbatim() {
        // A 1-run-day profile whose person doubles (62.5 kg, 34.7 y, 55.5 bpm) the
        // integer scales cannot represent. An untouched Finish passes the seed
        // doubles straight through (the composable's touched-flag gate keeps them),
        // and keepRunDays preserves the exact run-day count.
        val seed = ProfileDraft(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.FiveK,
            concurrentGoal = ConcurrentGoal.EndurancePriority,
            weeklySets = 0,
            runningDaysPerWeek = 1,
            runningKmPerWeek = 8.0,
            advanced = false,
            female = true,
            bodyweightKg = 62.5,
            ageYears = 34.7,
            restingHrBpm = 55.5,
            measuredHrMax = null,
            health = HealthScreen(),
        )
        val out = deriveDraft(
            modality = Modality.Run,
            liftFocus = null,
            goalDistance = GoalDistance.FiveK,
            bothPriority = null,
            daysPerWeek = 1,
            level = SetupLevel.Intermediate,
            // Untouched scales → the composable emits the seed doubles verbatim.
            ageYears = seed.ageYears,
            bodyweightKg = seed.bodyweightKg,
            female = true,
            restingHrBpm = seed.restingHrBpm,
            weightLb = false,
            health = HealthScreen(),
            initial = seed,
            keepLiftVolume = true,
            keepRunVolume = true,
            keepRunDays = true,
            keepAdvanced = true,
        )
        assertEquals(62.5, out.bodyweightKg!!, 0.0)
        assertEquals(34.7, out.ageYears!!, 0.0)
        assertEquals(55.5, out.restingHrBpm!!, 0.0)
        assertEquals(1, out.runningDaysPerWeek)
    }

    @Test
    fun rerun_preserves_training_context_fields() {
        // The wizard has no step for the advanced training-context fields, so a
        // re-run must carry the profile's values through verbatim, not null them.
        val seed = ProfileDraft(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 12,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 24.0,
            advanced = false,
            highLoadBlock = true,
            environment = Environment.Heat,
            envTempC = 32.0,
            envAltitudeM = 1500.0,
            weeksOff = 6.0,
        )
        val out = deriveDraft(
            modality = Modality.Both,
            liftFocus = LiftFocus.Muscle,
            goalDistance = GoalDistance.TenK,
            bothPriority = BothPriority.LiftLean,
            daysPerWeek = 6,
            level = SetupLevel.Intermediate,
            ageYears = 30.0,
            bodyweightKg = 80.0,
            female = false,
            restingHrBpm = 55.0,
            weightLb = false,
            health = HealthScreen(),
            initial = seed,
            keepLiftVolume = true,
            keepRunVolume = true,
            keepRunDays = true,
            keepAdvanced = true,
        )
        assertTrue(out.highLoadBlock)
        assertEquals(Environment.Heat, out.environment)
        assertEquals(32.0, out.envTempC!!, 0.0)
        assertEquals(1500.0, out.envAltitudeM!!, 0.0)
        assertEquals(6.0, out.weeksOff!!, 0.0)
    }

    @Test
    fun both_modality_rerun_seeds_total_days_as_run_plus_lift() {
        // A Both profile stores run days and lift days separately; a re-run
        // recovers the total as run days + lift days (the hybrid default 2 while
        // the lift days were never answered), not run × 2.
        val seed = ProfileDraft(
            progressionCadence = ProgressionCadence.WeekToWeek,
            liftGoal = LiftGoal.MaxStrength,
            goalDistance = GoalDistance.TenK,
            concurrentGoal = ConcurrentGoal.Strength,
            weeklySets = 12,
            runningDaysPerWeek = 3,
            runningKmPerWeek = 24.0,
            advanced = false,
        )
        assertEquals(5, seedTotalDays(Modality.Both, seed))
    }

    /** A first-run (no seed) Lift answer set with only the required age given. */
    private fun firstRunLift(ageYears: Double?, female: Boolean?, bodyweightKg: Double?, weightLb: Boolean) = deriveDraft(
        modality = Modality.Lift,
        liftFocus = LiftFocus.Strength,
        goalDistance = null,
        bothPriority = null,
        daysPerWeek = 3,
        level = SetupLevel.Novice,
        ageYears = ageYears,
        bodyweightKg = bodyweightKg,
        female = female,
        restingHrBpm = null,
        weightLb = weightLb,
        health = HealthScreen(),
        initial = null,
        keepLiftVolume = false,
        keepRunVolume = false,
        keepRunDays = false,
        keepAdvanced = false,
    )

    @Test
    fun finishWithOnlyAgeLeavesBodyweightAndSexUnset() {
        // Bodyweight and sex are optional: skipping them sends null (not a fabricated
        // default), while the required age goes through.
        val out = firstRunLift(ageYears = 35.0, female = null, bodyweightKg = null, weightLb = false)
        assertEquals(35.0, out.ageYears!!, 0.0)
        assertNull(out.bodyweightKg)
        assertNull(out.female)
        assertNull(out.restingHrBpm)
    }

    @Test
    fun localeUsFirstRunSeedsPounds() {
        // A first run follows the region: a US phone opens bodyweight in lb and the
        // Finish carries that preference; elsewhere stays kg. A re-run keeps the
        // profile's own choice regardless of region.
        assertEquals(WeightUnit.Lb, seedWeightUnit(null, Locale.US))
        assertEquals(WeightUnit.Kg, seedWeightUnit(null, Locale.GERMANY))
        val kgProfile = firstRunLift(30.0, null, null, weightLb = false)
        assertEquals(WeightUnit.Kg, seedWeightUnit(kgProfile, Locale.US))
        val out = firstRunLift(30.0, null, null, weightLb = seedWeightUnit(null, Locale.US) == WeightUnit.Lb)
        assertTrue(out.weightLb)
    }

    @Test
    fun experienceMapsToProgressionCadenceBothWays() {
        // The wizard level and the Profile "Experience" row are one mapping.
        assertEquals(ProgressionCadence.EverySession, firstRunLift(30.0, null, null, false).progressionCadence)
        for (level in SetupLevel.entries) {
            assertEquals(level, SetupLevel.fromCadence(level.cadence))
        }
        assertEquals(SetupLevel.Advanced, SetupLevel.fromCadence(ProgressionCadence.MonthToMonth))
    }

    @Test
    fun liftOnlyConcurrentGoalFollowsLiftGoal() {
        val out = firstRunLift(30.0, null, null, false)
        assertEquals(ConcurrentGoal.Strength, out.concurrentGoal)
        // Changing Training goal in Profile keeps a lift-only profile coherent.
        assertEquals(ConcurrentGoal.Power, out.withLiftGoal(LiftGoal.Power).concurrentGoal)
    }

    @Test
    fun bothTrainingGoalFollowsLeadingSide() {
        // Both + lifting leads: a new Training goal moves the concurrent goal with it.
        val liftLed = customised.copy(concurrentGoal = ConcurrentGoal.Strength)
        val muscle = liftLed.withLiftGoal(LiftGoal.Hypertrophy)
        assertEquals(LiftGoal.Hypertrophy, muscle.liftGoal)
        assertEquals(ConcurrentGoal.Hypertrophy, muscle.concurrentGoal)
        // Both + running leads: the lift goal changes, running still leads.
        val runLed = customised.copy(concurrentGoal = ConcurrentGoal.EndurancePriority)
        assertEquals(ConcurrentGoal.EndurancePriority, runLed.withLiftGoal(LiftGoal.Power).concurrentGoal)
        // The two-way "Which leads?" row maps onto the existing values.
        assertEquals(ConcurrentGoal.EndurancePriority, liftLed.withLead(BothPriority.RunLean).concurrentGoal)
        assertEquals(ConcurrentGoal.Hypertrophy, runLed.withLead(BothPriority.LiftLean).concurrentGoal)
        assertEquals(BothPriority.RunLean, ConcurrentGoal.EndurancePriority.lead())
        assertEquals(BothPriority.LiftLean, ConcurrentGoal.Power.lead())
    }

    @Test
    fun bothPathSplitsGoalsAndEndsInReview() {
        // One decision per screen: Both asks lifting goal, running goal and which
        // leads separately; every path ends on the review screen.
        val both = setupSteps(Modality.Both)
        assertEquals(10, both.size)
        assertEquals(
            listOf(SetupStep.LiftGoal, SetupStep.RunGoal, SetupStep.Lead),
            both.subList(1, 4),
        )
        assertEquals(SetupStep.Review, both.last())
        assertEquals(7, setupSteps(Modality.Lift).size)
        assertEquals(8, setupSteps(Modality.Run).size)
        assertFalse(SetupStep.Lead in setupSteps(Modality.Run))
        // Running sides ask what you run now; lifting-only doesn't.
        assertTrue(SetupStep.RunNow in both)
        assertFalse(SetupStep.RunNow in setupSteps(Modality.Lift))
        // Calisthenics adds placement right after the lifting goal, a race goal adds
        // race details right after the running goal (never before the deciding step).
        val full = setupSteps(Modality.Both, calisthenics = true, raceGoal = true)
        assertEquals(12, full.size)
        assertEquals(full.indexOf(SetupStep.LiftGoal) + 1, full.indexOf(SetupStep.Placement))
        assertEquals(full.indexOf(SetupStep.RunGoal) + 1, full.indexOf(SetupStep.Race))
        assertFalse(SetupStep.Placement in setupSteps(Modality.Run, calisthenics = true))
    }

    @Test
    fun youthAndHealthFlagsDefer() {
        assertTrue(healthDefers(16.0, HealthScreen()))
        assertFalse(healthDefers(30.0, HealthScreen()))
        assertTrue(healthDefers(30.0, HealthScreen(parq_positive = true)))
        assertFalse(healthDefers(30.0, HealthScreen(parq_positive = true, medically_cleared = true)))
        assertTrue(healthDefers(30.0, HealthScreen(acute_illness = true)))
    }

    /** A first-run answer set; only the arguments a test cares about change. */
    private fun firstRun(
        modality: Modality,
        days: Int,
        level: SetupLevel = SetupLevel.Intermediate,
        runLevel: SetupLevel? = null,
        goal: GoalDistance? = GoalDistance.TenK,
        lead: BothPriority? = null,
        liftFocus: LiftFocus? = LiftFocus.Strength,
        liftDays: Int? = null,
        weekdays: Int? = null,
        weeklyRunKm: Double? = null,
        raceDate: Long? = null,
        meetDate: Long? = null,
        health: HealthScreen = HealthScreen(),
    ) = deriveDraft(
        modality = modality,
        liftFocus = liftFocus,
        goalDistance = goal,
        bothPriority = lead,
        daysPerWeek = days,
        level = level,
        ageYears = 30.0,
        bodyweightKg = null,
        female = null,
        restingHrBpm = null,
        weightLb = false,
        health = health,
        initial = null,
        keepLiftVolume = false,
        keepRunVolume = false,
        keepRunDays = false,
        keepAdvanced = false,
        runLevel = runLevel,
        liftDays = liftDays,
        trainingWeekdays = weekdays,
        weeklyRunKm = weeklyRunKm,
        raceDateEpochDay = raceDate,
        meetEpochDay = meetDate,
    )

    @Test
    fun liftPathStoresTheDaysAnswerAsLiftDays() {
        val out = firstRun(Modality.Lift, days = 4)
        assertEquals(4, out.liftDaysPerWeek)
        assertEquals(0, out.runningDaysPerWeek)
        // A runner has no lifting days.
        assertNull(firstRun(Modality.Run, days = 4).liftDaysPerWeek)
        // A re-run reopens the days step on the stored lifting days.
        assertEquals(4, seedTotalDays(Modality.Lift, out))
    }

    @Test
    fun weekdayChipsBuildTheMondayFirstMask() {
        val satSun = toggleWeekday(toggleWeekday(null, 5), 6)
        assertEquals(0b1100000, satSun)
        assertEquals(2, weekdayCount(satSun))
        assertEquals(0b1100000, firstRun(Modality.Lift, days = 2, weekdays = satSun).trainingWeekdays)
        // Unticking every day is "Any days" (no mask on the wire).
        assertNull(toggleWeekday(toggleWeekday(satSun, 5), 6))
        assertNull(firstRun(Modality.Lift, days = 2).trainingWeekdays)
    }

    @Test
    fun answeredWeeklyDistanceIsSentAndASkipIsNotSet() {
        // Skipped: sent as NOT SET (0.0 + flag), never a shell-invented number;
        // the engine picks its own conservative start (C9).
        val skipped = firstRun(Modality.Run, days = 4)
        assertEquals(0.0, skipped.runningKmPerWeek, 0.0)
        assertFalse(skipped.runningKmPerWeekSet)
        assertTrue(skipped.toEvent().toJson().toString().contains("\"running_km_per_week_set\":false"))
        // Answered: exactly what the athlete runs now, including "I don't run yet".
        val answered = firstRun(Modality.Run, days = 4, weeklyRunKm = 20.0)
        assertEquals(20.0, answered.runningKmPerWeek, 1e-9)
        assertTrue(answered.runningKmPerWeekSet)
        val none = firstRun(Modality.Run, days = 4, weeklyRunKm = 0.0)
        assertEquals(0.0, none.runningKmPerWeek, 1e-9)
        assertTrue(none.runningKmPerWeekSet)
        // An answered profile keeps the legacy wire (no flag emitted).
        assertFalse(answered.toEvent().toJson().toString().contains("running_km_per_week_set"))
        // A lifter has no running to leave unset.
        assertTrue(firstRun(Modality.Lift, days = 3).runningKmPerWeekSet)
        // A display-unit answer converts back to canonical km.
        assertEquals(16.09, displayToKm(10.0, DistanceUnit.Mi), 1e-9)
    }

    @Test
    fun unchangedRerunKeepsAnUnsetWeeklyDistanceUnset() {
        val seed = firstRun(Modality.Run, days = 4)
        val out = deriveDraft(
            modality = Modality.Run,
            liftFocus = null,
            goalDistance = GoalDistance.TenK,
            bothPriority = null,
            daysPerWeek = 4,
            level = SetupLevel.Intermediate,
            ageYears = 30.0,
            bodyweightKg = null,
            female = null,
            restingHrBpm = null,
            weightLb = false,
            health = HealthScreen(),
            initial = seed,
            keepLiftVolume = true,
            keepRunVolume = true,
            keepRunDays = true,
            keepAdvanced = true,
        )
        assertFalse(out.runningKmPerWeekSet)
        assertEquals(0.0, out.runningKmPerWeek, 0.0)
    }

    @Test
    fun parqChestPainIsASymptomThatClearanceCannotLift() {
        val chest = HealthScreen().withParqAnswer(ParqItem.ChestPain, true)
        assertTrue(chest.symptomatic)
        assertFalse(chest.parq_positive)
        assertEquals(1, chest.parqYesCount)
        assertTrue(healthDefers(30.0, chest))
        assertTrue(healthDefers(30.0, chest.copy(medically_cleared = true)))
        assertTrue(firstRun(Modality.Lift, days = 3, health = chest).health.symptomatic)
        // Doctor follow-up items raise parq_positive, which clearance does lift.
        val heart = HealthScreen().withParqAnswer(ParqItem.HeartOrBloodPressure, true).copy(medically_cleared = true)
        assertTrue(heart.parq_positive)
        assertFalse(healthDefers(30.0, heart))
        // Answering it No again drops the follow-up AND its clearance.
        val cleared = heart.withParqAnswer(ParqItem.HeartOrBloodPressure, false)
        assertFalse(cleared.parq_positive)
        assertFalse(cleared.medically_cleared)
        // The bone/joint question is the injury/rehab hold.
        assertTrue(HealthScreen().withParqAnswer(ParqItem.BoneOrJoint, true).injury_or_rehab)
        // A legacy single-toggle answer stays raised until a question is answered.
        val legacy = HealthScreen(parq_positive = true)
        assertTrue(legacy.legacyParq)
        assertTrue(legacy.withParqAnswer(ParqItem.ChestPain, false).parq_positive)
    }

    @Test
    fun raceAndMeetDatesAreStoredForTheGoalsTheyShape() {
        assertEquals(20500L, firstRun(Modality.Run, days = 4, goal = GoalDistance.HalfMarathon, raceDate = 20500L).goalRaceEpochDay)
        // No race to date for general fitness.
        assertNull(firstRun(Modality.Run, days = 4, goal = GoalDistance.General, raceDate = 20500L).goalRaceEpochDay)
        assertEquals(20480L, firstRun(Modality.Lift, days = 3, meetDate = 20480L).meetEpochDay)
        assertNull(firstRun(Modality.Lift, days = 3, liftFocus = LiftFocus.Muscle, meetDate = 20480L).meetEpochDay)
    }

    @Test
    fun bothPathRunningVolumeFollowsTheRunningLevel() {
        val out = firstRun(
            Modality.Both, days = 5, level = SetupLevel.Advanced, runLevel = SetupLevel.Novice,
            lead = BothPriority.LiftLean,
        )
        // Lifting answers drive cadence and sets; running answers drive the km.
        assertEquals(ProgressionCadence.MonthToMonth, out.progressionCadence)
        assertEquals(16, out.weeklySets)
        assertEquals(2, out.liftDaysPerWeek)
        assertEquals(3, out.runningDaysPerWeek)
        // Running answers drive the km: none given, so it goes out as not set.
        assertFalse(out.runningKmPerWeekSet)
        assertFalse(out.advanced)
    }

    @Test
    fun hybridSplitStoresWhatTheEngineSchedules() {
        // Lifting leads: running is capped at 3 days, the rest are rest days.
        assertEquals(3, firstRun(Modality.Both, days = 7, lead = BothPriority.LiftLean).runningDaysPerWeek)
        // Running leads: every remaining day runs, lifting is held to 3 at most.
        assertEquals(5, firstRun(Modality.Both, days = 7, lead = BothPriority.RunLean).runningDaysPerWeek)
        assertEquals(2..3, hybridLiftDayOptions(7, BothPriority.RunLean))
        assertEquals(2..5, hybridLiftDayOptions(7, BothPriority.LiftLean))
        assertEquals(2..2, hybridLiftDayOptions(3, BothPriority.LiftLean))
        val fourLift = firstRun(Modality.Both, days = 6, lead = BothPriority.LiftLean, liftDays = 4)
        assertEquals(4, fourLift.liftDaysPerWeek)
        assertEquals(2, fourLift.runningDaysPerWeek)
        assertEquals(6, seedTotalDays(Modality.Both, fourLift))
    }

    @Test
    fun calisthenicsPlacementMapsToLadderRungs() {
        val events = placementEvents(listOf(CalisPlacement.PushKnee, CalisPlacement.PullMany, null), 1_759_700_000L)
        assertEquals(2, events.size)
        val pull = events.first { it.ladder == CalisLadder.VerticalPull }
        assertTrue(pull.rung >= 4) // at least Full pull-up
        assertEquals(3, events.first { it.ladder == CalisLadder.HorizontalPush }.rung)
        // "Not sure, start easy" sends nothing: the ladder starts at its easiest rung.
        assertTrue(placementEvents(listOf(null, null, null), 0L).isEmpty())
    }

    @Test
    fun rerunSeedsThePlacementsTheLaddersHoldAndSendsOnlyChanges() {
        val calis = firstRun(Modality.Lift, days = 3).copy(
            resistanceModality = ResistanceModality.Calisthenics,
            calisEquipment = CalisEquipment(pull_up_bar = true),
        )
        val rungs = currentPlacementRungs(
            listOf(
                CalisLadderView(ladder = CalisLadder.HorizontalPush, current_rung = 3, placed_rung = 3),
                // Incline push-ups and the like: no placement answer sits on rung 1.
                CalisLadderView(ladder = CalisLadder.VerticalPull, current_rung = 1),
                CalisLadderView(ladder = CalisLadder.Squat, current_rung = 1, placed_rung = 1),
                CalisLadderView(ladder = CalisLadder.Dip, current_rung = 2),
            ),
        )
        assertEquals(setOf(CalisLadder.HorizontalPush, CalisLadder.VerticalPull, CalisLadder.Squat), rungs.keys)
        val seed = seedAnswers(calis, today = 20_000L, recordedRunLevel = null, distanceUnit = DistanceUnit.Km, runStash = null, currentRungs = rungs)
        assertEquals(CalisPlacement.PushKnee, seed.pushPlace)
        assertNull(seed.pullPlace)
        assertEquals(CalisPlacement.SquatBodyweight, seed.squatPlace)
        // An untouched re-run re-places nothing; a changed answer is the only event.
        assertTrue(placementEvents(seed, seed, 0L).isEmpty())
        val changed = placementEvents(seed.copy(pushPlace = CalisPlacement.PushFull), seed, 0L)
        assertEquals(listOf(CalisLadder.HorizontalPush to 4), changed.map { it.ladder to it.rung })
        // Without anything to hang from, no pull-up placement is seeded.
        val noBar = seedAnswers(
            calis.copy(calisEquipment = CalisEquipment()), 20_000L, null, DistanceUnit.Km, null,
            mapOf(CalisLadder.VerticalPull to 4),
        )
        assertNull(noBar.pullPlace)
        // A Weights profile seeds no placement.
        val weights = seedAnswers(calis.copy(resistanceModality = ResistanceModality.Weights), 20_000L, null, DistanceUnit.Km, null, rungs)
        assertNull(weights.pushPlace)
    }

    @Test
    fun aFinishedSetupsUnstatedSexSeedsPreferNotToSay() {
        val noSex = firstRun(Modality.Lift, days = 3)
        assertEquals(SexAnswer.NotSaying, seedAnswers(noSex, 20_000L, null, DistanceUnit.Km, null).sex)
        assertEquals(SexAnswer.Female, seedAnswers(noSex.copy(female = true), 20_000L, null, DistanceUnit.Km, null).sex)
        // A first run asks afresh: nothing selected.
        assertNull(seedAnswers(null, 20_000L, null, DistanceUnit.Km, null).sex)
    }

    @Test
    fun raceTimeKeypadFillsFromTheRight() {
        assertEquals(1575, raceTimeSec("2615"))
        assertEquals("26:15", raceTimeDisplay("2615"))
        assertEquals("0:02", raceTimeDisplay("2"))
        assertEquals("1:52:00", raceTimeDisplay("15200"))
        assertEquals(6720, raceTimeSec("15200"))
        assertEquals("2615", raceTimeDigits(1575))
        assertEquals("15200", raceTimeDigits(6720))
        assertNull(raceTimeSec("2675")) // 75 seconds isn't a time
        assertTrue(plausibleRaceTime(5.0, 1575))
        assertFalse(plausibleRaceTime(5.0, 25)) // "25" typed for 25:00
    }

    @Test
    fun raceDigitEntryIsSharedBySetupAndProfile() {
        assertEquals("2", appendRaceDigit("", '2', fresh = true))
        assertEquals("26", appendRaceDigit("2", '6', fresh = false))
        // A fresh field starts over; a leading zero is dropped.
        assertEquals("7", appendRaceDigit("2615", '7', fresh = true))
        assertEquals("", appendRaceDigit("", '0', fresh = true))
        // h mm ss is six digits at most; non-digits are ignored.
        assertNull(appendRaceDigit("123456", '7', fresh = false))
        assertNull(appendRaceDigit("26", '.', fresh = false))
    }

    @Test
    fun reviewEditVisitsOnlyWhatItChanged() {
        val ready: (SetupStep) -> Boolean = { true }
        // Days changed from the review, nothing else: straight back to the review.
        val lift = setupSteps(Modality.Lift)
        val liftBase = ReviewReturn(Modality.Lift, calisthenics = false, raceGoal = false)
        assertNull(nextPendingStep(lift, lift.indexOf(SetupStep.Days), liftBase, Modality.Lift, ready))
        // Lift → Both: the new running screens plus days and level (their meaning
        // changes with the modality), in order.
        val both = setupSteps(Modality.Both)
        val walk = generateSequence(nextPendingStep(both, 0, liftBase, Modality.Both, ready)) { i ->
            nextPendingStep(both, i, liftBase, Modality.Both, ready)
        }.map { both[it] }.toList()
        assertEquals(listOf(SetupStep.RunGoal, SetupStep.Lead, SetupStep.Days, SetupStep.Level, SetupStep.RunNow), walk)
        // An answer the edit left missing is visited even on an unchanged path.
        val missingLevel: (SetupStep) -> Boolean = { it != SetupStep.Level }
        assertEquals(lift.indexOf(SetupStep.Level), nextPendingStep(lift, lift.indexOf(SetupStep.Days), liftBase, Modality.Lift, missingLevel))
    }

    @Test
    fun progressNeverMovesBackWhenAnAnswerAddsAScreen() {
        val plain = setupSteps(Modality.Run)
        val race = setupSteps(Modality.Run, raceGoal = true)
        val at = plain.indexOf(SetupStep.RunGoal)
        // Picking a race goal adds a screen after RunGoal: the bar stays put.
        assertEquals(setupProgress(plain, at, Modality.Run), setupProgress(race, race.indexOf(SetupStep.RunGoal), Modality.Run), 0f)
        assertEquals(1f, setupProgress(plain, plain.lastIndex, Modality.Run), 0f)
        // Step 1 already shows its share of the bar, never an empty track.
        assertEquals(1f / longestSetupSteps(Modality.Run).size, setupProgress(plain, 0, Modality.Run), 0f)
    }

    @Test
    fun weekdaysMustCoverTheDayCountInSetupAndProfile() {
        assertTrue(weekdaysCover(null, 5))
        // Mon, Wed, Thu, Fri ticked (bit0 = Monday).
        val monWedThuFri = 0b0011101
        // Five training days don't fit four ticked weekdays (the Profile bug).
        assertFalse(weekdaysCover(monWedThuFri, 5))
        assertTrue(weekdaysCover(monWedThuFri, 4))
        assertTrue(weekdaysCover(monWedThuFri, 3))
        // Profile's Training days total: lift days + run days for Both (the hybrid
        // default 2 while lift days were never answered), else the one side.
        val hybrid = customised.copy(liftDaysPerWeek = null, runningDaysPerWeek = 3)
        assertEquals(5, hybrid.trainingDayTotal())
        assertEquals(4, hybrid.copy(weeklySets = 0, runningDaysPerWeek = 4).trainingDayTotal())
        assertEquals(LIFT_ONLY_DEFAULT_DAYS, hybrid.copy(runningDaysPerWeek = 0).trainingDayTotal())
    }

    @Test
    fun focusSwitchSeedsOnlyDaysTheUserGave() {
        val lifter = firstRun(Modality.Lift, days = 3)
        // Back to running: the stashed run days, else asked afresh.
        assertEquals(4, switchSeedDays(Modality.Run, lifter, stashedRunDays = 4))
        assertNull(switchSeedDays(Modality.Run, lifter, stashedRunDays = null))
        assertEquals(6, switchSeedDays(Modality.Both, lifter, stashedRunDays = 3))
        // A runner has no lifting days to seed from.
        assertNull(switchSeedDays(Modality.Lift, firstRun(Modality.Run, days = 4), stashedRunDays = null))
        // Dropping a side keeps the other and its goal coherent.
        val liftOnly = customised.withoutSide(Focus.Lift)
        assertEquals(0, liftOnly.runningDaysPerWeek)
        assertEquals(0.0, liftOnly.runningKmPerWeek, 0.0)
        assertEquals(ConcurrentGoal.Hypertrophy, liftOnly.concurrentGoal)
        val runOnly = customised.withoutSide(Focus.Run)
        assertEquals(0, runOnly.weeklySets)
        assertEquals(ConcurrentGoal.EndurancePriority, runOnly.concurrentGoal)
    }

    @Test
    fun focusSwitchBackResumesASideSetUpBefore() {
        val runner = firstRun(Modality.Run, days = 4)
        // The lift days kept when lifting was switched off seed the setup.
        assertEquals(4, switchSeedDays(Modality.Lift, runner, stashedRunDays = null, stashedLiftDays = 4))
        val monWedThuFri = 0b0011101
        val lift = StashedLift(days = 4, goal = LiftGoal.MaxStrength, weeklySets = 12, weekdays = monWedThuFri)
        val resumed = runner.resumedSide(Focus.Lift, lift, run = null, today = 0L)
        assertEquals(12, resumed?.weeklySets)
        assertEquals(4, resumed?.liftDaysPerWeek)
        assertEquals(0, resumed?.runningDaysPerWeek)
        assertEquals(ConcurrentGoal.Strength, resumed?.concurrentGoal)
        assertEquals(monWedThuFri, resumed?.trainingWeekdays)
        // An answer missing (lift days never given) or weekdays no longer covering
        // the days: the guided setup asks instead.
        assertNull(runner.resumedSide(Focus.Lift, lift.copy(days = null), run = null, today = 0L))
        assertNull(runner.resumedSide(Focus.Lift, lift.copy(weekdays = 0b0000011), run = null, today = 0L))
        // Both always asks which side leads.
        assertNull(runner.resumedSide(Focus.Both, lift, run = null, today = 0L))

        val lifter = firstRun(Modality.Lift, days = 3)
        val run = StashedRun(days = 3, km = 24.0, goal = GoalDistance.TenK, raceEpochDay = 100L, weekdays = null)
        val back = lifter.resumedSide(Focus.Run, lift = null, run = run, today = 200L)
        assertEquals(3, back?.runningDaysPerWeek)
        assertEquals(0, back?.weeklySets)
        assertEquals(GoalDistance.TenK, back?.goalDistance)
        assertEquals(24.0, back?.runningKmPerWeek ?: -1.0, 0.0)
        // A race date already passed is no longer a target.
        assertNull(back?.goalRaceEpochDay)
        assertEquals(ConcurrentGoal.EndurancePriority, back?.concurrentGoal)
        assertNull(lifter.resumedSide(Focus.Run, lift = null, run = run.copy(goal = null), today = 200L))
    }

    @Test
    fun pullUpRungsNeedSomethingToHangFrom() {
        assertFalse(CalisEquipment().canHang)
        assertTrue(CalisEquipment(pull_up_bar = true).canHang)
        assertTrue(CalisEquipment(rings = true).canHang)
        assertFalse(CalisEquipment(dip_bars = true).canHang)
    }

    @Test
    fun theYouthHoldIsAboutAgeNotHealthAnswers() {
        assertTrue(isMinor(17.0))
        assertFalse(isMinor(18.0))
        assertFalse(isMinor(null))
        // The health answers alone don't defer a 16-year-old: the review shows the
        // hold under About you instead.
        assertFalse(healthDefers(null, HealthScreen()))
        assertTrue(healthDefers(16.0, HealthScreen()))
    }

    @Test
    fun ageOnDayCountsCompletedCalendarYears() {
        val born = 7_740L // 1991-03-12
        assertEquals(34, ageOnDay(born, 20_523L)) // 2026-03-11
        assertEquals(35, ageOnDay(born, 20_524L)) // 2026-03-12
        // A 29 February birthday completes its year on 1 March in a common year,
        // as the core counts it.
        val leap = 13_938L // 2008-02-29
        assertEquals(17, ageOnDay(leap, 20_512L)) // 2026-02-28
        assertEquals(18, ageOnDay(leap, 20_513L)) // 2026-03-01
        assertEquals(0, ageOnDay(20_600L, 20_500L))
    }

    @Test
    fun theBirthdayPickerOffersOnlyAgesFourteenToAHundred() {
        val today = 20_743L // 2026-10-17
        val range = birthdayRange(today)
        assertEquals(14, ageOnDay(range.last, today))
        assertEquals(13, ageOnDay(range.last + 1, today))
        assertEquals(100, ageOnDay(range.first, today))
        assertEquals(101, ageOnDay(range.first - 1, today))
        assertTrue("no future birthday", range.last < today)
    }

    @Test
    fun theBirthdayCalendarOpensOnTheBirthdayElseThirtyYearsBack() {
        val today = 20_743L // 2026-10-17
        assertEquals(java.time.LocalDate.of(1996, 10, 17).toEpochDay(), birthdayPickerStart(null, today))
        val born = 7_740L // 1991-03-12
        assertEquals(born, birthdayPickerStart(born, today))
        val range = birthdayRange(today)
        assertEquals("a stale birthday outside the range is clamped", range.first, birthdayPickerStart(range.first - 400, today))
    }

    @Test
    fun theBirthdayGivesTheAgeAndTheYouthHoldLiftsOnTheEighteenthBirthday() {
        val born = 14_170L // 2008-10-18
        val eveOfBirthday = 20_743L // 2026-10-17
        val seed = seedAnswers(null, eveOfBirthday, null, DistanceUnit.Km, null)
        val unanswered = seed.copy(modality = Modality.Lift, liftFocus = LiftFocus.of(LiftGoal.MaxStrength), days = 3, level = SetupLevel.Intermediate)
        assertFalse("the birthday needs an answer", SetupValues(unanswered, seed, null, null, DistanceUnit.Km, eveOfBirthday).ready(SetupStep.About))

        val answered = unanswered.copy(birthDay = born)
        val eve = SetupValues(answered, seed, null, null, DistanceUnit.Km, eveOfBirthday)
        assertTrue(eve.ready(SetupStep.About))
        assertEquals(17.0, eve.ageYears!!, 0.0)
        assertTrue(isMinor(eve.ageYears))
        val draft = eve.draft()!!
        assertEquals(born, draft.birthEpochDay)
        assertEquals(17.0, draft.ageYears!!, 0.0)

        val birthday = SetupValues(answered, seed, null, null, DistanceUnit.Km, eveOfBirthday + 1)
        assertEquals(18.0, birthday.ageYears!!, 0.0)
        assertFalse(isMinor(birthday.ageYears))
    }

    @Test
    fun aLegacyStatedAgeStandsUntilABirthdayIsPicked() {
        val today = 20_743L
        // A profile saved before birthdays: only the stated age.
        val seed = seedAnswers(customised, today, null, DistanceUnit.Km, null)
        assertNull(seed.birthDay)
        val legacy = SetupValues(seed, seed, customised, null, DistanceUnit.Km, today)
        assertEquals(30.0, legacy.ageYears!!, 0.0)
        assertTrue(legacy.ready(SetupStep.About))
        assertNull(legacy.draft()!!.birthEpochDay)
        assertEquals(30.0, legacy.draft()!!.ageYears!!, 0.0)

        // Adding the birthday replaces it, and a re-run seeds the birthday back.
        val added = SetupValues(seed.copy(birthDay = 7_740L), seed, customised, null, DistanceUnit.Km, today)
        assertEquals(35.0, added.ageYears!!, 0.0)
        assertEquals(7_740L, added.draft()!!.birthEpochDay)
        assertEquals(7_740L, seedAnswers(customised.copy(birthEpochDay = 7_740L), today, null, DistanceUnit.Km, null).birthDay)
    }

    @Test
    fun preferNotToSayAnswersTheBirthdayAndSendsNoAge() {
        val today = 20_743L
        val seed = seedAnswers(null, today, null, DistanceUnit.Km, null)
        assertFalse("a first run starts unanswered", seed.birthSkipped)
        val unanswered = seed.copy(modality = Modality.Lift, liftFocus = LiftFocus.of(LiftGoal.MaxStrength), days = 3, level = SetupLevel.Intermediate)
        val open = SetupValues(unanswered, seed, null, null, DistanceUnit.Km, today)
        assertFalse(open.ageAnswered)
        assertFalse(open.ready(SetupStep.About))

        // Choosing it after a birthday clears the birthday and opens Next.
        val skipped = unanswered.withBirthday(7_740L).withoutBirthday()
        assertTrue(skipped.birthSkipped)
        assertNull(skipped.birthDay)
        val v = SetupValues(skipped, seed, null, null, DistanceUnit.Km, today)
        assertTrue(v.ready(SetupStep.About))
        assertNull(v.ageYears)
        val draft = v.draft()!!
        assertNull(draft.birthEpochDay)
        assertNull(draft.ageYears)
        val wire = draft.toEvent().toJson().toString()
        assertFalse(wire.contains("birth_epoch_day"))
        assertFalse(wire.contains("age_years"))

        // Picking a birthday afterwards clears the skip.
        val picked = skipped.withBirthday(7_740L)
        assertFalse(picked.birthSkipped)
        assertEquals(35.0, SetupValues(picked, seed, null, null, DistanceUnit.Km, today).ageYears!!, 0.0)
    }

    @Test
    fun preferNotToSayDropsALegacyStatedAge_andANoAgeProfileReseedsIt() {
        val today = 20_743L
        val seed = seedAnswers(customised, today, null, DistanceUnit.Km, null)
        assertFalse("a stated age is an answer, not a skip", seed.birthSkipped)
        val dropped = SetupValues(seed.withoutBirthday(), seed, customised, null, DistanceUnit.Km, today)
        assertTrue(dropped.ready(SetupStep.About))
        assertNull(dropped.ageYears)
        assertNull(dropped.draft()!!.ageYears)
        assertNull(dropped.draft()!!.birthEpochDay)

        // A re-run of a profile saved with no age opens with the skip chosen.
        val noAge = customised.copy(ageYears = null, birthEpochDay = null)
        val reseed = seedAnswers(noAge, today, null, DistanceUnit.Km, null)
        assertTrue(reseed.birthSkipped)
        val rerun = SetupValues(reseed, reseed, noAge, null, DistanceUnit.Km, today)
        assertTrue(rerun.ready(SetupStep.About))
        assertNull(rerun.draft()!!.ageYears)
    }
}
