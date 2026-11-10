package app.milestone.widget

import app.milestone.DistanceUnit
import app.milestone.PrescriptionView
import app.milestone.ViewModel
import app.milestone.actionable
import app.milestone.isDone
import app.milestone.isReturnCheckItem
import app.milestone.isRunItem
import app.milestone.paceLabel
import app.milestone.runFaceHead
import app.milestone.sessionDiscipline
import app.milestone.today
import app.milestone.SessionPlanView
import kotlinx.datetime.TimeZone
import kotlinx.serialization.Serializable

/** Bumped on any change to [WidgetSnapshot] that an older reader would misread;
 *  a file with another version reads as missing ("Open lollop to update"). */
internal const val WIDGET_SNAPSHOT_VERSION = 1

/** What today is, for the widget and the Quick Settings tile. */
@Serializable
internal enum class WidgetDayKind {
    /** Today's plan session is a lifting (or calisthenics) session. */
    Lift,

    /** Today's plan session is a run. */
    Run,

    /** A plan exists but has no session today ([WidgetSnapshot.restDay] says if it is a plan rest day). */
    Rest,

    /** Training is paused ([WidgetSnapshot.holdReason]); also the under-18 pause. */
    Paused,

    /** A profile exists but no plan ([WidgetSnapshot.planWithheld] says why, if withheld). */
    NoPlan,

    /** No profile yet: guided setup has not run. */
    Setup,
}

@Serializable
internal enum class WidgetItemKind { Lift, Run, Check }

/** One item of today's session. [name] is the exercise for a lift (a run or the
 *  return check is named at render time, from resources); [detail] is the face
 *  line the Today card shows (sets × reps @ weight, a run's distance); [pace] is a
 *  run's target pace range in the display unit when [detail] is its volume head. */
@Serializable
internal data class WidgetItem(
    val kind: WidgetItemKind,
    val name: String = "",
    val detail: String = "",
    val done: Boolean = false,
    val pace: String = "",
)

/** A planned session day after the snapshot's day ([WidgetSnapshot.ahead]):
 *  [kind] is [WidgetDayKind.Lift] or [WidgetDayKind.Run]. */
@Serializable
internal data class WidgetAheadDay(
    val epochDay: Long,
    val kind: WidgetDayKind,
    val title: String = "",
    val items: List<WidgetItem> = emptyList(),
)

/** The next planned session when today has none, or today's is done. */
@Serializable
internal data class WidgetNext(val epochDay: Long, val title: String)

/**
 * Everything the home screen widget and the Quick Settings tile show, written by
 * the app on every published model ([WidgetSurfaces]) so neither surface boots the
 * Rust core. [epochDay] is the local day it describes: a reader whose today is a
 * different day treats it as missing ([WidgetSnapshotStore.today]), and [stale] is
 * set at midnight, on a clock or a time zone change (Android's `AndroidWidgets.dayChanged`) until
 * the app writes one for the new day (or, after a zone change within the day,
 * for the same day in the new zone).
 */
@Serializable
internal data class WidgetSnapshot(
    val version: Int = WIDGET_SNAPSHOT_VERSION,
    val epochDay: Long,
    val stale: Boolean = false,
    val kind: WidgetDayKind,
    /** Today's session title (Lift / Run), else empty. */
    val title: String = "",
    val items: List<WidgetItem> = emptyList(),
    /** Today's plan session is complete. */
    val sessionDone: Boolean = false,
    /** Today's session has an open run item: the widget offers Track run. */
    val trackable: Boolean = false,
    /** [WidgetDayKind.Rest] only: today is a plan rest day (else simply no session today). */
    val restDay: Boolean = false,
    val next: WidgetNext? = null,
    /** This plan week, planned vs done (planned 0 = nothing to count). */
    val weekDone: Int = 0,
    val weekPlanned: Int = 0,
    /** [WidgetDayKind.Paused]: the core's hold reason. The widget shows only that
     *  training is paused; the reason stays in the app. */
    val holdReason: String? = null,
    /** [WidgetDayKind.NoPlan]: the core's plan_withheld (health_hold | nothing_to_plan), or null. */
    val planWithheld: String? = null,
    /** The plan's session days after [epochDay] that the model knows (this and
     *  next week's plan and the next session), for the session reminder when the
     *  app has not run since; empty while paused, without a plan or a profile. */
    val ahead: List<WidgetAheadDay> = emptyList(),
    /** The time zone the snapshot was written in ([TimeZone] id); empty in a file
     *  written before it was recorded. */
    val zone: String = "",
    /** This plan week day by day, for the widget's week strip; empty while paused,
     *  without a plan, and in a file written before it was recorded. */
    val week: List<WidgetWeekDay> = emptyList(),
)

/** How one day of the plan week reads on the widget's week strip, the app's
 *  Today week strip in short ([app.milestone.ThisWeekCard]). */
@Serializable
internal enum class WidgetWeekMark {
    /** Trained (done, or done lighter). */
    Done,

    /** The plan's next session (adjusted or not). */
    Next,

    /** A session still to come. */
    Planned,

