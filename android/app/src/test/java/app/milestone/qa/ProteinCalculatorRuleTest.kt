package app.milestone.qa

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import app.milestone.CalculatorsCard
import app.milestone.CoachTool
import app.milestone.CoachToolSheet
import app.milestone.Event
import app.milestone.MilestoneTheme
import app.milestone.ProfileView
import app.milestone.ProteinInputView
import app.milestone.R
import app.milestone.ViewModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The KB backs a protein target only for masters (65+) or in a caloric deficit.
 * Under 65 the Profile row says it computes only in a deficit and the sheet
 * offers the deficit context alone; at 65+ (or with no age) both stay.
 */
@RunWith(RobolectricTestRunner::class)
class ProteinCalculatorRuleTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private fun model(age: Double?, input: ProteinInputView? = null) =
        ViewModel(profile = ProfileView(age_years = age, bodyweight_kg = 80.0), protein_input = input)

    private fun showRowAndSheet(vm: ViewModel, onEvent: (Event) -> Unit = {}) {
        rule.setContent {
            MilestoneTheme {
                Column {
                    CalculatorsCard(vm) {}
                    CoachToolSheet(CoachTool.Protein, vm, onEvent)
                }
            }
        }
    }

    private fun computeButton() =
        rule.onNode(hasText(str(R.string.log_btn_compute_protein)) and hasClickAction()).performScrollTo()

    @Test
    fun underSixtyFive_rowSaysDeficitOnly_andSheetSendsDeficitAlone() {
        val sent = mutableListOf<Event>()
        showRowAndSheet(model(age = 40.0)) { sent += it }
        rule.onNode(hasText(str(R.string.main_tool_protein_title)) and hasClickAction()).assertExists()
        rule.onNode(hasText(str(R.string.main_tool_protein_cta_deficit))).assertExists()
        rule.onNode(hasText(str(R.string.log_switch_masters))).assertDoesNotExist()
        rule.onNode(hasText(str(R.string.log_protein_deficit_only))).assertExists()
        computeButton().assertIsNotEnabled()

        rule.onNode(hasText(str(R.string.log_switch_deficit))).performScrollTo().performClick()
        computeButton().assertIsEnabled().performClick()
        val json = (sent.single() as Event.ComputeProtein).toJson().toString()
        assertTrue(json, json.contains("\"masters\":false") && json.contains("\"deficit\":true"))
    }

    @Test
    fun underSixtyFive_staleMastersQueryIsNeverSent() {
        val sent = mutableListOf<Event>()
        showRowAndSheet(model(age = 40.0, ProteinInputView(bodyweight_kg = 80.0, masters = true))) { sent += it }
        computeButton().assertIsNotEnabled()
        rule.onNode(hasText(str(R.string.log_switch_deficit))).performScrollTo().performClick()
        computeButton().performClick()
        val json = (sent.single() as Event.ComputeProtein).toJson().toString()
        assertTrue(json, json.contains("\"masters\":false"))
    }

    @Test
    fun sixtyFivePlus_keepsComputeCtaAndMastersSwitch() {
        showRowAndSheet(model(age = 70.0))
        rule.onNode(hasText(str(R.string.main_tool_protein_cta_compute))).assertExists()
        rule.onNode(hasText(str(R.string.main_tool_protein_cta_deficit))).assertDoesNotExist()
        rule.onNode(hasText(str(R.string.log_switch_masters))).assertExists()
        rule.onNode(hasText(str(R.string.log_protein_no_context))).assertExists()
    }

    @Test
    fun unknownAge_leavesBothContextsOpen() {
        showRowAndSheet(model(age = null))
        rule.onNode(hasText(str(R.string.main_tool_protein_cta_compute))).assertExists()
        assertEquals(1, rule.onAllNodes(hasText(str(R.string.log_switch_masters))).fetchSemanticsNodes().size)
    }
}
