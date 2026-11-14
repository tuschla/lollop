package app.milestone.widget

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DayMark
import app.milestone.PlatformResources
import app.milestone.RestNextSet
import app.milestone.RestRun
import app.milestone.RunSurfacePhase
import app.milestone.RunSurfaceState
import app.milestone.canPause
import app.milestone.countdownText
import app.milestone.getString
import app.milestone.planDateLabel
import app.milestone.resources.Res
import app.milestone.resources.log_rest_notif_done_title
import app.milestone.resources.log_rest_notif_title
import app.milestone.resources.run_stat_distance
import app.milestone.resources.track_action_pause
import app.milestone.resources.track_action_resume
import app.milestone.resources.track_btn_stop
import app.milestone.resources.track_stat_elapsed
import app.milestone.resources.track_stat_pace_now
import app.milestone.runSurfaceText
import app.milestone.weekdayStripLabel
import kotlinx.datetime.DatePeriod
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import kotlinx.datetime.atStartOfDayIn
import kotlinx.datetime.plus
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

// What the iOS widget extension draws (iosApp/LolloWidgets): the home and lock
// screen widgets, the Control Center control and the Live Activities of a run and
// a rest. The extension runs no Kotlin: the app resolves every text, line cut,
// glyph, icon and colour here and hands it over as JSON. Its Swift types
// (iosApp/Shared/SurfacePayload.swift) are generated from these classes by the
// host test GeneratedSourcesTest, which also writes the fixture the Swift tests read.

/** Bumped on any change an older widget extension would misread; a file of another
 *  version reads as missing. The extension's copy is generated (GeneratedSourcesTest). */
internal const val IOS_WIDGET_PAYLOAD_VERSION = 2

internal val iosSurfaceJson = Json { encodeDefaults = true }

/**
 * What the widgets and the control show (Swift `WidgetPayload`), written on every
 * change by `IosWidgetSurface`. [face] and [control] hold while [current] covers the
 * reader's clock and time zone; [staleFace] and [staleControl] ("Open lollo to
 * update") otherwise. [light] and [dark] are the colours for the system's light
 * and dark appearance (the same when the app forces one).
 */
@Serializable
internal data class IosWidgetPayload(
    val version: Int = IOS_WIDGET_PAYLOAD_VERSION,
    val current: IosCurrentDay?,
    val face: IosWidgetFace,
    val staleFace: IosWidgetFace,
    val control: IosControlFace,
    val staleControl: IosControlFace,
    val light: IosWidgetPalette,
    val dark: IosWidgetPalette,
)

/** The local day the snapshot describes, from [startMs] to [endMs] (epoch ms, the
 *  next local midnight) in time zone [zone]: Android's day rule ([WidgetSnapshot.currentOn]
 *  and the stale mark of a time zone change) as the extension checks it. */
@Serializable
internal data class IosCurrentDay(val zone: String, val startMs: Long, val endMs: Long)

/** [WidgetFace] as the extension draws it; [date] is the short date alone, the
 *  overline of a narrow card. */
@Serializable
internal data class IosWidgetFace(
    val overline: String,
    val date: String?,
    val title: String,
    val titleInk: WidgetInk,
    val status: String?,
    val brief: String?,
    val body: IosWidgetLines,
    /** [body] with the lone lead line in its short form ([WidgetFace.shortLead]),
     *  for a card where that line would wrap past two lines. */
    val shortBody: IosShortBody?,
    val hero: IosWidgetHero?,
    val week: IosWidgetWeek?,
    val actions: List<IosWidgetAction>,
)

/** A card's body lines: [fits] at index N holds what N line slots show
 *  ([visibleLines]), up to every line; the first [lead] lines are lead lines. */
@Serializable
internal data class IosWidgetLines(val lead: Int, val fits: List<List<String>>)

/** The short body and the width in em ([textEm]) of the lead line it replaces. */
@Serializable
internal data class IosShortBody(val leadEm: Float, val body: IosWidgetLines)

/** [WidgetHero] with the lines a card shows around it ([WidgetFace.heroLead],
 *  [WidgetFace.heroItems]). */
@Serializable
internal data class IosWidgetHero(val text: String, val note: String?, val figure: Boolean, val body: IosWidgetLines)

/** The week block: the count [text], [done] of [planned] for the lock screen
 *  gauge, and the plan week Monday to Sunday ([days], empty without a whole week). */
@Serializable
internal data class IosWidgetWeek(val text: String, val done: Int, val planned: Int, val days: List<IosWidgetDay>)

