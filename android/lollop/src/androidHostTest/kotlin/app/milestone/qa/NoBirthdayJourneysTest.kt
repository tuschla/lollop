package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.train_blocked
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The birthday is optional: "Prefer not to say" in setup finishes into a real
 * plan with no hold, and Profile › Age reads "Not given" with the birthday one
 * tap away (and back again).
 */
@RunWith(RobolectricTestRunner::class)
class NoBirthdayJourneysTest {

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
    private val ageLabel get() = str(R.string.setup_row_age)

    /** The birthday's "Prefer not to say" radio (the Sex row's value of the same
     *  words is a row, not a radio). */
    private fun skipChip() = hasText(str(R.string.setup_birthday_skip)) and hasClickAction() and
        SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)

    private fun lastProfilePayload() = journeyLog().last { it.variant == "SetProfile" }.payload

    @Test
    fun setupPreferNotToSay_generatesAPlanWithNoHold_andProfileAddsTheBirthdayLater() {
        rule.launchAppExpectingSetup()
        rule.walkLiftSetup(birthdayDigits = null)
        rule.assertCountEventually(hasText(str(R.string.setup_about_pick_hint)), 1)

        rule.skipBirthday()
        rule.assertCountEventually(hasText(str(R.string.setup_birthday_skipped_note)), 1)
        rule.onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).assertIsEnabled()
        rule.setupNext()
        rule.setupNext()
        rule.assertCountEventually(hasText(str(R.string.setup_review_age_not_given), substring = true), 1)
        rule.finishSetup()

        val profile = EventDispatcher.model.value!!.profile!!
        assertNull(profile.age_years)
        assertNull(profile.birth_epoch_day)
        val payload = lastProfilePayload()
        assertFalse("no age on the wire", payload.containsKey("age_years"))
        assertFalse("no birthday on the wire", payload.containsKey("birth_epoch_day"))

        rule.waitForState { EventDispatcher.model.value?.next_session != null }
        assertFalse(EventDispatcher.model.value!!.train_blocked)
        assertNull(EventDispatcher.model.value!!.hold_reason)
        rule.assertCountEventually(hasText(doNotTrain), 0)
        rule.assertCountEventually(sessionShown(), 1)

        rule.openProfileAnswersAt(profileRow(ageLabel))
        rule.onNode(profileRow(ageLabel) and hasText(str(R.string.setup_age_not_given))).assertIsDisplayed()
        rule.onNode(profileRow(ageLabel)).performClick()
        rule.waitForIdle()
        rule.onNode(skipChip()).performScrollTo().assertIsSelected()
        rule.assertCountEventually(hasText(str(R.string.setup_birthday_skipped_note)), 1)

        assertEquals(true, rule.pickBirthday(str(R.string.setup_birthday_add), ADULT_BIRTHDAY_DIGITS))
        rule.waitForState { EventDispatcher.model.value?.profile?.birth_epoch_day == adultBirthday }
        assertEquals(35.0, EventDispatcher.model.value!!.profile!!.age_years!!, 0.0)
        rule.assertCountEventually(profileRow(ageLabel) and hasText(born(adultBirthday, 35)), 1)
        rule.onNode(skipChip()).assertIsNotSelected()
        rule.assertCountEventually(hasText(str(R.string.setup_birthday_skipped_note)), 0)
        assertFalse(EventDispatcher.model.value!!.train_blocked)
    }

    @Test
    fun profilePreferNotToSay_clearsBirthdayAndAge_andThePlanStays() {
        QaHarness.seedLog(
            ctx,
            listOf(QaHarness.strengthProfile().copy(birthEpochDay = adultBirthday, ageYears = 35.0)),
        )
        rule.launchApp()
        rule.waitForState { EventDispatcher.model.value?.next_session != null }
        rule.openProfileAnswersAt(profileRow(ageLabel))
        rule.onNode(profileRow(ageLabel)).performClick()
        rule.waitForIdle()

        rule.onNode(skipChip()).performScrollTo().assertIsNotSelected().performClick()
        rule.waitForState { EventDispatcher.model.value?.profile?.let { it.birth_epoch_day == null && it.age_years == null } == true }
        val payload = lastProfilePayload()
        assertFalse(payload.containsKey("age_years"))
        assertFalse(payload.containsKey("birth_epoch_day"))
        rule.assertCountEventually(profileRow(ageLabel) and hasText(str(R.string.setup_age_not_given)), 1)
        rule.onNode(skipChip()).assertIsSelected()
        rule.assertCountEventually(hasText(str(R.string.setup_birthday_add)), 1)
        assertFalse(EventDispatcher.model.value!!.train_blocked)
        assertNull(EventDispatcher.model.value!!.hold_reason)
    }
}
