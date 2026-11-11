package app.milestone.widget

import androidx.compose.ui.graphics.Color
import app.milestone.DarkStatusColors
import app.milestone.DayMark
import app.milestone.HoldReason
import app.milestone.OpenDest
import app.milestone.PlanWithheld
import app.milestone.Palette
import app.milestone.PlatformResources
import app.milestone.Reminder
import app.milestone.StatusColors
import app.milestone.accentTint
import app.milestone.getQuantityString
import app.milestone.getString
import app.milestone.onAccent
import app.milestone.planDateLabel
import app.milestone.reminderLine
import app.milestone.weekdayShort
import app.milestone.resources.Res
import app.milestone.resources.main_plan_withheld_empty_title
import app.milestone.resources.main_plan_withheld_health_body
import app.milestone.resources.main_plan_withheld_health_title
import app.milestone.resources.main_run
import app.milestone.resources.main_status_on_hold
import app.milestone.resources.main_week_done_count
import app.milestone.resources.plan_check_title
import app.milestone.resources.widget_item_done
import app.milestone.resources.widget_item_line
import app.milestone.resources.widget_more_items
import app.milestone.resources.widget_next
import app.milestone.resources.widget_next_day
import app.milestone.resources.widget_no_plan
import app.milestone.resources.widget_no_plan_body
import app.milestone.resources.widget_no_session
import app.milestone.resources.widget_open_to_update
import app.milestone.resources.widget_overline_day
import app.milestone.resources.widget_overline_today
import app.milestone.resources.widget_paused_body
import app.milestone.resources.widget_rest_day
import app.milestone.resources.widget_session_done
import app.milestone.resources.widget_setup_body
import app.milestone.resources.widget_setup_title
import kotlinx.datetime.LocalDate
import kotlinx.serialization.Serializable

/** A colour role of the widgets: the same roles on the Glance widget and the iOS widgets. */
@Serializable
internal enum class WidgetInk { Ground, Body, Muted, Faint, Accent, OnAccent, AccentTint, Danger, Done }

/** [ink] in this palette; [dark]: a dark palette, whose done mark is the lighter green. */
internal fun Palette.ink(ink: WidgetInk, dark: Boolean): Color = when (ink) {
    WidgetInk.Ground -> bgTop
    WidgetInk.Body -> onBgBody
    WidgetInk.Muted -> onBgMuted
    WidgetInk.Faint -> onBgFaint
    WidgetInk.Accent -> accent
    WidgetInk.OnAccent -> onAccent
    WidgetInk.AccentTint -> accentTint()
    WidgetInk.Danger -> dangerText
    WidgetInk.Done -> (if (dark) DarkStatusColors else StatusColors()).done
}

/** How the widget colours its title: [ink]. */
internal enum class WidgetTone(val ink: WidgetInk) {
    /** A plan day, rest day or prompt. */
    Normal(WidgetInk.Body),

    /** Training is paused by a safety hold: the danger text colour, like the app's banner. */
    Paused(WidgetInk.Danger),

    /** Nothing current to show ("Open lollop to update"). */
    Muted(WidgetInk.Muted),
}

/** A week-strip day's mark as drawn: its [text] in [ink]. */
internal data class WeekGlyph(val text: String, val ink: WidgetInk)

/** ✓ trained, ● the next session, ○ planned, a muted ○ missed, nothing on a rest
 *  day, – before the plan. */
internal val DayMark.glyph: WeekGlyph
    get() = when (this) {
        DayMark.Done -> WeekGlyph("\u2713", WidgetInk.Done)
        DayMark.Next -> WeekGlyph("\u25CF", WidgetInk.Accent)
        DayMark.Planned -> WeekGlyph("\u25CB", WidgetInk.Faint)
        DayMark.Missed -> WeekGlyph("\u25CB", WidgetInk.Muted)
        DayMark.Rest -> WeekGlyph("\u00A0", WidgetInk.Faint)
        DayMark.None -> WeekGlyph("\u2013", WidgetInk.Faint)
    }

/** One button on the widget: its label, the screen it opens, and the tile [face]
 *  it shows (the platform draws that face's icon). */
internal data class WidgetAction(val label: String, val dest: OpenDest, val face: TileFace)

/**
 * A tall card's featured block: a figure ([figure], a run's distance or time, set
 * large) or a session (the next one, a name in bold), with [note] under it (the
 * run's pace, the next session's date).
 */
internal data class WidgetHero(val text: String, val note: String?, val figure: Boolean)

/**
 * The widget's text, resolved from a snapshot. [status] is the one line under the
 * title where only one fits (the next exercise or run, the next session, the
 * pause's pointer into the app). Taller sizes show [lead] lines first ("Session
 * done", the next session, a pause's pointer) and fill the space left with
 * [items]; [visibleLines] adds "N more items" for the ones cut. Where a card has
 * room for [hero] it shows [heroLead], the hero and [heroItems] instead. [week] is
 * the planned vs done count and [days] the plan week for the week strip.
 * [actions] leads with the Quick Settings tile's action ([tileFace]), then Log.
 */
