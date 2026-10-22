package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.ConcurrentGoal
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.GoalDistance
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import com.github.takahirom.roborazzi.captureScreenRoboImage
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
 * The calculator result cards and the Evidence & references sheet (ui-sweep §10,
 * §12), driven through the real app and core: each calculator's last query is in
 * the seeded log, so its Profile sheet opens with the result.
 */
@RunWith(RobolectricTestRunner::class)
class CalculatorResultsTest {

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

    @OptIn(com.github.takahirom.roborazzi.ExperimentalRoborazziApi::class)
    private fun shot(name: String) =
        rule.captureWithoutRipples { captureScreenRoboImage("src/test/screenshots/$name.png") }

    /** Lifts and runs, so all four calculators are offered. */
    private fun hybridProfile(): Event = Event.SetProfile(
        progressionCadence = ProgressionCadence.WeekToWeek,
        liftGoal = LiftGoal.Hypertrophy,
        goalDistance = GoalDistance.TenK,
        concurrentGoal = ConcurrentGoal.Hypertrophy,
        weeklySets = 12,
        runningDaysPerWeek = 3,
        runningKmPerWeek = 30.0,
        advanced = false,
        bodyweightKg = 81.7,
        ageYears = 30.0,
    )

    private val queries = listOf(
        Event.ComputeHrZones(30.0, null),
        Event.PlanHypertrophyMeso("chest", 4, notGrowing = false, recoveringEasily = false),
        Event.ComputeProtein(81.7, masters = false, deficit = true),
        Event.PredictRace(5000.0, 22 * 60.0, 10000.0, 40.0, null),
    )

    private val keypad = hasTestTag("numeric-keypad")
    private val result = hasTestTag("calc-result")

