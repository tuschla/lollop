package app.milestone.qa

import android.app.Application
import android.content.Context
import android.provider.Settings
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.RestAlarm
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowAlarmManager

/**
 * The set loop's exact-alarm notice over the real core (SDK 34, exact alarms not
 * allowed, as on a new Android 14+ install): it shows after the first logged set
 * starts a rest, "Dismiss" ends it for good, and "Allow exact alarms" opens the
 * system screen for this app.
 */
@RunWith(RobolectricTestRunner::class)
class ExactAlarmNoticeTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val notice = hasTestTag("exact-alarm-notice")

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
        ShadowAlarmManager.setCanScheduleExactAlarms(false)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private fun noticeShown() = rule.onAllNodes(notice).fetchSemanticsNodes().isNotEmpty()

    /** Open today's first plan item in the set loop; returns its planned set count. */
    private fun openLoop(): Int {
        QaHarness.seedLog(
            ctx,
            listOf(
                QaHarness.strengthProfile(),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(6)),
                QaHarness.loggedSquat(QaHarness.daysAgoAtMidday(4)),
            ),
        )
        rule.launchApp()
        val first = EventDispatcher.model.value!!.next_session!!.items.first { it.exercise.isNotEmpty() }
        rule.onNode(
            hasClickAction() and hasContentDescription(strPrefix(R.string.plan_log_set_n_of_n, 1), substring = true) and
                hasAnyAncestor(hasTestTag("plan-item:${first.exercise}")),
        ).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("editor-primary")).fetchSemanticsNodes().isNotEmpty() }
        return first.sets
    }

    private fun logSet(n: Int, of: Int) {
        // The lifter rests first (this also clears the loop's double-tap guard).
        if (n > 1) AppClock.offsetMillis += 90_000L
        rule.onNode(hasTestTag("editor-primary")).performClick()
        rule.waitUntil(10_000) {
            rule.onAllNodes(hasText(str(R.string.log_set_header_n_of_n, n + 1, of))).fetchSemanticsNodes().isNotEmpty()
        }
        assertNotNull("the set started a rest", RestAlarm.running.value)
        rule.waitForIdle()
    }

    @Test
    fun shownAfterTheFirstRestAndNotAgainOnceDismissed() {
        val sets = openLoop()
        assertEquals("no notice before a rest", false, noticeShown())
        logSet(1, sets)
        assertEquals("notice after the first rest", true, noticeShown())
        rule.onNode(hasText(str(R.string.log_exact_alarm_caption))).assertExists()
        rule.onNode(hasText(str(R.string.log_exact_alarm_dismiss)) and hasClickAction()).performClick()
        rule.waitForIdle()
        assertEquals("gone once dismissed", false, noticeShown())
        logSet(2, sets)
        assertEquals("not shown again", false, noticeShown())
    }

    @Test
    fun allowOpensTheSystemScreenForThisApp() {
        val sets = openLoop()
        logSet(1, sets)
        rule.onNode(hasText(str(R.string.log_exact_alarm_allow)) and hasClickAction()).performClick()
        rule.waitForIdle()
        val started = shadowOf(ctx as Application).nextStartedActivity
        assertEquals(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, started.action)
        assertEquals("package:${ctx.packageName}", started.dataString)
        assertEquals("gone once used", false, noticeShown())
    }
}