/** A week-strip day: its two-letter weekday, its mark ([DayMark.glyph])
 *  and whether it is the face's day. */
@Serializable
internal data class IosWidgetDay(val label: String, val glyph: String, val ink: WidgetInk, val today: Boolean)

/** A widget button: its label, the `lollo://open/<dest>` destination and its icon. */
@Serializable
internal data class IosWidgetAction(val label: String, val dest: String, val icon: SurfaceIcon)

/** The Control Center control: Android's Quick Settings tile face ([TileFace]). */
@Serializable
internal data class IosControlFace(val label: String, val subtitle: String?, val dest: String, val symbol: String)

/** One appearance's widget colours as "#AARRGGBB", one per [WidgetInk]. */
@Serializable
internal data class IosWidgetPalette(
    val ground: String,
    val body: String,
    val muted: String,
    val faint: String,
    val accent: String,
    val onAccent: String,
    val accentTint: String,
    val danger: String,
    val done: String,
)

/** What the extension shows before the app has written a payload; bundled with
 *  it (iosApp/LolloWidgets/fallback.json, GeneratedSourcesTest). */
@Serializable
internal data class IosWidgetFallback(val face: IosWidgetFace, val control: IosControlFace)

/**
 * The run's Live Activity (Android's run notification and its chip): the
 * notification's [title], the chip ([chip]: a state word, or the moving time
 * while [recording], when the extension counts the moving time on its own),
 * [text] in place of the figures where Android's card shows a fixed line, a
 * guided run's [step] line, the moving time ([movingSec], [time]), [distance] and
 * the current [pace] (null where Android's line has none), the stat and button
 * labels. [canPause]: Pause/Resume shows. [accentLight] / [accentDark]: the
 * theme's accent.
 */
@Serializable
internal data class IosRunLive(
    val recording: Boolean,
    val title: String,
    val chip: String,
    val text: String?,
    val step: String?,
    val movingSec: Long,
    val time: String,
    val distance: String?,
    val pace: String?,
    val movingLabel: String,
    val distanceLabel: String,
    val paceLabel: String,
    val canPause: Boolean,
    val paused: Boolean,
    val pauseLabel: String,
    val resumeLabel: String,
    val stopLabel: String,
    val accentLight: String,
    val accentDark: String,
)

/**
 * The rest's Live Activity (Android's countdown notification): "Resting ·
 * <exercise>", the countdown from [startMs] to [endMs] (wall clock, epoch ms),
 * the next set's line ([next], "Next: Set 3 of 4 · 100 kg × 5") and [doneTitle]
 * for a countdown that ran out while the app could not end it.
 */
@Serializable
internal data class IosRestLive(
    val title: String,
    val next: String?,
    val startMs: Long,
    val endMs: Long,
    val doneTitle: String,
    val accentLight: String,
    val accentDark: String,
)

internal fun Color.hex(): String = "#" + toArgb().toUInt().toString(16).padStart(8, '0').uppercase()

private fun paletteOf(theme: AppTheme, dark: Boolean): IosWidgetPalette {
    val p = if (dark) theme.dark else theme.light
    fun hex(ink: WidgetInk) = p.ink(ink, dark).hex()
    return IosWidgetPalette(
        ground = hex(WidgetInk.Ground),
        body = hex(WidgetInk.Body),
        muted = hex(WidgetInk.Muted),
        faint = hex(WidgetInk.Faint),
        accent = hex(WidgetInk.Accent),
        onAccent = hex(WidgetInk.OnAccent),
        accentTint = hex(WidgetInk.AccentTint),
        danger = hex(WidgetInk.Danger),
        done = hex(WidgetInk.Done),
    )
}

private fun linesOf(lead: List<String>, items: List<String>, res: PlatformResources) =
    IosWidgetLines(lead.size, (0..lead.size + items.size).map { visibleLines(lead, items, it, res) })

private fun iosFaceOf(face: WidgetFace, snapshot: WidgetSnapshot?, res: PlatformResources): IosWidgetFace = IosWidgetFace(
    overline = face.overline,
    date = face.day?.let { planDateLabel(it) },
    title = face.title,
    titleInk = face.tone.ink,
    status = face.status,
    brief = face.brief,
    body = linesOf(face.lead, face.items, res),
    shortBody = face.shortLead?.takeIf { it != face.lead.single() }?.let { short ->
        IosShortBody(textEm(face.lead.single()), linesOf(listOf(short), face.items, res))
    },
    hero = face.hero?.let { IosWidgetHero(it.text, it.note, it.figure, linesOf(face.heroLead, face.heroItems, res)) },
    week = face.week?.let { text ->
        IosWidgetWeek(
            text = text,
            done = snapshot?.weekDone ?: 0,
            planned = snapshot?.weekPlanned ?: 0,
            days = face.days.takeIf { it.size == 7 }.orEmpty().map { d ->
                val glyph = d.mark.glyph
                IosWidgetDay(weekdayStripLabel(LocalDate.fromEpochDays(d.epochDay).dayOfWeek), glyph.text, glyph.ink, d.epochDay == face.day)
            },
        )
    },
    actions = face.actions.map { IosWidgetAction(it.label, it.dest.wire, it.face.icon) },
)

