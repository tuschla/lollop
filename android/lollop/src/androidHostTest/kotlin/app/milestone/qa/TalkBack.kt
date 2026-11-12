package app.milestone.qa

import android.content.Context
import android.view.accessibility.AccessibilityManager
import androidx.compose.ui.platform.ViewRootForTest
import androidx.test.core.app.ApplicationProvider
import org.robolectric.Shadows.shadowOf

/** Accessibility and touch exploration on, as under TalkBack. */
fun talkBackOn() {
    val am = ApplicationProvider.getApplicationContext<Context>().getSystemService(AccessibilityManager::class.java)
    shadowOf(am).setEnabled(true)
    shadowOf(am).setTouchExplorationEnabled(true)
}

/** Every shown Compose root, window by window in the order the windows were added. */
fun composeRoots(): List<ViewRootForTest> =
    windowViews().flatMap { w -> w.descendants().filter { it is ViewRootForTest && it.isShown }.map { it as ViewRootForTest }.toList() }

/**
 * The semantics ids of [root]'s nodes in the order TalkBack visits them. Each
 * node's AccessibilityNodeInfo names the node read after it (its traversal-before
 * target); Compose puts that target's semantics id in the info's extras under
 * [TRAVERSAL_BEFORE_KEY]. Empty when no node names one.
 */
fun talkBackOrder(root: ViewRootForTest): List<Int> {
    root.forceAccessibilityForTesting(true)
    val provider = root.view.accessibilityNodeProvider ?: return emptyList()
    val next = HashMap<Int, Int>()
    for (n in flatten(root.semanticsOwner.unmergedRootSemanticsNode).drop(1)) {
        val before = provider.createAccessibilityNodeInfo(n.id)?.extras?.getInt(TRAVERSAL_BEFORE_KEY, -1) ?: -1
        if (before != -1) next[n.id] = before
    }
    val targets = next.values.toHashSet()
    val head = next.keys.firstOrNull { it !in targets } ?: return emptyList()
    val out = ArrayList<Int>()
    val seen = HashSet<Int>()
    var cur: Int? = head
    while (cur != null && seen.add(cur)) {
        out += cur
        cur = next[cur]
    }
    return out
}

/** The AccessibilityNodeInfo extra Compose fills with a node's traversal-before target. */
private const val TRAVERSAL_BEFORE_KEY = "android.view.accessibility.extra.EXTRA_DATA_TEST_TRAVERSALBEFORE_VAL"
