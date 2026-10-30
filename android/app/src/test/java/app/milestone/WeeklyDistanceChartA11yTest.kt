package app.milestone

import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import app.milestone.DistanceUnit
import app.milestone.MilestoneTheme
import app.milestone.RunWeekView
import app.milestone.WeeklyDistanceChart
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

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
    fun talkBackOn() {
        val am = ApplicationProvider.getApplicationContext<android.content.Context>()
            .getSystemService(android.view.accessibility.AccessibilityManager::class.java)
        shadowOf(am).setEnabled(true)
        shadowOf(am).setTouchExplorationEnabled(true)
    }

    @Test
    fun weeklyChart_summaryIsReadBeforeTheWeeks() {
        val weeks = (0 until 4).map { RunWeekView(week_start_epoch_day = 20_000L + 7L * it, km = 10.0 + it) }
        rule.setContent { MilestoneTheme { WeeklyDistanceChart(weeks, 0.0, DistanceUnit.Km, 120.dp) } }
        rule.waitForIdle()
        val root = composeRoots().single()
        root.forceAccessibilityForTesting(true)
        val all = flatten(root.semanticsOwner.unmergedRootSemanticsNode)
        val summary = all.single { cds(it).any { d -> d.startsWith("Weekly distance over") } }
        val weekIds = all.filter { cds(it).any { d -> d.startsWith("Week of") } }.map { it.id }
        assertEquals("one node per week", weeks.size, weekIds.size)
        // Building a node's info makes Compose fill its traversal-before map.
        root.view.accessibilityNodeProvider!!.createAccessibilityNodeInfo(summary.id)
        val order = traversal(root)
        val labels = order.map { id -> all.firstOrNull { it.id == id }?.let { cds(it).joinToString() + it.config.getOrNull(SemanticsProperties.Text).orEmpty().joinToString { t -> t.text } } }
        assertTrue(
            "summary \"${cds(summary).first().take(40)}…\" must come before the weeks; TalkBack order: $labels",
            summary.id in order && order.indexOf(summary.id) < weekIds.minOf { order.indexOf(it) },
        )
    }

    private fun cds(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()

    private fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap { flatten(it) }

    // As AccessibilityAuditTest: Compose's traversal-before map is what TalkBack follows.
    private fun traversal(root: ViewRootForTest): List<Int> {
        val view = root.view
        val f = generateSequence<Class<*>>(view.javaClass) { it.superclass }.firstNotNullOf { c ->
            runCatching { c.getDeclaredField("composeAccessibilityDelegate") }.getOrNull()
        }.apply { isAccessible = true }
        val delegate = f.get(view)!!
        val map = delegate.javaClass.getMethod("getIdToBeforeMap\$ui").invoke(delegate)
        val get = map.javaClass.getMethod("getOrDefault", Int::class.javaPrimitiveType, Int::class.javaPrimitiveType)
        val next = HashMap<Int, Int>()
        for (n in flatten(root.semanticsOwner.unmergedRootSemanticsNode)) {
            val v = get.invoke(map, n.id, Int.MIN_VALUE) as Int
            if (v != Int.MIN_VALUE) next[n.id] = v
        }
        val head = next.keys.firstOrNull { it !in next.values } ?: return emptyList()
        return generateSequence(head) { next[it] }.take(next.size + 1).toList()
    }

    @Suppress("UNCHECKED_CAST")
    private fun composeRoots(): List<ViewRootForTest> {
        val wmg = Class.forName("android.view.WindowManagerGlobal")
        val inst = wmg.getMethod("getInstance").invoke(null)
        val views = (wmg.getDeclaredField("mViews").apply { isAccessible = true }.get(inst) as List<View>).toList()
        fun View.all(): Sequence<View> = sequence {
            yield(this@all)
            if (this@all is ViewGroup) for (i in 0 until childCount) yieldAll(getChildAt(i).all())
        }
        return views.flatMap { w -> w.all().filter { it is ViewRootForTest && it.isShown }.map { it as ViewRootForTest }.toList() }
    }
}
