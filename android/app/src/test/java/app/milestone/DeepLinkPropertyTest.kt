package app.milestone

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.widgetFace
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The open-intent routing is total: every destination from every navigation state
 * the scaffold can be in (tracker shown or not, each tab, each sheet mode open or
 * closed, unsaved input, a tracker holding a service) and every kind of day (lift
 * open, started or done, run, rest, a moved session, a hold, the under-18 pause,
 * no plan, no profile) lands somewhere sensible, and any intent at all decodes to a
 * destination or to "not an open intent" without throwing. Each surface only ever
 * hands out a destination its state supports. Exhaustive over the navigation
 * states; `DEEPLINK_PROP_MODELS` random days (default 40, seed `DEEPLINK_PROP_SEED`).
 */
@RunWith(RobolectricTestRunner::class)
class DeepLinkPropertyTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val seed = System.getenv("DEEPLINK_PROP_SEED")?.toLongOrNull() ?: 20_261_030L
    private val day = 20_400L

    // ── Days ──────────────────────────────────────────────────────────────────

    private fun lift(name: String, done: Boolean = false, logged: Boolean = done) =
        PrescriptionView(exercise = name, sets = 3, face_lead = "3 × 5", done = done, sets_done = if (logged) 1 else 0)

    private fun runItem(done: Boolean = false) =
        PrescriptionView(run_head = "8 km", run_head_km = 8.0, run_target = RunTargetView(pace_label_km = "5:40–6:00 /km"), done = done)

    /** A random model of one day as the core publishes it, and its name. */
    private fun randomModel(r: Random): Pair<String, ViewModel> {
        val names = listOf("Back squat", "Bench press", "Row", "Deadlift").shuffled(r)
        val items = List(1 + r.nextInt(4)) { i -> lift(names[i]) }
        val profile = ProfileView()
        fun session(d: Long, type: String, status: String, its: List<PrescriptionView>) =
            SessionPlanView(epoch_day = d, title = "S$d", session_type = type, status = status, items = its)
        return when (r.nextInt(11)) {
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
                "hold" to ViewModel(today_epoch_day = day, profile = profile, train_blocked = true, hold_reason = "pain", next_session = blank, week_plan = listOf(blank))
            }
            7 -> {
                val blank = session(day, "Lift(Strength)", "blocked", emptyList())
                "under 18" to ViewModel(today_epoch_day = day, profile = profile, train_blocked = true, hold_reason = "youth", next_session = blank, week_plan = listOf(blank))
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
        val count = System.getenv("DEEPLINK_PROP_MODELS")?.toIntOrNull() ?: 40
        val models = List(count) { randomModel(r) }
        var cases = 0
        for ((what, m) in models) {
            val liftItems = todaysLiftItems(m)
            for (dest in OpenDest.entries) for (tracker in listOf(false, true)) for (tab in Dest.entries)
                for (open in listOf(false, true)) for (mode in LogMode.entries) for (busy in listOf(false, true)) for (pinned in listOf(false, true))
                    for (exercise in listOf(null, "Curl")) for (planDay in listOf(null, day - 1)) {
                        val nav = OpenNav(tracker, tab, open, mode, exercise, planDay)
                        val at = "$what: $dest from $nav busy=$busy pinned=$pinned"
                        val out = routeOpen(dest, nav, m, sheetBusy = busy, trackerPinned = pinned)
                        cases++
                        when {
                            dest == OpenDest.Track -> assertEquals(at, nav.copy(showTracker = true), out)
                            tracker && pinned -> assertEquals("$at: a tracker holding its service stays", nav, out)
                            else -> {
                                assertFalse(at, out.showTracker)
                                assertEquals(at, Dest.Today, out.selected)
                                if (open && busy) {
                                    assertEquals("$at: unsaved input stays", nav.copy(showTracker = false, selected = Dest.Today), out)
                                } else {
                                    when (dest) {
                                        OpenDest.Today -> assertFalse("$at: Today closes the sheet", out.sheetOpen)
                                        OpenDest.Log -> {
                                            assertTrue(at, out.sheetOpen)
                                            assertEquals(at, LogMode.Chooser, out.sheetMode)
                                            assertNull(at, out.sheetPlanExercise)
                                        }
                                        else -> {
                                            assertTrue(at, out.sheetOpen)
                                            assertEquals(at, LogMode.Set, out.sheetMode)
                                            assertNull("$at: no stale plan day", out.sheetPlanDay)
                                            val ex = out.sheetPlanExercise
                                            if (m.train_blocked) assertNull("$at: no prefill under a hold", ex)
                                            if (ex != null) assertTrue("$at: '$ex' is one of today's lift items", liftItems.any { it.exercise == ex })
                                            val open1 = m.next_session?.takeIf { it.epoch_day == day && it.actionable && !m.train_blocked && sessionDiscipline(it.session_type) == "Lift" }
                                                ?.items?.firstOrNull { !it.done }
                                            if (open1 != null) assertEquals("$at: the first open item", open1.exercise, ex)
                                        }
                                    }
                                    if (!out.sheetOpen) assertNull(at, out.sheetPlanExercise)
                                }
                            }
                        }
                    }
        }
        assertTrue("$cases cases", cases > 100_000)
    }

    // ── openDestOf ────────────────────────────────────────────────────────────

    @Test
    fun anyIntentDecodesWithoutThrowing() {
        val r = Random(seed)
        val actions = listOf(ACTION_OPEN, Intent.ACTION_MAIN, Intent.ACTION_VIEW, Intent.ACTION_SEND, null, ACTION_OPEN.uppercase(), "$ACTION_OPEN ", "")
        val wires = OpenDest.entries.map { it.wire } + listOf("", "TODAY", "Today", " log", "log_set\u0000", "settings", "track/../log", "\uD83C\uDFC3", "x".repeat(10_000))
        repeat(System.getenv("DEEPLINK_PROP_INTENTS")?.toIntOrNull() ?: 3_000) { i ->
            val action = actions[r.nextInt(actions.size)]
            val intent = Intent(action)
            if (r.nextBoolean()) intent.data = Uri.parse(listOf("lollop://today", "content://x/y", "geo:0,0", "")[r.nextInt(4)])
            intent.addFlags(r.nextInt())
            val wire = when (r.nextInt(5)) {
                0 -> { intent.putExtra(EXTRA_DEST, r.nextInt()); null }
                1 -> { intent.putExtra(EXTRA_DEST, arrayOf("log")); null }
                2 -> null
                else -> wires[r.nextInt(wires.size)].also { intent.putExtra(EXTRA_DEST, it) }
            }
            val got = openDestOf(intent)
            if (action != ACTION_OPEN) {
                assertNull("#$i $action is not an open intent", got)
            } else {
                assertEquals("#$i wire '$wire'", OpenDest.entries.firstOrNull { it.wire == wire } ?: OpenDest.Today, got)
            }
            // Anything an open intent decodes to round-trips through its own intent.
            if (got != null) assertEquals(got, openDestOf(openIntent(ctx, got)))
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

            // Tile: Start run only for an open run; Log set only for an open lift session.
            val tile = tileFace(s.takeIf { shownToday })
            when (tile) {
                TileFace.StartRun -> assertTrue(at, shownToday && s.kind == WidgetDayKind.Run && s.trackable)
                TileFace.LogSet -> assertTrue(at, shownToday && s.kind == WidgetDayKind.Lift && !s.sessionDone)
                TileFace.Log -> {}
                TileFace.Recording -> throw AssertionError("$at: recording without a run")
            }

            // Reminder: its action matches the day kind; never on a non-session day.
            val reminder = reminderOf(s, day, "UTC")
            if (reminder != null) {
                assertTrue("$at: reminder for ${s.kind}", s.kind == WidgetDayKind.Lift || s.kind == WidgetDayKind.Run)
                assertFalse("$at: reminder for a done session", s.sessionDone)
                assertEquals(at, if (reminder.kind == WidgetDayKind.Run) OpenDest.Track else OpenDest.LogSet, reminder.dest)
                assertNotNull(reminderActionLabel(res, reminder))
            }
        }
    }
}