private fun controlOf(today: WidgetSnapshot?, recording: Boolean, res: PlatformResources): IosControlFace {
    val face = tileFace(today, recording)
    return IosControlFace(res.getString(face.label), tileSubtitle(today, res), face.dest.wire, face.iosSymbol)
}

/**
 * The widget payload for [snapshot] while a run records ([recording]) or not, in
 * [theme] and [darkMode]. The faces are the common [widgetFace]s, so the widget
 * says what Android's says.
 */
internal fun iosWidgetPayload(
    snapshot: WidgetSnapshot,
    recording: Boolean,
    theme: AppTheme,
    darkMode: DarkMode,
    res: PlatformResources,
): IosWidgetPayload {
    val current = snapshot.currentOn(snapshot.epochDay)
    val day = current?.let {
        val zone = if (it.zone.isEmpty()) TimeZone.currentSystemDefault() else TimeZone.of(it.zone)
        val date = LocalDate.fromEpochDays(it.epochDay)
        IosCurrentDay(
            zone = zone.id,
            startMs = date.atStartOfDayIn(zone).toEpochMilliseconds(),
            endMs = date.plus(DatePeriod(days = 1)).atStartOfDayIn(zone).toEpochMilliseconds(),
        )
    }
    return IosWidgetPayload(
        current = day,
        face = iosFaceOf(widgetFace(snapshot, snapshot.epochDay, res, recording), current, res),
        staleFace = iosFaceOf(widgetFace(null, snapshot.epochDay, res, recording), null, res),
        control = controlOf(current, recording, res),
        staleControl = controlOf(null, recording, res),
        light = paletteOf(theme, darkMode.isDark(systemDark = false)),
        dark = paletteOf(theme, darkMode.isDark(systemDark = true)),
    )
}

/** The faces before the app has written anything: "Open lollo to update" and Log. */
internal fun iosWidgetFallback(res: PlatformResources): IosWidgetFallback =
    IosWidgetFallback(iosFaceOf(widgetFace(null, 0, res), null, res), controlOf(null, false, res))

/** [state] as its activity shows it in [theme]: the text of Android's run
 *  notification ([runSurfaceText]) split into the activity's tiles. */
internal fun runLiveOf(state: RunSurfaceState, theme: AppTheme, res: PlatformResources): IosRunLive {
    val text = runSurfaceText(state, res)
    return IosRunLive(
        recording = state.phase == RunSurfacePhase.Recording,
        title = text.title,
        chip = text.chip,
        text = text.text,
        step = text.step,
        movingSec = state.movingSec,
        time = text.time,
        distance = text.distance,
        pace = text.pace,
        movingLabel = res.getString(Res.string.track_stat_elapsed),
        distanceLabel = res.getString(Res.string.run_stat_distance),
        paceLabel = res.getString(Res.string.track_stat_pace_now),
        canPause = state.phase.canPause,
        paused = state.phase == RunSurfacePhase.Paused,
        pauseLabel = res.getString(Res.string.track_action_pause),
        resumeLabel = res.getString(Res.string.track_action_resume),
        stopLabel = res.getString(Res.string.track_btn_stop),
        accentLight = theme.light.accent.hex(),
        accentDark = theme.dark.accent.hex(),
    )
}

/** [run] and the [next] set as the rest's activity shows them in [theme] (Android's
 *  countdown notification: `log_rest_notif_title`, [countdownText]). */
internal fun restLiveOf(run: RestRun, next: RestNextSet?, theme: AppTheme, res: PlatformResources): IosRestLive = IosRestLive(
    title = res.getString(Res.string.log_rest_notif_title, run.exercise),
    next = countdownText(res, run.exercise, next),
    startMs = run.startMillis,
    endMs = run.endAtMillis,
    doneTitle = res.getString(Res.string.log_rest_notif_done_title),
    accentLight = theme.light.accent.hex(),
    accentDark = theme.dark.accent.hex(),
)
