package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assert
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.HeadlineKind
import app.milestone.HealthAnswer
import app.milestone.HealthScreen
import app.milestone.HoldReason
import app.milestone.ParqQuestion
import app.milestone.ProfileDraft
import app.milestone.R
import app.milestone.draftFacts
import app.milestone.train_blocked
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The owner's Profile journeys: the everyday settings first with the setup
 * answers folded, the hold banners' deep links landing on the answer that causes
 * the hold, and the birthday answer from setup through Profile, including the
 * under-18 hold it raises and clears.
 */
@RunWith(RobolectricTestRunner::class)
class OwnerProfileJourneysTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private val doNotTrain get() = str(R.string.main_do_not_train)

    private fun profileWith(health: HealthScreen = HealthScreen(), birthDay: Long? = null, age: Double = 30.0): Event =
        QaHarness.strengthProfile().copy(health = health, birthEpochDay = birthDay, ageYears = age)

    @Test
    fun profile_everydaySectionsComeFirst_andThePlanAnswersAreFolded() {
        QaHarness.seedLog(ctx, listOf(profileWith(birthDay = adultBirthday, age = 35.0)))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        fun header(res: Int) = str(res).uppercase(Locale.US)
        val everyday = listOf(
            R.string.main_overline_calculators, R.string.main_overline_units, R.string.main_appearance,
            R.string.main_overline_during_training, R.string.main_overline_your_data, R.string.main_overline_evidence,
        ).map(::header)
        val planAnswers = header(R.string.main_overline_plan_answers)
        rule.onNode(hasText(everyday.first())).assertIsDisplayed()
        val seen = mutableListOf<String>()
        val headers = everyday + planAnswers + header(R.string.main_overline_danger_zone)
        val list = rule.onAllNodes(hasScrollToNodeAction())[0]
        val rows = list.fetchSemanticsNode().config[SemanticsProperties.CollectionInfo].rowCount
        for (i in 0 until rows) {
            list.performScrollToIndex(i)
            rule.waitForIdle()
            headers.filter { it !in seen }
                .mapNotNull { h -> rule.onAllNodes(hasText(h)).fetchSemanticsNodes().firstOrNull()?.let { h to it.boundsInRoot.top } }
                .sortedBy { it.second }
                .forEach { seen += it.first }
        }
        val at = seen.indexOf(planAnswers)
        assertTrue("every section is listed: $seen", seen.containsAll(headers))
        assertTrue("every everyday section comes before the plan answers: $seen", everyday.all { seen.indexOf(it) < at })
        // Folded: the tappable tile says so, and none of the setup answers (e.g.
        // Age, the illness toggle) is composed.
        val tile = hasText(str(R.string.main_plan_answers_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(tile)
        rule.onNode(tile).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_collapsed)),
        )
        assertEquals(0, rule.onAllNodes(hasText(str(R.string.setup_row_age))).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(illToggle()).fetchSemanticsNodes().size)
        rule.openPlanAnswers()
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(profileRow(str(R.string.setup_row_age)))
        rule.onNode(profileRow(str(R.string.setup_row_age))).assertIsDisplayed()
        rule.selectTab(str(R.string.main_dest_today))
        rule.selectTab(str(R.string.main_dest_profile))
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(tile)
        rule.onNode(tile).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_expanded)),
        )
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(profileRow(str(R.string.setup_row_age)))
        rule.onNode(profileRow(str(R.string.setup_row_age))).assertIsDisplayed()
    }

    @Test
    fun under18Card_checkAgeOpensProfileWithTheBirthdayRevealed() {
        QaHarness.seedLog(ctx, listOf(profileWith(birthDay = youthBirthday, age = 15.0)))
        rule.launchApp()
        // The calm Today card, never the red DO NOT TRAIN banner.
        rule.assertCountEventually(hasText(str(R.string.main_hold_youth_title).uppercase(Locale.US)), 1)
        rule.assertCountEventually(hasText(doNotTrain), 0)
        assertEquals(HoldReason.Youth, EventDispatcher.model.value!!.hold_reason)

        rule.onNode(hasText(str(R.string.main_hold_check_age)) and hasClickAction()).performClick()

        val age = str(R.string.setup_row_age)
        rule.waitUntilDisplayed(expandedRow(age))
        // The picker pill is ready to change it, and the birthday is stated once
        // (the row's value, not again on the pill).
        rule.waitUntilDisplayed(birthdayPill())
        assertEquals(
            "the birthday is stated once in the opened Age row",
            1,
            rule.onAllNodes(hasText(born(youthBirthday, 15), substring = true)).fetchSemanticsNodes().size,
        )
    }

    /** Profile › Age's picker pill once a birthday is set ("Change birthday"), the
     *  radio beside "Prefer not to say". */
    private fun birthdayPill() = hasText(str(R.string.setup_birthday_change)) and hasClickAction() and
        SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)

    @Test
    fun parqHold_reviewHealthAnswersRevealsTheQuestionsAndTheirClearance() {
        reviewHealthLands(draftFacts(ProfileDraft(answer = HealthAnswer.Parq(ParqQuestion.HeartOrBloodPressure, true))).health)
        // A doctor-follow-up Yes is lifted by the clearance right under the
        // questions: that row is on screen (off), with the questions unfolded
        // above it and the Yes that raised the hold switched on.
        val cleared = toggleRow(str(R.string.setup_health_cleared_label))
        rule.waitUntilDisplayed(cleared)
        rule.onNode(cleared).assertIsOff()
        rule.onNode(expandedRow(str(R.string.setup_row_health_questions))).assertExists()
        rule.onNode(toggleRow(str(R.string.setup_parq_q1))).assertIsOn()
    }

    @Test
    fun pregnancyHold_reviewHealthAnswersRevealsThePregnancyToggle() {
        reviewHealthLands(HealthScreen(pregnant = true))
        val cause = toggleRow(str(R.string.setup_health_pregnant_label))
        rule.waitUntilDisplayed(cause)
        rule.onNode(cause).assertIsOn()
        // Landed in the upper part of the list, not on its bottom edge: the
        // warning-sign answer right below it is on screen too.
        rule.waitForIdle()
        val list = rule.onAllNodes(hasScrollToNodeAction())[0].fetchSemanticsNode().boundsInRoot
        val row = rule.onNode(cause).fetchSemanticsNode().boundsInRoot
        assertTrue(
            "the pregnancy row ($row) sits in the upper half of the list ($list)",
            row.top - list.top < list.height / 2,
        )
        rule.onNode(toggleRow(str(R.string.setup_health_pregnancy_warning_label))).assertIsDisplayed()
    }

    @Test
    fun pregnancyHold_todayCardCarriesOnlyWhatTheBannerLeavesOut() {
        QaHarness.seedLog(ctx, listOf(profileWith(health = HealthScreen(pregnant = true), birthDay = adultBirthday, age = 35.0)))
        rule.launchApp()
        rule.assertCountEventually(hasText(doNotTrain), 1)
        rule.assertCountEventually(hasText(str(R.string.main_hold_reason_pregnancy)), 1)
        // One SAFETY card with the activity reference, not the banner's "won't plan" again.
        rule.assertCountEventually(hasText(str(R.string.main_chip_safety)), 1)
        rule.assertCountEventually(hasText("150 minutes", substring = true), 1)
        assertEquals(0, rule.onAllNodes(hasText("won't plan", substring = true)).fetchSemanticsNodes().size)
    }

    @Test
    fun pregnancyWarningHold_todayShowsTheBannerWithoutARepeatingCard() {
        val health = HealthScreen(pregnant = true, pregnancy_warning_sign = true)
        QaHarness.seedLog(ctx, listOf(profileWith(health = health, birthDay = adultBirthday, age = 35.0)))
        rule.launchApp()
        rule.assertCountEventually(hasText(doNotTrain), 1)
        rule.assertCountEventually(hasText(str(R.string.main_hold_reason_pregnancy_warning)), 1)
        rule.waitForIdle()
        assertEquals(0, rule.onAllNodes(hasText(str(R.string.main_chip_safety))).fetchSemanticsNodes().size)
        // The core's own warning instruction stays behind the banner's closed "?":
        // no card on Today restates what the banner already says.
        val coreHold = EventDispatcher.model.value!!.today_headline.takeIf { it.kind == HeadlineKind.SafetyHold }?.summary.orEmpty()
        assertTrue("the core states the warning hold", coreHold.isNotBlank())
        assertEquals(0, rule.onAllNodes(hasText(coreHold, substring = true)).fetchSemanticsNodes().size)
    }

    /** A profile holding training for [health]: Today's banner › "Review health answers". */
    private fun reviewHealthLands(health: HealthScreen) {
        QaHarness.seedLog(ctx, listOf(profileWith(health = health, birthDay = adultBirthday, age = 35.0)))
        rule.launchApp()
        rule.assertCountEventually(hasText(doNotTrain), 1)
        rule.onNode(hasText(str(R.string.main_hold_review_health)) and hasClickAction()).performClick()
    }

    @Test
    fun setupAbout_requiresABirthdayFromThePicker_refusesAFutureDate_andProfileShowsTheAge() {
        rule.launchAppExpectingSetup()
        rule.walkLiftSetup(birthdayDigits = null)
        val next = hasText(str(R.string.setup_btn_next)) and hasClickAction()
        rule.onNode(next).assertIsNotEnabled()
        rule.assertCountEventually(hasText(str(R.string.setup_about_pick_hint)), 1)

        rule.pickBirthday(str(R.string.setup_birthday_pick), "01012030")
        rule.assertCountEventually(hasText(str(R.string.setup_birthday_pick)), 1)
        rule.onNode(next).assertIsNotEnabled()

        assertTrue("the picker took the date", rule.pickBirthday(str(R.string.setup_birthday_pick), ADULT_BIRTHDAY_DIGITS))
        rule.assertCountEventually(hasText(born(adultBirthday, 35)), 1)
        rule.onNode(next).assertIsEnabled()
        rule.setupNext()
        rule.setupNext()
        rule.finishSetup()
        assertEquals(adultBirthday, EventDispatcher.model.value!!.profile!!.birth_epoch_day)

        val age = str(R.string.setup_row_age)
        rule.openProfileAnswersAt(profileRow(age))
        rule.onNode(profileRow(age) and hasText(born(adultBirthday, 35), substring = true)).assertIsDisplayed()
    }

    @Test
    fun profileBirthday_age15HoldsTraining_andAnAdultBirthdayClearsIt() {
        QaHarness.seedLog(ctx, listOf(profileWith(birthDay = adultBirthday, age = 35.0)))
        rule.launchApp()
        rule.waitForState { EventDispatcher.model.value?.next_session != null }
        assertTrue(!EventDispatcher.model.value!!.train_blocked)
        val age = str(R.string.setup_row_age)
        rule.openProfileAnswersAt(profileRow(age))
        rule.onNode(profileRow(age)).performClick()
        rule.waitForIdle()

        val change = str(R.string.setup_birthday_change)
        assertTrue("the picker took the date", rule.pickBirthday(change, YOUTH_BIRTHDAY_DIGITS))

        rule.waitForState { EventDispatcher.model.value?.hold_reason == HoldReason.Youth }
        // The under-18 pause pins nothing over Profile: no DO NOT TRAIN banner.
        rule.assertCountEventually(hasText(doNotTrain), 0)
        assertTrue(EventDispatcher.model.value!!.train_blocked)
        assertEquals(youthBirthday, EventDispatcher.model.value!!.profile!!.birth_epoch_day)
        rule.onAllNodesWithText(born(youthBirthday, 15), substring = true).fetchSemanticsNodes()
            .let { assertTrue("the Age row shows the new birthday", it.isNotEmpty()) }

        rule.waitUntilDisplayed(birthdayPill())
        assertTrue("the picker took the date", rule.pickBirthday(change, ADULT_BIRTHDAY_DIGITS))

        rule.waitForState { EventDispatcher.model.value?.train_blocked == false }
        rule.assertCountEventually(hasText(doNotTrain), 0)
        rule.selectTab(str(R.string.main_dest_today))
        rule.waitForState { EventDispatcher.model.value?.next_session != null }
        rule.assertCountEventually(sessionShown(), 1)
    }
}
