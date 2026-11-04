package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.milestone.AppDatePickerDialog
import app.milestone.LocalWindowLayout
import app.milestone.MilestoneTheme
import app.milestone.R
import app.milestone.WindowLayout
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * The typed date field in a landscape phone's window once the keyboard is up
 * (`AdaptiveLayoutTest.phoneLandscapeAfterBars_datePickerResizesForTheKeyboard`
 * checks the dialog window resizes for it): ~130dp are left of a 411dp-tall
 * window. Bug (e37 emulator): the platform panned the dialog, the field at the top
 * edge and Cancel/OK under the keyboard
 * (`~/.cache/ux6/UxDevice3/after/e37-land-datepicker-typing.png`).
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w891dp-h130dp-land-xxhdpi")
class DatePickerKeyboardTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private fun assertWhollyInDialog(what: String, n: SemanticsNode) {
        val dialog = rule.onNode(isDialog()).fetchSemanticsNode().boundsInRoot
        val b = n.boundsInRoot
        assertTrue("$what is cut: shown ${b.height}px of ${n.size.height}px at $b", b.height >= n.size.height - 2f && b.width >= n.size.width - 2f)
        assertTrue("$what at $b is outside the dialog window $dialog", b.top >= dialog.top - 1f && b.bottom <= dialog.bottom + 1f)
    }

    @OptIn(ExperimentalMaterial3Api::class)
    @Test
    fun keyboardUp_theFocusedDateFieldAndOkStayInView() {
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(LocalWindowLayout provides WindowLayout(navRail = true, shortHeight = true, twoPane = true, sideControls = true)) {
                    AppDatePickerDialog(rememberDatePickerState(initialSelectedDateMillis = 1_792_000_000_000L), onDismiss = {}, onConfirm = {})
                }
            }
        }
        rule.waitForIdle()
        val field = hasAnyAncestor(isDialog()) and hasSetTextAction()
        rule.onNode(field).performClick()
        rule.waitForIdle()
        // Its 56dp entry box must show whole (the label notch above it and the
        // empty error line below may sit at the scroll edge).
        val f = rule.onNode(field).fetchSemanticsNode()
        val dialog = rule.onNode(isDialog()).fetchSemanticsNode().boundsInRoot
        val shown = with(rule.density) { f.boundsInRoot.height.toDp().value }
        assertTrue("the date field shows ${shown}dp at ${f.boundsInRoot}, under its 56dp entry box", shown >= 56f)
        assertTrue("the date field at ${f.boundsInRoot} is outside the dialog window $dialog", f.boundsInRoot.top >= dialog.top - 1f && f.boundsInRoot.bottom <= dialog.bottom + 1f)
        assertWhollyInDialog("OK", rule.onNode(hasAnyAncestor(isDialog()) and hasClickAction() and hasText(str(R.string.log_ok))).fetchSemanticsNode())
        assertWhollyInDialog("Cancel", rule.onNode(hasAnyAncestor(isDialog()) and hasClickAction() and hasText(str(R.string.main_cancel))).fetchSemanticsNode())
    }

    /**
     * Bug (e37 emulator, `~/.cache/ux6/UxDevice5/live/shots/bd-strip.png`): a partly
     * typed date ("06/15/") turns the field to its error state. With the title,
     * headline and Cancel/OK above and below the field, ~70dp were left: the label
     * and half the entry box showed, the error line that says why OK is off sat
     * below the window. The short dialog is the field alone, Cancel/OK beside it.
     */
    @OptIn(ExperimentalMaterial3Api::class)
    @Test
    fun keyboardUp_aPartlyTypedDateShowsTheWholeFieldAndItsError() {
        rule.setContent {
            MilestoneTheme {
                CompositionLocalProvider(LocalWindowLayout provides WindowLayout(navRail = true, shortHeight = true, twoPane = true, sideControls = true)) {
                    AppDatePickerDialog(rememberDatePickerState(), onDismiss = {}, onConfirm = {})
                }
            }
        }
        rule.waitForIdle()
        val field = hasAnyAncestor(isDialog()) and hasSetTextAction()
        rule.onNode(field).performClick()
        rule.waitForIdle()
        rule.onNode(field).performTextInput("0615")
        rule.waitForIdle()
        // The field's node runs from the label notch over the 56dp entry box to the
        // error line; the notch may sit at the scroll edge, never the box or the
        // error under it.
        val f = rule.onNode(field).fetchSemanticsNode()
        assertTrue("the field is not in its error state", SemanticsProperties.Error in f.config)
        val error = rule.onNode(hasAnyAncestor(isDialog()) and hasText("expected pattern", substring = true), useUnmergedTree = true).fetchSemanticsNode()
        assertWhollyInDialog("the error line", error)
        val fullBottom = f.positionInRoot.y + f.size.height
        assertTrue("the date field's bottom is cut at ${f.boundsInRoot} (whole to $fullBottom)", f.boundsInRoot.bottom >= fullBottom - 2f)
        val shown = with(rule.density) { f.boundsInRoot.height.toDp().value }
        val errorDp = with(rule.density) { error.size.height.toDp().value }
        assertTrue("the date field shows ${shown}dp, under its 56dp entry box and ${errorDp}dp error line", shown >= 56f + errorDp)
        assertWhollyInDialog("OK", rule.onNode(hasAnyAncestor(isDialog()) and hasClickAction() and hasText(str(R.string.log_ok))).fetchSemanticsNode())
    }
}