    /** A past session not logged, or only partly done. */
    Missed,

    /** A plan rest day. */
    Rest,

    /** Before the plan started. */
    None,
}

@Serializable
internal data class WidgetWeekDay(val epochDay: Long, val mark: WidgetWeekMark)

private fun weekMarkOf(s: SessionPlanView): WidgetWeekMark = when {
    s.isDone -> WidgetWeekMark.Done
    s.status == "next" || s.status == "adjusted" -> WidgetWeekMark.Next
    s.status == "missed" || s.status == "partial" -> WidgetWeekMark.Missed
    s.status == "rest" -> WidgetWeekMark.Rest
    s.status == "before_plan" -> WidgetWeekMark.None
    else -> WidgetWeekMark.Planned
}

/**
 * The snapshot of [model] for its own day ([ViewModel.today], the day the core
 * last set), written in time zone [zone]. Mirrors the Today screen's gates: a hold
 * wins over any plan, then the setup / plan prompts, then today's session or the
 * next one.
 */
internal fun widgetSnapshotOf(
    model: ViewModel,
    distanceUnit: DistanceUnit,
    zone: String = TimeZone.currentSystemDefault().id,
): WidgetSnapshot {
    val today = model.today()
    val consistency = model.consistency?.takeIf { model.week_plan.isNotEmpty() && it.this_week_planned > 0 }
    val base = WidgetSnapshot(
        epochDay = today,
        kind = WidgetDayKind.Setup,
        weekDone = consistency?.this_week_done ?: 0,
        weekPlanned = consistency?.this_week_planned ?: 0,
        zone = zone,
        week = if (consistency == null) emptyList() else model.week_plan.map { WidgetWeekDay(it.epoch_day, weekMarkOf(it)) },
    )
    if (model.train_blocked) {
        return base.copy(kind = WidgetDayKind.Paused, holdReason = model.hold_reason, weekDone = 0, weekPlanned = 0, week = emptyList())
    }
    val ns = model.next_session
    if (ns == null) {
        return if (model.profile == null) {
            base
        } else {
            base.copy(kind = WidgetDayKind.NoPlan, planWithheld = model.plan_withheld)
        }
    }
    val ahead = aheadDays(model, ns, today, distanceUnit)
    // Once today's session is finished the core moves next_session on; today's
    // row in week_plan still says what was done.
    val doneToday = model.week_plan.firstOrNull { it.epoch_day == today && it.isDone && it.items.isNotEmpty() }
    val session = ns.takeIf { it.epoch_day == today } ?: doneToday
        ?: return base.copy(
            kind = WidgetDayKind.Rest,
            restDay = model.week_plan.any { it.epoch_day == today && it.status == "rest" },
            next = WidgetNext(ns.epoch_day, ns.title),
            ahead = ahead,
        )
    val discipline = sessionDiscipline(session.session_type)
    val items = session.items.map { widgetItemOf(it, session.session_type, distanceUnit) }
    val done = session === doneToday
    return base.copy(
        kind = if (discipline == "Run") WidgetDayKind.Run else WidgetDayKind.Lift,
        title = session.title,
        items = items,
        sessionDone = done,
        trackable = !done && session.actionable && items.any { it.kind == WidgetItemKind.Run && !it.done },
        next = WidgetNext(ns.epoch_day, ns.title).takeIf { done },
        ahead = ahead,
    )
}

/** The planned (not yet trained) lift and run days after [today] in this and
 *  next week's plan and [ns], by day. */
private fun aheadDays(model: ViewModel, ns: SessionPlanView, today: Long, unit: DistanceUnit): List<WidgetAheadDay> =
    (model.week_plan + model.next_week_plan + ns)
        .filter { it.epoch_day > today && it.items.isNotEmpty() && (it.actionable || it.status == "planned") }
        .distinctBy { it.epoch_day }
        .sortedBy { it.epoch_day }
        .mapNotNull { s ->
            val discipline = sessionDiscipline(s.session_type)
            val kind = when (discipline) {
                "Run" -> WidgetDayKind.Run
                "Lift" -> WidgetDayKind.Lift
                else -> return@mapNotNull null
            }
            WidgetAheadDay(s.epoch_day, kind, s.title, s.items.map { widgetItemOf(it, s.session_type, unit) })
        }

private fun widgetItemOf(item: PrescriptionView, sessionType: String, unit: DistanceUnit): WidgetItem {
    val check = isReturnCheckItem(item, sessionType)
    val run = isRunItem(item, sessionType)
    val runHead = if (run) runFaceHead(item, unit) else null
    return WidgetItem(
        kind = when {
            check -> WidgetItemKind.Check
            run -> WidgetItemKind.Run
            else -> WidgetItemKind.Lift
        },
        name = if (check || run) "" else item.exercise,
        detail = when {
            check -> ""
            run -> runHead ?: item.face_lead
            else -> item.face_lead
        },
        done = item.done,
        // Only beside a volume head: a run's full summary already states its pace.
        pace = item.run_target?.takeIf { runHead != null }?.paceLabel(unit).orEmpty(),
    )
}