internal data class WidgetFace(
    val overline: String,
    val title: String,
    val tone: WidgetTone,
    val status: String? = null,
    /** [status] short enough for the 2×1 (the next exercise, the next session's date). */
    val brief: String? = status,
    val lead: List<String> = emptyList(),
    val items: List<String> = emptyList(),
    val hero: WidgetHero? = null,
    /** With [hero]: the lines above it ("Session done"). */
    val heroLead: List<String> = emptyList(),
    /** With [hero]: the lines under it (the run day's other items, the next session's). */
    val heroItems: List<String> = emptyList(),
    val week: String? = null,
    val days: List<WidgetWeekDay> = emptyList(),
    val actions: List<WidgetAction> = emptyList(),
    /** The day the face describes ([overline]'s date), or null when nothing current shows. */
    val day: Long? = null,
)

/** The lone lead line's short form ([WidgetFace.brief]), which a card shows where
 *  that line would wrap past two lines; null without a lone lead line. */
internal val WidgetFace.shortLead: String?
    get() = brief?.takeIf { lead.size == 1 }

/** The widget button for the tile's [face]: the same label, icon and destination. */
internal fun widgetAction(face: TileFace, res: PlatformResources) = WidgetAction(res.getString(face.label), face.dest, face)

/**
 * The face for [snapshot] on local day [today], [recording] while a GPS run
 * records in this process. A missing or stale snapshot, or one written for another
 * day, reads "Open lollop to update" and never shows that day's session as today's.
 */
internal fun widgetFace(snapshot: WidgetSnapshot?, today: Long, res: PlatformResources, recording: Boolean = false): WidgetFace {
    val todayLabel = res.getString(Res.string.widget_overline_today)
    val current = snapshot?.currentOn(today)
    val primary = widgetAction(tileFace(current, recording), res)
    val actions = listOf(primary) + listOfNotNull(widgetAction(TileFace.Log, res).takeIf { primary.dest != OpenDest.Log })
    if (current == null) {
        return WidgetFace(todayLabel, res.getString(Res.string.widget_open_to_update), WidgetTone.Muted, actions = actions)
    }
    val week = current.weekPlanned.takeIf { it > 0 }?.let {
        res.getQuantityString(Res.plurals.main_week_done_count, it, current.weekDone, it)
    }
    // The overline names the day the snapshot describes, so a frame the host keeps
    // past midnight (before the midnight tick lands) still reads true.
    val overline = res.getString(Res.string.widget_overline_day, todayLabel, planDateLabel(current.epochDay))
    val next = current.next?.let { res.getString(Res.string.widget_next, it.title, planDateLabel(it.epochDay)) }
    // The next session as a tall card's hero: its name, its date, then its open items.
    val nextHero = current.next?.takeIf { it.title.isNotBlank() }?.let { n ->
        WidgetHero(n.title, res.getString(Res.string.widget_next_day, planDateLabel(n.epochDay)), figure = false)
    }
    val nextItems = current.next?.let { n -> current.ahead.firstOrNull { it.epochDay == n.epochDay } }
        ?.items.orEmpty().filter { !it.done }.map { itemLine(it, res) }
    val base = WidgetFace(overline, "", WidgetTone.Normal, week = week, days = current.week, actions = actions, day = current.epochDay)
    return when (current.kind) {
        // A home screen is seen by anyone near the phone: the hold's reason stays in
        // the app. The under-18 pause keeps the calm (non-danger) colour.
        WidgetDayKind.Paused -> res.getString(Res.string.widget_paused_body).let { body ->
            base.copy(
                title = res.getString(Res.string.main_status_on_hold),
                tone = if (current.holdReason == HoldReason.Youth) WidgetTone.Normal else WidgetTone.Paused,
                status = body,
                brief = body,
                lead = listOf(body),
            )
        }
        WidgetDayKind.Setup -> res.getString(Res.string.widget_setup_body).let { body ->
            base.copy(title = res.getString(Res.string.widget_setup_title), status = body, brief = body, lead = listOf(body), actions = emptyList())
        }
        WidgetDayKind.NoPlan -> {
            val (title, body) = when (current.planWithheld) {
                PlanWithheld.HealthHold -> Res.string.main_plan_withheld_health_title to Res.string.main_plan_withheld_health_body
                PlanWithheld.NothingToPlan -> Res.string.main_plan_withheld_empty_title to Res.string.widget_setup_body
                null -> Res.string.widget_no_plan to Res.string.widget_no_plan_body
            }
            res.getString(body).let { b -> base.copy(title = res.getString(title), status = b, brief = b, lead = listOf(b)) }
        }
        WidgetDayKind.Rest -> base.copy(
            title = res.getString(if (current.restDay) Res.string.widget_rest_day else Res.string.widget_no_session),
            status = next,
            // "Next: Thu" within the week, the date further out.
            brief = current.next?.let {
                val day = if (it.epochDay - current.epochDay < 7) weekdayShort(LocalDate.fromEpochDays(it.epochDay).dayOfWeek) else planDateLabel(it.epochDay)
                res.getString(Res.string.widget_next_day, day)
            },
            lead = listOfNotNull(next),
            hero = nextHero,
            heroItems = nextItems,
        )
        WidgetDayKind.Lift, WidgetDayKind.Run -> {
            val (open, done) = current.items.partition { !it.done }
            val lines = (open + done).map { itemLine(it, res) }
            val sessionDone = res.getString(Res.string.widget_session_done)
            val firstLift = open.firstOrNull { it.kind == WidgetItemKind.Lift }
            val firstRun = open.firstOrNull { it.kind == WidgetItemKind.Run }
            // A run day's hero is the run's figure: its volume head ("8 km") over its
            // pace, or a detail that is only a figure ("3.4 mi", "45 min", "6 × 800 m");
            // a full summary ("8 km easy") stays a line.
            val heroRun = firstRun?.takeIf {
                current.kind == WidgetDayKind.Run && it.detail.isNotBlank() && (it.pace.isNotBlank() || isFigure(it.detail))
            }
            base.copy(
                title = current.title,
                // The reminder's line: what the next tap logs or tracks.
                status = if (current.sessionDone) {
                    sessionDone
                } else {
                    reminderLine(res, Reminder(current.kind, current.title, current.items)) ?: lines.firstOrNull()
                },
                brief = when {
                    current.sessionDone -> sessionDone
                    current.kind == WidgetDayKind.Run && firstRun != null && firstRun.detail.isNotBlank() -> firstRun.detail
                    firstLift != null -> firstLift.name
                    else -> lines.firstOrNull()
                },
                lead = if (current.sessionDone) listOfNotNull(sessionDone, next) else emptyList(),
                items = lines,
                hero = when {
                    current.sessionDone -> nextHero
                    heroRun != null -> WidgetHero(heroRun.detail, heroRun.pace.ifBlank { null }, figure = true)
                    else -> null
                },
                heroLead = if (current.sessionDone) listOf(sessionDone) else emptyList(),
                heroItems = when {
                    current.sessionDone -> nextItems
                    heroRun != null -> (open - heroRun + done).map { itemLine(it, res) }
                    else -> emptyList()
                },
            )
        }
    }
}

