package app.milestone

import android.content.Context
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.ProcessDeath
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.fixture
import app.milestone.qa.QaHarness.onAppScope
import app.milestone.qa.launchApp
import java.io.ByteArrayInputStream
import java.io.File
import java.time.ZoneId
import kotlin.math.abs
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.double
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Upgrading keeps the user's data: the event logs, a data backup and an
 * interrupted run's sidecar written by older builds (shared/tests/fixtures/upgrade,
 * see shared/tests/upgrade_logs.rs for what each holds) boot, import and recover
 * in this build with every History entry the old build showed. The expected
 * entries are the old core's own view (`<sha>.history.json`).
 */
@RunWith(RobolectricTestRunner::class)
class UpgradeFixturesTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val versions = listOf("203ab56", "4728546", "901d239")

    /** The day and offset the fixtures' expected views were taken at. */
    private val fixtureToday = Event.SetToday(20725, 7200)

    @Before
    fun setUp() = QaHarness.reset(ctx)

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
    }

    private fun expected(v: String): JsonObject = Json.parseToJsonElement(fixture("upgrade/$v.history.json").decodeToString()).jsonObject

    private val logFile get() = QaHarness.logFile(ctx)

    /** Install [v]'s log as the file the old build left, as an in-place update does. */
    private fun installOldLog(v: String) {
        logFile.parentFile!!.mkdirs()
        logFile.writeBytes(fixture("upgrade/$v.log.ndjson"))
    }

    /** Boot the core over the log on disk, dated to the fixtures' day. */
    private fun bootCore(): SendResult {
        Core.restore(ctx)
        return Core.send(fixtureToday)
    }

    /** Every History entry [v]'s build listed is in [view], unchanged and in order.
     *  A field the old build's view did not have is not compared. */
    private fun assertSameHistory(what: String, v: String, view: ViewModel) {
        val e = expected(v)
        val lifts = e["lifts"]!!.jsonArray
        assertEquals("$what: sets", lifts.size, view.lifts.size)
        lifts.zip(view.lifts).forEachIndexed { i, (x, l) ->
            val o = x.jsonObject
            val at = "$what: set #$i"
            assertEquals(at, o["entry_id"]!!.jsonPrimitive.long, l.entry_id)
            assertEquals(at, o["exercise"]!!.jsonPrimitive.content, l.exercise)
            assertEquals(at, o["weight_kg"]!!.jsonPrimitive.double, l.weight_kg, 1e-9)
            assertEquals(at, o["reps"]!!.jsonPrimitive.int, l.reps)
            assertEquals(at, o["rpe"]!!.jsonPrimitive.double, l.rpe, 1e-9)
            assertEquals(at, o["observed_at"]!!.jsonPrimitive.long, l.observed_at)
            o["hold_sec"]?.let { assertEquals("$at hold", it.jsonPrimitive.intOrNull, l.hold_sec) }
        }
        val runs = e["runs"]!!.jsonArray
        assertEquals("$what: runs", runs.size, view.runs.size)
        runs.zip(view.runs).forEachIndexed { i, (x, r) ->
            val o = x.jsonObject
            val at = "$what: run #$i"
            assertEquals(at, o["entry_id"]!!.jsonPrimitive.long, r.entry_id)
            assertEquals(at, o["observed_at"]!!.jsonPrimitive.long, r.observed_at)
            val km = o["distance_km"]!!.jsonPrimitive.double
            assertTrue("$at distance $km vs ${r.distance_km}", abs(km - r.distance_km) <= 1e-9 * km.coerceAtLeast(1.0))
            val min = o["duration_min"]!!.jsonPrimitive.double
            assertTrue("$at duration $min vs ${r.duration_min}", abs(min - r.duration_min) <= 1e-9 * min.coerceAtLeast(1.0))
            o["workout_type"]?.let { assertEquals("$at type", it.jsonPrimitive.contentOrNull, r.workout_type) }
            o["session_rpe"]?.let { assertEquals("$at session RPE", it.jsonPrimitive.intOrNull, r.session_rpe) }
            o["avg_hr_bpm"]?.let { assertEquals("$at avg HR", it.jsonPrimitive.doubleOrNull, r.avg_hr_bpm) }
        }
        assertEquals("$what: safety tier", e["safety_tier"]!!.jsonPrimitive.content, view.safety_tier)
        assertEquals("$what: training paused", e["train_blocked"]!!.jsonPrimitive.boolean, view.train_blocked)
        assertNotNull("$what: profile", view.profile)
    }

    @Test
    fun anOldLogBootsWithEveryEntryAndKeepsThemAcrossTheNextBoot() {
        for (v in versions) {
            QaHarness.reset(ctx)
            installOldLog(v)
            val first = bootCore()
            assertTrue("$v: the core takes the boot clock", first.accepted)
            assertFalse("$v: the replay skipped lines", first.stale)
            assertSameHistory("$v first boot", v, first.view)
            assertNotNull("$v: the plan", first.view.program)
            // The first boot may rewrite the log (compaction): the next process reads
            // that rewrite and must still list everything.
            ProcessDeath.kill(ctx)
            val second = bootCore()
            assertFalse("$v: the rewritten log skipped lines", second.stale)
            assertSameHistory("$v after the rewrite", v, second.view)
            assertEquals("$v: the plan after the rewrite", first.view.program, second.view.program)
            assertEquals("$v: the week after the rewrite", first.view.week_plan, second.view.week_plan)
        }
    }

    @Test
    fun anUpdatedInstallOpensOnItsHistoryAndPlanWithoutReSetup() {
        for (v in versions) {
            QaHarness.teardown()
            QaHarness.reset(ctx)
            installOldLog(v)
            // The old build's prefs: setup was done long ago.
            ctx.setupPrefs().edit().putBoolean("onboarding_done", true).commit()
            val before = QaHarness.logLines(ctx)
            rule.launchApp()
            val model = checkNotNull(EventDispatcher.model.value) { "$v: no model after boot" }
            // The app runs on its own (pinned) clock here; the History is clock-free.
            assertEquals("$v: sets", expected(v)["lifts"]!!.jsonArray.size, model.lifts.size)
            assertEquals("$v: runs", expected(v)["runs"]!!.jsonArray.size, model.runs.size)
            assertNotNull("$v: the plan", model.program)
            // Boot must not re-anchor the kept plan with a fresh GeneratePlan.
            val plans = { lines: List<String> -> lines.count { EventLog.variantOf(it) == "GeneratePlan" } }
            assertTrue("$v: boot generated a new plan", plans(QaHarness.logLines(ctx)) <= plans(before))
        }
    }

    /** 203ab56's compaction dropped a removed plan's GeneratePlan together with its
     *  ClearPlan, so after the update only that build's `plan_cleared` pref says the
     *  user removed the plan: boot must not generate a new one. */
    @Test
    fun aPlanRemovedInTheOldBuildStaysRemoved() {
        logFile.parentFile!!.mkdirs()
        logFile.writeBytes(fixture("upgrade/203ab56-plan-removed.log.ndjson"))
        assertTrue("the old rewrite kept no plan line", QaHarness.logLines(ctx).none { EventLog.variantOf(it) in setOf("GeneratePlan", "ClearPlan") })
        ctx.setupPrefs().edit().putBoolean("onboarding_done", true).putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        val model = checkNotNull(EventDispatcher.model.value)
        assertEquals("the set", 1, model.lifts.size)
        assertNotNull("the profile", model.profile)
        assertNull("no plan", model.program)
        assertTrue("boot generated a plan", QaHarness.logLines(ctx).none { EventLog.variantOf(it) == "GeneratePlan" })
    }

    @Test
    fun aBackupExportedByTheOldBuildImportsWithEveryEntry() {
        // Another phone with its own data, about to restore the old backup.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(1))))
        bootCore()
        val backup = fixture("upgrade/4728546.backup.ndjson")
        val result = streamValidateImport(ByteArrayInputStream(backup), MAX_IMPORT_BYTES)
        check(result is ImportCheck.Valid) { "the 4728546 backup is refused: $result" }
        val lines = result.lines
        assertEquals("every backup line is read", backup.decodeToString().lines().count { it.isNotBlank() }, lines.size)
        val summary = summarizeImport(lines, ZoneId.of("Europe/Berlin"))
        val e = expected("4728546")
        // Sets, runs and the activity, plus the 32 check-ins and the 3 readiness
        // reports (a dated RemoveReadiness keeps its submit line: log.rs rule 0).
        val logged = e["lifts"]!!.jsonArray.size + e["runs"]!!.jsonArray.size + e["activities"]!!.jsonArray.size
        assertEquals("entries the confirmation counts", logged + 32 + 3, summary.entries)
        assertEquals("oldest entry day (the legacy set)", 20666L, summary.firstDay)
        assertEquals("newest entry day", 20724L, summary.lastDay)
        assertEquals(DataImportResult.Replaced, onAppScope { replaceEventLog(ctx, lines) })
        assertFalse("the backup's plan is not removed", ctx.setupPrefs().getBoolean(KEY_PLAN_CLEARED, true))
        ProcessDeath.kill(ctx)
        val restored = bootCore()
        assertFalse(restored.stale)
        assertSameHistory("imported 4728546 backup", "4728546", restored.view)
    }

    @Test
    fun anOldInterruptedRunSidecarRecoversEveryIntactFix() {
        val firstFix = 20725L * 86_400 + 7 * 3600 - 7200
        for ((v, starts) in listOf("203ab56" to emptyList(), "4728546" to listOf(25))) {
            QaHarness.resetActiveRunStore(ctx)
            File(ctx.filesDir, "active-run.ndjson").writeBytes(fixture("upgrade/$v.active-run.ndjson"))
            assertTrue("$v: the sidecar marks a run in progress", ActiveRunStore.hasActiveRun(ctx))
            val track = ActiveRunStore.recover(ctx)
            // 40 fixes; the torn last line (killed mid-append) is not one.
            assertEquals("$v: fixes", 40, track.size)
            assertEquals("$v: segment starts", starts, track.segmentStarts)
            assertEquals("$v: first fix", firstFix, track.first().observedAt)
            assertEquals("$v: last fix", firstFix + 24 * 5 + 120 + 14 * 5, track.last().observedAt)
            File(ctx.filesDir, "active-run.ndjson").delete()
        }
    }
}
