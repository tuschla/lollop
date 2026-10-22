package app.milestone

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire contracts for the Today coaching events and view fields: the
 * MoveSession / ResolveHold / RestartHrvBaseline encoders match the Rust serde
 * shapes, the Today view fields decode from the core's JSON, and the plan-face
 * formatting helpers stay pure.
 */
class TodayEventJsonTest {

    private fun fields(e: Event, variant: String) = (e.toJson() as JsonObject)[variant]!!.jsonObject

    @Test
    fun moveSessionCarriesBothDays() {
        val f = fields(Event.MoveSession(20_360L, 20_366L), "MoveSession")
        assertEquals(20_360L, f["from_epoch_day"]!!.jsonPrimitive.content.toLong())
        assertEquals(20_366L, f["to_epoch_day"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun resolveHoldIsDatedWithTheSignalName() {
        val f = fields(Event.ResolveHold(ReadinessSignal.Illness, 1_759_700_000L), "ResolveHold")
        assertEquals("Illness", f["signal"]!!.jsonPrimitive.content)
        assertEquals(1_759_700_000L, f["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun restartHrvBaselineIsDated() {
        val f = fields(Event.RestartHrvBaseline(1_759_700_000L), "RestartHrvBaseline")
        assertEquals(1_759_700_000L, f["observed_at"]!!.jsonPrimitive.content.toLong())
    }

    @Test
    fun newTodayViewFieldsDecode() {
        val json = coreJson
        val vm = json.decodeFromString(
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
    fun plateFigureHelper() {
        assertEquals("20", plateFigure(20.0))
        assertEquals("2.5", plateFigure(2.5))
        assertEquals("1.25", plateFigure(1.25))
    }

    /** A run face leads with the core's volume head only when the item carries a
     *  run target, whose pace / heart-rate / effort lines then render under it. */
    @Test
    fun runFaceHeadIsTheCoreVolumeHeadUnderATarget() {
        val item = coreJson.decodeFromString(
            PrescriptionView.serializer(),
            """
            {"summary":"6 × 800 m · 3:40–3:50 /km (interval) · effort 8 of 10","run_head":"6 × 800 m","face_lead":"6 × 800 m",
             "run_target":{"band":"interval","pace_label_km":"3:40–3:50 /km","effort_cue":"effort 8 of 10","effort_detail":"8 of 10"}}
            """.trimIndent(),
        )
        assertEquals("6 × 800 m", runFaceHead(item, DistanceUnit.Km))
        // A rep head has no plain distance: it reads the same in miles.
        assertEquals("6 × 800 m", runFaceHead(item, DistanceUnit.Mi))
        assertEquals("8 of 10", item.run_target?.effort_detail)
        // No target, or no volume head (a pain stop): the full summary stays.
        assertEquals(null, runFaceHead(item.copy(run_target = null), DistanceUnit.Km))
        assertEquals(null, runFaceHead(item.copy(run_head = ""), DistanceUnit.Km))
    }

    /** A distance head and distance prose read in the display unit from the core's
     *  structured km figure and its miles twins, never by rewriting the km text. */
    @Test
    fun distancesFollowTheDisplayUnit() {
        val item = PrescriptionView(
            run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(band = "easy"),
            safety_reason = "Capped at 8 km: at most 10% over your longest recent run",
            safety_reason_mi = "Capped at 5 mi: at most 10% over your longest recent run",
        )
        assertEquals("8 km", runFaceHead(item, DistanceUnit.Km))
        assertEquals("5 mi", runFaceHead(item, DistanceUnit.Mi))
        assertEquals(item.safety_reason, safetyReasonText(item, DistanceUnit.Km))
        assertEquals(item.safety_reason_mi, safetyReasonText(item, DistanceUnit.Mi))
        // A reason stating no distance has no twin: the km text stands.
        val kept = item.copy(safety_reason = "Kept short and easy", safety_reason_mi = "")
        assertEquals("Kept short and easy", safetyReasonText(kept, DistanceUnit.Mi))
        assertEquals(null, safetyReasonText(PrescriptionView(), DistanceUnit.Mi))
        val fb = FeedbackView(message = "Run done: 10.2 km in 47 min, as planned.", message_mi = "Run done: 6.3 mi in 47 min, as planned.")
        assertEquals(fb.message, feedbackMessage(fb, DistanceUnit.Km))
        assertEquals(fb.message_mi, feedbackMessage(fb, DistanceUnit.Mi))
        assertEquals("Even split.", feedbackMessage(FeedbackView(message = "Even split."), DistanceUnit.Mi))
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
