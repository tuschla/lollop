package app.milestone.widget

import android.content.Context
import android.util.Log
import app.milestone.DistanceUnit
import app.milestone.PrescriptionView
import app.milestone.ViewModel
import app.milestone.actionable
import app.milestone.isDone
import app.milestone.isReturnCheckItem
import app.milestone.isRunItem
import app.milestone.runFaceHead
import app.milestone.sessionDiscipline
import app.milestone.today
import app.milestone.todayEpochDay
import app.milestone.SessionPlanView
import app.milestone.SessionReminder
import app.milestone.requestTileRefresh
import java.io.File
import java.time.ZoneId
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

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
 * the app on every published model ([WidgetSync]) so neither surface boots the
 * Rust core. [epochDay] is the local day it describes: a reader whose today is a
 * different day treats it as missing ([WidgetSnapshotStore.today]), and [stale] is
 * set at midnight, on a clock or a time zone change ([TodayWidgetReceiver]) until
 * the app writes a fresh one.
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
    /** The plan's session days after [epochDay] that the model knows (this week's
     *  plan and the next session), for the session reminder when the app has not
     *  run since; empty while paused, without a plan or a profile. */
    val ahead: List<WidgetAheadDay> = emptyList(),
    /** The time zone the snapshot was written in ([ZoneId] id); empty in a file
     *  written before it was recorded. */
    val zone: String = "",
)

/**
 * The snapshot of [model] for its own day ([ViewModel.today], the day the core
 * last set), written in time zone [zone]. Mirrors the Today screen's gates: a hold
 * wins over any plan, then the setup / plan prompts, then today's session or the
 * next one.
 */
internal fun widgetSnapshotOf(
    model: ViewModel,
    distanceUnit: DistanceUnit,
    zone: String = ZoneId.systemDefault().id,
): WidgetSnapshot {
    val today = model.today()
    val consistency = model.consistency?.takeIf { model.week_plan.isNotEmpty() && it.this_week_planned > 0 }
    val base = WidgetSnapshot(
        epochDay = today,
        kind = WidgetDayKind.Setup,
        weekDone = consistency?.this_week_done ?: 0,
        weekPlanned = consistency?.this_week_planned ?: 0,
        zone = zone,
    )
    if (model.train_blocked) {
        return base.copy(kind = WidgetDayKind.Paused, holdReason = model.hold_reason, weekDone = 0, weekPlanned = 0)
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

/** The planned (not yet trained) lift and run days after [today] in this week's
 *  plan and [ns], by day. */
private fun aheadDays(model: ViewModel, ns: SessionPlanView, today: Long, unit: DistanceUnit): List<WidgetAheadDay> =
    (model.week_plan + ns)
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
        pace = item.run_target?.takeIf { runHead != null }?.let {
            if (unit == DistanceUnit.Mi) it.pace_label_mi.ifBlank { it.pace_label_km } else it.pace_label_km.ifBlank { it.pace_label_mi }
        }.orEmpty(),
    )
}

/**
 * The snapshot file (`filesDir/widget-today.json`). Writes go through a temp file
 * and a rename, so a reader never sees half a file. [revision] ticks on every
 * write or stale mark, which recomposes a widget session that is still running.
 */
internal object WidgetSnapshotStore {
    const val FILE_NAME = "widget-today.json"

    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val lock = Any()
    private val _revision = MutableStateFlow(0L)
    val revision: StateFlow<Long> = _revision.asStateFlow()
    private val _staleMarks = MutableStateFlow(0L)

    /** Ticks on every stale mark ([markStale]), so [WidgetSync] rewrites the
     *  snapshot from the model the app process holds: a time zone change that
     *  keeps the local day publishes no new model. */
    val staleMarks: StateFlow<Long> = _staleMarks.asStateFlow()

    private fun file(ctx: Context) = File(ctx.filesDir, FILE_NAME)

    /** The stored snapshot as written, or null when missing, unreadable or of another version. */
    fun read(ctx: Context): WidgetSnapshot? = synchronized(lock) {
        val f = file(ctx)
        if (!f.exists()) return null
        runCatching { json.decodeFromString<WidgetSnapshot>(f.readText()) }
            .onFailure { Log.w("WidgetSnapshot", "unreadable snapshot", it) }
            .getOrNull()
            ?.takeIf { it.version == WIDGET_SNAPSHOT_VERSION }
    }

    /**
     * The stored snapshot when it describes [today] and is not marked stale; null
     * otherwise, so a snapshot from another day never shows as today.
     */
    fun today(ctx: Context, today: Long = todayEpochDay()): WidgetSnapshot? =
        read(ctx)?.takeIf { !it.stale && it.epochDay == today }

    /**
     * Store [snapshot]; false (and no write) when the file already holds it. A
     * changed snapshot redraws the Quick Settings tile, which shows the same day,
     * and updates a posted session reminder ([SessionReminder.onSnapshot]).
     */
    fun write(ctx: Context, snapshot: WidgetSnapshot): Boolean {
        synchronized(lock) {
            if (read(ctx) == snapshot) return false
            val f = file(ctx)
            val tmp = File(f.parentFile, "$FILE_NAME.tmp")
            tmp.writeText(json.encodeToString(WidgetSnapshot.serializer(), snapshot))
            if (!tmp.renameTo(f)) {
                tmp.delete()
                error("could not replace $f")
            }
            _revision.value += 1
        }
        requestTileRefresh(ctx)
        SessionReminder.onSnapshot(ctx, snapshot)
        return true
    }

    /**
     * Mark the stored snapshot stale: always when [force] (a time zone change),
     * else only when it describes a day other than [today] (the midnight tick, a
     * clock change). The next published model, or [WidgetSync] at once when the
     * app process holds a model, writes a fresh one.
     */
    fun markStale(ctx: Context, force: Boolean, today: Long = todayEpochDay()): Boolean {
        val marked = synchronized(lock) {
            val current = read(ctx) ?: return false
            if (current.stale || (!force && current.epochDay == today)) return false
            write(ctx, current.copy(stale = true))
        }
        if (marked) _staleMarks.value += 1
        return marked
    }
}
