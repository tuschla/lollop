package app.milestone.qa

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteractionsProvider
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp

/**
 * Reusable UX-invariant checker run over a whole rendered scenario screen. It
 * catches the classes of UI bug the owner would otherwise hit first-hand:
 * placeholder/garbage text leaking to the surface, unlabeled or finger-hostile
 * tap targets, and layout that collapses at a larger font scale.
 *
 * It reads the Compose semantics tree (what accessibility services and the user
 * actually perceive), never the source, so it stays honest about the shipped UI.
 * Each [Violation] names the offending node's path so a failure points straight
 * at the widget.
 */
object UxInvariants {

    /** Text a finished screen must never show the user. */
    private val FORBIDDEN_SUBSTRINGS = listOf(
        "NaN", "Infinity", "null", "@ RPE 0", "0 kg e1RM",
    )

    /** An internal KB claim id ("AUTOREG-RIR-001", "SAFE-PAIN-001"): engine
     *  bookkeeping that must never reach user-facing copy. */
    private val CLAIM_ID = Regex("""\b[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*-\d{3}\b""")

    /** Visible text at least this long is a sentence-level summary; the same
     *  summary rendered twice on one screen is a duplicate render. */
    private const val SUMMARY_MIN_CHARS = 30

    /** Android's minimum comfortable touch target. */
    private val MIN_TOUCH = 48.dp

    data class Violation(val kind: String, val detail: String, val path: String) {
        override fun toString() = "[$kind] $detail  @ $path"
    }

    /** A flattened, test-friendly snapshot of one semantics node. */
    private class NodeInfo(
        val label: String,
        val clickable: Boolean,
        val widthDp: Float,
        val heightDp: Float,
        val leftDp: Float,
        val topDp: Float,
        val rightDp: Float,
        val visible: Boolean,
        val texts: List<String>,
        // Rendered Text only (no contentDescription), for duplicate-render checks.
        val bodyTexts: List<String>,
        val role: String?,
        val path: String,
        val cardKey: String?,
    )

    fun check(
        provider: SemanticsNodeInteractionsProvider,
        density: Density,
        // The root bounds define the visible surface; an action whose bounds
        // spill past the right edge is treated as clipped.
        checkClipping: Boolean = false,
    ): List<Violation> {
        // Every window root: the activity plus any open bottom sheet or dialog,
        // each of which is its own semantics root.
        val roots = provider.onAllNodes(isRoot(), useUnmergedTree = false).fetchSemanticsNodes()
        val rootRightDp = with(density) { roots.maxOf { it.boundsInRoot.right }.toDp().value }
        val nodes = mutableListOf<NodeInfo>()
        roots.forEachIndexed { i, root -> collect(root, density, "root$i", null, nodes) }

        val violations = mutableListOf<Violation>()

        // 1) No garbage/placeholder text anywhere on the surface.
        for (n in nodes) {
            for (t in n.texts) {
                for (bad in FORBIDDEN_SUBSTRINGS) {
                    if (t.contains(bad)) {
                        violations += Violation("forbidden-text", "\"$t\" contains \"$bad\"", n.path)
                    }
                }
                CLAIM_ID.find(t)?.let { m ->
                    violations += Violation("forbidden-text", "\"$t\" contains claim id \"${m.value}\"", n.path)
                }
            }
        }

        // 2) Every actionable node has a label and a >=48dp touch target.
        for (n in nodes.filter { it.clickable }) {
            if (n.label.isBlank()) {
                violations += Violation(
                    "unlabeled-action",
                    "clickable ${n.role ?: "node"} at (${fmt(n.leftDp)},${fmt(n.topDp)}) " +
                        "${fmt(n.widthDp)}x${fmt(n.heightDp)}dp has no text/contentDescription",
                    n.path,
                )
            }
            // A currently-visible action must meet the touch minimum. Size is the
            // node's own (unclipped) layout size, so an action partly scrolled
            // below the fold is judged by what the finger gets once it scrolls in.
            if (n.visible && (n.widthDp + 0.5f < MIN_TOUCH.value || n.heightDp + 0.5f < MIN_TOUCH.value)) {
                violations += Violation(
                    "small-touch-target",
                    "clickable node is ${fmt(n.widthDp)}x${fmt(n.heightDp)}dp (<${fmt(MIN_TOUCH.value)}dp)",
                    n.path,
                )
            }
        }

        // 3) No duplicate action labels inside one card (a repeated pill in the
        //    same card is an ambiguity a screen reader can't disambiguate).
        nodes.filter { it.clickable && it.cardKey != null && it.label.isNotBlank() }
            .groupBy { it.cardKey }
            .forEach { (card, group) ->
                group.groupBy { it.label }.filterValues { it.size > 1 }.forEach { (label, dupes) ->
                    violations += Violation(
                        "duplicate-action-in-card",
                        "action \"$label\" appears ${dupes.size}x in card \"$card\"",
                        dupes.first().path,
                    )
                }
            }

        // 4) Required actions are not clipped off the right edge at large fonts.
        if (checkClipping) {
            for (n in nodes.filter { it.clickable && it.visible && it.label.isNotBlank() }) {
                // 1dp tolerance for sub-pixel rounding; a genuinely clipped
                // action overhangs by far more.
                if (n.rightDp > rootRightDp + 1f) {
                    violations += Violation(
                        "clipped-action",
                        "action \"${n.label}\" extends past the ${fmt(rootRightDp)}dp surface edge",
                        n.path,
                    )
                }
            }
        }

        // 5) No sentence-level summary rendered twice on one screen (an
        //    adjustment shown as a headline AND again nested in the session card).
        //    The same prescription line on different plan items ("2×5 · pick a
        //    weight for ~3 reps left" under Deadlift and under Bench press) is each
        //    item's own line, not a repeated summary.
        nodes.filter { it.visible }
            .flatMap { n -> n.bodyTexts.filter { it.length >= SUMMARY_MIN_CHARS }.distinct().map { it to n } }
            .groupBy({ it.first }, { it.second })
            .filterValues { it.size > 1 }
            .filterValues { dupes ->
                val items = dupes.map { it.cardKey }
                !(items.all { it?.startsWith("plan-item:") == true } && items.distinct().size == items.size)
            }
            .forEach { (text, dupes) ->
                violations += Violation(
                    "duplicate-summary-on-screen",
                    "\"$text\" renders ${dupes.size}x",
                    dupes.first().path,
                )
            }

        return violations
    }

