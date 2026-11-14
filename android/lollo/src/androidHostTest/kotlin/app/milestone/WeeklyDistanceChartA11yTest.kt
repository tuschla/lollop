package app.milestone

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import app.milestone.qa.composeRoots
import app.milestone.qa.flatten
import app.milestone.qa.talkBackOn
import app.milestone.qa.talkBackOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The weekly chart's summary (total, busiest week, this week, plan) is the only
 * place the busiest week and the plan are spoken, so it must stay in TalkBack's
 * order ahead of the per-week nodes. A group node with children is not focusable,
 * so the summary is its own leaf inside the chart's traversal group.
 */
@RunWith(RobolectricTestRunner::class)
class WeeklyDistanceChartA11yTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun setUp() = talkBackOn()

    @Test
    fun weeklyChart_summaryIsReadBeforeTheWeeks() {
        val weeks = (0 until 4).associate { 20_000L + 7L * it to 10.0 + it }
        rule.setContent { MilestoneTheme { WeeklyDistanceChart(weeks, 0.0, DistanceUnit.Km, 120.dp) } }
        rule.waitForIdle()
        val root = composeRoots().single()
        val all = flatten(root.semanticsOwner.unmergedRootSemanticsNode)
        val summary = all.single { cds(it).any { d -> d.startsWith("Weekly distance over") } }
        val weekIds = all.filter { cds(it).any { d -> d.startsWith("Week of") } }.map { it.id }
        assertEquals("one node per week", weeks.size, weekIds.size)
        val order = talkBackOrder(root)
        val labels = order.map { id -> all.firstOrNull { it.id == id }?.let { cds(it).joinToString() + it.config.getOrNull(SemanticsProperties.Text).orEmpty().joinToString { t -> t.text } } }
        assertTrue(
            "summary \"${cds(summary).first().take(40)}…\" must come before the weeks; TalkBack order: $labels",
            summary.id in order && order.indexOf(summary.id) < weekIds.minOf { order.indexOf(it) },
        )
    }

    private fun cds(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
}
