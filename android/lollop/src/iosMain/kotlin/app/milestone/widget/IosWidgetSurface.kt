package app.milestone.widget

import androidx.compose.ui.graphics.Color
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DarkStatusColors
import app.milestone.IosSurfaces
import app.milestone.Palette
import app.milestone.PlatformContext
import app.milestone.PlatformResources
import app.milestone.ProcessLocal
import app.milestone.StatusColors
import app.milestone.WeekdayWidth
import app.milestone.accentTint
import app.milestone.appLocale
import app.milestone.getQuantityString
import app.milestone.getString
import app.milestone.hex
import app.milestone.planDateLabel
import app.milestone.resources.Res
import app.milestone.resources.widget_more_items
import app.milestone.surfaceJson
import app.milestone.systemResources
import app.milestone.weekdayName
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.datetime.LocalDate
import kotlinx.serialization.Serializable

// The home and lock screen widgets and the Control Center control on iOS (Android:
// the Glance widget and the Quick Settings tile), drawn by the Swift widget
// extension from the payload the app resolves here ([IosSurfaces.publishWidget]).

/** Bumped on any change an older widget extension would misread. */
internal const val IOS_WIDGET_PAYLOAD_VERSION = 1

/**
 * What the widgets and the control show, written on every change by [IosWidgetSurface]
 * (Android: `widget-today.json` and what the Glance widget resolves from it).
 * [snapshot] is the common day snapshot; [face] is the widget for its day and
 * [staleFace] the one for any other day ("Open lollop to update"), which the
 * extension shows from local midnight, after a time zone change ([WidgetSnapshot.zone])
 * or when [WidgetSnapshot.stale]. [moreItems] holds "N more items" for N = 1, 2, …
 * (index N - 1) for the extension's line fitting (`visibleLines`). [control] and
 * [staleControl] are the Control Center control's faces for the same days.
 */
@Serializable
internal data class IosWidgetPayload(
    val version: Int = IOS_WIDGET_PAYLOAD_VERSION,
    val snapshot: WidgetSnapshot,
    val recording: Boolean,
    val face: IosWidgetFace,
    val staleFace: IosWidgetFace,
    val moreItems: List<String>,
    val control: IosControlFace,
    val staleControl: IosControlFace,
    val palette: IosWidgetPalette,
)

/** [WidgetFace] for the extension; [date] is the short date alone, the overline of a narrow card. */
@Serializable
internal data class IosWidgetFace(
    val overline: String,
    val date: String?,
    val title: String,
    /** normal | paused | muted ([WidgetTone]). */
    val tone: String,
    val status: String?,
    val brief: String?,
    val lead: List<String>,
    val items: List<String>,
    val hero: IosWidgetHero?,
    val heroLead: List<String>,
    val heroItems: List<String>,
    val week: String?,
    val days: List<IosWidgetDay>,
    val actions: List<IosWidgetAction>,
)

@Serializable
internal data class IosWidgetHero(val text: String, val note: String?, val figure: Boolean)

/** A week strip day: its two-letter weekday, its mark (done | next | planned |
 *  missed | rest | none) and whether it is the face's day. */
@Serializable
internal data class IosWidgetDay(val label: String, val mark: String, val today: Boolean)

/** A widget button: its label, the `lollop://open/<dest>` destination and the
 *  tile face whose icon it shows ([TileFace.wire]). */
@Serializable
internal data class IosWidgetAction(val label: String, val dest: String, val face: String)

/** The Control Center control: Android's Quick Settings tile face. */
@Serializable
internal data class IosControlFace(val label: String, val subtitle: String?, val dest: String, val face: String, val active: Boolean)

/** The theme's colours as the widget uses them ("#AARRGGBB"), light and dark;
 *  [mode] is the app's dark mode choice (system | light | dark). */
@Serializable
internal data class IosWidgetPalette(val mode: String, val light: IosWidgetColors, val dark: IosWidgetColors)

