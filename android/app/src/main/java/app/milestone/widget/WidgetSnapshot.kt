package app.milestone.widget

import android.content.Context
import android.util.Log
import app.milestone.DistanceUnit
import app.milestone.PrescriptionView
import app.milestone.ViewModel
import app.milestone.isReturnCheckItem
import app.milestone.runFaceHead
import app.milestone.sessionDiscipline
import app.milestone.today
import app.milestone.todayEpochDay
import app.milestone.requestTileRefresh
import java.io.File
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
 *  line the Today card shows (sets × reps @ weight, a run's distance). */
@Serializable
internal data class WidgetItem(
    val kind: WidgetItemKind,
    val name: String = "",
    val detail: String = "",
    val done: Boolean = false,
)

/** The next planned session when today has none. */
@Serializable
internal data class WidgetNext(val epochDay: Long, val title: String)

/**
 * Everything the home screen widget and the Quick Settings tile show, written by
 * the app on every published model ([WidgetSync]) so neither surface boots the
 * Rust core. [epochDay] is the local day it describes: a reader whose today is a
 * different day treats it as missing ([WidgetSnapshotStore.today]), and [stale] is
 * set at midnight, on a date change or a time zone change ([TodayWidgetReceiver])
 * until the app writes a fresh one.
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
    /** [WidgetDayKind.Paused]: the core's hold reason and tier, resolved to text at render. */
    val holdReason: String? = null,
    val safetyTier: String? = null,
    val pregnancyWarning: Boolean = false,
    /** Under-18 pause: this week's active minutes against the weekly target, when known. */
    val youthMinutes: Int? = null,
    val youthTargetMinutes: Int = 0,
    /** [WidgetDayKind.NoPlan]: the core's plan_withheld (health_hold | nothing_to_plan), or null. */
    val planWithheld: String? = null,
)

/**
 * The snapshot of [model] for its own day ([ViewModel.today], the day the core
 * last set). Mirrors the Today screen's gates: a hold wins over any plan, then
 * the setup / plan prompts, then today's session or the next one.
 */
internal fun widgetSnapshotOf(model: ViewModel, distanceUnit: DistanceUnit): WidgetSnapshot {
    val today = model.today()
    val consistency = model.consistency?.takeIf { model.week_plan.isNotEmpty() && it.this_week_planned > 0 }
    val base = WidgetSnapshot(
        epochDay = today,
        kind = WidgetDayKind.Setup,
        weekDone = consistency?.this_week_done ?: 0,
        weekPlanned = consistency?.this_week_planned ?: 0,
    )
    if (model.train_blocked) {
        val youth = model.hold_reason == "youth"
        return base.copy(
            kind = WidgetDayKind.Paused,
            holdReason = model.hold_reason,
            safetyTier = model.safety_tier,
            pregnancyWarning = model.profile?.health?.pregnancy_warning_sign == true,
            youthMinutes = model.youth_activity?.minutes?.takeIf { youth },
            youthTargetMinutes = model.youth_activity?.weekly_target_minutes?.takeIf { youth } ?: 0,
            weekDone = 0,
            weekPlanned = 0,
        )
    }
    val ns = model.next_session
    if (ns == null) {
        return if (model.profile == null) {
            base
        } else {
            base.copy(kind = WidgetDayKind.NoPlan, planWithheld = model.plan_withheld)
        }
    }
    if (ns.epoch_day != today) {
        return base.copy(
            kind = WidgetDayKind.Rest,
            restDay = model.week_plan.any { it.epoch_day == today && it.status == "rest" },
            next = WidgetNext(ns.epoch_day, ns.title),
        )
    }
    val discipline = sessionDiscipline(ns.session_type)
    val actionable = ns.status == "next" || ns.status == "adjusted"
    val items = ns.items.map { widgetItemOf(it, ns.session_type, discipline, distanceUnit) }
    val done = model.session_done?.let { it.epoch_day == today && it.items.isNotEmpty() } == true ||
        (ns.items.isNotEmpty() && ns.items.all { it.done })
    return base.copy(
        kind = if (discipline == "Run") WidgetDayKind.Run else WidgetDayKind.Lift,
        title = ns.title,
        items = items,
        sessionDone = done,
        trackable = actionable && !done && items.any { it.kind == WidgetItemKind.Run && !it.done },
    )
}

private fun widgetItemOf(item: PrescriptionView, sessionType: String, discipline: String, unit: DistanceUnit): WidgetItem {
    val check = isReturnCheckItem(item, sessionType)
    val run = !check && (discipline == "Run" || item.exercise.isEmpty())
    return WidgetItem(
        kind = when {
            check -> WidgetItemKind.Check
            run -> WidgetItemKind.Run
            else -> WidgetItemKind.Lift
        },
        name = if (check || run) "" else item.exercise,
        detail = when {
            check -> ""
            run -> runFaceHead(item, unit) ?: item.face_lead
            else -> item.face_lead
        },
        done = item.done,
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
     * changed snapshot redraws the Quick Settings tile, which shows the same day.
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
        return true
    }

    /**
     * Mark the stored snapshot stale: always when [force] (a time zone change),
     * else only when it describes a day other than [today] (the midnight tick, a
     * date or clock change). The next published model writes a fresh one.
     */
    fun markStale(ctx: Context, force: Boolean, today: Long = todayEpochDay()): Boolean = synchronized(lock) {
        val current = read(ctx) ?: return false
        if (current.stale || (!force && current.epochDay == today)) return false
        write(ctx, current.copy(stale = true))
    }
}
