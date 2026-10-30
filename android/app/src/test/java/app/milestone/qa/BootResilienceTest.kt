package app.milestone.qa

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import app.milestone.ActiveRunStore
import app.milestone.AppClock
import app.milestone.AppSettings
import app.milestone.Core
import app.milestone.CoreFault
import app.milestone.EventDispatcher
import app.milestone.GpsPoint
import app.milestone.R
import app.milestone.RunSession
import app.milestone.RunTrackingService
import app.milestone.exportDataTo
import java.io.File
import java.util.Locale
import kotlinx.coroutines.launch
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/**
 * Boot resilience:
 *
 *  - A run resumed from the crash-recovery prompt after its location grant was
 *    revoked must not fire a recording start it can't satisfy; it waits for the
 *    grant, offers Save/Discard meanwhile, and restarts recording once granted.
 *  - A native core that can't answer (every JNI call null, a library that never
 *    loaded, a core that refuses the boot clock) must boot into a clear error
 *    state: no "Set up your training", no event-log writes, the raw log still
 *    exportable.
 */
@RunWith(RobolectricTestRunner::class)
class BootResilienceTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val app get() = ctx as Application
    private val logFile get() = File(ctx.filesDir, "event-log.ndjson")

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        clearSidecar()
        RunSession.reset()
        shadowOf(app).grantPermissions(
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.ACCESS_COARSE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS,
        )
        AppSettings.setShowRouteMaps(ctx, false)
    }

    @After
    fun tearDown() {
        Core.faultForTest = null
        Core.clearUnavailableForTest()
        QaHarness.teardown()
        ProcessDeath.teardown()
        RunSession.reset()
        clearSidecar()
        AppSettings.showRouteMaps.value = true
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
    }

    private fun clearSidecar() {
        QaHarness.resetActiveRunStore(ctx)
        ActiveRunStore.clearSync()
        File(ctx.filesDir, "active-run.ndjson").delete()
    }

    private fun count(text: String) = rule.onAllNodes(hasText(text, substring = true)).fetchSemanticsNodes().size

    private fun waitForText(text: String, timeoutMs: Long = 15_000) = rule.waitForState(timeoutMs) { count(text) > 0 }

    /** Every service start the app issued since the last drain, by action. */
    private fun drainStartedServiceActions(): List<String?> {
        val out = ArrayList<String?>()
        while (true) out += (shadowOf(app).nextStartedService ?: break).action
        return out
    }

    private fun fix(northM: Double, t: Long) = GpsPoint(52.52 + northM / 111_195.0, 13.405, t, 5.0)

    // ── B1: resume after a location revoke ──────────────────────────────────

    /** A live run's sidecar, then process death and a location revoke (the
     *  platform kills the process on revoke). */
    private fun interruptedRunWithLocationRevoked(): Int {
        // The resume gate ages the sidecar against the wall clock, so run on it.
        AppClock.offsetMillis = 0
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        val t0 = System.currentTimeMillis() / 1000 - 600
        val pts = (0 until 60).map { fix(3.0 * it, t0 + it) }
        ActiveRunStore.begin(ctx)
        pts.forEach { p -> if (RunSession.add(p, (5_000L + (p.observedAt - t0) * 1000L) * 1_000_000L)) ActiveRunStore.append(p) }
        RunSession.tracking.value = true
        ProcessDeath.kill(ctx)
        shadowOf(app).denyPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        return pts.size
    }

    private fun resumeFromPrompt() {
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_resume_run_title), 1)
        drainStartedServiceActions()
        rule.onNode(hasText(str(R.string.main_resume)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
    }

    @Test
    fun resumingWithoutLocationWaitsForTheGrantThenRestartsRecording() {
        val n = interruptedRunWithLocationRevoked()
        resumeFromPrompt()
        rule.waitForState { RunSession.resumeAwaitingLocation.value && RunSession.points.value.size == n }
        waitForText(str(R.string.track_resume_perm_caption))
        assertFalse("nothing records before the grant", RunSession.tracking.value)
        assertEquals("no recording start without a grant", emptyList<String?>(), drainStartedServiceActions())
        // The run so far stays savable while it waits.
        assertTrue(count(str(R.string.track_btn_save_run)) > 0)

        // The user allows location from the tracker's rationale (Continue).
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        rule.onNode(hasText(str(R.string.track_btn_continue)) and hasClickAction()).performClick()
        rule.waitForState { "app.milestone.action.START" in drainStartedServiceActions() }

        // The service the platform would start for that intent records again: the
        // crash-resume start path, with the restored fixes kept.
        Robolectric.buildService(RunTrackingService::class.java, Intent(ctx, RunTrackingService::class.java).setAction("app.milestone.action.START"))
            .create()
            .startCommand(0, 1)
        rule.waitForState { RunSession.tracking.value && !RunSession.resumeAwaitingLocation.value }
        waitForText(str(R.string.track_btn_stop))
        assertEquals(n, RunSession.points.value.size)
        assertFalse(RunSession.locationFailed.value)
    }

    @Test
    fun aDenialKeepsTheResumedRunSavableNeverSilentlyDiscarded() {
        val n = interruptedRunWithLocationRevoked()
        resumeFromPrompt()
        waitForText(str(R.string.track_resume_perm_caption))
        // ✕ on the waiting run asks before deleting anything.
        rule.onNode(androidx.compose.ui.test.hasContentDescription(str(R.string.track_cd_discard_run)) and hasClickAction()).performClick()
        rule.waitForState { rule.onAllNodes(isDialog()).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(n, RunSession.points.value.size)
        assertTrue("the sidecar survives the unconfirmed ✕", File(ctx.filesDir, "active-run.ndjson").length() > 0)
        rule.onNode(hasText(str(R.string.main_cancel)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
        // Save keeps what was recorded before the revoke.
        rule.waitForState { rule.onAllNodes(isDialog()).fetchSemanticsNodes().isEmpty() }
        rule.onNode(hasText(str(R.string.track_btn_save_run)) and hasClickAction()).performClick()
        // The core publishes the saved run before the save's IO block clears the
        // sidecar and resets the session (runSaveCriticalSection), so wait for the
        // reset too: it releases the Save claim after clearing the resume flag.
        rule.waitForState { EventDispatcher.model.value?.runs?.size == 1 && !RunSession.saving.value }
        assertEquals(1, logFile.readLines().count { "LogRunTrack" in it })
        assertFalse(RunSession.resumeAwaitingLocation.value)
    }

    // ── Core unavailable at boot ────────────────────────────────────────────

    private val seeded = listOf(
        QaHarness.strengthProfile(),
        QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(2)),
        QaHarness.loggedRun(QaHarness.daysAgoAtMidday(1)),
    )

    /** Boot on [fault] over a user's log and assert the error state: no setup, no
     *  scaffold, the log byte-identical, nothing hydrated. */
    private fun bootsIntoCoreUnavailable(fault: CoreFault) {
        QaHarness.seedLog(ctx, seeded)
        val before = logFile.readBytes()
        Core.faultForTest = fault
        lastScenario = launchMain()
        waitForText(str(R.string.main_core_unavailable_title), 30_000)
        rule.waitForIdle()
        assertTrue(Core.unavailable)
        assertEquals(0, count(str(R.string.setup_step_train_q)))
        assertEquals(0, count(str(R.string.main_dest_history)))
        assertTrue(count(str(R.string.profile_export_data)) > 0)
        assertNull("no model hydrated from a dead core", EventDispatcher.model.value)
        assertArrayEquals("the event log is untouched", before, logFile.readBytes())

        // Even an event that reaches the dispatcher can't append: the core refuses
        // every event once it is unavailable, whatever the library would answer.
        Core.faultForTest = object : CoreFault {
            override fun update(event: ByteArray): ByteArray = "[]".toByteArray()
            override fun view(): ByteArray? = null
        }
        EventDispatcher.send(QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(0)))
        val r = Core.send(QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(0)))
        assertFalse(r.accepted)
        rule.waitForIdle()
        assertArrayEquals("no write after boot either", before, logFile.readBytes())

        // The raw log can still leave the phone.
        val out = File(ctx.cacheDir, "export-${System.nanoTime()}.ndjson")
        // Off the test (main) thread's stack: the export's exclusive turn runs on the
        // main-dispatched consumer, so blocking main on it would deadlock.
        var exported: Boolean? = null
        EventDispatcher.scope.launch { exported = exportDataTo(ctx, Uri.fromFile(out)) }
        rule.waitForState(30_000) { exported != null }
        assertEquals("export works without the core", true, exported)
        assertEquals(seeded.map { it.toJson().toString() }, out.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun aCoreAnsweringNullBootsIntoTheErrorStateNotSetup() = bootsIntoCoreUnavailable(
        object : CoreFault {
            override fun update(event: ByteArray): ByteArray? = null
            override fun view(): ByteArray? = null
        },
    )

    @Test
    fun aLibraryThatNeverLoadedBootsIntoTheErrorState() = bootsIntoCoreUnavailable(
        object : CoreFault {
            override fun update(event: ByteArray): ByteArray? = throw UnsatisfiedLinkError("libshared.so not found")
            override fun view(): ByteArray? = throw UnsatisfiedLinkError("libshared.so not found")
        },
    )

    @Test
    fun aCoreThatRefusesTheBootClockBootsIntoTheErrorState() = bootsIntoCoreUnavailable(
        object : CoreFault {
            // A view that decodes, but every event rejected (empty response).
            override fun update(event: ByteArray): ByteArray = ByteArray(0)
            override fun view(): ByteArray = "{}".toByteArray()
        },
    )

    @Test
    fun aWorkingCoreStillBootsNormally() {
        QaHarness.seedLog(ctx, seeded)
        rule.launchApp()
        assertFalse(Core.unavailable)
        assertFalse(EventDispatcher.coreUnavailable.value)
        assertEquals(1, EventDispatcher.model.value?.runs?.size)
        assertEquals(0, count(str(R.string.main_core_unavailable_title)))
    }

    /** Rev3 D4: the boot effect re-runs on every rotation; on the same day it reuses
     *  the published model instead of sending the core a fresh SetToday. */
    @Test
    fun aRotationDoesNotResendTheBootClock() {
        QaHarness.seedLog(ctx, seeded)
        rule.launchApp()
        val lastSetToday = Core.javaClass.getDeclaredField("lastSetToday").apply { isAccessible = true }
        assertTrue("boot sent the clock", lastSetToday.get(Core) != null)
        lastSetToday.set(Core, null)

        checkNotNull(lastScenario).recreate()
        rule.waitForIdle()
        // The re-run's exclusive turn is queued by now; one queued after it settles last.
        var settled = false
        EventDispatcher.scope.launch {
            EventDispatcher.exclusive { null }
            settled = true
        }
        rule.waitForState(30_000) { settled }

        assertNull("no SetToday reached the core on the rotation", lastSetToday.get(Core))
        assertEquals(1, EventDispatcher.model.value?.runs?.size)
    }
}
