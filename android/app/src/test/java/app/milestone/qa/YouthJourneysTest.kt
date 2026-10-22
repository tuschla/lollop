package app.milestone.qa

import android.app.Application
import android.content.Context
import android.content.Intent
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import java.time.LocalDate
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/**
 * The under-18 journey: the plan hold stays, Today shows the WHO weekly activity
 * card instead, other active time is logged from that card and counts at once,
 * and the banner's next step shares a plain-text summary for a coach.
 */
@RunWith(RobolectricTestRunner::class)
class YouthJourneysTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    /** 15 Mar 2011: 15 on the pinned Monday. */
    private val youthBirthday = LocalDate.of(2011, 3, 15).toEpochDay()

    private fun youthProfile(): Event =
        (QaHarness.strengthProfile() as Event.SetProfile).copy(birthEpochDay = youthBirthday, ageYears = 15.0)

    /** Scroll Today's list to [matcher]. */
    private fun scrollTodayTo(matcher: SemanticsMatcher) {
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(matcher)
        rule.waitForIdle()
    }

    @Test
    fun under18_holdStays_activityCardCounts_andTheCoachSummaryCarriesIt() {
        QaHarness.seedLog(ctx, listOf(youthProfile()))
        rule.launchApp()
        // A calm card at the top of Today, never the red DO NOT TRAIN banner,
        // and no SAFETY card repeating it below.
        rule.assertTextCountEventually(str(R.string.main_hold_youth_title).uppercase(Locale.US), 1)
        rule.assertTextCountEventually(str(R.string.main_do_not_train), 0)
        assertEquals("youth", EventDispatcher.model.value!!.hold_reason)
        assertTrue(EventDispatcher.model.value!!.train_blocked)
        rule.assertTextCountEventually(str(R.string.main_hold_reason_youth), 1)
        rule.assertTextCountEventually(str(R.string.main_chip_safety), 0)

        // The weekly card, at zero, where the hero would be.
        rule.waitForState { EventDispatcher.model.value?.youth_activity != null }
        val card = EventDispatcher.model.value!!.youth_activity!!
        assertEquals(0, card.minutes)
        val target = card.weekly_target_minutes
        assertTrue("the weekly target comes from the core", target > 0)
        scrollTodayTo(hasText(str(R.string.activity_week_minutes, 0, target)))
        rule.waitUntilDisplayed(hasText(str(R.string.activity_week_minutes, 0, target)))

        // Log activity from the card: 45 min of sport, vigorous.
        scrollTodayTo(hasText(str(R.string.activity_log_action)) and hasClickAction())
        rule.onNode(hasText(str(R.string.activity_log_action)) and hasClickAction()).performClick()
        // The editor is open (its title shares the card action's label).
        rule.waitUntilDisplayed(hasText(str(R.string.activity_kind_sport)) and hasClickAction())
        rule.tapText(str(R.string.activity_kind_sport))
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.activity_field_minutes), substring = true))
            .performScrollTo().performClick()
        "45".forEach { c ->
            rule.onNode(
                hasClickAction() and (hasText("$c") or hasAnyDescendant(hasText("$c"))) and
                    hasAnyAncestor(hasTestTag("numeric-keypad")),
            ).performScrollTo().performClick()
        }
        rule.onNode(hasText(str(R.string.activity_effort_vigorous)) and hasClickAction()).performScrollTo().performClick()
        rule.onNode(hasText(str(R.string.activity_strengthening)) and hasClickAction()).performScrollTo().performClick()
        rule.onNode(hasTestTag("editor-primary")).performClick()

        // The card counts it at once and lists it with its Remove.
        rule.waitForState { EventDispatcher.model.value?.youth_activity?.minutes == 45 }
        val after = EventDispatcher.model.value!!.youth_activity!!
        assertEquals(1, after.vigorous_days)
        assertEquals(1, after.strengthening_days)
        assertEquals(45, after.activity_minutes)
        assertEquals(1, after.week_entries.size)
        scrollTodayTo(hasText(str(R.string.activity_week_minutes, 45, target)))
        rule.waitUntilDisplayed(hasText(str(R.string.activity_week_minutes, 45, target)))
        // The logged row names the strengthening that counted toward its day.
        val strengthening = str(R.string.activity_entry_line_strengthening, "").trim()
        scrollTodayTo(hasText(strengthening, substring = true))
        rule.waitUntilDisplayed(hasText(strengthening, substring = true))

        // The card's next step: the coach summary, through the share sheet.
        scrollTodayTo(hasText(str(R.string.coach_export_action)) and hasClickAction())
        rule.onNode(hasText(str(R.string.coach_export_action)) and hasClickAction()).performClick()
        rule.waitForIdle()
        val chooser = shadowOf(ApplicationProvider.getApplicationContext<Application>()).nextStartedActivity
        assertNotNull("the share sheet opened", chooser)
        assertEquals(Intent.ACTION_CHOOSER, chooser.action)
        @Suppress("DEPRECATION")
        val send = chooser.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)!!
        assertEquals(Intent.ACTION_SEND, send.action)
        assertEquals("text/plain", send.type)
        assertEquals(str(R.string.coach_export_subject), send.getStringExtra(Intent.EXTRA_TITLE))
        val text = send.getStringExtra(Intent.EXTRA_TEXT).orEmpty()
        val activityLine = str(
            R.string.coach_export_activity,
            str(R.string.activity_kind_sport),
            45,
            str(R.string.activity_effort_vigorous_short),
        )
        assertTrue(text, text.contains(activityLine))
    }

    @Test
    fun under18_profileOffersNoVolumePlannerOrProteinCalculator() {
        QaHarness.seedLog(ctx, listOf(youthProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(str(R.string.main_hold_youth_title).uppercase(Locale.US), 1)
        rule.selectTab(str(R.string.main_dest_profile))
        rule.waitUntilDisplayed(hasText(str(R.string.main_overline_units).uppercase(Locale.US)))
        // No autonomous load prescription for youth (safety-011): neither tool is offered.
        rule.assertTextCountEventually(str(R.string.main_tool_volume_title), 0)
        rule.assertTextCountEventually(str(R.string.main_tool_protein_title), 0)
    }
}
