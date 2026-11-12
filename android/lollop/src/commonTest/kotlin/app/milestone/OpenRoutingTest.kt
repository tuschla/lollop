package app.milestone

import kotlin.random.Random
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Where an open request lands, on both platforms: the `log_set` target, and
 * routing that is total (every destination from every scaffold navigation state on
 * every kind of day lands somewhere sensible). The Android intent shapes and the
 * real app's landings are DeepLinkTest / DeepLinkPropertyTest.
 */
class OpenRoutingTest {
    private val day = 20_400L

    private fun lift(name: String) = PrescriptionView(exercise = name, sets = 3, face_lead = "3 × 5")

    private fun runItem() =
        PrescriptionView(run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(pace_label = "5:40–6:00 /km"))

    private fun liftSession(vararg items: PrescriptionView, status: SessionStatus = SessionStatus.Next, epochDay: Long = day) =
        SessionPlanView(epoch_day = epochDay, title = "Lower", session_type = SessionType.LiftMaxEffort, status = status, items = items.toList())

    @Test
    fun logSetTargetsTodaysOpenLiftItemOnly() {
        val squat = PrescriptionView(exercise = "Back Squat", sets = 3)
        val bench = PrescriptionView(exercise = "Bench Press", sets = 3)
        val liftDay = ViewModel(today_epoch_day = day, next_session = liftSession(squat, bench), week_plan = listOf(liftSession(squat, bench)))
        assertEquals("Back Squat", logSetTarget(liftDay))
        val squatDone = squat.copy(done = true, sets_done = 3)
        assertEquals("Bench Press", logSetTarget(liftDay.copy(next_session = liftSession(squatDone, bench))))
        // After Finish the core moves next_session off today: the started session resumes.
        val finished = liftDay.copy(next_session = liftSession(squat, epochDay = day + 2), week_plan = listOf(liftSession(squatDone, bench)))
        assertEquals("Bench Press", logSetTarget(finished))
        assertNull(logSetTarget(liftDay.copy(next_session = liftSession(squat, epochDay = day + 1), week_plan = emptyList())))
        val run = SessionPlanView(epoch_day = day, session_type = SessionType.RunEasy, status = SessionStatus.Next, items = listOf(PrescriptionView()))
        assertNull(logSetTarget(ViewModel(today_epoch_day = day, next_session = run, week_plan = listOf(run))))
        assertNull(logSetTarget(ViewModel(today_epoch_day = day)))
        assertNull(logSetTarget(liftDay.copy(hold_reason = HoldReason.Pain)))
    }

