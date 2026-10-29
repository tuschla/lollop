package app.milestone.widget

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.ConsistencyView
import app.milestone.DistanceUnit
import app.milestone.HealthScreen
import app.milestone.OpenDest
import app.milestone.PrescriptionView
import app.milestone.ProfileView
import app.milestone.SessionDoneItemView
import app.milestone.SessionDoneView
import app.milestone.SessionPlanView
import app.milestone.ViewModel
import app.milestone.YouthActivityView
import app.milestone.planDateLabel
import java.io.File
import java.time.LocalDateTime
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The widget's data path: snapshot from a model, the file round trip, the stale
 *  day rules, and the text each state puts on the widget. */
@RunWith(RobolectricTestRunner::class)
class WidgetSnapshotTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val res get() = ctx.resources
    private val day = 20_389L // Tue 2025-10-28

    @Before
    fun clean() {
        File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    private fun lift(name: String, lead: String, done: Boolean = false) =
        PrescriptionView(exercise = name, face_lead = lead, done = done)

    private fun model(
        ns: SessionPlanView? = null,
        week: List<SessionPlanView> = listOfNotNull(ns),
        profile: ProfileView? = ProfileView(),
    ) = ViewModel(
        today_epoch_day = day,
        profile = profile,
        next_session = ns,
        week_plan = week,
        consistency = ConsistencyView(this_week_done = 2, this_week_planned = 4),
    )

    private fun face(m: ViewModel) = widgetFace(widgetSnapshotOf(m, DistanceUnit.Km), day, res)

    // ── Store ────────────────────────────────────────────────────────────────

    @Test
    fun snapshotWriteReadRoundtrip() {
        val snap = widgetSnapshotOf(
            model(SessionPlanView(day, "Lower body", "Lift(MaxEffort)", "next", listOf(lift("Squat", "3 × 5 @ 100 kg")))),
            DistanceUnit.Km,
        )
        val before = WidgetSnapshotStore.revision.value
        assertTrue(WidgetSnapshotStore.write(ctx, snap))
        assertEquals(snap, WidgetSnapshotStore.read(ctx))
        assertEquals(snap, WidgetSnapshotStore.today(ctx, day))
        assertEquals(before + 1, WidgetSnapshotStore.revision.value)
        // The same snapshot again is no write and no widget update.
        assertFalse(WidgetSnapshotStore.write(ctx, snap))
        assertEquals(before + 1, WidgetSnapshotStore.revision.value)
        // The file is versioned JSON.
        val text = File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME).readText()
        assertTrue(text, text.contains("\"version\":$WIDGET_SNAPSHOT_VERSION"))
        assertTrue(text, text.contains("\"kind\":\"Lift\""))
    }

    @Test
    fun otherVersionOrBrokenFileReadsAsMissing() {
        val f = File(ctx.filesDir, WidgetSnapshotStore.FILE_NAME)
        f.writeText("""{"version":${WIDGET_SNAPSHOT_VERSION + 1},"epochDay":$day,"kind":"Lift"}""")
        assertNull(WidgetSnapshotStore.read(ctx))
        f.writeText("{not json")
        assertNull(WidgetSnapshotStore.read(ctx))
        assertEquals(res.getString(app.milestone.R.string.widget_open_to_update), widgetFace(WidgetSnapshotStore.read(ctx), day, res).title)
    }

    // ── Stale day ────────────────────────────────────────────────────────────

    @Test
    fun snapshotFromAnotherDayNeverShowsAsToday() {
        val snap = widgetSnapshotOf(
            model(SessionPlanView(day, "Lower body", "Lift(MaxEffort)", "next", listOf(lift("Squat", "3 × 5 @ 100 kg")))),
            DistanceUnit.Km,
        )
        WidgetSnapshotStore.write(ctx, snap)
        // The next day, before anything marks it: the reader and the face refuse it.
        assertNull(WidgetSnapshotStore.today(ctx, day + 1))
        val next = widgetFace(WidgetSnapshotStore.read(ctx), day + 1, res)
        assertEquals(res.getString(app.milestone.R.string.widget_open_to_update), next.title)
        assertEquals(WidgetTone.Muted, next.tone)
        assertTrue(next.items.isEmpty() && next.lead.isEmpty() && next.week == null)
        assertEquals(listOf(OpenDest.Log), next.actions.map { it.dest })
        // A missing snapshot reads the same.
        assertEquals(next, widgetFace(null, day, res))
    }

    @Test
    fun midnightMarksOnlyAnOldDayStaleAndTimeZoneChangeAlways() {
        val snap = widgetSnapshotOf(model(), DistanceUnit.Km)
        WidgetSnapshotStore.write(ctx, snap)
        // The midnight tick, a date or clock change on the same day (an early or
        // repeated alarm, a network time correction) keeps it.
        assertFalse(WidgetSnapshotStore.markStale(ctx, force = false, today = day))
        assertEquals(snap, WidgetSnapshotStore.today(ctx, day))
        // A time zone change marks it even on the same day.
        assertTrue(WidgetSnapshotStore.markStale(ctx, force = true, today = day))
        assertNull(WidgetSnapshotStore.today(ctx, day))
        assertTrue(WidgetSnapshotStore.read(ctx)!!.stale)
        assertEquals(res.getString(app.milestone.R.string.widget_open_to_update), widgetFace(WidgetSnapshotStore.read(ctx), day, res).title)
        // Already stale: no second write.
        assertFalse(WidgetSnapshotStore.markStale(ctx, force = true, today = day))
        // The next published model writes a fresh one over it.
        assertTrue(WidgetSnapshotStore.write(ctx, snap))
        assertEquals(snap, WidgetSnapshotStore.today(ctx, day))
        // Midnight on the next day marks it.
        assertTrue(WidgetSnapshotStore.markStale(ctx, force = false, today = day + 1))
        assertTrue(WidgetSnapshotStore.read(ctx)!!.stale)
    }

    @Test
    fun midnightAlarmIsTheNextLocalMidnight() {
        val zone = ZoneId.of("Europe/Berlin")
        val now = LocalDateTime.of(2026, 10, 24, 23, 59, 30).atZone(zone).toInstant().toEpochMilli()
        val expected = LocalDateTime.of(2026, 10, 25, 0, 0).atZone(zone).toInstant().toEpochMilli()
        assertEquals(expected, WidgetMidnight.nextMidnightMillis(now, zone))
        // Exactly at midnight: the following one, never now.
        assertEquals(expected + 24 * 3_600_000L + 3_600_000L, WidgetMidnight.nextMidnightMillis(expected, zone))
    }

    // ── Content per state ────────────────────────────────────────────────────

    @Test
    fun liftDayShowsTitleItemsWeekAndLog() {
        val ns = SessionPlanView(
            day, "Lower body", "Lift(MaxEffort)", "next",
            listOf(lift("Squat", "3 × 5 @ 100 kg", done = true), lift("Romanian deadlift", "3 × 8 @ 80 kg")),
        )
        val snap = widgetSnapshotOf(model(ns), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Lift, snap.kind)
        assertFalse(snap.sessionDone)
        assertFalse(snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals("Today · ${planDateLabel(day)}", f.overline)
        assertEquals("Lower body", f.title)
        // Open items first, settled ones after.
        assertEquals(listOf("Romanian deadlift · 3 × 8 @ 80 kg", "Squat · Done"), f.items)
        assertEquals("2 of 4 sessions done", f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
    }

    @Test
    fun doneLiftDaySaysSo() {
        val ns = SessionPlanView(day, "Lower body", "Lift(MaxEffort)", "done", listOf(lift("Squat", "3 × 5 @ 100 kg", done = true)))
        val m = model(ns).copy(session_done = SessionDoneView(day, "Lower body", listOf(SessionDoneItemView("Squat", met = true, status = "done"))))
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertTrue(snap.sessionDone)
        assertEquals(listOf("Session done"), widgetFace(snap, day, res).lead)
    }

    @Test
    fun runDayOffersTrackRun() {
        val ns = SessionPlanView(day, "Easy run", "Run(Easy)", "next", listOf(PrescriptionView(face_lead = "8 km easy")))
        val snap = widgetSnapshotOf(model(ns), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Run, snap.kind)
        assertTrue(snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals("Easy run", f.title)
        assertEquals(listOf("Run · 8 km easy"), f.items)
        assertEquals(listOf(OpenDest.Track, OpenDest.Log), f.actions.map { it.dest })
        assertEquals("Track run", f.actions.first().label)
        // Once the run is logged, no Track run.
        val logged = widgetSnapshotOf(model(ns.copy(items = listOf(PrescriptionView(face_lead = "8 km easy", done = true)))), DistanceUnit.Km)
        assertFalse(logged.trackable)
        assertEquals(listOf(OpenDest.Log), widgetFace(logged, day, res).actions.map { it.dest })
        // A future run day (not today) is never trackable from the widget.
        val future = widgetSnapshotOf(model(ns.copy(epoch_day = day + 1)), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Rest, future.kind)
        assertFalse(future.trackable)
    }

    @Test
    fun restDayShowsTheNextSession() {
        val next = SessionPlanView(day + 1, "Upper body", "Lift(Hypertrophy)", "planned")
        val rest = SessionPlanView(day, "", "Rest", "rest")
        val snap = widgetSnapshotOf(model(next, week = listOf(rest, next)), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Rest, snap.kind)
        assertTrue(snap.restDay)
        val f = widgetFace(snap, day, res)
        assertEquals("Rest day", f.title)
        assertEquals(listOf("Next: Upper body, ${planDateLabel(day + 1)}"), f.lead)
        assertEquals("2 of 4 sessions done", f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        // No plan day today at all (before the plan starts) reads as no session.
        val none = widgetSnapshotOf(model(next, week = listOf(next)), DistanceUnit.Km)
        assertEquals("No session today", widgetFace(none, day, res).title)
    }

    @Test
    fun holdSaysTrainingIsPausedAndWhy() {
        val ns = SessionPlanView(day, "Lower body", "Lift(MaxEffort)", "blocked", listOf(lift("Squat", "3 × 5 @ 100 kg")))
        val m = model(ns).copy(train_blocked = true, hold_reason = "pain", safety_tier = "Pain")
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertEquals(WidgetDayKind.Paused, snap.kind)
        // A paused day carries no session, items, week count or Track run.
        assertTrue(snap.items.isEmpty() && snap.title.isEmpty() && snap.weekPlanned == 0 && !snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals("Training paused", f.title)
        assertEquals(WidgetTone.Paused, f.tone)
        assertEquals(listOf("You reported pain today."), f.lead)
        assertNull(f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        // The pregnancy warning sign keeps its own wording.
        val warn = m.copy(hold_reason = "pregnancy", safety_tier = "MedicalReferral", profile = ProfileView(health = HealthScreen(pregnant = true, pregnancy_warning_sign = true)))
        assertEquals(
            listOf("You noted a pregnancy warning sign. Stop exercising and contact your doctor or midwife."),
            face(warn).lead,
        )
        // A tier without a reason falls back to the tier's line.
        assertEquals(listOf("Your HRV has been well below your normal."), face(m.copy(hold_reason = null, safety_tier = "HrvTrend")).lead)
    }

    @Test
    fun underEighteenShowsThePlanPauseAndActivity() {
        val m = model().copy(
            train_blocked = true,
            hold_reason = "youth",
            youth_activity = YouthActivityView(minutes = 150, weekly_target_minutes = 420),
        )
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertEquals(WidgetDayKind.Paused, snap.kind)
        val f = widgetFace(snap, day, res)
        assertEquals("Plans paused under 18", f.title)
        assertEquals(WidgetTone.Calm, f.tone)
        assertEquals(
            listOf("150 of 420 active minutes this week", "Logging, history and your weekly activity still work."),
            f.lead,
        )
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        // Before the activity card exists, only the pause line.
        assertEquals(listOf("Logging, history and your weekly activity still work."), face(m.copy(youth_activity = null)).lead)
    }

    @Test
    fun noPlanAndNoProfile() {
        val noPlan = widgetSnapshotOf(model(), DistanceUnit.Km)
        assertEquals(WidgetDayKind.NoPlan, noPlan.kind)
        val f = widgetFace(noPlan, day, res)
        assertEquals("No plan yet", f.title)
        assertEquals(listOf("Open lollop to make a plan."), f.lead)
        assertNull(f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        assertEquals("Nothing to plan yet", face(model().copy(plan_withheld = "nothing_to_plan")).title)
        assertEquals("Your plan is paused", face(model().copy(plan_withheld = "health_hold")).title)

        val setup = widgetSnapshotOf(model(profile = null), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Setup, setup.kind)
        val s = widgetFace(setup, day, res)
        assertEquals("Not set up yet", s.title)
        assertEquals(listOf("Open lollop to answer the setup questions."), s.lead)
        assertTrue(s.actions.isEmpty())
    }

    @Test
    fun cutItemsEndInACount() {
        val f = WidgetFace("o", "t", WidgetTone.Normal, lead = listOf("lead"), items = listOf("a", "b", "c", "d"))
        assertEquals(listOf("lead"), visibleLines(f, 1, res))
        assertEquals(listOf("lead", "a", "3 more items"), visibleLines(f, 3, res))
        assertEquals(listOf("lead", "a", "b", "c", "d"), visibleLines(f, 5, res))
        assertEquals(listOf("lead", "a", "b", "c", "d"), visibleLines(f, 6, res))
        // A count never stands in for a single item: that slot shows the item.
        assertEquals(listOf("lead", "2 more items"), visibleLines(f.copy(items = listOf("a", "b")), 2, res))
    }
}
