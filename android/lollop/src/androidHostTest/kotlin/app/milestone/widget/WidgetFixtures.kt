package app.milestone.widget

import android.content.res.Resources
import app.milestone.DayMark
import app.milestone.HoldReason

internal data class WidgetState(val snapshot: WidgetSnapshot, val recording: Boolean = false)

/**
 * The widget states Android's widget screenshots (ScreenshotWidgetTest) and the iOS
 * extension's fixture (GeneratedSourcesTest) draw: Tuesday 2025-10-28 in Berlin.
 * Item text is as the core writes it: a figure keeps its unit and "@" its load
 * with non-breaking spaces (`text_parts::keep_units`).
 */
internal object WidgetFixtures {
    const val DAY = 20_389L

    /** Mon trained, today (Tue) next, Wed rest, Thu and Fri planned, a rest weekend. */
    val week = listOf(
        DayMark.Done, DayMark.Next, DayMark.Rest, DayMark.Planned,
        DayMark.Planned, DayMark.Rest, DayMark.Rest,
    ).mapIndexed { i, m -> WidgetWeekDay(DAY - 1 + i, m) }

    private fun lift(name: String, detail: String, done: Boolean = false) = WidgetItem(WidgetItemKind.Lift, name, detail, done)

    fun states(res: Resources): LinkedHashMap<String, WidgetState> {
        val zone = "Europe/Berlin"
        val base = WidgetSnapshot(epochDay = DAY, kind = WidgetDayKind.Lift, weekDone = 1, weekPlanned = 4, zone = zone, week = week)
        val liftItems = listOf(lift("Back Squat", "5×3 @\u00A0117.5\u00A0kg"), lift("Deadlift", "3×5"), lift("Bench press", "3×5"), lift("Cable row", "3×5"))
        val runItems = listOf(WidgetItem(WidgetItemKind.Run, detail = "3.4\u00A0mi", pace = "9:00–9:40\u00A0/mi", figure = true))
        val next = WidgetNext(DAY + 2, "Heavy volume day")
        val ahead = listOf(
            WidgetAheadDay(
                DAY + 2, WidgetDayKind.Lift, "Heavy volume day",
                listOf(lift("Back Squat", "4×5 @\u00A0112.5\u00A0kg"), lift("Deadlift", "4×5 @\u00A057.5\u00A0kg"), lift("Bench press", "4×5 @\u00A057.5\u00A0kg")),
            ),
        )
        fun onDay(mark: DayMark) = week.map { if (it.epochDay == DAY) it.copy(mark = mark) else it }
        return linkedMapOf(
            "lift" to WidgetState(base.copy(title = "Heavy day", items = liftItems)),
            "liftstarted" to WidgetState(base.copy(title = "Heavy day", items = listOf(lift("Back Squat", "5×3 @\u00A0117.5\u00A0kg", true)) + liftItems.drop(1))),
            "run" to WidgetState(base.copy(kind = WidgetDayKind.Run, title = "Easy run", items = runItems, trackable = true)),
            "recording" to WidgetState(base.copy(kind = WidgetDayKind.Run, title = "Easy run", items = runItems, trackable = true), recording = true),
            "rest" to WidgetState(base.copy(kind = WidgetDayKind.Rest, restDay = true, next = next, ahead = ahead, week = onDay(DayMark.Rest))),
            "done" to WidgetState(base.copy(kind = WidgetDayKind.Run, title = "Easy run", items = runItems.map { it.copy(done = true) }, sessionDone = true, next = next, ahead = ahead, weekDone = 2, week = onDay(DayMark.Done))),
            "hold" to WidgetState(WidgetSnapshot(epochDay = DAY, kind = WidgetDayKind.Paused, holdReason = HoldReason.Pain, zone = zone)),
            "noplan" to WidgetState(WidgetSnapshot(epochDay = DAY, kind = WidgetDayKind.NoPlan, zone = zone)),
            "stale" to WidgetState(base.copy(stale = true)),
            "preview" to WidgetState(previewSnapshot(res, DAY).copy(zone = zone)),
        )
    }
}