    /** A model of one day of [kind] (0..10) as the core publishes it, and its name. */
    private fun dayModel(kind: Int, r: Random): Pair<String, ViewModel> {
        val names = listOf("Back squat", "Bench press", "Row", "Deadlift").shuffled(r)
        val items = List(1 + r.nextInt(4)) { i -> lift(names[i]) }
        val profile = Profile()
        fun session(d: Long, type: SessionType, status: SessionStatus, its: List<PrescriptionView>) =
            SessionPlanView(epoch_day = d, title = "S$d", session_type = type, status = status, items = its)
        return when (kind) {
            0 -> "lift open" to session(day, SessionType.LiftMaxEffort, SessionStatus.Next, items).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            1 -> {
                val k = r.nextInt(1, items.size + 1)
                val started = items.mapIndexed { i, it -> if (i < k) it.copy(done = i < k - 1 || r.nextBoolean(), sets_done = 1) else it }
                "lift started" to session(day, SessionType.LiftMaxEffort, SessionStatus.Adjusted, started).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            }
            2 -> {
                // Finished: the core moves next_session on; today's row says what was done.
                val done = items.map { it.copy(done = true, sets_done = 3) }
                val next = session(day + 2, SessionType.RunEasy, SessionStatus.Next, listOf(runItem()))
                "lift done" to ViewModel(today_epoch_day = day, profile = profile, next_session = next, week_plan = listOf(session(day, SessionType.LiftMaxEffort, SessionStatus.Done, done), next))
            }
            3 -> "run open" to session(day, SessionType.RunEasy, SessionStatus.Next, listOf(runItem())).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            4 -> {
                val next = session(day + 1, SessionType.LiftMaxEffort, SessionStatus.Next, items)
                "rest" to ViewModel(today_epoch_day = day, profile = profile, next_session = next, week_plan = listOf(session(day, SessionType.Rest, SessionStatus.Rest, emptyList()), next))
            }
            5 -> {
                // Today's lift moved to a later day: today is a rest day.
                val moved = session(day + 3, SessionType.LiftMaxEffort, SessionStatus.Next, items).copy(moved_from_epoch_day = day)
                "moved" to ViewModel(today_epoch_day = day, profile = profile, next_session = moved, week_plan = listOf(session(day, SessionType.Rest, SessionStatus.Rest, emptyList()), moved))
            }
            6 -> {
                val blank = session(day, SessionType.LiftMaxEffort, SessionStatus.Blocked, emptyList())
                "hold" to ViewModel(today_epoch_day = day, profile = profile, hold_reason = HoldReason.Pain, next_session = blank, week_plan = listOf(blank))
            }
            7 -> {
                val blank = session(day, SessionType.LiftMaxEffort, SessionStatus.Blocked, emptyList())
                "under 18" to ViewModel(today_epoch_day = day, profile = profile, hold_reason = HoldReason.Youth, next_session = blank, week_plan = listOf(blank))
            }
            8 -> "no plan" to ViewModel(today_epoch_day = day, profile = profile, plan_withheld = listOf(null, PlanWithheld.NothingToPlan).random(r))
            9 -> "no profile" to ViewModel(today_epoch_day = day)
            else -> "missed yesterday, lift today" to session(day, SessionType.LiftMaxEffort, SessionStatus.Next, items).let {
                ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(session(day - 1, SessionType.RunEasy, SessionStatus.Missed, listOf(runItem())), it))
            }
        }
    }

    /** The lift items of today's session the set editor may prefill from: the hero
     *  when it is today's actionable lift, else today's week row with logged work. */
    private fun todaysLiftItems(m: ViewModel): List<PrescriptionView> {
        val ns = m.next_session?.takeIf { it.epoch_day == day && it.items.isNotEmpty() && it.session_type.discipline == SessionDiscipline.Lift }
        val row = m.week_plan.firstOrNull { it.epoch_day == day && it.session_type.discipline == SessionDiscipline.Lift }
        return (ns?.items ?: emptyList()) + (row?.items ?: emptyList())
    }

    @Test
    fun everyDestFromEveryStateLandsSensibly() {
        val r = Random(SEED)
        val models = (0..10).flatMap { kind -> List(4) { dayModel(kind, r) } }
        for ((what, m) in models) {
            val liftItems = todaysLiftItems(m)
            for (dest in OpenDest.entries) for (tracker in listOf(false, true)) for (tab in Dest.entries)
                for (open in listOf(false, true)) for (mode in LogMode.entries) for (busy in listOf(false, true)) for (pinned in listOf(false, true))
                    for (exercise in listOf(null, "Curl")) for (planDay in listOf(null, day - 1)) {
                        val nav = OpenNav(tracker, tab, open, mode, exercise, planDay)
                        val at = "$what: $dest from $nav busy=$busy pinned=$pinned"
                        val out = routeOpen(dest, nav, m, sheetBusy = busy, trackerPinned = pinned)
                        when {
                            dest == OpenDest.Track -> assertEquals(nav.copy(showTracker = true), out, at)
                            tracker && pinned -> assertEquals(nav, out, "$at: a tracker holding its service stays")
                            open && busy -> assertEquals(nav.copy(showTracker = false, selected = Dest.Today), out, "$at: unsaved input stays")
                            dest == OpenDest.Today -> assertEquals(OpenNav(false, Dest.Today, false, LogMode.Chooser, null, null), out, "$at: Today with the sheet closed")
                            dest == OpenDest.Log -> assertEquals(OpenNav(false, Dest.Today, true, LogMode.Chooser, null, null), out, "$at: the Log chooser")
                            else -> {
                                assertEquals(OpenNav(false, Dest.Today, true, LogMode.Set, out.sheetPlanExercise, null), out, "$at: the set editor over Today")
                                val ex = out.sheetPlanExercise
                                assertEquals(logSetTarget(m), ex, "$at: the log_set target")
                                if (m.train_blocked) assertNull(ex, "$at: no prefill under a hold")
                                if (ex != null) assertTrue(liftItems.any { it.exercise == ex }, "$at: '$ex' is one of today's lift items")
                            }
                        }
                    }
        }
    }

    private companion object {
        const val SEED = 20_261_030L
    }
}
