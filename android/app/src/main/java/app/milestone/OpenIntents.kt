package app.milestone

import android.app.PendingIntent
import android.content.Context
import android.content.Intent

/**
 * The one explicit "open the app at X" contract shared by every system surface
 * (Live Update notifications, the home screen widget, the Quick Settings tile).
 * [MainActivity] reads [ACTION_OPEN] + [EXTRA_DEST] on a cold start and in
 * onNewIntent; [CoachScaffold] applies the destination once boot has run.
 */
internal enum class OpenDest(val wire: String) {
    /** The Today tab. */
    Today("today"),

    /** The Log chooser sheet over Today. */
    Log("log"),

    /** The run tracker (its pre-start screen, or the live run). */
    Track("track"),

    /** The set editor, prefilled from today's next open lift item, else blank. */
    LogSet("log_set"),
    ;

    companion object {
        /** An unknown or missing value opens Today. */
        fun fromWire(wire: String?): OpenDest = entries.firstOrNull { it.wire == wire } ?: Today
    }
}

internal const val ACTION_OPEN = "app.milestone.action.OPEN"
internal const val EXTRA_DEST = "dest"

/** The explicit intent that opens [MainActivity] at [dest]. */
internal fun openIntent(ctx: Context, dest: OpenDest): Intent =
    Intent(ACTION_OPEN)
        .setClass(ctx, MainActivity::class.java)
        .putExtra(EXTRA_DEST, dest.wire)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

/**
 * Immutable activity PendingIntent for [openIntent]. PendingIntents match on the
 * intent without its extras, so each destination gets its own request code;
 * FLAG_UPDATE_CURRENT keeps the extra current if an older one exists.
 */
internal fun openPendingIntent(ctx: Context, dest: OpenDest): PendingIntent =
    PendingIntent.getActivity(
        ctx,
        OPEN_REQUEST_BASE + dest.ordinal,
        openIntent(ctx, dest),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

/** The destination an incoming intent asks for, or null when it isn't an open intent. */
internal fun openDestOf(intent: Intent?): OpenDest? =
    if (intent?.action == ACTION_OPEN) OpenDest.fromWire(intent.getStringExtra(EXTRA_DEST)) else null

// Request codes 0x4F50_0000.. (no other getActivity caller in the app uses this range).
private const val OPEN_REQUEST_BASE = 0x4F500000

/** The scaffold navigation an open intent can change ([CoachScaffold]'s hoisted state). */
internal data class OpenNav(
    val showTracker: Boolean,
    val selected: Dest,
    val sheetOpen: Boolean,
    val sheetMode: LogMode,
    val sheetPlanExercise: String?,
    val sheetPlanDay: Long?,
)

/**
 * Where [dest] lands from [nav]. Nothing typed is thrown away: an open editor with
 * unsaved edits or a set loop in progress ([sheetBusy]) stays as it is, and a
 * tracker holding an armed or locating service ([trackerPinned]) stays in front,
 * since it is the only place to cancel that service. A tracker over a recording run
 * steps aside like Back does (the run keeps recording; the in-app chip leads back).
 * `track` always opens the tracker; during a hold it shows the safety banner itself.
 * The guided setup is handled by the caller: while it shows, it stays.
 */
internal fun routeOpen(
    dest: OpenDest,
    nav: OpenNav,
    model: ViewModel,
    sheetBusy: Boolean,
    trackerPinned: Boolean,
): OpenNav {
    if (dest == OpenDest.Track) return nav.copy(showTracker = true)
    if (nav.showTracker && trackerPinned) return nav
    val today = nav.copy(showTracker = false, selected = Dest.Today)
    if (nav.sheetOpen && sheetBusy) return today
    return when (dest) {
        OpenDest.Today -> today.copy(sheetOpen = false, sheetMode = LogMode.Chooser, sheetPlanExercise = null, sheetPlanDay = null)
        OpenDest.Log -> today.copy(sheetOpen = true, sheetMode = LogMode.Chooser, sheetPlanExercise = null, sheetPlanDay = null)
        OpenDest.LogSet -> today.copy(sheetOpen = true, sheetMode = LogMode.Set, sheetPlanExercise = logSetTarget(model), sheetPlanDay = null)
        OpenDest.Track -> today // returned above
    }
}

/**
 * The exercise `log_set` prefills: the open item of today's lift session, by the
 * same rule as the Log chooser's "Today's plan" and "Resume" rows. Null (a blank
 * set editor) under a training hold, on a run or rest day, or without a plan.
 */
internal fun logSetTarget(model: ViewModel): String? {
    if (model.train_blocked) return null
    val today = model.today()
    val ns = model.next_session
    if (ns != null && ns.epoch_day == today && ns.items.isNotEmpty() &&
        (ns.status == "next" || ns.status == "adjusted") && sessionDiscipline(ns.session_type) == "Lift"
    ) {
        return (firstOpenItem(ns.items) ?: ns.items.first()).exercise.takeIf { it.isNotBlank() }
    }
    val started = model.week_plan.firstOrNull {
        it.epoch_day == today && it.items.isNotEmpty() && sessionDiscipline(it.session_type) == "Lift" &&
            it.items.any { i -> i.sets_done > 0 || i.logged_sets.isNotEmpty() }
    } ?: return null
    return (firstOpenItem(started.items) ?: started.items.last()).exercise.takeIf { it.isNotBlank() }
}
