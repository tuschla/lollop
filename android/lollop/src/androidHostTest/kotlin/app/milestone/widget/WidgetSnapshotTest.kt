package app.milestone.widget

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppFileNames
import app.milestone.ConsistencyView
import app.milestone.DistanceUnit
import app.milestone.HealthScreen
import app.milestone.HoldReason
import app.milestone.ItemStatus
import app.milestone.OpenDest
import app.milestone.PlanWithheld
import app.milestone.PrescriptionView
import app.milestone.Profile
import app.milestone.R
import app.milestone.SafetyTier
import app.milestone.SessionDoneItemView
import app.milestone.SessionDoneView
import app.milestone.SessionPlanView
import app.milestone.SessionStatus
import app.milestone.SessionType
import app.milestone.ViewModel
import app.milestone.YouthActivityView
import app.milestone.nextLocalMidnightMillis
import app.milestone.planDateLabel
import java.io.File
import java.time.LocalDateTime
import java.time.ZoneId
import kotlinx.datetime.toKotlinTimeZone
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

    private fun str(id: Int, vararg args: Any): String = res.getString(id, *args)
    private fun item(name: String, detail: String) = keepUnits(str(R.string.widget_item_line, name, detail))
    private fun weekDone(done: Int, planned: Int) = res.getQuantityString(R.plurals.main_week_done_count, planned, done, planned)

    @Before
    fun clean() {
        File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT).delete()
    }

    private fun lift(name: String, lead: String, done: Boolean = false) =
        PrescriptionView(exercise = name, face_lead = lead, done = done)

    private fun model(
        ns: SessionPlanView? = null,
        week: List<SessionPlanView> = listOfNotNull(ns),
        profile: Profile? = Profile(),
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
            model(SessionPlanView(day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Next, listOf(lift("Squat", "3 × 5 @ 100 kg")))),
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
        val text = File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT).readText()
        assertTrue(text, text.contains("\"version\":$WIDGET_SNAPSHOT_VERSION"))
        assertTrue(text, text.contains("\"kind\":\"Lift\""))
    }

    @Test
    fun otherVersionOrBrokenFileReadsAsMissing() {
        val f = File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT)
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
            model(SessionPlanView(day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Next, listOf(lift("Squat", "3 × 5 @ 100 kg")))),
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

    /** A zone change back across midnight (or a clock set forward and back) brings
     *  a stale snapshot's own day back: the mark ticks [WidgetSnapshotStore.staleMarks]
     *  so [WidgetSurfaces] derives it again from the model the process holds. */
    @Test
    fun aStaleSnapshotWhoseDayComesBackIsDerivedAgain() {
        WidgetSnapshotStore.write(ctx, widgetSnapshotOf(model(), DistanceUnit.Km))
        val marks = WidgetSnapshotStore.staleMarks.value
        // West across zones: the local day is the one before.
        assertTrue(WidgetSnapshotStore.markStale(ctx, force = true, today = day - 1))
        assertEquals("another day: nothing to derive", marks, WidgetSnapshotStore.staleMarks.value)
        // Local midnight: the snapshot's day again.
        assertFalse(WidgetSnapshotStore.markStale(ctx, force = false, today = day))
        assertEquals(marks + 1, WidgetSnapshotStore.staleMarks.value)
    }

    @Test
    fun midnightAlarmIsTheNextLocalMidnight() {
        val zone = ZoneId.of("Europe/Berlin")
        val now = LocalDateTime.of(2026, 10, 24, 23, 59, 30).atZone(zone).toInstant().toEpochMilli()
        val expected = LocalDateTime.of(2026, 10, 25, 0, 0).atZone(zone).toInstant().toEpochMilli()
        assertEquals(expected, nextLocalMidnightMillis(now, zone.toKotlinTimeZone()))
        // Exactly at midnight: the following one, never now.
        assertEquals(expected + 24 * 3_600_000L + 3_600_000L, nextLocalMidnightMillis(expected, zone.toKotlinTimeZone()))
    }

    // ── Content per state ────────────────────────────────────────────────────

    @Test
    fun liftDayShowsTitleItemsWeekAndTheTilesLogSet() {
        val ns = SessionPlanView(
            day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Next,
            listOf(lift("Squat", "3 × 5 @ 100 kg", done = true), lift("Romanian deadlift", "3 × 8 @ 80 kg")),
        )
        val snap = widgetSnapshotOf(model(ns), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Lift, snap.kind)
        assertFalse(snap.sessionDone)
        assertFalse(snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals(str(R.string.widget_overline_day, str(R.string.widget_overline_today), planDateLabel(day)), f.overline)
        assertEquals("Lower body", f.title)
        // Open items first, settled ones after.
        assertEquals(listOf(item("Romanian deadlift", "3 × 8 @ 80 kg"), item("Squat", str(R.string.widget_item_done))), f.items)
        assertEquals(weekDone(2, 4), f.week)
        assertEquals(listOf(OpenDest.LogSet, OpenDest.Log), f.actions.map { it.dest })
        assertEquals(str(R.string.tile_log_set), f.actions.first().label)
        // The one line of the smallest size: what the next set is.
        assertEquals(str(R.string.reminder_line_progress, 1, 2, "Romanian deadlift"), f.status)
        assertEquals("Romanian deadlift", f.brief)
    }

    @Test
    fun weekStripCarriesEachPlanDayAndGoesWithAHold() {
        val ns = SessionPlanView(day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Next, listOf(lift("Squat", "3 × 5 @ 100 kg")))
        val week = listOf(
            SessionPlanView(day - 3, "", SessionType.LiftMaxEffort, SessionStatus.BeforePlan),
            SessionPlanView(day - 2, "Run", SessionType.RunEasy, SessionStatus.Missed),
            SessionPlanView(day - 1, "Upper", SessionType.LiftMaxEffort, SessionStatus.DoneLighter),
            ns,
            SessionPlanView(day + 1, "", SessionType.Rest, SessionStatus.Rest),
            SessionPlanView(day + 2, "Run", SessionType.RunEasy, SessionStatus.Planned),
            SessionPlanView(day + 3, "Upper", SessionType.LiftMaxEffort, SessionStatus.Adjusted),
        )
        val snap = widgetSnapshotOf(model(ns, week = week), DistanceUnit.Km)
        assertEquals(
            listOf(
                WidgetWeekMark.None, WidgetWeekMark.Missed, WidgetWeekMark.Done, WidgetWeekMark.Next,
                WidgetWeekMark.Rest, WidgetWeekMark.Planned, WidgetWeekMark.Next,
            ),
            snap.week.map { it.mark },
        )
        assertEquals(week.map { it.epoch_day }, snap.week.map { it.epochDay })
        assertEquals(snap.week, widgetFace(snap, day, res).days)
        // A hold shows no week: it carries no plan.
        assertTrue(widgetSnapshotOf(model(ns, week = week).copy(hold_reason = HoldReason.Pain, safety_tier = SafetyTier.Pain), DistanceUnit.Km).week.isEmpty())
        // A file written before the week was recorded still reads, without a strip.
        val f = File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT)
        f.writeText("""{"version":$WIDGET_SNAPSHOT_VERSION,"epochDay":$day,"kind":"Rest"}""")
        assertEquals(emptyList<WidgetWeekDay>(), WidgetSnapshotStore.read(ctx)!!.week)
    }

    @Test
    fun finishedDayShowsSessionDoneAndTheNextSession() {
        // Once today's session is done the core moves next_session to a later day and
        // keeps today's row done in week_plan (calendar_edges.rs, "the hero moves on").
        val today = SessionPlanView(day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Done, listOf(lift("Squat", "3 × 5 @ 100 kg", done = true)))
        val next = SessionPlanView(day + 2, "Upper body", SessionType.LiftMaxEffort, SessionStatus.Next, listOf(lift("Bench", "")))
        val m = model(next, week = listOf(today, next))
            .copy(session_done = SessionDoneView(day, "Lower body", listOf(SessionDoneItemView("Squat", met = true, status = ItemStatus.Done))))
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertEquals(WidgetDayKind.Lift, snap.kind)
        assertTrue(snap.sessionDone)
        assertFalse(snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals("Lower body", f.title)
        assertEquals(listOf(str(R.string.widget_session_done), str(R.string.widget_next, "Upper body", planDateLabel(day + 2))), f.lead)
        assertEquals(listOf(item("Squat", str(R.string.widget_item_done))), f.items)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        assertEquals(str(R.string.widget_session_done), f.status)
    }

    @Test
    fun runDayOffersStartRunLikeTheTile() {
        val ns = SessionPlanView(day, "Easy run", SessionType.RunEasy, SessionStatus.Next, listOf(PrescriptionView(face_lead = "8 km easy")))
        val snap = widgetSnapshotOf(model(ns), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Run, snap.kind)
        assertTrue(snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals("Easy run", f.title)
        assertEquals(listOf(item(str(R.string.main_run), "8 km easy")), f.items)
        assertEquals(listOf(OpenDest.Track, OpenDest.Log), f.actions.map { it.dest })
        assertEquals(str(R.string.tile_start_run), f.actions.first().label)
        assertEquals(keepUnits("8 km easy"), f.status)
        assertEquals(keepUnits("8 km easy"), f.brief)
        // A full run summary ("8 km easy") is a line, not a hero figure; a detail
        // that is only a figure is one, with or without a pace.
        assertNull(f.hero)
        assertTrue(isFigure("3.4 mi") && isFigure("45 min") && isFigure("6 × 800 m"))
        assertFalse(isFigure("8 km easy") || isFigure("Easy run") || isFigure(""))
        // A recording run in this process: the tile's Recording action.
        assertEquals(str(R.string.tile_run_recording), widgetFace(snap, day, res, recording = true).actions.first().label)
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
        val next = SessionPlanView(day + 1, "Upper body", SessionType.LiftMaxEffort, SessionStatus.Planned)
        val rest = SessionPlanView(day, "", SessionType.Rest, SessionStatus.Rest)
        val snap = widgetSnapshotOf(model(next, week = listOf(rest, next)), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Rest, snap.kind)
        assertTrue(snap.restDay)
        val f = widgetFace(snap, day, res)
        assertEquals(str(R.string.widget_rest_day), f.title)
        assertEquals(listOf(str(R.string.widget_next, "Upper body", planDateLabel(day + 1))), f.lead)
        assertEquals(f.lead.single(), f.status)
        // "Next: Wed" within the week, the date further out.
        assertEquals(str(R.string.widget_next_day, java.time.LocalDate.ofEpochDay(day + 1).dayOfWeek.getDisplayName(java.time.format.TextStyle.SHORT, java.util.Locale.US)), f.brief)
        assertEquals(weekDone(2, 4), f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        // A tall card's hero: the next session by name over its date.
        assertEquals(WidgetHero("Upper body", str(R.string.widget_next_day, planDateLabel(day + 1)), figure = false), f.hero)
        // No plan day today at all (before the plan starts) reads as no session.
        val none = widgetSnapshotOf(model(next, week = listOf(next)), DistanceUnit.Km)
        assertEquals(str(R.string.widget_no_session), widgetFace(none, day, res).title)
    }

    @Test
    fun holdSaysTrainingIsPausedButNotWhy() {
        val ns = SessionPlanView(day, "Lower body", SessionType.LiftMaxEffort, SessionStatus.Blocked, listOf(lift("Squat", "3 × 5 @ 100 kg")))
        val m = model(ns).copy(hold_reason = HoldReason.Pain, safety_tier = SafetyTier.Pain)
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertEquals(WidgetDayKind.Paused, snap.kind)
        // A paused day carries no session, items, week count or Track run.
        assertTrue(snap.items.isEmpty() && snap.title.isEmpty() && snap.weekPlanned == 0 && !snap.trackable)
        val f = widgetFace(snap, day, res)
        assertEquals(str(R.string.main_status_on_hold), f.title)
        assertEquals(WidgetTone.Paused, f.tone)
        assertEquals(listOf(str(R.string.widget_paused_body)), f.lead)
        assertNull(f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        // A home screen never names the health reason behind a hold.
        val warn = m.copy(hold_reason = HoldReason.Pregnancy, safety_tier = SafetyTier.MedicalReferral, profile = Profile(health = HealthScreen(pregnant = true, pregnancy_warning_sign = true)))
        assertEquals(f.title to f.lead, face(warn).let { it.title to it.lead })
    }

    @Test
    fun underEighteenPauseIsGenericAndNotInTheDangerColour() {
        val m = model().copy(
            hold_reason = HoldReason.Youth,
            youth_activity = YouthActivityView(minutes = 150, weekly_target_minutes = 420),
        )
        val snap = widgetSnapshotOf(m, DistanceUnit.Km)
        assertEquals(WidgetDayKind.Paused, snap.kind)
        val f = widgetFace(snap, day, res)
        assertEquals(str(R.string.main_status_on_hold), f.title)
        assertEquals(listOf(str(R.string.widget_paused_body)), f.lead)
        assertTrue(f.tone != WidgetTone.Paused)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
    }

    @Test
    fun noPlanAndNoProfile() {
        val noPlan = widgetSnapshotOf(model(), DistanceUnit.Km)
        assertEquals(WidgetDayKind.NoPlan, noPlan.kind)
        val f = widgetFace(noPlan, day, res)
        assertEquals(str(R.string.widget_no_plan), f.title)
        assertEquals(listOf(str(R.string.widget_no_plan_body)), f.lead)
        assertNull(f.week)
        assertEquals(listOf(OpenDest.Log), f.actions.map { it.dest })
        assertEquals(str(R.string.main_plan_withheld_empty_title), face(model().copy(plan_withheld = PlanWithheld.NothingToPlan)).title)
        assertEquals(str(R.string.main_plan_withheld_health_title), face(model().copy(plan_withheld = PlanWithheld.HealthHold)).title)

        val setup = widgetSnapshotOf(model(profile = null), DistanceUnit.Km)
        assertEquals(WidgetDayKind.Setup, setup.kind)
        val s = widgetFace(setup, day, res)
        assertEquals(str(R.string.widget_setup_title), s.title)
        assertEquals(listOf(str(R.string.widget_setup_body)), s.lead)
        assertTrue(s.actions.isEmpty())
    }

    @Test
    fun cutItemsEndInACount() {
        val f = WidgetFace("o", "t", WidgetTone.Normal, lead = listOf("lead"), items = listOf("a", "b", "c", "d"))
        assertEquals(listOf("lead"), visibleLines(f, 1, res))
        assertEquals(listOf("lead", "a", res.getQuantityString(R.plurals.widget_more_items, 3, 3)), visibleLines(f, 3, res))
        assertEquals(listOf("lead", "a", "b", "c", "d"), visibleLines(f, 5, res))
        assertEquals(listOf("lead", "a", "b", "c", "d"), visibleLines(f, 6, res))
        // A count never stands in for a single item: that slot shows the item.
        assertEquals(listOf("lead", "a", "b"), visibleLines(f.copy(items = listOf("a", "b")), 3, res))
        // Nor for every item: one slot left shows the first item, not "N more items".
        assertEquals(listOf("lead", "a"), visibleLines(f.copy(items = listOf("a", "b")), 2, res))
        assertEquals(listOf("a"), visibleLines(f.copy(lead = emptyList()), 1, res))
    }
}
