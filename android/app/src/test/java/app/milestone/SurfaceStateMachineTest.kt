package app.milestone

import android.Manifest
import android.app.AlarmManager
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.Intent
import android.os.Bundle
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.widget.TodayWidgetReceiver
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetMidnight
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import app.milestone.widget.WidgetTone
import app.milestone.widget.widgetFace
import app.milestone.widget.widgetSnapshotOf
import java.io.File
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.util.TimeZone
import java.util.concurrent.TimeUnit
import kotlin.random.Random
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadow.api.Shadow
import org.robolectric.shadows.ShadowBroadcastPendingResult
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter

/**
 * Seeded walks over the surfaces that read the widget snapshot (session reminder,
 * home screen widget, Quick Settings tile): clock, zone, plan, run and process
 * steps in [Walk.step], invariants in [Walk.check]. `SURFACE_SM_SEEDS` (12) ×
 * `SURFACE_SM_STEPS` (150) from `SURFACE_SM_SEED`; a failure prints its replay line.
 */
@RunWith(RobolectricTestRunner::class)
class SurfaceStateMachineTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val res get() = app.resources
    private val nm get() = app.getSystemService(NotificationManager::class.java)
    private val am get() = app.getSystemService(AlarmManager::class.java)
    private val savedZone: TimeZone = TimeZone.getDefault()

    @Before
    fun setUp() {
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        clean()
    }

    @After
    fun tearDown() {
        clean()
        TimeZone.setDefault(savedZone)
    }

    private fun clean() {
        AppClock.offsetMillis = 0L
        // A fresh install per walk: the reminder's setting and the day it last reminded.
        app.getSharedPreferences("session_reminder", 0).edit().clear().commit()
        RunSession.reset()
        File(app.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
        nm.cancelAll()
        shadowOf(am).scheduledAlarms.toList().forEach { it.operation?.let(am::cancel) }
    }

    // ── Pure: the next alarm times against an independent oracle ─────────────

    /** Every tz rule set, instants around each 2025–2028 transition: the reminder's next
     *  fire is the first valid local reminder time strictly after now (a skipped
     *  time moves forward by the gap, a repeated one fires at its first pass) and
     *  the midnight tick is the start of the next local day. */
    @Test
    fun nextFireAndMidnightMatchTheOracleInEveryZone() {
        val rng = Random(SEED_BASE)
        val minutes = listOf(0, 1, 30, 59, 60, 90, 119, 120, 150, 179, 180, 7 * 60 + 30, 23 * 60 + 59) + List(4) { rng.nextInt(MINUTES_PER_DAY) }
        for (zone in ZoneId.getAvailableZoneIds().sorted().map(ZoneId::of).distinctBy { it.rules }) {
            val id = zone.id
            val instants = mutableListOf<Long>()
            var t = Instant.parse("2025-01-01T00:00:00Z")
            val end = Instant.parse("2028-12-31T00:00:00Z")
            while (true) {
                val tr = zone.rules.nextTransition(t) ?: break
                if (tr.instant.isAfter(end)) break
                val at = tr.instant.toEpochMilli()
                for (d in longArrayOf(-26 * HOUR, -HOUR, -1, 0, 1, HOUR / 2, HOUR, 3 * HOUR)) instants += at + d
                instants += at + rng.nextLong(-2 * DAY, 2 * DAY)
                t = tr.instant
            }
            repeat(3) { instants += rng.nextLong(Instant.parse("2025-01-01T00:00:00Z").toEpochMilli(), end.toEpochMilli()) }
            for (now in instants) {
                for (m in minutes) {
                    assertEquals("$id now=${Instant.ofEpochMilli(now)} minute=$m", oracleNextFire(now, m, zone), SessionReminder.nextFireMillis(now, m, zone))
                }
                assertEquals("$id midnight after ${Instant.ofEpochMilli(now)}", oracleNextMidnight(now, zone), WidgetMidnight.nextMidnightMillis(now, zone))
            }
        }
    }

    // ── The real core ────────────────────────────────────────────────────────

    /**
     * Day by day through the real core's plans (lifter, hybrid 10K, under 18,
     * calisthenics), training, moving or skipping each day: every model's surfaces
     * agree with it, and each morning the reminder an older snapshot would post from
     * its days ahead is today's own. `SURFACE_CORE_SEEDS` (4) × `SURFACE_CORE_DAYS` (28).
     */
    @Test
    fun realCoreDaysKeepTheSurfacesInStep() {
        val seeds = System.getenv("SURFACE_CORE_SEEDS")?.toIntOrNull() ?: 4
        val days = System.getenv("SURFACE_CORE_DAYS")?.toIntOrNull() ?: 28
        for (i in 0 until seeds) {
            val seed = SEED_BASE + i
            try {
                seededWalk("core seed=$seed", "SURFACE_SM_SEED=$seed SURFACE_CORE_SEEDS=1", keep = 20) { note -> coreWalk(seed, days, note) }
            } finally {
                Core.resetForTest()
            }
        }
    }

    private fun coreWalk(seed: Long, days: Int, note: (String) -> Unit) {
        val rng = Random(seed)
        Core.resetForTest()
        val base = QaHarness.strengthProfile()
        val (name, profile) = when (Math.floorMod(seed, 4L).toInt()) {
            0 -> "lifter" to base
            1 -> "hybrid 10K" to QaHarness.hybridTenKProfile()
            2 -> "under 18" to base.copy(ageYears = 15.0)
            else -> "calisthenics" to QaHarness.calisthenicsProfile()
        }
        val day0 = 20_400L + rng.nextInt(7)
        Core.send(Event.SetToday(day0))
        Core.send(profile)
        var v = Core.send(Event.GeneratePlan(day0)).view
        note("$name from ${LocalDate.ofEpochDay(day0)}")
        // The snapshot the app last wrote: the app is not opened every day.
        var stored: WidgetSnapshot? = null
        for (d in day0 until day0 + days) {
            // SetToday is the clock only (not persisted): the model of day d whether or
            // not the app is opened.
            v = Core.send(Event.SetToday(d)).view
            val date = LocalDate.ofEpochDay(d)
            note("$date ${date.dayOfWeek}: ns=${v.next_session?.let { "${LocalDate.ofEpochDay(it.epoch_day)} ${it.session_type} ${it.status}" }} blocked=${v.train_blocked}")
            stored?.takeIf { it.epochDay < d && d - it.epochDay <= SessionReminder.AHEAD_MAX_DAYS }?.let { y ->
                val ahead = reminderOf(y, d, "UTC")
                val own = reminderOf(widgetSnapshotOf(v, DistanceUnit.Km, "UTC"), d, "UTC")
                if (own?.let { it.kind to it.title } != ahead?.let { it.kind to it.title }) {
                    val row = v.week_plan.firstOrNull { it.epoch_day == d }
                    // A reminder about a session the day does not have is wrong; no
                    // reminder for a session it has is a missed one.
                    fail(
                        "${if (ahead != null) "WRONG" else "missed"} $name $date ${date.dayOfWeek}: the ${LocalDate.ofEpochDay(y.epochDay)} snapshot reminds " +
                            "${ahead?.let { "${it.kind} '${it.title}'" }}, the day's model ${own?.let { "${it.kind} '${it.title}'" }} " +
                            "(today's row ${row?.session_type} ${row?.status}, ns ${v.next_session?.let { "${LocalDate.ofEpochDay(it.epoch_day)} ${it.session_type}" }})",
                    )
                }
            }
            checkCoreModel(v, d, "morning")
            if (rng.nextInt(100) < SKIP_PERCENT) {
                note("  app not opened")
                continue
            }
            val ns = v.next_session?.takeIf { it.epoch_day == d && it.items.isNotEmpty() }
            val r = rng.nextInt(100)
            when {
                ns == null -> {}
                r < 55 -> {
                    // Train today: every item, or the first few.
                    val items = if (rng.nextInt(3) == 0) ns.items.take(rng.nextInt(1, ns.items.size + 1)) else ns.items
                    v = QaHarness.logPlanItems(d, ns.session_type, items, firstId = d * 1_000) { m, item ->
                        checkCoreModel(m, d, "after logging ${item.exercise}")
                    } ?: v
                    note("  trained ${items.size}/${ns.items.size}")
                }
                r < 65 && ns.move_targets.isNotEmpty() -> {
                    val to = ns.move_targets[rng.nextInt(ns.move_targets.size)]
                    v = Core.send(Event.MoveSession(d, to)).view
                    note("  moved to ${LocalDate.ofEpochDay(to)}")
                }
                r < 68 -> {
                    v = Core.send(Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, d * 86_400L + 30_000L)).view
                    note("  pain report")
                }
                else -> note("  skipped")
            }
            checkCoreModel(v, d, "evening")
            val s = widgetSnapshotOf(v, DistanceUnit.Km, "UTC")
            stored = s
            note("  evening snapshot ${s.kind} done=${s.sessionDone} ahead=${s.ahead.map { "${LocalDate.ofEpochDay(it.epochDay)} ${it.kind}" }} ns=${v.next_session?.let { "${LocalDate.ofEpochDay(it.epoch_day)} ${it.status} items=${it.items.size}" }}")
        }
    }

    /** The surfaces' reading of one real model on day [d]. */
    private fun checkCoreModel(v: ViewModel, d: Long, at: String) {
        val s = widgetSnapshotOf(v, DistanceUnit.Km, "UTC")
        assertEquals("$at: snapshot day", d, s.epochDay)
        val tile = tileFace(s)
        val reminder = reminderOf(s, d, "UTC")
        val target = logSetTarget(v)
        val today = v.todaysPlan()
        if (v.train_blocked) {
            assertEquals("$at: held", WidgetDayKind.Paused, s.kind)
            assertEquals("$at: held tile", TileFace.Log, tile)
            assertNull("$at: held reminder", reminder)
            assertNull("$at: held set editor prefill", target)
            assertTrue("$at: held days ahead", s.ahead.isEmpty())
            return
        }
        if (tile == TileFace.LogSet || reminder?.kind == WidgetDayKind.Lift) {
            assertNotNull("$at: tile $tile / reminder ${reminder?.title} open the set editor with no plan item (today=${today?.session_type} ${today?.status})", target)
        }
        if (reminder?.kind == WidgetDayKind.Lift) {
            val item = (today?.items.orEmpty() + v.resumableLiftDay()?.items.orEmpty()).firstOrNull { it.exercise == target }
            assertTrue("$at: the lift reminder's set editor opens at a done item '$target'", item != null && !item.done)
        }
        if (tile == TileFace.StartRun || reminder?.kind == WidgetDayKind.Run) {
            assertNotNull("$at: Start run with no session today", today)
            assertEquals("$at: Start run on a ${today!!.session_type} day", "Run", sessionDiscipline(today.session_type))
            assertTrue("$at: Start run with no open run", today.items.any { isRunItem(it, today.session_type) && !it.done })
        }
    }

    // ── The state machine ────────────────────────────────────────────────────

    @Test
    fun randomWalksKeepTheSurfacesConsistent() {
        val seeds = System.getenv("SURFACE_SM_SEEDS")?.toIntOrNull() ?: DEFAULT_SEEDS
        val steps = System.getenv("SURFACE_SM_STEPS")?.toIntOrNull() ?: DEFAULT_STEPS
        for (i in 0 until seeds) {
            val seed = SEED_BASE + i
            clean()
            Walk(seed).run(steps)
        }
    }

    private enum class K { Lift, Run, Rest }

    private enum class PlanState { Plan, NoPlan, Setup }

    private class Session(val kind: K, val title: String, val count: Int)

    /** The simulated athlete's plan and state: what each local day truly holds. */
    private class World(private val seed: Long) {
        private val sessions = HashMap<Long, Session>()
        val done = HashMap<Long, Int>()
        val movedFrom = HashMap<Long, Long>()
        var hold = false
        var youth = false
        var plan = PlanState.Plan
        var withheld: String? = null

        fun session(day: Long): Session = sessions.getOrPut(day) {
            val r = Random(seed * 1_000_003 + day)
            val date = LocalDate.ofEpochDay(day)
            when (r.nextInt(10)) {
                in 0..3 -> Session(K.Lift, "Lift $date", 1 + r.nextInt(4))
                in 4..6 -> Session(K.Run, "Run $date", 1)
                else -> Session(K.Rest, "Rest", 0)
            }
        }

        fun doneCount(day: Long) = done[day] ?: 0
        fun open(day: Long) = session(day).kind != K.Rest && doneCount(day) < session(day).count
        val blocked get() = hold || youth || plan != PlanState.Plan

        /** Today's session to remind about, or null. */
        fun openToday(day: Long): Session? = session(day).takeIf { !blocked && open(day) }

        fun weekOf(day: Long): LongRange {
            val monday = LocalDate.ofEpochDay(day).with(DayOfWeek.MONDAY).toEpochDay()
            return monday..monday + 6
        }

        /** Move [day]'s untouched session to a later rest day of its week. */
        fun move(day: Long, rng: Random): Boolean {
            val s = session(day)
            if (s.kind == K.Rest || doneCount(day) > 0) return false
            val targets = weekOf(day).filter { it > day && session(it).kind == K.Rest }
            if (targets.isEmpty()) return false
            val to = targets[rng.nextInt(targets.size)]
            sessions[to] = s
            sessions[day] = Session(K.Rest, "Rest", 0)
            movedFrom[to] = day
            return true
        }

        private fun items(day: Long): List<PrescriptionView> {
            val s = session(day)
            val n = doneCount(day)
            return List(s.count) { i ->
                val done = i < n
                when (s.kind) {
                    K.Run -> PrescriptionView(
                        run_head = "8 km", run_head_km = 8.0,
                        run_target = RunTargetView(pace_label_km = "5:40–6:00 /km"), done = done,
                    )
                    else -> PrescriptionView(
                        exercise = LIFTS[i], sets = 3, face_lead = "3 × 5 @ 100 kg", done = done, sets_done = if (done) 3 else 0,
                    )
                }
            }
        }

        private fun row(d: Long, today: Long, next: Boolean): SessionPlanView {
            val s = session(d)
            if (s.kind == K.Rest) return SessionPlanView(d, "Rest", "Rest", "rest")
            val items = items(d)
            val all = items.all { it.done }
            val status = when {
                next -> "next"
                d < today -> if (all) "done" else if (items.any { it.done }) "partial" else "missed"
                d == today && all -> "done"
                else -> "planned"
            }
            val type = if (s.kind == K.Run) "Run(Easy)" else "Lift(Strength)"
            return SessionPlanView(d, s.title, type, status, items, moved_from_epoch_day = movedFrom[d])
        }

        /** The model the core would publish with [today] as its day. */
        fun model(today: Long): ViewModel {
            if (plan == PlanState.Setup) return ViewModel(today_epoch_day = today)
            val profile = ProfileView()
            val noPlan = plan == PlanState.NoPlan
            val nsDay = if (noPlan) null else (today..today + 13).firstOrNull { session(it).kind != K.Rest && (it > today || open(it)) }
            val ns = nsDay?.let { row(it, today, next = true) }
            val week = if (noPlan) emptyList() else weekOf(today).map { d -> if (d == nsDay) ns!! else row(d, today, next = false) }
            // A hold (or the under-18 pause) wins over the plan, as in the core.
            if (hold || youth) {
                fun blank(s: SessionPlanView) = s.copy(status = "blocked", items = emptyList())
                return ViewModel(
                    today_epoch_day = today, profile = profile,
                    hold_reason = if (youth) "youth" else "pain", next_session = ns?.let(::blank), week_plan = week.map(::blank),
                )
            }
            if (noPlan) return ViewModel(today_epoch_day = today, profile = profile, plan_withheld = withheld)
            return ViewModel(today_epoch_day = today, profile = profile, next_session = ns, week_plan = week)
        }

        fun describe(day: Long) =
            "day ${LocalDate.ofEpochDay(day)}: ${session(day).kind} '${session(day).title}' done ${doneCount(day)}/${session(day).count}" +
                " hold=$hold youth=$youth plan=$plan"
    }

    private inner class Walk(private val seed: Long) {
        private val rng = Random(seed)
        private val world = World(seed)
        private var note: (String) -> Unit = {}
        private val tile = Robolectric.buildService(QuickTileService::class.java).create().get()

        // The app process: alive with the model it last published, or gone.
        private var alive = false
        private var held: ViewModel? = null

        // The reminder as last seen: its object, when it was (re)drawn and for which day.
        private var shown: Notification? = null
        private var drawnAt = 0L
        private var shownFor = 0L
        // Local days the alarm posted a new reminder on: at most one per day.
        private val postedDays = HashSet<Long>()
        // Local days the alarm posted a reminder for: it posts at most once per day.
        private val alarmDays = HashSet<Long>()

        private val now get() = AppClock.nowMillis()
        private val today get() = todayEpochDay()
        private val zone get() = ZoneId.systemDefault()

        fun run(steps: Int) = seededWalk("seed=$seed", "SURFACE_SM_SEED=$seed SURFACE_SM_SEEDS=1") { n ->
            note = n
            start()
            check("start")
            for (i in 0 until steps) {
                val op = step()
                note("#$i $op @ ${Instant.ofEpochMilli(now).atZone(zone)}")
                check(op)
            }
        }

        private fun setNow(t: Long) {
            AppClock.offsetMillis = t - System.currentTimeMillis()
            expire()
        }

        private fun start() {
            TimeZone.setDefault(TimeZone.getTimeZone(ZONES[rng.nextInt(ZONES.size)]))
            val year = Instant.parse("2026-01-01T00:00:00Z").toEpochMilli()
            var t = year + rng.nextLong(2 * 365 * DAY)
            if (rng.nextBoolean()) zone.rules.nextTransition(Instant.ofEpochMilli(t))?.let { t = it.instant.toEpochMilli() - rng.nextLong(3 * DAY) }
            setNow(t + rng.nextLong(1_000, 59_000))
            world.youth = rng.nextInt(12) == 0
            if (rng.nextInt(10) == 0) world.plan = PlanState.NoPlan
            SessionReminder.setMinuteOfDay(app, pickMinute())
            SessionReminder.setEnabled(app, rng.nextInt(5) != 0)
            // The user puts the tile on the panel (no widget is placed).
            tile.onTileAdded()
            note("start zone=${zone.id} at ${Instant.ofEpochMilli(now).atZone(zone)} setting=${SessionReminder.setting(app)}")
        }

        private fun pickMinute(): Int = when (rng.nextInt(6)) {
            0 -> SessionReminder.DEFAULT_MINUTE_OF_DAY
            1 -> 2 * 60 + rng.nextInt(60) // inside the usual spring-forward gap
            2 -> listOf(0, 1, 23 * 60 + 59)[rng.nextInt(3)]
            3 -> Math.floorMod(Instant.ofEpochMilli(now).atZone(zone).let { it.hour * 60 + it.minute } + rng.nextInt(-3, 4), MINUTES_PER_DAY)
            else -> rng.nextInt(MINUTES_PER_DAY)
        }

        // ── Steps ────────────────────────────────────────────────────────────

        private fun step(): String {
            val w = rng.nextInt(100)
            return when {
                w < 12 -> advance(rng.nextLong(MINUTE, 3 * HOUR)).let { "advance $it" }
                w < 22 -> advanceToReminder()
                w < 30 -> advanceToMidnight()
                w < 34 -> advanceToTransition()
                w < 37 -> advance(rng.nextLong(1, 9) * DAY + rng.nextLong(HOUR)).let { "advance days $it" }
                w < 47 -> { publish(); "open app" }
                w < 53 -> logLiftSet()
                w < 57 -> runStart()
                w < 61 -> runStop(save = rng.nextInt(5) != 0)
                w < 63 -> moveToday()
                w < 66 -> setHold(!world.hold)
                w < 67 -> { world.youth = !world.youth; publish(); "youth=${world.youth}" }
                w < 69 -> togglePlan()
                w < 72 -> { val on = !SessionReminder.setting(app).enabled; SessionReminder.setEnabled(app, on); "reminder on=$on" }
                w < 75 -> { val m = pickMinute(); SessionReminder.setMinuteOfDay(app, m); "reminder minute=$m" }
                w < 80 -> changeZone()
                w < 82 -> changeClock()
                w < 85 -> { nm.cancel(SessionReminder.NOTIF_ID); "dismiss reminder" }
                w < 87 -> tapReminder(content = true)
                w < 90 -> tapReminder(content = false)
                w < 94 -> { processDeath(); "process death" }
                w < 95 -> reboot()
                else -> tapTile()
            }
        }

        /** Move the clock forward by [ms], delivering every alarm it passes in order. */
        private fun advance(ms: Long): String {
            var target = now + ms
            while (true) {
                val due = shadowOf(am).scheduledAlarms
                    .filter { (it.type == AlarmManager.RTC || it.type == AlarmManager.RTC_WAKEUP) && it.triggerAtTime <= target }
                    .minByOrNull { it.triggerAtTime } ?: break
                // Inexact alarms come a little late, never early.
                val at = maxOf(now, due.triggerAtTime + rng.nextLong(1_000, 120_000))
                if (at > target) target = at
                setNow(at)
                val op = checkNotNull(due.operation)
                val intent = Intent(shadowOf(op).savedIntent)
                am.cancel(op)
                deliver(intent)
            }
            setNow(target)
            return "${ms / MINUTE} min"
        }

        private fun advanceToReminder(): String {
            val m = SessionReminder.setting(app).minuteOfDay
            val next = oracleNextFire(now, m, zone)
            advance(next - now + rng.nextLong(-5 * MINUTE, 10 * MINUTE).coerceAtLeast(-(next - now) + 1_000))
            return "advance to reminder time"
        }

        private fun advanceToMidnight(): String {
            val next = oracleNextMidnight(now, zone)
            advance(next - now + rng.nextLong(-10 * MINUTE, 30 * MINUTE).coerceAtLeast(-(next - now) + 1_000))
            return "advance to midnight"
        }

        private fun advanceToTransition(): String {
            val tr = zone.rules.nextTransition(Instant.ofEpochMilli(now)) ?: return advance(DAY)
            val to = tr.instant.toEpochMilli() + rng.nextLong(-2 * HOUR, 2 * HOUR)
            if (to - now > 400 * DAY) return advance(DAY)
            advance((to - now).coerceAtLeast(1_000))
            return "advance to transition ${tr}"
        }

        private fun deliver(intent: Intent) {
            when (intent.action) {
                SessionReminder.ACTION_FIRE -> {
                    val expect = expectPost()
                    SessionReminderReceiver().onReceive(app, intent)
                    observe("reminder alarm", fromAlarm = true)
                    if (expect != null) assertNotNull("the reminder alarm posted nothing for ${world.describe(today)}", shown)
                }
                TodayWidgetReceiver.ACTION_MIDNIGHT -> dayChanged(intent)
                else -> fail("unexpected alarm ${intent.action}")
            }
        }

        /** Today's open session when the stored snapshot itself is today's and fresh
         *  and the alarm has not posted a reminder for today's date yet (the reminder
         *  must then post); null when it need not. */
        private fun expectPost(): Session? {
            val s = WidgetSnapshotStore.read(app) ?: return null
            if (!SessionReminder.setting(app).enabled || s.stale || s.epochDay != today || today in alarmDays) return null
            val open = world.openToday(today) ?: return null
            return open.takeIf { !(it.kind == K.Run && RunSession.tracking.value) }
        }

        /** [TodayWidgetReceiver] gets a day-change broadcast. A stale mark that ticks
         *  [WidgetSnapshotStore.staleMarks] makes [app.milestone.widget.WidgetSync]
         *  derive the snapshot again from the model the process holds. */
        private fun dayChanged(intent: Intent) {
            val marks = WidgetSnapshotStore.staleMarks.value
            receive(TodayWidgetReceiver(), intent)
            if (WidgetSnapshotStore.staleMarks.value != marks && alive) writeHeld()
        }

        /** [receiver] gets [intent] as from the system (with a pending result for
         *  goAsync); returns once it finished its work. */
        private fun receive(receiver: BroadcastReceiver, intent: Intent) {
            val pending = ReflectionHelpers.callStaticMethod<BroadcastReceiver.PendingResult>(
                ShadowBroadcastPendingResult::class.java, "create",
                ClassParameter.from(Int::class.javaPrimitiveType, 0), ClassParameter.from(String::class.java, null),
                ClassParameter.from(Bundle::class.java, null), ClassParameter.from(Boolean::class.javaPrimitiveType, false),
            )
            ReflectionHelpers.callInstanceMethod<Any?>(receiver, "setPendingResult", ClassParameter.from(BroadcastReceiver.PendingResult::class.java, pending))
            receiver.onReceive(app, intent)
            Shadow.extract<ShadowBroadcastPendingResult>(pending).future.get(10, TimeUnit.SECONDS)
        }

        private fun writeHeld() {
            val m = held ?: return
            WidgetSnapshotStore.write(app, widgetSnapshotOf(m, DistanceUnit.Km, zone.id))
            observe("snapshot write")
        }

        /** The app comes to the front: SetToday, then the model is published. */
        private fun publish() {
            if (!alive) RecordingWatch.start(app)
            alive = true
            held = world.model(today)
            writeHeld()
        }

        private fun logLiftSet(): String {
            val s = world.session(today)
            if (world.blocked || s.kind != K.Lift || !world.open(today)) { publish(); return "open app (no lift to log)" }
            world.done[today] = world.doneCount(today) + 1
            publish()
            return "log lift set ${world.doneCount(today)}/${s.count}"
        }

        private fun runStart(): String {
            publish()
            RunSession.tracking.value = true
            shadowOf(Looper.getMainLooper()).idle()
            return "run start"
        }

        private fun runStop(save: Boolean): String {
            if (!RunSession.tracking.value) return "run stop (none)"
            RunSession.tracking.value = false
            if (save && !world.blocked && world.session(today).kind == K.Run) world.done[today] = world.session(today).count
            publish()
            return "run stop save=$save"
        }

        private fun moveToday(): String {
            val moved = !world.blocked && world.move(today, rng)
            publish()
            return "move today's session moved=$moved"
        }

        private fun setHold(on: Boolean): String {
            world.hold = on
            publish()
            return "hold=$on"
        }

        private fun togglePlan(): String {
            world.plan = when (world.plan) {
                PlanState.Plan -> if (rng.nextInt(4) == 0) PlanState.Setup else PlanState.NoPlan
                else -> PlanState.Plan
            }
            world.withheld = if (rng.nextBoolean()) null else "nothing_to_plan"
            publish()
            return "plan=${world.plan}"
        }

        private fun changeZone(): String {
            val to = ZONES[rng.nextInt(ZONES.size)]
            TimeZone.setDefault(TimeZone.getTimeZone(to))
            val changed = Intent(Intent.ACTION_TIMEZONE_CHANGED)
            SessionReminderReceiver().onReceive(app, changed)
            dayChanged(changed)
            return "zone -> $to"
        }

        private fun changeClock(): String {
            val jump = rng.nextLong(-30 * HOUR, 30 * HOUR)
            if (jump > 0) {
                advance(jump)
            } else {
                setNow(now + jump)
            }
            val changed = Intent(Intent.ACTION_TIME_CHANGED)
            SessionReminderReceiver().onReceive(app, changed)
            dayChanged(changed)
            return "clock ${jump / MINUTE} min"
        }

        private fun tapReminder(content: Boolean): String {
            val n = shown ?: return "tap reminder (none)"
            val pi = if (content) n.contentIntent else n.actions.single().actionIntent
            val dest = openDestOf(shadowOf(pi).savedIntent)
            // Auto-cancel covers a tap on the notification itself, not on its action.
            if (content) nm.cancel(SessionReminder.NOTIF_ID)
            publish()
            return "tap reminder ${if (content) "content" else "action"} -> $dest"
        }

        private fun processDeath() {
            alive = false
            held = null
            RunSession.reset()
        }

        private fun reboot(): String {
            processDeath()
            nm.cancelAll()
            shadowOf(am).scheduledAlarms.toList().forEach { it.operation?.let(am::cancel) }
            SessionReminderReceiver().onReceive(app, Intent(if (rng.nextBoolean()) Intent.ACTION_BOOT_COMPLETED else Intent.ACTION_MY_PACKAGE_REPLACED))
            // System UI draws the tile again (the panel is opened).
            tile.onStartListening()
            return "reboot"
        }

        private fun tapTile(): String {
            val face = QuickTileService.currentFace(app)
            val dest = openDestOf(shadowOf(clickTile()).savedIntent)
            assertEquals("tile $face opened $dest", face.dest, dest)
            return "tile click $face"
        }

        /** Remove a reminder whose timeout ran out (the system does this). */
        private fun expire() {
            val n = shadowOf(nm).getNotification(SessionReminder.NOTIF_ID) ?: return
            if (n === shown && n.timeoutAfter > 0 && now >= drawnAt + n.timeoutAfter) nm.cancel(SessionReminder.NOTIF_ID)
        }

        /** Note a posted or redrawn reminder. Only the alarm posts one: a post that
         *  is new for its local day (nothing shown, or yesterday's still shown) counts
         *  against that day. A redraw (a snapshot for today) makes it today's. */
        private fun observe(why: String, fromAlarm: Boolean = false) {
            val n = shadowOf(nm).getNotification(SessionReminder.NOTIF_ID)
            if (n == null) {
                shown = null
                return
            }
            if (n !== shown) {
                if (!fromAlarm) assertTrue("a reminder appeared without its alarm ($why)", shown != null)
                if (fromAlarm) alarmDays += today
                if (fromAlarm && (shown == null || shownFor != today)) {
                    assertTrue("a second reminder on ${LocalDate.ofEpochDay(today)} ($why)", postedDays.add(today))
                }
                shownFor = today
                drawnAt = now
            }
            shown = n
        }

        // ── Invariants ───────────────────────────────────────────────────────

        private fun check(op: String) {
            observe(op)
            val setting = SessionReminder.setting(app)
            val alarms = shadowOf(am).scheduledAlarms
            fun withAction(a: String) = alarms.filter { it.operation?.let { pi -> shadowOf(pi).savedIntent.action } == a }

            // The reminder alarm: only while on, at the next valid local time.
            val reminderAlarms = withAction(SessionReminder.ACTION_FIRE)
            if (setting.enabled) {
                val a = reminderAlarms.singleOrNull() ?: throw AssertionError("reminder on, alarms: ${reminderAlarms.size}")
                assertEquals(
                    "reminder alarm for minute ${setting.minuteOfDay} in ${zone.id} after ${Instant.ofEpochMilli(now).atZone(zone)} " +
                        "was ${Instant.ofEpochMilli(a.triggerAtTime).atZone(zone)}",
                    oracleNextFire(now, setting.minuteOfDay, zone), a.triggerAtTime,
                )
            } else {
                assertTrue("reminder off but ${reminderAlarms.size} alarms", reminderAlarms.isEmpty())
                assertNull("reminder off but shown", shown)
            }

            // The midnight tick: the start of the next local day.
            val tick = withAction(TodayWidgetReceiver.ACTION_MIDNIGHT).singleOrNull() ?: throw AssertionError("no single midnight tick")
            assertEquals("midnight tick in ${zone.id}", oracleNextMidnight(now, zone), tick.triggerAtTime)

            // The snapshot describes today and is not marked stale. While the app
            // process holds today's model, the snapshot it wrote must be that one.
            val stored = WidgetSnapshotStore.read(app)
            val fresh = stored != null && !stored.stale && stored.epochDay == today
            if (held?.today_epoch_day == today) {
                assertTrue("the app holds today's model but the snapshot is ${stored?.let { "${LocalDate.ofEpochDay(it.epochDay)} stale=${it.stale}" }}", fresh)
            }

            checkReminder()
            checkWidget(stored, fresh)
            checkTile(fresh)
        }

        private fun checkReminder() {
            val n = shown ?: return
            if (shownFor != today) {
                fail(
                    "the reminder for ${LocalDate.ofEpochDay(shownFor)} is still up on ${LocalDate.ofEpochDay(today)}: drawn " +
                        "${Instant.ofEpochMilli(drawnAt).atZone(zone)}, timeout ${n.timeoutAfter} ms, '${n.extras.getCharSequence(Notification.EXTRA_TITLE)}'",
                )
            }
            val open = world.openToday(today) ?: throw AssertionError("a reminder on a day with nothing open: ${world.describe(today)}")
            assertEquals("reminder title", open.title, n.extras.getCharSequence(Notification.EXTRA_TITLE)?.toString())
            val action = n.actions.single()
            val dest = openDestOf(shadowOf(action.actionIntent).savedIntent)
            assertEquals("reminder action dest for ${open.kind}", if (open.kind == K.Run) OpenDest.Track else OpenDest.LogSet, dest)
            assertEquals(OpenDest.Today, openDestOf(shadowOf(n.contentIntent).savedIntent))
            val label = when {
                open.kind == K.Run -> R.string.reminder_action_start_run
                world.doneCount(today) > 0 -> R.string.reminder_action_log_next_set
                else -> R.string.reminder_action_log_first_set
            }
            assertEquals(res.getString(label), action.title.toString())
            if (open.kind == K.Run) assertTrue("'Start run' reminder while a run records", !RunSession.tracking.value)
        }

        private fun checkWidget(stored: WidgetSnapshot?, fresh: Boolean) {
            val face = widgetFace(stored, today, res)
            assertEquals("widget reads 'Open lollop to update' (fresh=$fresh)", !fresh, face.tone == WidgetTone.Muted)
            if (!fresh) return
            assertTrue("overline ${face.overline} names today", face.overline.contains(planDateLabel(today)))
            val truth = world.session(today)
            val expected = when {
                world.plan == PlanState.Setup -> res.getString(R.string.widget_setup_title)
                world.hold || world.youth -> res.getString(R.string.main_status_on_hold)
                world.plan == PlanState.NoPlan ->
                    res.getString(if (world.withheld == "nothing_to_plan") R.string.main_plan_withheld_empty_title else R.string.widget_no_plan)
                truth.kind == K.Rest -> res.getString(R.string.widget_rest_day)
                else -> truth.title
            }
            assertEquals("widget title for ${world.describe(today)}", expected, face.title)
        }

        private fun checkTile(fresh: Boolean) {
            val face = QuickTileService.currentFace(app)
            if (RunSession.tracking.value) {
                assertEquals(TileFace.Recording, face)
                return
            }
            val open = world.openToday(today)
            val expected = when {
                !fresh || open == null -> TileFace.Log
                open.kind == K.Run -> TileFace.StartRun
                else -> TileFace.LogSet
            }
            assertEquals("tile for ${world.describe(today)} fresh=$fresh", expected, face)
        }
    }

    companion object {
        private const val MINUTE = 60_000L
        private const val HOUR = 60 * MINUTE
        private const val DAY = 24 * HOUR
        private const val MINUTES_PER_DAY = 24 * 60
        private const val DEFAULT_SEEDS = 12
        private const val DEFAULT_STEPS = 150
        private val SEED_BASE = System.getenv("SURFACE_SM_SEED")?.toLongOrNull() ?: 20_261_030L
        private val LIFTS = listOf("Back squat", "Bench press", "Row", "Deadlift")

        /** Share of days the app is not opened in the real-core walk. */
        private const val SKIP_PERCENT = 25

        /** DST at 02:00/01:00 (Europe, US), at midnight (Santiago, Havana), a half-hour
         *  shift (Lord Howe), +12:45 with DST (Chatham), no DST, both sides of the date line. */
        private val ZONES = listOf(
            "Europe/Berlin", "Europe/London", "America/New_York", "America/Santiago", "America/Havana",
            "Australia/Lord_Howe", "Pacific/Chatham", "Asia/Kolkata", "UTC", "Pacific/Kiritimati",
            "Pacific/Pago_Pago", "America/St_Johns", "Africa/Casablanca",
        )

        /** The first instant strictly after [now] whose local time in [zone] is
         *  [minute]: on a day the time is skipped, that time shifted by the gap; on
         *  a day it repeats, its first pass. */
        private fun oracleNextFire(now: Long, minute: Int, zone: ZoneId): Long {
            var date = Instant.ofEpochMilli(now).atZone(zone).toLocalDate()
            while (true) {
                val ldt = LocalDateTime.of(date, LocalTime.of(minute / 60, minute % 60))
                val offsets = zone.rules.getValidOffsets(ldt)
                val at = if (offsets.isEmpty()) {
                    val tr = zone.rules.getTransition(ldt)
                    ldt.plus(tr.duration).toInstant(tr.offsetAfter)
                } else {
                    offsets.minOf { ldt.toInstant(it) }
                }.toEpochMilli()
                if (at > now) return at
                date = date.plusDays(1)
            }
        }

        /** The first instant of the local day after [now]'s (the end of the gap when
         *  midnight is skipped). */
        private fun oracleNextMidnight(now: Long, zone: ZoneId): Long {
            val ldt = Instant.ofEpochMilli(now).atZone(zone).toLocalDate().plusDays(1).atStartOfDay()
            val offsets = zone.rules.getValidOffsets(ldt)
            return if (offsets.isEmpty()) {
                zone.rules.getTransition(ldt).instant.toEpochMilli()
            } else {
                offsets.minOf { ldt.toInstant(it) }.toEpochMilli()
            }
        }
    }
}