@Serializable
internal data class IosWidgetColors(
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

private fun colorsOf(p: Palette, done: Color) = IosWidgetColors(
    ground = p.bgTop.hex(),
    body = p.onBgBody.hex(),
    muted = p.onBgMuted.hex(),
    faint = p.onBgFaint.hex(),
    accent = p.accent.hex(),
    onAccent = p.bgTop.hex(),
    accentTint = p.accentTint().hex(),
    danger = p.dangerText.hex(),
    done = done.hex(),
)

/** [theme]'s widget colours, as Android's `widgetColors` (iOS has no system accent). */
internal fun widgetPaletteOf(theme: AppTheme, darkMode: DarkMode) = IosWidgetPalette(
    mode = darkMode.name.lowercase(),
    light = colorsOf(theme.light, StatusColors().done),
    dark = colorsOf(theme.dark, DarkStatusColors.done),
)

private fun iosFaceOf(face: WidgetFace): IosWidgetFace = IosWidgetFace(
    overline = face.overline,
    date = face.day?.let { planDateLabel(it) },
    title = face.title,
    tone = when (face.tone) {
        WidgetTone.Normal -> "normal"
        WidgetTone.Paused -> "paused"
        WidgetTone.Muted -> "muted"
    },
    status = face.status,
    brief = face.brief,
    lead = face.lead,
    items = face.items,
    hero = face.hero?.let { IosWidgetHero(it.text, it.note, it.figure) },
    heroLead = face.heroLead,
    heroItems = face.heroItems,
    week = face.week,
    days = face.days.map { d ->
        IosWidgetDay(
            label = weekdayName(LocalDate.fromEpochDays(d.epochDay).dayOfWeek, WeekdayWidth.ShortStandalone, appLocale()).take(2),
            mark = d.mark.name.lowercase(),
            today = d.epochDay == face.day,
        )
    },
    actions = face.actions.map { IosWidgetAction(it.label, it.dest.wire, it.face.wire) },
)

private fun controlOf(today: WidgetSnapshot?, recording: Boolean, res: PlatformResources): IosControlFace {
    val face = tileFace(today, recording)
    return IosControlFace(res.getString(face.label), tileSubtitle(today, res), face.dest.wire, face.wire, face.active)
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
    val face = widgetFace(snapshot, snapshot.epochDay, res, recording)
    val current = snapshot.takeIf { !it.stale }
    val most = maxOf(face.items.size, face.heroItems.size, 1)
    return IosWidgetPayload(
        snapshot = snapshot,
        recording = recording,
        face = iosFaceOf(face),
        staleFace = iosFaceOf(widgetFace(null, snapshot.epochDay, res, recording)),
        moreItems = (1..most).map { n -> res.getQuantityString(Res.plurals.widget_more_items, n, n) },
        control = controlOf(current, recording, res),
        staleControl = controlOf(null, recording, res),
        palette = widgetPaletteOf(theme, darkMode),
    )
}

internal actual val widgetSurfacePort: WidgetSurfacePort = IosWidgetSurface

/**
 * The widget payload for the Swift side: built from every change of the inputs,
 * handed over when it differs from the last. A time zone or clock change derives
 * it again (the snapshot records the zone it was written in). Built on the main
 * thread, as the notifications build their text.
 */
internal object IosWidgetSurface : WidgetSurfacePort {
    private class State {
        var last: String? = null
        val ticks = MutableStateFlow(0L)
    }

    private val state by ProcessLocal { State() }

    override val ticks: Flow<Long> get() = state.ticks

    override fun start(ctx: PlatformContext) {}

    override suspend fun show(ctx: PlatformContext, inputs: WidgetInputs, previous: WidgetInputs?) {
        val snapshot = inputs.snapshot() ?: return
        val payload = iosWidgetPayload(snapshot, inputs.recording, inputs.look.theme, inputs.look.darkMode, systemResources())
        val json = surfaceJson.encodeToString(IosWidgetPayload.serializer(), payload)
        if (json == state.last) return
        state.last = json
        IosSurfaces.publishWidget(json)
    }

    override fun onTimeChanged(ctx: PlatformContext, zoneChanged: Boolean) {
        state.ticks.value += 1
    }

    /** Hand the last payload over again (a new host). */
    fun republish() {
        state.last?.let(IosSurfaces::publishWidget)
    }
}
