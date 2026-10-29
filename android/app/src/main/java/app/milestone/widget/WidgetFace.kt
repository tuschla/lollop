package app.milestone.widget

import android.content.res.Resources
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.planDateLabel

/** How the widget colours its title. */
internal enum class WidgetTone {
    /** A plan day, rest day or prompt. */
    Normal,

    /** Training is paused by a safety hold: the danger text colour, like the app's banner. */
    Paused,

    /** Nothing current to show ("Open lollop to update"). */
    Muted,
}

/** One button on the widget: its label and the screen it opens. */
internal data class WidgetAction(val label: String, val dest: OpenDest)

/**
 * The widget's text, resolved from a snapshot. [lead] lines always show ("Session
 * done", the next session, a pause's pointer into the app); [items] fill the space
 * left, and [visibleLines] adds "N more items" for the ones cut. [week] is the
 * planned vs done count, shown when the widget is tall enough.
 */
internal data class WidgetFace(
    val overline: String,
    val title: String,
    val tone: WidgetTone,
    val lead: List<String> = emptyList(),
    val items: List<String> = emptyList(),
    val week: String? = null,
    val actions: List<WidgetAction> = emptyList(),
)

/**
 * The face for [snapshot] on local day [today]. A missing or stale snapshot, or
 * one written for another day, reads "Open lollop to update" and never shows that
 * day's session as today's.
 */
internal fun widgetFace(snapshot: WidgetSnapshot?, today: Long, res: Resources): WidgetFace {
    val todayLabel = res.getString(R.string.widget_overline_today)
    val log = WidgetAction(res.getString(R.string.main_log), OpenDest.Log)
    if (snapshot == null || snapshot.stale || snapshot.epochDay != today) {
        return WidgetFace(todayLabel, res.getString(R.string.widget_open_to_update), WidgetTone.Muted, actions = listOf(log))
    }
    val week = snapshot.weekPlanned.takeIf { it > 0 }?.let {
        res.getQuantityString(R.plurals.main_week_done_count, it, snapshot.weekDone, it)
    }
    // The overline names the day the snapshot describes, so a frame the host keeps
    // past midnight (before the midnight tick lands) still reads true.
    val overline = res.getString(R.string.widget_overline_day, todayLabel, planDateLabel(snapshot.epochDay))
    val next = snapshot.next?.let { res.getString(R.string.widget_next, it.title, planDateLabel(it.epochDay)) }
    return when (snapshot.kind) {
        // A home screen is seen by anyone near the phone: the hold's reason stays in
        // the app. The under-18 pause keeps the calm (non-danger) colour.
        WidgetDayKind.Paused -> WidgetFace(
            overline,
            res.getString(R.string.main_status_on_hold),
            if (snapshot.holdReason == "youth") WidgetTone.Normal else WidgetTone.Paused,
            lead = listOf(res.getString(R.string.widget_paused_body)),
            actions = listOf(log),
        )
        WidgetDayKind.Setup -> WidgetFace(
            overline,
            res.getString(R.string.widget_setup_title),
            WidgetTone.Normal,
            lead = listOf(res.getString(R.string.widget_setup_body)),
        )
        WidgetDayKind.NoPlan -> {
            val (title, body) = when (snapshot.planWithheld) {
                "health_hold" -> R.string.main_plan_withheld_health_title to R.string.main_plan_withheld_health_body
                "nothing_to_plan" -> R.string.main_plan_withheld_empty_title to R.string.widget_setup_body
                else -> R.string.widget_no_plan to R.string.widget_no_plan_body
            }
            WidgetFace(overline, res.getString(title), WidgetTone.Normal, lead = listOf(res.getString(body)), actions = listOf(log))
        }
        WidgetDayKind.Rest -> WidgetFace(
            overline,
            res.getString(if (snapshot.restDay) R.string.widget_rest_day else R.string.widget_no_session),
            WidgetTone.Normal,
            lead = listOfNotNull(next),
            week = week,
            actions = listOf(log),
        )
        WidgetDayKind.Lift, WidgetDayKind.Run -> {
            val (open, done) = snapshot.items.partition { !it.done }
            WidgetFace(
                overline,
                snapshot.title,
                WidgetTone.Normal,
                lead = if (snapshot.sessionDone) listOfNotNull(res.getString(R.string.widget_session_done), next) else emptyList(),
                items = (open + done).map { itemLine(it, res) },
                week = week,
                actions = listOfNotNull(
                    WidgetAction(res.getString(R.string.main_plan_item_track), OpenDest.Track).takeIf { snapshot.trackable },
                    log,
                ),
            )
        }
    }
}

private fun itemLine(item: WidgetItem, res: Resources): String {
    val name = when (item.kind) {
        WidgetItemKind.Run -> res.getString(R.string.main_run)
        WidgetItemKind.Check -> res.getString(R.string.plan_check_title)
        WidgetItemKind.Lift -> item.name
    }
    val detail = if (item.done) res.getString(R.string.widget_item_done) else item.detail.takeIf { it.isNotBlank() && it != name }
    return detail?.let { res.getString(R.string.widget_item_line, name, it) } ?: name
}

/**
 * The body lines that fit [maxLines]: every lead line first, then items; when
 * items are cut, the last slot reads "N more items" instead.
 */
internal fun visibleLines(face: WidgetFace, maxLines: Int, res: Resources): List<String> {
    val lead = face.lead.take(maxLines)
    val room = maxLines - lead.size
    if (room <= 0 || face.items.isEmpty()) return lead
    if (face.items.size <= room) return lead + face.items
    val shown = face.items.take(room - 1)
    val cut = face.items.size - shown.size
    return lead + shown + res.getQuantityString(R.plurals.widget_more_items, cut, cut)
}
