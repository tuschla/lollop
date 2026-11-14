package app.milestone

import android.content.Context
import android.net.Uri
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.qa.QaHarness.onAppScope
import app.milestone.qa.launchApp
import app.milestone.qa.launchAppExpectingSetup
import java.io.File
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
 * Guards for behaviours no other test pins (see
 * planning/review/mutation/AUDIT.md). Each test names the mutant of
 * `planning/review/mutation/kotlin/mutate_kt.py` it kills.
 */
@RunWith(RobolectricTestRunner::class)
class MutationGuardsTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
    }

    private fun logLines(): List<String> = QaHarness.logLines(ctx)

    private fun legacyMarker() = ctx.setupPrefs().contains(KEY_PLAN_CLEARED)

    /** The variant of the last `GeneratePlan` / `ClearPlan` line, or null when none. */
    private fun lastPlanEvent(lines: List<String>): String? =
        lines.map(EventLog::variantOf).lastOrNull { it == "GeneratePlan" || it == "ClearPlan" }

    private fun restoreAndRestart(lines: List<String>) {
        assertEquals(DataImportResult.Replaced, onAppScope { replaceEventLog(ctx, lines) })
        QaHarness.killProcess()
        rule.launchApp()
    }

    /**
     * Kotlin M06a. A log rewritten by an older build lost the final `ClearPlan`
     * with the plan it cleared, so only that build's `plan_cleared` pref
     * ([KEY_PLAN_CLEARED]) remembers the removal. Boot's one-time upgrade writes
     * the `ClearPlan` into the log, so the backup records the removal and
     * restoring it does not generate a plan the user had removed (BUGS.md
     * 2026-10-23: "a backup of a removed plan restored a generated plan").
     */
    @Test
    fun aLegacyLogThatLostItsClearPlanStillBacksUpTheRemovedPlan() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        assertNull("the legacy log holds no plan line", lastPlanEvent(logLines()))
        ctx.setupPrefs().edit().putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        assertNull("the removed plan stays removed", EventDispatcher.model.value!!.program)
        assertEquals("the log records the removal", "ClearPlan", lastPlanEvent(logLines()))
        assertFalse("the legacy pref is read once", legacyMarker())

        val backup = File(ctx.cacheDir, "backup/legacy.ndjson").apply { parentFile!!.mkdirs() }
        assertTrue(onAppScope { exportDataTo(ctx, Uri.fromFile(backup)) })
        val lines = backup.readLines().filter { it.isNotBlank() }
        assertEquals("the backup records the removal", "ClearPlan", lastPlanEvent(lines))

        QaHarness.teardown()
        QaHarness.reset(ctx)
        rule.launchAppExpectingSetup()
        restoreAndRestart(lines)
        assertEquals(1, EventDispatcher.model.value!!.lifts.size)
        assertNull("restoring a removed plan must not invent one", EventDispatcher.model.value!!.program)
        assertTrue(EventDispatcher.model.value!!.plan_removed)
    }

    /**
     * Kotlin M06b. Boot removes the legacy `plan_cleared` pref once it has read it:
     * the pref described this device's old log and must not outlive it. Restoring
     * a backup whose log never removed a plan then plans for the set-up profile,
     * as a fresh install with that log would. A pref left in place would remove
     * the restored log's plan on the next boot.
     */
    @Test
    fun restoringALogThatNeverRemovedAPlanPlansDespiteALegacyMarker() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        val backup = logLines()
        assertNull(lastPlanEvent(backup))

        // This device: an older build recorded the removal in the log and the pref.
        QaHarness.reset(ctx)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7), Event.ClearPlan))
        ctx.setupPrefs().edit().putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        assertNull(EventDispatcher.model.value!!.program)
        assertFalse("the legacy pref is read once", legacyMarker())

        restoreAndRestart(backup)
        assertNotNull("a set-up profile without a removed plan is planned", EventDispatcher.model.value!!.program)
        assertFalse(EventDispatcher.model.value!!.plan_removed)
    }
}