    private fun collect(
        node: androidx.compose.ui.semantics.SemanticsNode,
        density: Density,
        path: String,
        cardKey: String?,
        out: MutableList<NodeInfo>,
    ) {
        val texts = mutableListOf<String>()
        node.config.getOrNull(SemanticsProperties.Text)?.forEach { texts += it.text }
        val bodyTexts = texts.toList()
        node.config.getOrNull(SemanticsProperties.ContentDescription)?.forEach { texts += it }
        node.config.getOrNull(SemanticsProperties.EditableText)?.let { texts += it.text }
        val label = texts.firstOrNull { it.isNotBlank() } ?: ""
        val clickable = node.config.getOrNull(SemanticsActions.OnClick) != null
        // Touch-target size: the larger of the node's own (unclipped) layout size
        // and its touch bounds, which include the minimum-touch-target expansion
        // `minimumInteractiveComponentSize` grants a smaller visual pill. Clipped
        // bounds only decide whether the node is on screen at all (Robolectric
        // reports a node below the scroll fold at 0x0).
        val touch = node.touchBoundsInRoot
        val w = with(density) { maxOf(node.size.width.toFloat(), touch.width).toDp().value }
        val h = with(density) { maxOf(node.size.height.toFloat(), touch.height).toDp().value }
        val leftDp = with(density) { node.positionInRoot.x.toDp().value }
        val topDp = with(density) { node.positionInRoot.y.toDp().value }
        val rightDp = with(density) { node.boundsInRoot.right.toDp().value }
        val visible = node.boundsInRoot.width > 0f && node.boundsInRoot.height > 0f
        val role = node.config.getOrNull(SemanticsProperties.Role)?.toString()

        // A "card" is the nearest testTagged plan item, else the labelled
        // traversal group we descended through, so duplicate-action grouping is
        // scoped to a single visual unit.
        val tag = node.config.getOrNull(SemanticsProperties.TestTag)
        val thisCard = tag?.takeIf { it.startsWith("plan-item:") } ?: cardKey

        val labelForPath = label.take(24).ifBlank { tag ?: "node" }
        out += NodeInfo(label, clickable, w, h, leftDp, topDp, rightDp, visible, texts, bodyTexts, role, "$path/$labelForPath", thisCard)

        node.children.forEachIndexed { i, child ->
            collect(child, density, "$path/$labelForPath[$i]", thisCard, out)
        }
    }

    private fun fmt(v: Float) = "%.0f".format(v)
}
