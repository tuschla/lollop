package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals

/** The heatmap's month labels ([spacedMonthTicks]): a label that would run into the
 *  next one gives way. */
class MonthTicksTest {

    @Test
    fun aLeadingPartialMonthGivesWayWhenItsLabelWouldTouchTheNext() {
        val ticks = listOf("Jul", "Aug", "", "", "", "Sep", "", "", "", "Oct", "", "")
        // Large font: "Jul" (61 px) + a word space (21 px) overruns one 79 px column.
        assertEquals(
            listOf("", "Aug", "", "", "", "Sep", "", "", "", "Oct", "", ""),
            spacedMonthTicks(ticks, listOf(61, 64, 0, 0, 0, 62, 0, 0, 0, 60, 0, 0), pitchPx = 79, minGapPx = 21),
        )
        // Default font: it fits, so every month keeps its tick.
        assertEquals(ticks, spacedMonthTicks(ticks, listOf(47, 50, 0, 0, 0, 48, 0, 0, 0, 46, 0, 0), pitchPx = 79, minGapPx = 21))
    }
}
