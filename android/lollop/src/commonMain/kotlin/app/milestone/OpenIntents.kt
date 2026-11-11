package app.milestone

import kotlinx.coroutines.flow.MutableStateFlow

/**
 * The one explicit "open the app at X" contract shared by every system surface
 * (Live Update notifications, the home screen widget, the Quick Settings tile, a
 * reminder). On Android `MainActivity` reads the open intent (OpenIntents.android.kt)
 * on a cold start and in onNewIntent; on iOS a `lollop://open/<dest>` URL or a
 * reminder tap asks for it. Either way the host's [OpenRequests] carries it to
 * [LollopApp], whose scaffold applies the destination once boot has run.
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

/**
 * A pending [OpenDest] not yet applied, the host's deep-link state: an open intent or
 * URL sets it, [CoachScaffold] applies it once boot has run and clears it, so a cold
 * start waits for the model before it navigates and a request is applied once.
 */
internal typealias OpenRequests = MutableStateFlow<OpenDest?>

/** The URL scheme of [openDestOfUrl]. */
internal const val OPEN_URL_SCHEME = "lollop"

/**
 * The destination a `lollop://open/<dest>` URL asks for ([OpenDest.wire]; an unknown
 * dest opens Today, as an open intent's does), or null when [url] is no open URL.
 */
internal fun openDestOfUrl(url: String): OpenDest? {
    val prefix = "$OPEN_URL_SCHEME://open"
    if (!url.startsWith(prefix, ignoreCase = true)) return null
    val rest = url.substring(prefix.length)
    if (rest.isNotEmpty() && rest[0] != '/' && rest[0] != '?' && rest[0] != '#') return null
    val dest = rest.removePrefix("/").substringBefore('?').substringBefore('#').substringBefore('/')
    return OpenDest.fromWire(dest)
}

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
 * Without a tracker ([canTrack] false: the platform records no runs) `track` opens
 * the run editor on Today instead, under the same rules as the other destinations.
 * The guided setup is handled by the caller: while it shows, it stays.
 */
internal fun routeOpen(
    dest: OpenDest,
    nav: OpenNav,
    model: ViewModel,
    sheetBusy: Boolean,
    trackerPinned: Boolean,
    canTrack: Boolean = true,
): OpenNav {
    if (dest == OpenDest.Track && canTrack) return nav.copy(showTracker = true)
    if (nav.showTracker && trackerPinned) return nav
    val today = nav.copy(showTracker = false, selected = Dest.Today)
    if (nav.sheetOpen && sheetBusy) return today
    return when (dest) {
        OpenDest.Log -> today.copy(sheetOpen = true, sheetMode = LogMode.Chooser, sheetPlanExercise = null, sheetPlanDay = null)
        OpenDest.LogSet -> today.copy(sheetOpen = true, sheetMode = LogMode.Set, sheetPlanExercise = logSetTarget(model), sheetPlanDay = null)
        OpenDest.Track -> today.copy(sheetOpen = true, sheetMode = LogMode.Run, sheetPlanExercise = null, sheetPlanDay = null)
        OpenDest.Today -> today.copy(sheetOpen = false, sheetMode = LogMode.Chooser, sheetPlanExercise = null, sheetPlanDay = null)
    }
}

/**
 * The exercise `log_set` prefills: the open item of today's lift session, by the
 * same rule as the Log chooser's "Today's plan" and "Resume" rows. Null (a blank
 * set editor) under a training hold, on a run or rest day, or without a plan.
 */
internal fun logSetTarget(model: ViewModel): String? {
    if (model.train_blocked) return null
    val plan = model.todaysPlan()
    val item = if (plan != null && plan.session_type.discipline == SessionDiscipline.Lift) {
        firstOpenItem(plan.items) ?: plan.items.first()
    } else {
        val started = model.resumableLiftDay() ?: return null
        firstOpenItem(started.items) ?: started.items.last()
    }
    return item.exercise.takeIf { it.isNotBlank() }
}