    /** Seed the profile and every calculator query, open Profile and tap [titleRes]'s row. */
    private fun openCalculator(titleRes: Int, mode: DarkMode = DarkMode.Light) {
        AppSettings.setDarkMode(ctx, mode)
        QaHarness.seedLog(ctx, listOf(hybridProfile()) + queries)
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(str(titleRes)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.onAllNodes(row).onFirst().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(result).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
    }

    private fun textSizeSp(node: androidx.compose.ui.semantics.SemanticsNode): Float? {
        val results = mutableListOf<TextLayoutResult>()
        val key = androidx.compose.ui.semantics.SemanticsActions.GetTextLayoutResult
        if (key in node.config) node.config[key].action?.invoke(results)
        return results.firstOrNull()?.layoutInput?.style?.fontSize?.value
    }

    /** The answer is the largest text in the sheet, and no text is larger. */
    private fun assertHeadlineIsLargest() {
        val headline = rule.onAllNodes(hasTestTag("calc-headline"), useUnmergedTree = true).onFirst().fetchSemanticsNode()
        val headlineSp = checkNotNull(textSizeSp(headline))
        val others = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Text), useUnmergedTree = true)
            .fetchSemanticsNodes()
            .filter { it.id != headline.id }
            .mapNotNull { textSizeSp(it) }
        assertTrue("headline ${headlineSp}sp vs ${others.maxOrNull()}sp", others.all { it <= headlineSp })
    }

    /** No calculator opens with its keypad up; the result card is on screen. */
    private fun assertOpensOnTheResult() {
        assertEquals(0, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
        val root = rule.onAllNodes(hasTestTag("calc-headline"), useUnmergedTree = true).onFirst().fetchSemanticsNode()
        val screenH = ctx.resources.displayMetrics.heightPixels
        assertTrue("headline below the fold: ${root.boundsInRoot}", root.boundsInRoot.bottom in 0f..screenH.toFloat())
    }

    @Test
    fun hrZones_oneCardOneQuestionMark_tableFitsOnScreen_noDegenerateRange() {
        openCalculator(R.string.main_tool_hr_title)
        assertOpensOnTheResult()
        val overline = str(R.string.main_calc_hr_overline)
        val oneQuestion = hasContentDescription(str(R.string.main_details_subject, overline))
        assertEquals(1, rule.onAllNodes(oneQuestion).fetchSemanticsNodes().size)
        // The card carries every "?" of the result: none of its own rows has one.
        assertEquals(
            1,
            rule.onAllNodes(hasAnyAncestor(result) and hasContentDescription(str(R.string.main_details), substring = true))
                .fetchSemanticsNodes().size,
        )
        rule.onNode(hasText("187")).assertExists()
        assertEquals(0, rule.onAllNodes(hasText("100–100", substring = true)).fetchSemanticsNodes().size)
        assertEquals(2, rule.onAllNodes(hasText(str(R.string.main_calc_hr_by_pace))).fetchSemanticsNodes().size)
        // One screen at default scale: the whole card is no taller than the display.
        val card = rule.onNode(result).fetchSemanticsNode()
        assertTrue(card.size.height <= ctx.resources.displayMetrics.heightPixels)
        assertHeadlineIsLargest()
        shot("calc-hr-zones-result-light")
        // Its "?" stacks the evidence of every claim on the card.
        rule.onNode(oneQuestion).performClick()
        rule.waitForIdle()
        assertTrue(rule.onAllNodes(hasText("Easy", substring = true) and hasAnyAncestor(result)).fetchSemanticsNodes().size >= 2)
    }

    @Test
    fun hrZonesDark() {
        openCalculator(R.string.main_tool_hr_title, DarkMode.Dark)
        shot("calc-hr-zones-result-dark")
    }

    @Test
    fun volume_headlineChartAndKeyValueRows() {
        openCalculator(R.string.main_tool_volume_title)
        assertOpensOnTheResult()
        rule.onNode(hasTestTag("volume-ramp-chart")).assertExists()
        for (label in listOf(R.string.main_calc_kv_rir, R.string.main_calc_kv_frequency, R.string.main_calc_kv_rest_heavy, R.string.main_calc_kv_rest_isolation, R.string.main_calc_kv_shape)) {
            rule.onNode(hasText(str(label), substring = true)).assertExists()
        }
        assertHeadlineIsLargest()
        shot("calc-volume-planner-result-light")
    }

    @Test
    fun protein_answerIsTheHeadline() {
        openCalculator(R.string.main_tool_protein_title)
        assertOpensOnTheResult()
        val fig = app.milestone.EventDispatcher.model.value!!.protein_figures.single()
        val grams = "${Math.round(fig.low_g_per_day)}–${Math.round(fig.high_g_per_day)}"
        rule.onNode(hasTestTag("calc-headline") and hasText(grams), useUnmergedTree = true).assertExists()
        rule.onNode(hasText(str(R.string.main_calc_protein_caption))).assertExists()
        assertHeadlineIsLargest()
        shot("calc-protein-result-light")
    }

    @Test
    fun race_finishTimeIsTheHeadline() {
        openCalculator(R.string.main_tool_race_title)
        assertOpensOnTheResult()
        rule.onNode(hasText(str(R.string.main_calc_race_caption, "10K"))).assertExists()
        assertHeadlineIsLargest()
        shot("calc-race-predictor-result-light")
    }

    @Test
    fun keypadOpensOnlyWhileAFieldIsActive() {
        openCalculator(R.string.main_tool_hr_title)
        assertEquals(0, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
        rule.onNode(hasClickAction() and hasContentDescription(str(R.string.log_field_age), substring = true))
            .performScrollTo().performClick()
        rule.waitForIdle()
        assertEquals(1, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
        rule.onNode(hasText(str(R.string.log_keypad_done)) and hasClickAction()).performScrollTo().performClick()
        rule.waitForIdle()
        assertEquals(0, rule.onAllNodes(keypad).fetchSemanticsNodes().size)
    }

    @Test
    fun glossary_opensFromTheCardsTerms_searchFiltersAndSaysNoMatch() {
        openCalculator(R.string.main_tool_hr_title)
        rule.onNode(hasContentDescription(str(R.string.main_details_subject, str(R.string.main_calc_hr_overline)))).performClick()
        rule.waitForIdle()
        val hrmax = str(R.string.main_glossary_hrmax_term)
        rule.onNode(hasContentDescription(str(R.string.main_explain, hrmax)) and hasClickAction()).performScrollTo().performClick()
        rule.waitUntil(10_000) { rule.onAllNodes(hasTestTag("glossary-search")).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()
        shot("sheet-glossary-light")
        val search = rule.onNode(hasTestTag("glossary-search"))
        search.performTextInput("tonn")
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.main_glossary_tonnage_term))).assertExists()
        assertEquals(0, rule.onAllNodes(hasText(str(R.string.main_glossary_rpe_term))).fetchSemanticsNodes().size)
        search.performTextClearance()
        search.performTextInput("xyz")
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.main_glossary_no_match, "xyz"))).assertExists()
    }

    private fun openReferences() {
        QaHarness.seedLog(ctx, listOf(hybridProfile()))
        rule.launchApp()
        rule.selectTab(str(R.string.main_dest_profile))
        val row = hasText(str(R.string.main_evidence_references_title)) and hasClickAction()
        rule.onAllNodes(hasScrollToNodeAction()).onFirst().performScrollToNode(row)
        rule.onAllNodes(row).onFirst().performClick()
        rule.waitUntil(10_000) {
            rule.onAllNodes(hasText(str(R.string.main_evidence_references_body))).fetchSemanticsNodes().isNotEmpty()
        }
        rule.waitForIdle()
    }

    @Test
    fun references_groupRowsCarryCountPills_andOpenInPlace() {
        openReferences()
        val strength = hasText("Strength") and hasClickAction()
        rule.onNode(strength).assertExists()
        shot("sheet-evidence-references-light")
        rule.onNode(strength).performClick()
        rule.waitForIdle()
        rule.onNode(strength).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_expanded)),
        )
        shot("sheet-evidence-references-expanded-light")
    }
}