/** Whether [text] is only a figure and its units ("3.4 mi", "45 min", "6 × 800 m"):
 *  it leads with a digit and has no word longer than a unit. */
internal fun isFigure(text: String): Boolean =
    text.firstOrNull()?.isDigit() == true && text.split(' ', '\u00A0').none { t -> t.count(Char::isLetter) > 3 }

private fun itemLine(item: WidgetItem, res: PlatformResources): String {
    val name = when (item.kind) {
        WidgetItemKind.Run -> res.getString(Res.string.main_run)
        WidgetItemKind.Check -> res.getString(Res.string.plan_check_title)
        WidgetItemKind.Lift -> item.name
    }
    val detail = if (item.done) {
        res.getString(Res.string.widget_item_done)
    } else {
        // A run's volume head with its pace range ("8 km · 5:40–6:00 /km"), as the reminder.
        listOf(item.detail.takeIf { it != name }.orEmpty(), item.pace).filter { it.isNotBlank() }.joinToString(" \u00b7 ").ifBlank { null }
    }
    return detail?.let { res.getString(Res.string.widget_item_line, name, it) } ?: name
}

/**
 * The body lines that fit [maxLines]: every [lead] line first, then [items]; when
 * items are cut, the last slot reads "N more items" instead, unless it is the
 * only item slot: that one shows the first item, never a count of everything.
 */
internal fun visibleLines(lead: List<String>, items: List<String>, maxLines: Int, res: PlatformResources): List<String> {
    val shownLead = lead.take(maxLines)
    val room = maxLines - shownLead.size
    if (room <= 0 || items.isEmpty()) return shownLead
    if (items.size <= room || room == 1) return shownLead + items.take(room)
    val shown = items.take(room - 1)
    val cut = items.size - shown.size
    return shownLead + shown + res.getQuantityString(Res.plurals.widget_more_items, cut, cut)
}

/**
 * A character's advance in em, for estimating where a line wraps: about the
 * system font's (Roboto) widths by class, a little wide, so a line estimated to
 * fit does.
 */
private fun charEm(c: Char): Float = when {
    c == ' ' || c == '\u00A0' -> 0.27f
    c in "iljtfrI.,:;·'!|()" -> 0.32f
    c in "mwMW@" -> 0.82f
    c.isUpperCase() -> 0.64f
    c.isDigit() -> 0.57f
    else -> 0.54f
}

/** [text]'s estimated width in em of its font size ([charEm]). */
internal fun textEm(text: String): Float = text.sumOf { charEm(it).toDouble() }.toFloat()
