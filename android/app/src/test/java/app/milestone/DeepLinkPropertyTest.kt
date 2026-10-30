package app.milestone

import android.content.Context
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.widgetFace
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Open-intent routing is total: every destination from every scaffold navigation
 * state on every kind of day lands somewhere sensible, every intent shape decodes,
 * and the widget only offers destinations its day supports.
 */
@RunWith(RobolectricTestRunner::class)
class DeepLinkPropertyTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val seed = 20_261_030L
    private val day = 20_400L

    // ── Days ──────────────────────────────────────────────────────────────────

    private fun lift(name: String) = PrescriptionView(exercise = name, sets = 3, face_lead = "3 × 5")

    private fun runItem() =
        PrescriptionView(run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(pace_label_km = "5:40–6:00 /km"))

    /** A model of one day of [kind] (0..10) as the core publishes it, and its name. */
    private fun dayModel(kind: Int, r: Random): Pair<String, ViewModel> {
        val names = listOf("Back squat", "Bench press", "Row", "Deadlift").shuffled(r)
        val items = List(1 + r.nextInt(4)) { i -> lift(names[i]) }
        val profile = ProfileView()
        fun session(d: Long, type: String, status: String, its: List<PrescriptionView>) =
            SessionPlanView(epoch_day = d, title = "S$d", session_type = type, status = status, items = its)
        return when (kind) {
            0 -> "lift open" to session(day, "Lift(Strength)", "next", items).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            1 -> {
                val k = r.nextInt(1, items.size + 1)
                val started = items.mapIndexed { i, it -> if (i < k) it.copy(done = i < k - 1 || r.nextBoolean(), sets_done = 1) else it }
                "lift started" to session(day, "Lift(Strength)", "adjusted", started).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            }
            2 -> {
                // Finished: the core moves next_session on; today's row says what was done.
                val done = items.map { it.copy(done = true, sets_done = 3) }
                val next = session(day + 2, "Run(Easy)", "next", listOf(runItem()))
                "lift done" to ViewModel(today_epoch_day = day, profile = profile, next_session = next, week_plan = listOf(session(day, "Lift(Strength)", "done", done), next))
            }
            3 -> "run open" to session(day, "Run(Easy)", "next", listOf(runItem())).let { ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(it)) }
            4 -> {
                val next = session(day + 1, "Lift(Strength)", "next", items)
                "rest" to ViewModel(today_epoch_day = day, profile = profile, next_session = next, week_plan = listOf(session(day, "Rest", "rest", emptyList()), next))
            }
            5 -> {
                // Today's lift moved to Thursday: today is a rest day.
                val moved = session(day + 3, "Lift(Strength)", "next", items).copy(moved_from_epoch_day = day)
                "moved" to ViewModel(today_epoch_day = day, profile = profile, next_session = moved, week_plan = listOf(session(day, "Rest", "rest", emptyList()), moved))
            }
            6 -> {
                val blank = session(day, "Lift(Strength)", "blocked", emptyList())
                "hold" to ViewModel(today_epoch_day = day, profile = profile, hold_reason = "pain", next_session = blank, week_plan = listOf(blank))
            }
            7 -> {
                val blank = session(day, "Lift(Strength)", "blocked", emptyList())
                "under 18" to ViewModel(today_epoch_day = day, profile = profile, hold_reason = "youth", next_session = blank, week_plan = listOf(blank))
            }
            8 -> "no plan" to ViewModel(today_epoch_day = day, profile = profile, plan_withheld = listOf(null, "nothing_to_plan").random(r))
            9 -> "no profile" to ViewModel(today_epoch_day = day)
            else -> "missed yesterday, lift today" to session(day, "Lift(Strength)", "next", items).let {
                ViewModel(today_epoch_day = day, profile = profile, next_session = it, week_plan = listOf(session(day - 1, "Run(Easy)", "missed", listOf(runItem())), it))
            }
        }
    }

    /** The lift items of today's session the set editor may prefill from: the hero
     *  when it is today's actionable lift, else today's week row with logged work. */
    private fun todaysLiftItems(m: ViewModel): List<PrescriptionView> {
        val ns = m.next_session?.takeIf { it.epoch_day == day && it.items.isNotEmpty() && sessionDiscipline(it.session_type) == "Lift" }
        val row = m.week_plan.firstOrNull { it.epoch_day == day && sessionDiscipline(it.session_type) == "Lift" }
        return (ns?.items ?: emptyList()) + (row?.items ?: emptyList())
    }

    // ── routeOpen ─────────────────────────────────────────────────────────────

    @Test
    fun everyDestFromEveryStateLandsSensibly() {
        val r = Random(seed)
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
                            dest == OpenDest.Track -> assertEquals(at, nav.copy(showTracker = true), out)
                            tracker && pinned -> assertEquals("$at: a tracker holding its service stays", nav, out)
                            open && busy -> assertEquals("$at: unsaved input stays", nav.copy(showTracker = false, selected = Dest.Today), out)
                            dest == OpenDest.Today -> assertEquals("$at: Today with the sheet closed", OpenNav(false, Dest.Today, false, LogMode.Chooser, null, null), out)
                            dest == OpenDest.Log -> assertEquals("$at: the Log chooser", OpenNav(false, Dest.Today, true, LogMode.Chooser, null, null), out)
                            else -> {
                                assertEquals("$at: the set editor over Today", OpenNav(false, Dest.Today, true, LogMode.Set, out.sheetPlanExercise, null), out)
                                val ex = out.sheetPlanExercise
                                assertEquals("$at: the log_set target", logSetTarget(m), ex)
                                if (m.train_blocked) assertNull("$at: no prefill under a hold", ex)
                                if (ex != null) assertTrue("$at: '$ex' is one of today's lift items", liftItems.any { it.exercise == ex })
                            }
                        }
                    }
        }
    }

    // ── openDestOf ────────────────────────────────────────────────────────────

    @Test
    fun everyIntentShapeDecodes() {
        val byWire = mapOf("today" to OpenDest.Today, "log" to OpenDest.Log, "track" to OpenDest.Track, "log_set" to OpenDest.LogSet)
        val actions = listOf(ACTION_OPEN, Intent.ACTION_MAIN, Intent.ACTION_VIEW, Intent.ACTION_SEND, null, ACTION_OPEN.uppercase(), "$ACTION_OPEN ", "")
        val wires = byWire.keys.toList() + listOf("", "TODAY", "Today", " log", "log_set\u0000", "settings", "track/../log", "\uD83C\uDFC3", "x".repeat(10_000))
        // Each wire as a string extra, then a non-string extra and none.
        val extras: List<Any?> = wires + listOf(7, arrayOf("log"), null)
        for (action in actions) for (x in extras) {
            val intent = Intent(action)
            when (x) {
                is String -> intent.putExtra(EXTRA_DEST, x)
                is Int -> intent.putExtra(EXTRA_DEST, x)
                is Array<*> -> intent.putExtra(EXTRA_DEST, x.map { it as String }.toTypedArray())
            }
            val expected = if (action != ACTION_OPEN) null else byWire[x as? String] ?: OpenDest.Today
            assertEquals("$action dest=${x.toString().take(20)}", expected, openDestOf(intent))
        }
    }

    // ── What the surfaces hand out ────────────────────────────────────────────

    @Test
    fun surfacesOnlyOfferDestinationsTheirDaySupports() {
        val r = Random(seed)
        val res = ctx.resources
        repeat(2_000) { i ->
            val kind = WidgetDayKind.entries[r.nextInt(WidgetDayKind.entries.size)]
            val items = List(r.nextInt(4)) {
                WidgetItem(WidgetItemKind.entries[r.nextInt(3)], name = "E$it", detail = "3 × 5", done = r.nextBoolean())
            }
            val done = r.nextBoolean()
            val s = WidgetSnapshot(
                epochDay = day + r.nextInt(-1, 2), stale = r.nextInt(5) == 0, kind = kind, title = "T", items = items,
                sessionDone = done,
                trackable = (kind == WidgetDayKind.Run || kind == WidgetDayKind.Lift) && !done && items.any { it.kind == WidgetItemKind.Run && !it.done },
            )
            val at = "#$i $s"
            val shownToday = !s.stale && s.epochDay == day

            // Widget: Track only for today's trackable session; Log on every face but setup.
            val face = widgetFace(s, day, res)
            val dests = face.actions.map { it.dest }
            if (OpenDest.Track in dests) assertTrue("$at: Track offered", shownToday && s.trackable)
            if (shownToday && s.kind != WidgetDayKind.Setup) assertTrue("$at: Log offered", OpenDest.Log in dests)
            assertFalse("$at: the widget never offers log_set", OpenDest.LogSet in dests)
        }
    }
}
