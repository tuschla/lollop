package app.milestone

import android.content.Context
import android.net.Uri
import android.os.Looper
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.ProcessDeath
import app.milestone.qa.QaHarness
import app.milestone.qa.launchApp
import app.milestone.qa.launchAppExpectingSetup
import java.io.File
import java.util.Locale
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

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
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        RunSession.reset()
    }

    /** Run [block] on the app's process scope, pumping the paused main looper
     *  (same driver as ImportExportRoundtripTest.onAppScope). */
    @OptIn(ExperimentalCoroutinesApi::class)
    private fun <T> onAppScope(timeoutMs: Long = 30_000, block: suspend () -> T): T {
        val job = EventDispatcher.scope.async { block() }
        val end = System.currentTimeMillis() + timeoutMs
        while (!job.isCompleted) {
            shadowOf(Looper.getMainLooper()).idle()
            if (System.currentTimeMillis() > end) fail("app-scope work did not finish in ${timeoutMs}ms")
            Thread.sleep(2)
        }
        return job.getCompleted()
    }

    private fun logLines(): List<String> =
        File(ctx.filesDir, "event-log.ndjson").readLines().filter { it.isNotBlank() }

    private fun planCleared() = ctx.setupPrefs().getBoolean(KEY_PLAN_CLEARED, false)

    private fun restoreAndRestart(lines: List<String>) {
        assertEquals(DataImportResult.Replaced, onAppScope { replaceEventLog(ctx, lines) })
        ProcessDeath.kill(ctx)
        rule.launchApp()
    }

    /**
     * Kotlin M06a. A log rewritten by an older build lost the final `ClearPlan`
     * with the plan it cleared, so only the device's "plan removed" marker
     * remembers the removal. The backup must still end with a `ClearPlan`
     * (Export.kt exportDataTo doc), else restoring it generates a plan the
     * user had removed (BUGS.md 2026-10-23: "a backup of a removed plan
     * restored a generated plan"). The standing ImportExportRoundtripTest case
     * seeds a log that still holds its `ClearPlan`, where the append is moot.
     */
    @Test
    fun aLegacyLogThatLostItsClearPlanStillBacksUpTheRemovedPlan() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        ctx.setupPrefs().edit().putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        assertNull("the removed plan stays removed", EventDispatcher.model.value!!.program)
        assertNull("the legacy log holds no plan line", lastPlanEvent(logLines()))

        val backup = File(ctx.cacheDir, "backup/legacy.ndjson").apply { parentFile!!.mkdirs() }
        assertTrue(onAppScope { exportDataTo(ctx, Uri.fromFile(backup)) })
        val lines = backup.readLines().filter { it.isNotBlank() }
        assertEquals("the backup records the removal", "ClearPlan", lastPlanEvent(lines))

        // Restored onto a fresh install: still no plan.
        QaHarness.teardown()
        QaHarness.reset(ctx)
        rule.launchAppExpectingSetup()
        restoreAndRestart(lines)
        assertEquals(1, EventDispatcher.model.value!!.lifts.size)
        assertNull("restoring a removed plan must not invent one", EventDispatcher.model.value!!.program)
    }

    /**
     * Kotlin M06b. The "plan removed" marker describes the log, so restoring a
     * backup whose log never removed a plan clears this device's marker
     * (Export.kt replaceEventLog doc: "this device's own marker never outlives
     * its replaced log"), and the next boot plans for the set-up profile as a
     * fresh install with that log would. Boot's own reconcile only re-ARMS the
     * marker from a log ending in `ClearPlan`; nothing else clears a stale one.
     */
    @Test
    fun restoringALogThatNeverRemovedAPlanClearsThisDevicesMarker() {
        // The backup: a set-up profile and a set, never a plan line.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2))))
        val backup = logLines()
        assertNull(lastPlanEvent(backup))

        // This device: the user removed their plan.
        QaHarness.reset(ctx)
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.GeneratePlan(todayEpochDay() - 7), Event.ClearPlan))
        ctx.setupPrefs().edit().putBoolean(KEY_PLAN_CLEARED, true).commit()
        rule.launchApp()
        assertNull(EventDispatcher.model.value!!.program)
        assertTrue(planCleared())

        restoreAndRestart(backup)
        assertFalse("the replaced log's marker is gone", planCleared())
        assertNotNull("a set-up profile without a removed plan is planned", EventDispatcher.model.value!!.program)
    }
}
