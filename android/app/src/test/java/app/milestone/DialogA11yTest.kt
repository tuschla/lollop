package app.milestone

import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import app.milestone.ConfirmDialog
import app.milestone.DateAnswerRow
import app.milestone.MilestoneTheme
import app.milestone.ObservedAtRow
import app.milestone.PrescriptionView
import app.milestone.R
import app.milestone.SetSlotsRow
import org.junit.Assert.assertTrue
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

    private fun str(id: Int) = ApplicationProvider.getApplicationContext<android.content.Context>().getString(id)

    @org.junit.Before
    fun talkBackOn() {
        val am = ApplicationProvider.getApplicationContext<android.content.Context>()
            .getSystemService(android.view.accessibility.AccessibilityManager::class.java)
        org.robolectric.Shadows.shadowOf(am).setEnabled(true)
        org.robolectric.Shadows.shadowOf(am).setTouchExplorationEnabled(true)
    }

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

    // ── helpers ──────────────────────────────────────────────────────────────

    private fun openFirstClickableWithLabel(label: String) {
        rule.onNode(
            androidx.compose.ui.test.SemanticsMatcher("onClick label $label") {
                it.config.getOrNull(androidx.compose.ui.semantics.SemanticsActions.OnClick)?.label == label
            },
        ).performClick()
        rule.waitForIdle()
    }

    /** In the top window, TalkBack reaches [first] before [second]. */
    private fun assertStartToEnd(first: String, second: String) {
        val root = composeRoots().last()
        root.forceAccessibilityForTesting(true)
        val unmerged = root.semanticsOwner.unmergedRootSemanticsNode
        root.view.accessibilityNodeProvider?.createAccessibilityNodeInfo(flatten(unmerged).drop(1).first().id)
        val order = traversal(root)
        val byId = flatten(unmerged).associateBy { it.id }
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

    private fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flatten)

    private fun traversal(root: ViewRootForTest): List<Int> {
        val view = root.view
        val f = generateSequence<Class<*>>(view.javaClass) { it.superclass }.firstNotNullOf { c ->
            runCatching { c.getDeclaredField("composeAccessibilityDelegate") }.getOrNull()
        }.apply { isAccessible = true }
        val delegate = f.get(view)!!
        val map = delegate.javaClass.getMethod("getIdToBeforeMap\$ui").invoke(delegate)
        val get = map.javaClass.getMethod("getOrDefault", Int::class.javaPrimitiveType, Int::class.javaPrimitiveType)
        val next = HashMap<Int, Int>()
        val targets = HashSet<Int>()
        for (n in flatten(root.semanticsOwner.unmergedRootSemanticsNode)) {
            val v = get.invoke(map, n.id, Int.MIN_VALUE) as Int
            if (v != Int.MIN_VALUE) {
                next[n.id] = v
                targets += v
            }
        }
        val head = next.keys.firstOrNull { it !in targets } ?: return emptyList()
        val out = ArrayList<Int>()
        var cur: Int? = head
        val seen = HashSet<Int>()
        while (cur != null && seen.add(cur)) {
            out += cur
            cur = next[cur]
        }
        return out
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
