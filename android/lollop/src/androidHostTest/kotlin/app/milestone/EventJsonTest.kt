package app.milestone

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the serde wire form between the shell and the Rust core: events encode as
 * the core expects (unit variants as a bare string, struct variants as a
 * single-key object with snake_case fields) and the view JSON decodes into the
 * Kotlin view classes. A drift here means the FFI silently rejects or mis-reads
 * data, so these guard the contract without the native library.
 */
class EventJsonTest {

    private fun obj(e: Event) = (e.toJson() as JsonObject)

    /** A SetProfile with only the eight required fields; tests `copy` in the rest. */
    private fun profile() = Event.SetProfile(
        ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
        ConcurrentGoal.Strength, 12, 4, 45.0, false,
    )

    @Test
    fun everyUnitEventIsItsBareVariantName() {
        // Each `data object` Event encodes as the bare Rust variant name.
        val units = Event::class.java.declaredClasses.mapNotNull { c ->
            runCatching { c.getField("INSTANCE").get(null) }.getOrNull() as? Event
        }
        assertTrue("found ${units.size} unit events", units.size >= 13)
        for (u in units) assertEquals("\"${u.javaClass.simpleName}\"", u.toJson().toString())
    }

    @Test
    fun logSetHasSnakeCaseFields() {
        val fields = obj(Event.LogSet("Back Squat", 100.0, 5, 8.0, 1_700_000_000L))["LogSet"]!!.jsonObject
        assertEquals("Back Squat", fields["exercise"]!!.jsonPrimitive.content)
        assertEquals(100.0, fields["weight_kg"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(5, fields["reps"]!!.jsonPrimitive.content.toInt())
        assertEquals(8.0, fields["rpe"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        // Log time rides on the wire as snake_case unix seconds so the core can
        // carry it back into the history view (LiftResultView.observed_at).
        assertEquals(1_700_000_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setProfileEmitsTheEightRequiredFields() {
        val fields = obj(profile().copy(advanced = true))["SetProfile"]!!.jsonObject
        // Unset optional fields are omitted, so the core's serde defaults fill them.
        assertEquals(
            setOf(
                "progression_cadence", "lift_goal", "goal_distance", "concurrent_goal",
                "weekly_sets", "running_days_per_week", "running_km_per_week", "advanced",
            ),
            fields.keys,
        )
        // Enum values are the variant name verbatim (serde reads these).
        assertEquals("WeekToWeek", fields["progression_cadence"]!!.jsonPrimitive.content)
        assertEquals("TenK", fields["goal_distance"]!!.jsonPrimitive.content)
    }

    @Test
    fun setProfileEmitsCoachingInputsAndRungEvent() {
        val fields = obj(
            profile().copy(
                liftDaysPerWeek = 4, trainingWeekdays = 0b1100000, goalRaceEpochDay = 20400L,
                recentRace = RecentRace(5.0, 1575), statedLongestRecentKm = 12.0,
                layoffReturnedEpochDay = 20360L, meetEpochDay = 20420L,
                calisEquipment = CalisEquipment(pull_up_bar = true), specializationMuscle = "side delts",
                runnerStrength = true,
                health = HealthScreen().withParqAnswer(ParqItem.ChestPain, true).copy(acute_illness = true),
            )
        )["SetProfile"]!!.jsonObject
        assertEquals(4, fields["lift_days_per_week"]!!.jsonPrimitive.content.toInt())
        assertEquals(96, fields["training_weekdays"]!!.jsonPrimitive.content.toInt())
        assertEquals(20400L, fields["goal_race_epoch_day"]!!.jsonPrimitive.content.toLong())
        val race = fields["recent_race"]!!.jsonObject
        assertEquals(1575, race["time_sec"]!!.jsonPrimitive.content.toInt())
        assertFalse(race.containsKey("epoch_day"))
        assertEquals("side delts", fields["specialization_muscle"]!!.jsonPrimitive.content)
        assertTrue(fields["calis_equipment"]!!.jsonObject["pull_up_bar"]!!.jsonPrimitive.content.toBoolean())
        assertTrue(fields["runner_strength"]!!.jsonPrimitive.content.toBoolean())
        val health = fields["health"]!!.jsonObject
        assertTrue(health["symptomatic"]!!.jsonPrimitive.content.toBoolean())
        assertTrue(health["acute_illness"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(2, health["parq_yes"]!!.jsonPrimitive.content.toInt())
        assertFalse(fields.containsKey("endurance_intensity_pct_vo2max"))

        val rung = obj(Event.SetCalisthenicsRung(CalisLadder.VerticalPull, 4, 1_759_700_000L))["SetCalisthenicsRung"]!!.jsonObject
        assertEquals("VerticalPull", rung["ladder"]!!.jsonPrimitive.content)
        assertEquals(4, rung["rung"]!!.jsonPrimitive.content.toInt())
        assertEquals(1_759_700_000L, rung["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setProfileEmitsPersonFieldsWhenSet() {
        val fields = obj(
            profile().copy(
                female = true, bodyweightKg = 62.5, ageYears = 34.0,
                restingHrBpm = 54.0, measuredHrMax = 188.0,
            )
        )["SetProfile"]!!.jsonObject
        // snake_case names the core parses back; values verbatim.
        assertTrue(fields["female"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(62.5, fields["bodyweight_kg"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(34.0, fields["age_years"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(54.0, fields["resting_hr_bpm"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(188.0, fields["measured_hr_max"]!!.jsonPrimitive.content.toDouble(), 1e-9)
    }

    @Test
    fun setProfileEmitsTheBirthdayAndTheDraftRoundTripsIt() {
        val view = ProfileView(progression_cadence = "WeekToWeek", lift_goal = "MaxStrength", goal_distance = "TenK", concurrent_goal = "Strength", weekly_sets = 12, birth_epoch_day = 7_740L, age_years = 35.0)
        val draft = Event.SetProfile.from(view)
        assertEquals(7_740L, draft.birthEpochDay)
        val fields = obj(draft)["SetProfile"]!!.jsonObject
        assertEquals(7_740L, fields["birth_epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setProfileOmitsFemaleWhenUnanswered() {
        val fields = obj(
            profile().copy(
                female = null, bodyweightKg = 80.0,
            )
        )["SetProfile"]!!.jsonObject
        assertFalse(fields.containsKey("female"))
        assertTrue(fields.containsKey("bodyweight_kg"))
    }

    @Test
    fun setProfileEmitsAnExplicitMaleAnswer() {
        val fields = obj(
            profile().copy(
                female = false,
            )
        )["SetProfile"]!!.jsonObject
        assertFalse(fields["female"]!!.jsonPrimitive.content.toBoolean())
    }

    @Test
    fun setProfileEmitsAdvancedContextFieldsWhenSet() {
        // high_load_block, environment, env_temp_c/_m and weeks_off make
        // autoreg-029, the environment modifiers and REENTRY-001 reachable. Each
        // rides the wire in the snake_case name / bare-variant form the core parses.
        val fields = obj(
            profile().copy(
                highLoadBlock = true, environment = Environment.Heat,
                envTempC = 28.0, envAltitudeM = 1800.0, weeksOff = 6.0,
            )
        )["SetProfile"]!!.jsonObject
        assertTrue(fields["high_load_block"]!!.jsonPrimitive.content.toBoolean())
        // Environment crosses as the bare Rust variant name.
        assertEquals("Heat", fields["environment"]!!.jsonPrimitive.content)
        assertEquals(28.0, fields["env_temp_c"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1800.0, fields["env_altitude_m"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(6.0, fields["weeks_off"]!!.jsonPrimitive.content.toDouble(), 1e-9)
    }

    @Test
    fun setProfileEmitsTheEffectiveDayWhenSet() {
        val fields = obj(profile().copy(effectiveFromEpochDay = 20354L))["SetProfile"]!!.jsonObject
        assertEquals(20354L, fields["effective_from_epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun planHypertrophyMesoEmitsNextMesoFlagsWhenSet() {
        // not_growing + recovering_easily arm the hypertrophy-008 next-meso
        // decision (+2 sets next block). Emitted in the snake_case names the core
        // deserializes, only when true.
        val fields = obj(
            Event.PlanHypertrophyMeso(
                muscle = "chest", weeks = 4,
                notGrowing = true, recoveringEasily = true,
            )
        )["PlanHypertrophyMeso"]!!.jsonObject
        assertEquals("chest", fields["muscle"]!!.jsonPrimitive.content)
        assertEquals(4, fields["weeks"]!!.jsonPrimitive.content.toInt())
        assertTrue(fields["not_growing"]!!.jsonPrimitive.content.toBoolean())
        assertTrue(fields["recovering_easily"]!!.jsonPrimitive.content.toBoolean())
    }

    @Test
    fun planHypertrophyMesoOmitsNextMesoFlagsWhenFalse() {
        // A plain plan keeps the identical two-field wire (flags default false
        // → omitted → core serde-defaults them).
        val fields = obj(
            Event.PlanHypertrophyMeso(muscle = "chest", weeks = 4)
        )["PlanHypertrophyMeso"]!!.jsonObject
        assertEquals(setOf("muscle", "weeks"), fields.keys)
    }

    @Test
    fun submitReviewKeepsRequiredFlagsAndTheStampAndOmitsNullOptionals() {
        val fields = obj(
            Event.SubmitReview(overtrainingSignalCount = 2, observedAt = 1_700_000_777L)
        )["SubmitReview"]!!.jsonObject
        assertEquals(2, fields["overtraining_signal_count"]!!.jsonPrimitive.content.toInt())
        assertTrue(fields.containsKey("bone_pain_red_flag"))
        assertTrue(fields.containsKey("bad_day"))
        // The log stamp always rides the wire, so a backdated review keeps its day.
        assertEquals(1_700_000_777L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        // Null optionals are omitted (serde skips them on the Rust side too).
        assertFalse(fields.containsKey("decoupling"))
    }

    @Test
    fun readinessSignalsIncludeTheReferralTiersAndSorenessInSchemaOrder() {
        // The top-of-ladder signals must exist or the safety tier is unreachable.
        val names = ReadinessSignal.entries.map { it.name }
        assertTrue(names.containsAll(listOf("Pain", "Illness", "RedS", "CardiacRedFlag", "BoneStress")))
        // schema.rs orders Soreness between Pain and Illness: the manual picker
        // iterates entries, so wrong ordering would misplace it in the UI.
        assertEquals(names.indexOf("Pain") + 1, names.indexOf("Soreness"))
        assertEquals(names.indexOf("Soreness") + 1, names.indexOf("Illness"))
    }

    @Test
    fun aBareReadinessReportSendsStreakZeroAndNoPainObject() {
        val fields = obj(
            Event.SubmitReadiness(ReadinessSignal.Soreness, 6.0, 1_600_000_000L),
        )["SubmitReadiness"]!!.jsonObject
        assertEquals("Soreness", fields["signal"]!!.jsonPrimitive.content)
        assertEquals(6.0, fields["value"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1_600_000_000L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(0, fields["streak"]!!.jsonPrimitive.content.toInt())
        // serde reads the absent pain object as None.
        assertFalse(fields.containsKey("pain"))
    }

    @Test
    fun submitReadinessCarriesFullPainDetail() {
        // A characterized pain report reaches the core's graded pain
        // gate. The nested PainDetail must match the Rust serde shape exactly
        // (snake_case, enum variant names verbatim, streak alongside).
        val fields = obj(
            Event.SubmitReadiness(
                signal = ReadinessSignal.Pain,
                value = 1.0,
                observedAt = 100L,
                pain = PainDetail(
                    kind = PainKind.TendonLoadRelated,
                    severity = 4,
                    trend = PainTrend.Rising,
                    persists = false,
                    location = "Left knee",
                ),
            )
        )["SubmitReadiness"]!!.jsonObject
        assertEquals(0, fields["streak"]!!.jsonPrimitive.content.toInt())
        val pain = fields["pain"]!!.jsonObject
        assertEquals("TendonLoadRelated", pain["kind"]!!.jsonPrimitive.content)
        assertEquals(4, pain["severity"]!!.jsonPrimitive.content.toInt())
        assertEquals("Rising", pain["trend"]!!.jsonPrimitive.content)
        assertEquals(false, pain["persists"]!!.jsonPrimitive.content.toBoolean())
        assertEquals("Left knee", pain["location"]!!.jsonPrimitive.content)
    }

    @Test
    fun painDetailOmitsAnUnpickedLocationAndAnUnratedSeverity() {
        // Both are optional (serde default None): no body area picked sends no
        // location, and an unrated report sends no number, never an invented 0–10.
        val pain = obj(
            Event.SubmitReadiness(
                ReadinessSignal.Pain, 1.0, 5L,
                pain = PainDetail(kind = PainKind.Other, trend = PainTrend.Stable),
            )
        )["SubmitReadiness"]!!.jsonObject["pain"]!!.jsonObject
        assertEquals("Other", pain["kind"]!!.jsonPrimitive.content)
        assertFalse(pain.containsKey("location"))
        assertFalse(pain.containsKey("severity"))
    }

    @Test
    fun submitReadinessEmitsEffortMinOnlyWhenPresent() {
        // AerobicDecoupling is duration-gated (valid only >20 min), so the editor
        // sends the run length for the core to validate. effort_min is snake_case,
        // matching ReadinessInput's serde field, and OMITTED when not supplied.
        val withDuration = obj(
            Event.SubmitReadiness(
                signal = ReadinessSignal.AerobicDecoupling,
                value = 12.0,
                observedAt = 7L,
                effortMin = 30.0,
            )
        )["SubmitReadiness"]!!.jsonObject
        assertEquals(30.0, withDuration["effort_min"]!!.jsonPrimitive.content.toDouble(), 0.0)
        // A signal without a duration omits the field (serde default None).
        val bare = obj(
            Event.SubmitReadiness(ReadinessSignal.WellnessZ, -1.5, 7L)
        )["SubmitReadiness"]!!.jsonObject
        assertFalse(bare.containsKey("effort_min"))
    }

    @Test
    fun viewModelDecodesCoreSplitVerdict() {
        // runs[i].split decodes, and its absence (hand-entered run) stays null:
        // the shell renders it purely from the wire, no local thresholds.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":2,
                "lifts":[
                  {"exercise":"Squat","weight_kg":100.0,"reps":5,"rpe":8.0,
                   "e1rm_kg":116.7,"pct_1rm":86.0,"rir":2.0,"summary":"","observed_at":1},
                  {"exercise":"Squat","weight_kg":102.5,"reps":5,"rpe":8.0,
                   "e1rm_kg":119.6,"pct_1rm":86.0,"rir":2.0,
                   "e1rm_delta_kg":2.9,"e1rm_direction":"up","summary":"","observed_at":2}],
                "runs":[
                  {"zone":"Z1","pace":"6:00/km","distance_km":8.0,"spike_flag":false,
                   "spike_note":"","split_pct":5.2,
                   "split":{"verdict":"fade","label":"FADE +5%",
                     "message":"Start easier next time.","grade":"Moderate",
                     "citation":"feedback-016","confidence":0.7,
                     "safety_critical":false,"contested":false},
                   "summary":"","citation":"","observed_at":3},
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":false,
                   "spike_note":"","split_pct":null,"split":null,
                   "summary":"","citation":"","observed_at":4}],
                "guidance":[],"feedback":null,"reference":[]}"""
        )
        val split = vm.runs[0].split!!
        assertEquals("fade", split.verdict)
        assertEquals("FADE +5%", split.label)
        assertEquals("Moderate", split.grade)
        assertEquals("feedback-016", split.citation)
        assertEquals(null, vm.runs[1].split)
    }

    @Test
    fun viewWithClampedIntegersDecodes() {
        // Ingest clamps reps to 10 000 and entry_id to i64::MAX; the resulting view
        // JSON must decode inside Kotlin's Int/Long range (no wire overflow).
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,
                "lifts":[
                  {"exercise":"Leg Press","weight_kg":200.0,"reps":10000,"rpe":8.0,
                   "e1rm_kg":0.0,"pct_1rm":0.0,"rir":2.0,"summary":"",
                   "observed_at":1,"entry_id":9223372036854775807}],
                "runs":[],"guidance":[],"feedback":null,"reference":[]}"""
        )
        assertEquals(10000, vm.lifts[0].reps)
        assertEquals(9223372036854775807L, vm.lifts[0].entry_id)
    }

    @Test
    fun removeReadinessCarriesSignalAndObservedAt() {
        // The removal time dates the retraction: the core keeps the input for
        // days before it and drops it from that day on.
        val fields = obj(
            Event.RemoveReadiness(ReadinessSignal.Pain, observedAt = 1_700_000_888L)
        )["RemoveReadiness"]!!.jsonObject
        assertEquals("Pain", fields["signal"]!!.jsonPrimitive.content)
        assertEquals(1_700_000_888L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun viewModelDecodesReadinessSummaryHeadlineAndSignalGroups() {
        // Rust→Kotlin contract for the KB-honest readiness rework: per-signal
        // states (+ their judging rule's evidence), the core-owned today
        // headline, and the static signal→group fence metadata all decode.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[],
                "readiness_summary":[
                  {"signal":"HrvLnRmssd","group":"metric","value":-1.0,"streak":0,
                   "state":"suppressed","grade":"Moderate","citation":"Kiviniemi 2007",
                   "confidence":0.65,"safety_critical":false,"contested":false},
                  {"signal":"Pain","group":"red_flag","value":1.0,"streak":0,
                   "state":"red flag - stop","grade":"Strong","citation":"safety",
                   "confidence":0.9,"safety_critical":true,"contested":false}],
                "today_headline":{"kind":"safety_hold","summary":"Stop - do not train",
                  "grade":"Strong","citation":"safety","confidence":0.9,
                  "safety_critical":true,"contested":false},
                "signal_groups":[
                  {"signal":"Rpe","group":"metric"},
                  {"signal":"Pain","group":"red_flag"}]}"""
        )
        assertEquals("suppressed", vm.readiness_summary[0].state)
        assertEquals("metric", vm.readiness_summary[0].group)
        assertTrue(vm.readiness_summary[1].safety_critical)
        assertEquals("safety_hold", vm.today_headline!!.kind)
        assertEquals("red_flag", vm.signal_groups.first { it.signal == "Pain" }.group)
    }

    @Test
    fun viewModelDecodesWhyDisclosureAndGradeDefinitions() {
        // The three-part why? block on action cards and the
        // core-provided evidence-grade legend decode from the wire.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,
                "adjustments":[
                  {"summary":"Downgrade to an easier session","grade":"Moderate",
                   "citation":"Plews 2013","confidence":0.65,"safety_critical":false,
                   "contested":false,
                   "why":{"basis":"Your 7-day HRV average is below your normal band.",
                     "grade_note":"Moderate evidence - mixed or limited randomized trials.",
                     "improves":"Keep logging morning check-ins to tighten the baseline."}}],
                "review_adjustments":[],"input_count":1,"lifts":[],"runs":[],
                "guidance":[
                  {"section":"Heart-rate zones","summary":"Estimated HRmax: 187 bpm",
                   "grade":"Weak","citation":"Tanaka","confidence":0.4,
                   "safety_critical":false,"contested":false,
                   "why":{"basis":"Estimated from your age (30) with the Tanaka formula.",
                     "grade_note":"Weak evidence - from mechanism or observation.",
                     "improves":"Log a measured max HR from an all-out effort to replace the age-based estimate."}}],
                "feedback":null,"reference":[],
                "today_headline":{"kind":"adjustment","summary":"Downgrade to an easier session",
                  "grade":"Moderate","citation":"Plews 2013","confidence":0.65,
                  "safety_critical":false,"contested":false,
                  "why":{"basis":"HRV below band.","grade_note":"Moderate evidence.","improves":"More check-ins."}},
                "grade_definitions":[
                  {"grade":"Strong","label":"Strong",
                   "definition":"Well-replicated meta-analyses or randomized controlled trials.","confidence":0.9},
                  {"grade":"Weak","label":"Weak",
                   "definition":"Mechanistic or observational evidence only.","confidence":0.4}]}"""
        )
        // why? triad decodes on adjustment, guidance, and headline.
        assertEquals("Your 7-day HRV average is below your normal band.", vm.adjustments[0].why.basis)
        assertTrue(vm.adjustments[0].why.improves.contains("check-ins"))
        assertTrue(vm.guidance[0].why.basis.contains("Tanaka"))
        assertTrue(vm.guidance[0].why.improves.contains("measured max HR"))
        assertEquals("adjustment", vm.today_headline!!.kind)
        assertTrue(vm.today_headline.why.grade_note.contains("Moderate"))
        // Grade legend from core data.
        assertEquals(2, vm.grade_definitions.size)
        val strong = vm.grade_definitions.first { it.grade == "Strong" }
        assertTrue(strong.definition.contains("meta-analyses"))
        assertEquals(0.9f, strong.confidence, 1e-6f)
    }

    @Test
    fun viewModelDecodesTrainingLoadWeeklyReportLiftAuditCrossCheckAndDisclosures() {
        // The core's training_load (session counts, method), weekly_report
        // (overuse rows), lift_audit, trend, provisional + autoreg_source
        // (feedback-040/047/048 honesty disclosures) and LiftResultView.cross_check
        // (strength-006 multi-formula e1RM range) decode.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":3,
                "lifts":[
                  {"exercise":"Back Squat","weight_kg":140.0,"reps":3,"rpe":8.0,
                   "e1rm_kg":152.6,"pct_1rm":92.0,"rir":2.0,
                   "cross_check":{"low_kg":150.2,"high_kg":155.0,"formulas":3,
                     "grade":"Moderate","citation":"strength-006","confidence":0.6,
                     "safety_critical":false,"contested":false},
                   "summary":"Back Squat","observed_at":1700000000,"entry_id":7}],
                "runs":[],"guidance":[],"feedback":null,"reference":[],
                "training_load":{"ctl":45.2,"atl":52.7,"tsb":-7.5,"days":30,
                  "sessions_counted":12,"sessions_skipped":2,
                  "method":"Lucia TRIMP (3-zone HR)",
                  "summary":"Fitness (CTL) 45.2 · Fatigue (ATL) 52.7 · Form (TSB) -7.5",
                  "grade":"Moderate","citation":"LOAD-PMC-001","confidence":0.6,
                  "safety_critical":false,"contested":false},
                "weekly_report":[
                  {"section":"Running","summary":"Volume up 18% on last week",
                   "grade":"Moderate","citation":"daniels","confidence":0.6,
                   "safety_critical":true,"contested":false}],
                "lift_audit":[
                  {"section":"Strength","summary":"Prilepin: 3x3 at 92% is on target",
                   "grade":"Moderate","citation":"prilepin","confidence":0.55,
                   "safety_critical":false,"contested":false}],
                "trend":{"summary":"Squat e1RM trending up over 4 weeks",
                  "grade":"Weak","citation":"feedback-027","confidence":0.4,
                  "safety_critical":false,"contested":false},
                "provisional":{"summary":"Still provisional: fewer than 14 logged days",
                  "grade":"Moderate","citation":"feedback-040","confidence":0.6,
                  "safety_critical":false,"contested":false},
                "autoreg_source":{"summary":"Using subjective + performance signals",
                  "grade":"Moderate","citation":"autoreg-047","confidence":0.6,
                  "safety_critical":false,"contested":false}}"""
        )
        assertEquals(12, vm.training_load!!.sessions_counted)
        assertTrue(vm.training_load.method.contains("TRIMP"))
        assertEquals(1, vm.weekly_report.size)
        assertTrue(vm.weekly_report[0].safety_critical)
        assertEquals(1, vm.lift_audit.size)
        assertTrue(vm.lift_audit[0].summary.contains("Prilepin"))
        assertEquals("feedback-027", vm.trend!!.citation)
        assertTrue(vm.provisional!!.summary.contains("provisional"))
        assertEquals("autoreg-047", vm.autoreg_source!!.citation)
        // cross_check rides on the lift row itself, not the top-level model.
        val cc = vm.lifts[0].cross_check!!
        assertEquals(150.2, cc.low_kg, 1e-9)
        assertEquals(155.0, cc.high_kg, 1e-9)
        assertEquals(3, cc.formulas)
    }

    @Test
    fun submitCheckinEmitsAnsweredItemsAndOmitsUnansweredOnes() {
        // Answered items ride the wire as snake_case. An unanswered optional item
        // (watch numbers, the KB short-form's fatigue and stress) is OMITTED so the
        // core reads None and never fabricates a channel the user didn't provide.
        val full = obj(
            Event.SubmitCheckin(
                observedAt = 1_700_000_000L, sleepQuality = 2, soreness = 4, mood = 3,
                restingHrBpm = 52.0, fatigue = 5, stress = 1,
            )
        )["SubmitCheckin"]!!.jsonObject
        assertEquals(1_700_000_000L, full["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(2, full["sleep_quality"]!!.jsonPrimitive.content.toInt())
        assertEquals(4, full["soreness"]!!.jsonPrimitive.content.toInt())
        assertEquals(3, full["mood"]!!.jsonPrimitive.content.toInt())
        assertEquals(52.0, full["resting_hr_bpm"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(5, full["fatigue"]!!.jsonPrimitive.content.toInt())
        assertEquals(1, full["stress"]!!.jsonPrimitive.content.toInt())
        assertFalse(full.containsKey("hrv_rmssd_ms"))
        val bare = obj(
            Event.SubmitCheckin(observedAt = 1_700_000_000L, sleepQuality = 2, soreness = 4, mood = 3)
        )["SubmitCheckin"]!!.jsonObject
        assertFalse(bare.containsKey("fatigue"))
        assertFalse(bare.containsKey("stress"))
    }

    @Test
    fun viewModelDecodesCheckinEchoAndBaselineStatus() {
        // The echoed check-in (rehydration) and the "collecting baseline" rows decode.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":0,"lifts":[],"runs":[],
                "guidance":[],"feedback":null,"reference":[],
                "latest_checkin":{"observed_at":1700000000,"sleep_quality":2,
                  "soreness":4,"mood":3,"resting_hr_bpm":52.0,"hrv_rmssd_ms":null},
                "baseline_status":[
                  {"signal":"WellnessZ","label":"Sleep, soreness & mood","have":1,"need":7}]}"""
        )
        assertEquals(2, vm.latest_checkin!!.sleep_quality)
        assertEquals(4, vm.latest_checkin.soreness)
        assertEquals(null, vm.latest_checkin.hrv_rmssd_ms)
        assertEquals("WellnessZ", vm.baseline_status[0].signal)
        assertEquals(1, vm.baseline_status[0].have)
    }

    @Test
    fun logRunTrackNestsPointsWithSnakeCaseFields() {
        val fields = obj(
            Event.LogRunTrack(listOf(GpsPoint(52.5, 13.4, 1000L, 4.5)), 80.0, 1_700_000_500L)
        )["LogRunTrack"]!!.jsonObject
        val pt = fields["points"]!!.let { (it as kotlinx.serialization.json.JsonArray)[0] }.jsonObject
        assertEquals(52.5, pt["lat"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(1000L, pt["observed_at"]!!.jsonPrimitive.content.toLong())
        assertEquals(4.5, pt["accuracy_m"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(80.0, fields["hr_pct_max"]!!.jsonPrimitive.content.toDouble(), 1e-9)
        // Session log time is the run-level stamp, distinct from the per-fix one.
        assertEquals(1_700_000_500L, fields["observed_at"]!!.jsonPrimitive.content.toLong())
        // An un-paused save carries NO segment_starts key → matches serde's
        // `#[serde(default)]` empty Vec, so old logs / this common case ride
        // the wire unchanged (back-compat).
        assertFalse("un-paused run omits segment_starts", fields.containsKey("segment_starts"))
    }

    @Test
    fun logRunTrackEmitsSegmentStartsArrayWhenPaused() {
        // A pause + relocation carries the boundary indices (into `points`) so the
        // segment-aware core skips each pause-bridge leg and breaks the GPX <trkseg>.
        val fields = obj(
            Event.LogRunTrack(
                points = listOf(
                    GpsPoint(0.0, 0.000, 0L, 5.0),
                    GpsPoint(0.0, 0.001, 10L, 5.0),
                    GpsPoint(0.0, 1.000, 70L, 5.0),
                    GpsPoint(0.0, 1.001, 80L, 5.0),
                ),
                hrPctMax = 0.0,
                observedAt = 0L,
                segmentStarts = listOf(2),
            )
        )["LogRunTrack"]!!.jsonObject
        val starts = fields["segment_starts"]!! as kotlinx.serialization.json.JsonArray
        assertEquals(1, starts.size)
        assertEquals(2, starts[0].jsonPrimitive.content.toInt())
    }

    @Test
    fun trimRunTrackEmitsSnakeCaseBoundsWireShape() {
        val fields = obj(
            Event.TrimRunTrack(entryId = 42L, observedAtFallback = 1700L, keepFromSec = 1720L, keepUntilSec = 1900L)
        )["TrimRunTrack"]!!.jsonObject
        assertEquals(42L, fields["entry_id"]!!.jsonPrimitive.content.toLong())
        assertEquals(1700L, fields["observed_at_fallback"]!!.jsonPrimitive.content.toLong())
        assertEquals(1720L, fields["keep_from_sec"]!!.jsonPrimitive.content.toLong())
        assertEquals(1900L, fields["keep_until_sec"]!!.jsonPrimitive.content.toLong())
        // Exactly the four keys the Rust TrimRunTrack variant expects — a rename
        // or an extra key would be a silent FFI drop of the trim on replay.
        assertEquals(
            setOf("entry_id", "observed_at_fallback", "keep_from_sec", "keep_until_sec"),
            fields.keys,
        )
    }

    // ── user-declared workout-type tag (USER DATA, no evidence) ─────────

    @Test
    fun workoutTypeIsOmittedWhenUntaggedAndEmittedAsBareVariantWhenSet() {
        // Untagged (null) → the key is ABSENT on the wire, so the event matches
        // serde's `#[serde(default)]` shape and old logs/replay are unaffected.
        val untagged = obj(
            Event.LogRun(8.0, 48.0, 70.0, 1_700_000_000L)
        )["LogRun"]!!.jsonObject
        assertFalse(untagged.containsKey("workout_type"))

        // Tagged → the field carries the EXACT serde variant string (the Rust
        // `WorkoutType` deserializes external-form, so it must be the bare name).
        val tagged = obj(
            Event.LogRun(8.0, 48.0, 70.0, 1_700_000_000L, workoutType = WorkoutType.Interval)
        )["LogRun"]!!.jsonObject
        assertEquals("Interval", tagged["workout_type"]!!.jsonPrimitive.content)

        // Same contract on the GPS + amend events.
        val track = obj(
            Event.LogRunTrack(listOf(GpsPoint(0.0, 0.0, 0L, 5.0)), 0.0, 0L, workoutType = WorkoutType.LongRun)
        )["LogRunTrack"]!!.jsonObject
        assertEquals("LongRun", track["workout_type"]!!.jsonPrimitive.content)
        val amend = obj(
            Event.AmendRun(9L, 5.0, 30.0, 0.0, 1_700_000_000L, workoutType = WorkoutType.Recovery)
        )["AmendRun"]!!.jsonObject
        assertEquals("Recovery", amend["workout_type"]!!.jsonPrimitive.content)
    }

    @Test
    fun runResultViewMapsItsWorkoutTypeLabel() {
        val json = coreJson
        // A tagged run row echoes the label, and fromWire maps it back to the enum.
        val tagged = json.decodeFromString<RunResultView>(
            """{"zone":"Z2","pace":"5:00/km","distance_km":10.0,"summary":"","citation":"",
                "observed_at":0,"workout_type":"Tempo"}"""
        )
        assertEquals("Tempo", tagged.workout_type)
        assertEquals(WorkoutType.Tempo, WorkoutType.fromWire(tagged.workout_type))

        // Decode-safe: an unknown/future variant string still decodes (raw String)
        // and maps to null rather than throwing: the run just renders untagged.
        val future = json.decodeFromString<RunResultView>(
            """{"zone":"Z2","pace":"5:00/km","distance_km":10.0,"summary":"","citation":"",
                "observed_at":0,"workout_type":"Fartlek"}"""
        )
        assertEquals("Fartlek", future.workout_type)
        assertEquals(null, WorkoutType.fromWire(future.workout_type))
    }

    @Test
    fun amendRunMetaUsesSnakeCaseWireNames() {
        val f = obj(Event.AmendRunMeta(42L, 1_700_000_000L, 7, WorkoutType.LongRun))["AmendRunMeta"]!!.jsonObject
        assertEquals(42L, f["entry_id"]!!.jsonPrimitive.content.toLong())
        assertEquals(1_700_000_000L, f["observed_at_fallback"]!!.jsonPrimitive.content.toLong())
        assertEquals(7, f["session_rpe"]!!.jsonPrimitive.content.toInt())
        assertEquals("LongRun", f["workout_type"]!!.jsonPrimitive.content)
    }

    @Test
    fun amendRunMetaSendsAClearedRatingOrTagAsAnExplicitNull() {
        // The core REPLACES both fields, so a cleared one travels as an explicit
        // JSON null, never dropped.
        val f = obj(Event.AmendRunMeta(42L, 0L, null, null))["AmendRunMeta"]!!.jsonObject
        assertEquals(JsonNull, f["session_rpe"])
        assertEquals(JsonNull, f["workout_type"])
    }

    // ── Coach-as-planner events ─────────────────────────────────────────

    @Test
    fun generatePlanHasSnakeCaseStartDay() {
        val fields = obj(Event.GeneratePlan(20_300L))["GeneratePlan"]!!.jsonObject
        assertEquals(20_300L, fields["start_epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun setTodayHasSnakeCaseEpochDay() {
        val fields = obj(Event.SetToday(20_301L))["SetToday"]!!.jsonObject
        assertEquals(20_301L, fields["epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun todayCoachingEventsCarryTheirDaysAndSignal() {
        val move = obj(Event.MoveSession(20_360L, 20_366L))["MoveSession"]!!.jsonObject
        assertEquals(20_360L, move["from_epoch_day"]!!.jsonPrimitive.content.toLong())
        assertEquals(20_366L, move["to_epoch_day"]!!.jsonPrimitive.content.toLong())
        val hold = obj(Event.ResolveHold(ReadinessSignal.Illness, 1_759_700_000L))["ResolveHold"]!!.jsonObject
        assertEquals("Illness", hold["signal"]!!.jsonPrimitive.content)
        assertEquals(1_759_700_000L, hold["observed_at"]!!.jsonPrimitive.content.toLong())
        val restart = obj(Event.RestartHrvBaseline(1_759_700_000L))["RestartHrvBaseline"]!!.jsonObject
        assertEquals(1_759_700_000L, restart["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    // ── Structured HRmax / protein / spike-baseline fields ──────────────

    @Test
    fun viewModelDecodesHrMaxProteinFiguresAndSpikeBaseline() {
        // The structured HRmax, protein and spike-baseline fields decode with the
        // core's serde names.
        val json = coreJson
        val vm = json.decodeFromString<ViewModel>(
            """{"safety_tier":null,"train_blocked":false,"adjustments":[],
                "review_adjustments":[],"input_count":1,
                "runs":[
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":true,
                   "spike_note":"","summary":"","citation":"",
                   "observed_at":4,"spike_has_baseline":true}],
                "guidance":[],"feedback":null,"reference":[],
                "protein_figures":[
                  {"kind":"masters","low_g_per_day":120.0,"high_g_per_day":140.0,"refused":false},
                  {"kind":"deficit","low_g_per_day":0.0,"high_g_per_day":0.0,"refused":true}],
                "hr_max":{"bpm":187.0,"measured":false,"age_years":30.0,
                   "tanaka_intercept":208.0,"tanaka_slope":0.7}}"""
        )
        val hm = vm.hr_max!!
        assertEquals(187, hm.bpm.toInt())
        assertFalse(hm.measured)
        assertEquals(30, hm.age_years.toInt())
        assertEquals(208.0, hm.tanaka_intercept, 1e-9)
        assertEquals(0.7, hm.tanaka_slope, 1e-9)
        // The tile reads the first non-refused figure as "120–140".
        val figure = vm.protein_figures.first { !it.refused }
        assertEquals("masters", figure.kind)
        assertEquals(120, figure.low_g_per_day.toInt())
        assertEquals(140, figure.high_g_per_day.toInt())
        assertTrue(vm.protein_figures.last().refused)
        assertTrue(vm.runs[0].spike_has_baseline)
    }

    @Test
    fun logActivityMatchesTheCoreWireShape() {
        val e = Event.LogActivity(
            ActivityKind.Sport, 45.0, vigorous = true, strengthening = false,
            observedAt = 1_760_000_000L, entryId = 123L,
        )
        assertEquals(
            """{"LogActivity":{"kind":"Sport","minutes":45.0,"vigorous":true,"strengthening":false,"observed_at":1760000000,"entry_id":123}}""",
            e.toJson().toString(),
        )
    }

    @Test
    fun deleteEntryTargetsAnActivity() {
        assertEquals(
            """{"DeleteEntry":{"kind":"Activity","entry_id":123,"observed_at_fallback":1760000000}}""",
            Event.DeleteEntry(Event.EntryKind.Activity, 123L, observedAtFallback = 1_760_000_000L).toJson().toString(),
        )
    }

    @Test
    fun youthActivityViewDecodesWithItsEvidenceFlattened() {
        val vm = coreJson.decodeFromString(
            ViewModel.serializer(),
            """{"activities":[{"entry_id":7,"observed_at":1760000000,"kind":"Cycling","minutes":30.0,"vigorous":false,"strengthening":true}],
              "youth_activity":{"week_monday_epoch_day":20376,"minutes":95,"run_minutes":30,"lift_minutes":35,"activity_minutes":30,
              "daily_target_minutes":60,"weekly_target_minutes":420,"vigorous_days":1,"strengthening_days":2,"target_days":3,
              "week_entries":[],"grade":"Strong","citation":"Bull et al. 2020","confidence":0.8,"safety_critical":false,
              "contested":false,"why":{"basis":"WHO 2020","grade_note":"","improves":""}}}""",
        )
        assertEquals(ActivityKind.Cycling, vm.activities.single().kind)
        assertTrue(vm.activities.single().strengthening)
        val y = vm.youth_activity!!
        assertEquals(95, y.minutes)
        assertEquals(420, y.weekly_target_minutes)
        assertEquals(3, y.target_days)
        assertEquals("Strong", y.grade)
        assertEquals("WHO 2020", y.why.basis)
    }

    @Test
    fun aViewFromAnOlderCoreDecodesToDefaults() {
        // A view or row without the newer fields (an older core, a stale replay)
        // decodes with null / empty / false in their place, never a crash.
        val vm = coreJson.decodeFromString(
            ViewModel.serializer(),
            """{"adjustments":[
                  {"summary":"Take a rest day","grade":"Strong","citation":"x",
                   "confidence":0.9,"safety_critical":true,"contested":false}],
                "lifts":[
                  {"exercise":"Bench","weight_kg":80.0,"reps":5,"rpe":8.0,
                   "e1rm_kg":93.3,"pct_1rm":85.0,"rir":2.0,"summary":"Bench","observed_at":0}],
                "runs":[
                  {"zone":"Z2","pace":"5:30/km","distance_km":5.0,"spike_flag":true,
                   "spike_note":"","summary":"","citation":"","observed_at":4}]}""",
        )
        assertEquals(null, vm.today_headline)
        assertTrue(vm.readiness_summary.isEmpty())
        assertTrue(vm.signal_groups.isEmpty())
        assertTrue(vm.grade_definitions.isEmpty())
        assertEquals(null, vm.training_load)
        assertTrue(vm.weekly_report.isEmpty())
        assertTrue(vm.lift_audit.isEmpty())
        assertEquals(null, vm.trend)
        assertEquals(null, vm.provisional)
        assertEquals(null, vm.autoreg_source)
        assertEquals(null, vm.latest_checkin)
        assertTrue(vm.baseline_status.isEmpty())
        assertEquals(null, vm.hr_max)
        assertTrue(vm.protein_figures.isEmpty())
        assertTrue(vm.activities.isEmpty())
        assertEquals(null, vm.youth_activity)
        assertEquals("", vm.adjustments.single().why.basis)
        assertEquals("", vm.adjustments.single().why.grade_note)
        assertEquals(null, vm.lifts.single().cross_check)
        val run = vm.runs.single()
        assertTrue(run.spike_flag)
        assertFalse(run.spike_has_baseline)
        assertEquals(null, run.workout_type)
        assertEquals(null, WorkoutType.fromWire(run.workout_type))
    }

    @Test
    fun todayViewFieldsDecode() {
        val vm = coreJson.decodeFromString(
            ViewModel.serializer(),
            """
            {"session_done":{"epoch_day":20352,"title":"Heavy day","items":[{"exercise":"Back Squat","summary":"3 of 3 sets","met":true}]},
             "hold_resolutions":[{"signal":"Pain","paused_text":"Paused.","action_label":"I'm pain-free now","confirm_text":"Sure?"}],
             "consistency":{"this_week_done":2,"this_week_planned":3,"weeks":[{"week_monday_epoch_day":20345,"done":3,"planned":3}],"share_pct":100,"repeating_week":false,"notes":[]},
             "next_session":{"epoch_day":20352,"title":"Heavy day","session_type":"Lift(MaxEffort)","status":"next","items":[],
               "move_targets":[20358],"moved_from_epoch_day":20350,"move_note":null},
             "program":{"name":"x","goal":"y","phase":"Build","week":1,"weeks_total":4,"grade":"Strong","citation":"c","confidence":0.9,
               "safety_critical":false,"contested":false,
               "muscle_volume":[{"muscle":"Side delts","target_sets":12,"delivered_sets":12.0,"label":"Side delts: 12 sets · target 12"}],
               "muscle_volume_note":"note"}}
            """.trimIndent(),
        )
        assertEquals("Heavy day", vm.session_done?.title)
        assertTrue(vm.session_done!!.items.single().met)
        assertEquals("I'm pain-free now", vm.hold_resolutions.single().action_label)
        assertEquals(2, vm.consistency?.this_week_done)
        assertEquals(listOf(20_358L), vm.next_session?.move_targets)
        assertEquals(20_350L, vm.next_session?.moved_from_epoch_day)
        assertEquals("Side delts", vm.program?.muscle_volume?.single()?.muscle)
    }

    @Test
    fun structuredFaceFieldsDecode() {
        val vm = coreJson.decodeFromString(
            ViewModel.serializer(),
            """
            {"next_session":{"epoch_day":20352,"title":"Heavy day","session_type":"Lift(MaxEffort)","status":"next","items":[
               {"exercise":"Back Squat","summary":"Back Squat: 5×3 @ 100 kg · ~2 reps left","face_name":"Back Squat",
                "face_detail":"5×3 @ 100 kg · ~2 reps left","face_lead":"5×3 @ 100 kg",
                "anchored_on":"Last: 95 kg × 3, 3, 3","anchored_on_last":true}]},
             "calisthenics_ladders":[{"ladder":"Core","pattern":"Core","rungs":[{"rung":0,"name":"Plank","target":"20–45 s hold","available":true,"timed_hold":true}]}],
             "guidance":[{"section":"Profile","summary":"Training age: Novice","training_age":"Novice"}]}
            """.trimIndent(),
        )
        val item = vm.next_session!!.items.single()
        assertEquals("Back Squat" to "5×3 @ 100 kg · ~2 reps left", item.face_name to item.face_detail)
        assertEquals("5×3 @ 100 kg", item.face_lead)
        assertTrue(item.anchored_on_last)
        assertTrue(vm.calisthenics_ladders.single().rungs.single().timed_hold)
        assertEquals("Novice", vm.guidance.single().training_age)
    }
}
