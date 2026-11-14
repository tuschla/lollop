package app.milestone.qa

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.text.TextLayoutResult
import app.milestone.R

/** The layout itself dropped text: lines beyond maxLines or below a height clip
 *  (both in [TextLayoutResult.didOverflowHeight]), an ellipsis, or an unwrapped
 *  line wider than the box it got. */
val TextLayoutResult.cutsOffText: Boolean
    get() = didOverflowHeight ||
        (0 until lineCount).any { isLineEllipsized(it) } ||
        (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width + 0.5f)

/**
 * Visible text the layout cut off ([cutsOffText]) or that overlaps another visible
 * text in the same window, across every window. A text containing one of [typed]
 * (what a user typed) is not judged for cut-off.
 */
fun ComposeTestRule.textProblems(typed: Collection<String> = emptyList()): List<String> {
    val out = ArrayList<String>()
    val d = density.density
    val slack = 2f * d
    // Floating layers ride over the scrolling list by design: the "Log" button
    // (the list's end clearance is RedesignFlowsTest's gate) and a snackbar
    // (its host is the node with a dismiss action), so text under them is not
    // an overlap.
    val logLabel = str(R.string.main_log)
    val floating = onAllNodes(isRoot()).fetchSemanticsNodes().firstOrNull()?.let { r ->
        flatten(r).filter { n ->
            (
                n.config.getOrNull(SemanticsActions.OnClick) != null &&
                    n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it == logLabel }
                ) || n.config.getOrNull(SemanticsActions.Dismiss) != null
        }.map { it.boundsInRoot }
    }.orEmpty()
    val yearOption = libraryString("m3c_date_picker_navigate_to_year_description")?.substringBefore("%")?.trim()
    fun Rect.dp() = "(${(left / d).toInt()},${(top / d).toInt()})-(${(right / d).toInt()},${(bottom / d).toInt()})dp"
    onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().forEachIndexed { ri, root ->
        val boxes = ArrayList<Pair<String, Rect>>()
        fun visit(n: SemanticsNode) {
            // A composed-but-unplaced group (SetEditor's shownOnly) draws nothing.
            if (!n.layoutInfo.isPlaced) return
            val b = n.boundsInRoot
            if (b.width > 0f && b.height > 0f) {
                n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.let { get ->
                    val layouts = ArrayList<TextLayoutResult>()
                    if (get(layouts)) {
                        layouts.filter { it.cutsOffText }
                            .filterNot { l -> typed.any { it.isNotBlank() && l.layoutInput.text.text.contains(it.take(20)) } }
                            .forEach { l ->
                                val why = when {
                                    l.didOverflowHeight -> "taller than its ${(l.size.height / d).toInt()}dp box"
                                    (0 until l.lineCount).any { l.isLineEllipsized(it) } -> "ellipsized at ${l.lineCount} line(s)"
                                    else -> "one line wider than its ${(l.size.width / d).toInt()}dp box"
                                }
                                out += "cut off: \"${l.layoutInput.text.text}\" ($why, font ${l.layoutInput.style.fontSize}, node at ${(b.left / d).toInt()},${(b.top / d).toInt()}dp)"
                            }
                    }
                }
                n.config.getOrNull(SemanticsProperties.Text)?.let { t -> boxes += t.joinToString(" ") { it.text } to b }
            }
            n.children.forEach(::visit)
        }
        visit(root)
        for (i in boxes.indices) for (j in i + 1 until boxes.size) {
            val a = boxes[i].second
            val b = boxes[j].second
            val w = minOf(a.right, b.right) - maxOf(a.left, b.left)
            val hgt = minOf(a.bottom, b.bottom) - maxOf(a.top, b.top)
            val under = ri == 0 && floating.any { f -> a.overlaps(f) || b.overlaps(f) }
            // The M3 date picker keeps its year grid composed behind the day grid.
            val yearGrid = yearOption != null && listOf(boxes[i].first, boxes[j].first).any { it.startsWith(yearOption) }
            if (w > slack && hgt > slack && !under && !yearGrid) {
                out += "overlap: \"${boxes[i].first}\" ${a.dp()} × \"${boxes[j].first}\" ${b.dp()}"
            }
        }
    }
    return out.distinct()
}
