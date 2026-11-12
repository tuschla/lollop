package app.milestone

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.milestone.qa.composeRoots
import app.milestone.qa.flatten
import app.milestone.qa.str
import app.milestone.qa.talkBackOn
import app.milestone.qa.talkBackOrder
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Every dialog window's top container is a traversal group ([traversalGroup])
 * because a Compose window's root sorts left to right, so under RTL TalkBack
 * would read a dialog's button row end to start. The date pickers go through
 * [AppDatePickerDialog] like the other dialogs go through [AppDialog].
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "ar-rEG-ldrtl-w411dp-h891dp-xhdpi")
class DialogA11yTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun setUp() = talkBackOn()

    @Test
    fun control_confirmDialogReadsCancelBeforeOkUnderRtl() {
        rule.setContent {
            MilestoneTheme { ConfirmDialog("Title", null, str(R.string.log_ok), onDismiss = {}, onConfirm = {}) }
        }
        rule.waitForIdle()
        assertStartToEnd(str(R.string.main_cancel), str(R.string.log_ok))
    }

    /** A one-set prescription is reachable: the taper cut floors at one set
     *  (shared/src/plan/lift.rs `.max(1)`) and `plannedSetCount` keeps 1. */
    @Config(qualifiers = "en-rUS-ldltr-w411dp-h891dp-xhdpi")
    @Test
    fun aOneSetPrescriptionSpeaksTheSingular() {
        rule.setContent { MilestoneTheme { SetSlotsRow(PrescriptionView(exercise = "Bench press", sets = 1)) } }
        val cd = rule.onNodeWithTag("set-slots:Bench press").fetchSemanticsNode()
            .config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().joinToString()
        assertTrue("one planned set reads as singular, was \"$cd\"", !cd.contains("1 sets"))
    }

    @Test
    fun observedAtDatePickerReadsCancelBeforeOkUnderRtl() {
        rule.setContent { MilestoneTheme { ObservedAtRow(epochSec = 1_790_000_000L, withTime = false) {} } }
        openFirstClickableWithLabel(str(R.string.log_logged_change_cd))
        assertStartToEnd(str(R.string.main_cancel), str(R.string.log_ok))
    }

    @Test
    fun setupDatePickerReadsCancelBeforeOkUnderRtl() {
        rule.setContent { MilestoneTheme { DateAnswerRow(value = null, today = 20_700L, future = false) {} } }
        rule.onNodeWithText(str(R.string.setup_date_pick)).performClick()
        rule.waitForIdle()
        assertStartToEnd(str(R.string.main_cancel), str(R.string.log_ok))
    }

    private fun openFirstClickableWithLabel(label: String) {
        rule.onNode(
            androidx.compose.ui.test.SemanticsMatcher("onClick label $label") {
                it.config.getOrNull(androidx.compose.ui.semantics.SemanticsActions.OnClick)?.label == label
            },
        ).performClick()
        rule.waitForIdle()
    }

    private fun assertStartToEnd(first: String, second: String) {
        val root = composeRoots().last()
        val order = talkBackOrder(root)
        val byId = flatten(root.semanticsOwner.unmergedRootSemanticsNode).associateBy { it.id }
        fun indexOf(label: String) = order.indexOfFirst { id -> byId[id]?.let { textUnder(it).contains(label) } == true }
        val a = indexOf(first)
        val b = indexOf(second)
        val labels = order.map { byId[it]?.let(::textUnder) }
        assertTrue("both buttons in the traversal ($a, $b): $labels", a >= 0 && b >= 0)
        assertTrue("under RTL \"$first\" (start) must be read before \"$second\" (end); order: $labels", a < b)
    }

    private fun textUnder(n: SemanticsNode): String =
        (listOfNotNull(n.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }) +
            n.children.map(::textUnder)).joinToString(" ")
}
