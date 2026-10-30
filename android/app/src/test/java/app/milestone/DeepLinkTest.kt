package app.milestone

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.qa.lastScenario
import app.milestone.qa.launchMain
import app.milestone.qa.str
import app.milestone.qa.strPrefix
import app.milestone.qa.waitForBoot
import app.milestone.qa.waitForNode
import app.milestone.qa.countOf
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/**
 * The open-intent contract (action `app.milestone.action.OPEN`, extra `dest`) the
 * widget, the Quick Settings tile and the Live Update notifications use: the intent
 * itself, the `log_set` target, and the real app landing on each destination from a
 * cold start and from onNewIntent, including setup and a training hold. The routing
 * from every navigation state is [DeepLinkPropertyTest].
 */
@RunWith(RobolectricTestRunner::class)
class DeepLinkTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    // ── The intent contract ───────────────────────────────────────────────

    @Test
    fun destWireValuesAndUnknownFallBackToToday() {
        assertEquals(listOf("today", "log", "track", "log_set"), OpenDest.entries.map { it.wire })
        for (d in OpenDest.entries) assertEquals(d, OpenDest.fromWire(d.wire))
        assertEquals(OpenDest.Today, OpenDest.fromWire("settings"))
        assertEquals(OpenDest.Today, OpenDest.fromWire(null))
        assertEquals(OpenDest.Today, openDestOf(Intent(ACTION_OPEN).putExtra(EXTRA_DEST, "nope")))
        // Only the open action is a deep link: a launcher or share intent is not.
        assertNull(openDestOf(Intent(Intent.ACTION_MAIN).putExtra(EXTRA_DEST, "log")))
        assertNull(openDestOf(null))
    }

    @Test
    fun openIntentIsExplicitAndCarriesItsDest() {
        for (d in OpenDest.entries) {
            val i = openIntent(ctx, d)
            assertEquals(MainActivity::class.java.name, i.component?.className)
            assertEquals(ctx.packageName, i.component?.packageName)
            assertEquals(ACTION_OPEN, i.action)
            assertEquals(d.wire, i.getStringExtra(EXTRA_DEST))
            assertTrue(i.flags and Intent.FLAG_ACTIVITY_NEW_TASK != 0)
            assertEquals(d, openDestOf(i))
        }
    }

    @Test
    fun pendingIntentsAreImmutableAndOnePerDest() {
        val all = OpenDest.entries.map { openPendingIntent(ctx, it) }
        // Extras do not tell PendingIntents apart; each dest must still keep its own.
        assertEquals(OpenDest.entries.size, all.toSet().size)
        all.zip(OpenDest.entries).forEach { (pi, d) ->
            val s = shadowOf(pi)
            assertTrue(s.isActivityIntent)
            assertEquals(PendingIntent.FLAG_IMMUTABLE, s.flags and PendingIntent.FLAG_IMMUTABLE)
            assertEquals(d.wire, s.savedIntent.getStringExtra(EXTRA_DEST))
        }
        assertEquals(all[1], openPendingIntent(ctx, OpenDest.Log))
        assertNotEquals(all[0], all[1])
    }

    // ── Routing (pure) ────────────────────────────────────────────────────

    private val day = 20_000L
    private fun lift(vararg items: PrescriptionView, status: String = "next", epochDay: Long = day) =
        SessionPlanView(epoch_day = epochDay, title = "Lower", session_type = "LiftStrength", status = status, items = items.toList())

    private val squat = PrescriptionView(exercise = "Back Squat", sets = 3)
    private val bench = PrescriptionView(exercise = "Bench Press", sets = 3)
    private val liftDay = ViewModel(today_epoch_day = day, next_session = lift(squat, bench), week_plan = listOf(lift(squat, bench)))

    @Test
    fun logSetTargetsTodaysOpenLiftItemOnly() {
        assertEquals("Back Squat", logSetTarget(liftDay))
        val squatDone = squat.copy(done = true, sets_done = 3)
        assertEquals("Bench Press", logSetTarget(liftDay.copy(next_session = lift(squatDone, bench))))
        // After Finish the core moves next_session off today: the started session resumes.
        val finished = liftDay.copy(next_session = lift(squat, epochDay = day + 2), week_plan = listOf(lift(squatDone, bench)))
        assertEquals("Bench Press", logSetTarget(finished))
        // A future session, a run day, no plan and a hold open the blank editor.
        assertNull(logSetTarget(liftDay.copy(next_session = lift(squat, epochDay = day + 1), week_plan = emptyList())))
        val run = SessionPlanView(epoch_day = day, session_type = "RunEasy", status = "next", items = listOf(PrescriptionView()))
        assertNull(logSetTarget(ViewModel(today_epoch_day = day, next_session = run, week_plan = listOf(run))))
        assertNull(logSetTarget(ViewModel(today_epoch_day = day)))
        assertNull(logSetTarget(liftDay.copy(hold_reason = "pain")))
    }

    // ── The real app ──────────────────────────────────────────────────────

    private fun launchAt(dest: String?) {
        val intent = Intent(ACTION_OPEN).setClass(ctx, MainActivity::class.java)
        if (dest != null) intent.putExtra(EXTRA_DEST, dest)
        lastScenario = launchMain(intent)
    }

    private val setHeader get() = hasText(strPrefix(R.string.log_set_header_n_of_n, 1), substring = true)
    private val trackerStart get() = hasText(str(R.string.track_btn_start))

    @Test
    fun coldStartLogSetOpensTodaysPlanItem() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        launchAt("log_set")
        rule.waitForNode(setHeader, 30_000)
        val target = logSetTarget(checkNotNull(EventDispatcher.model.value))
        assertNotNull("the pinned Monday is a lift day", target)
        assertTrue(rule.countOf(hasText(target!!, substring = true)) > 0)
    }

    @Test
    fun coldStartTrackOpensTheTrackerBeforeStart() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        grantLocation()
        launchAt("track")
        rule.waitForNode(trackerStart, 30_000)
        assertFalse(RunSession.tracking.value)
    }

    /** With the grant the tracker shows its Start control (without, its permission panel). */
    private fun grantLocation() = shadowOf(ctx as android.app.Application)
        .grantPermissions(android.Manifest.permission.ACCESS_FINE_LOCATION, android.Manifest.permission.ACCESS_COARSE_LOCATION)

    @Test
    fun unknownDestOpensToday() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        launchAt("bogus")
        rule.waitForNode(hasText(str(R.string.main_dest_history)), 30_000)
        rule.waitForBoot()
        assertEquals(0, rule.countOf(setHeader))
        assertEquals(0, rule.countOf(trackerStart))
        assertEquals(0, rule.countOf(chooserPlanRow))
        assertEquals(1, rule.countOf(hasTestTag("log-fab")))
    }

    @Test
    fun beforeSetupTheGuidedSetupStays() {
        launchAt("log_set")
        rule.waitForNode(hasText(str(R.string.setup_step_train_q)), 30_000)
        rule.waitForBoot()
        assertEquals(0, rule.countOf(setHeader))
        assertEquals(0, rule.countOf(hasTestTag("exercise-search")))
        assertTrue(rule.countOf(hasText(str(R.string.setup_step_train_q))) > 0)
    }

    @Test
    fun newIntentWhileRunningOpensTheLogChooser() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        lastScenario = launchMain()
        rule.waitForNode(hasText(str(R.string.main_dest_history)), 30_000)
        rule.waitForBoot()
        assertEquals(0, rule.countOf(chooserPlanRow))
        checkNotNull(lastScenario).onActivity { activity ->
            MainActivity::class.java.getDeclaredMethod("onNewIntent", Intent::class.java)
                .apply { isAccessible = true }
                .invoke(activity, openIntent(activity, OpenDest.Log))
        }
        // The chooser, led by today's plan row.
        rule.waitForNode(chooserPlanRow, 30_000)
        // Applied once: the request is cleared.
        assertEquals(null, readOpenRequest())
    }

    private val chooserPlanRow get() = hasText(strPrefix(R.string.main_todays_plan), substring = true)

    private fun readOpenRequest(): OpenDest? {
        var value: OpenDest? = null
        checkNotNull(lastScenario).onActivity { activity ->
            @Suppress("UNCHECKED_CAST")
            val flow = MainActivity::class.java.getDeclaredField("openRequest").apply { isAccessible = true }
                .get(activity) as kotlinx.coroutines.flow.MutableStateFlow<OpenDest?>
            value = flow.value
        }
        return value
    }

    @Test
    fun duringAHoldLogSetAndTrackStillOpenUnderTheBanner() {
        val now = QaHarness.PINNED_NOW_MILLIS / 1000
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile(), Event.SubmitReadiness(ReadinessSignal.Pain, 1.0, now - 600)))
        launchAt("log_set")
        rule.waitForNode(hasTestTag("exercise-search"), 30_000)
        assertTrue("the hold is active", checkNotNull(EventDispatcher.model.value).train_blocked)
        // Held days carry no items: a blank editor, no plan prefill.
        assertEquals(0, rule.countOf(setHeader))
        assertTrue(rule.countOf(hasTestTag("safety-banner")) > 0)
        QaHarness.teardown()

        grantLocation()
        launchAt("track")
        rule.waitForNode(trackerStart, 30_000)
        assertTrue(rule.countOf(hasTestTag("safety-banner")) > 0)
    }
}
