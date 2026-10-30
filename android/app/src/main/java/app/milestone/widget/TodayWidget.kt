package app.milestone.widget

import android.content.Context
import android.os.Build
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.Button
import androidx.glance.ButtonDefaults
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.LocalContext
import androidx.glance.LocalSize
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.appWidgetBackground
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.semantics.contentDescription
import androidx.glance.semantics.semantics
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.OpenDest
import app.milestone.Palette
import app.milestone.R
import app.milestone.latinDigitResources
import app.milestone.openIntent
import app.milestone.todayEpochDay
import app.milestone.withDynamicAccent
import androidx.glance.color.ColorProvider as DayNightColorProvider

// The size breakpoints (visual-design-system §10, "Home screen widget").
private val MinSide = 110.dp
private val TwoButtonsFrom = 180.dp
private val ThreeLinesFrom = 150.dp
private val WeekFrom = 180.dp
private val SixLinesFrom = 250.dp

/**
 * The sizes the widget lays out for: one step per breakpoint of [widgetLayout],
 * so the step the host picks for a widget (the largest that fits; the smallest
 * when none does) lays out the same as the widget's real size.
 */
internal val WidgetSizes: Set<DpSize> = listOf(MinSide, TwoButtonsFrom).flatMap { width ->
    listOf(MinSide, ThreeLinesFrom, WeekFrom, SixLinesFrom).map { height -> DpSize(width, height) }
}.toSet()

/**
 * The home screen widget: today's session and its items, the rest day and the
 * next session, or the pause and its reason, with Log (and Track run on a run
 * day). It reads [WidgetSnapshotStore], never the Rust core; [WidgetSync] writes
 * the snapshot and updates the widget whenever the app publishes a model.
 */
internal class TodayWidget : GlanceAppWidget() {
    override val sizeMode = SizeMode.Responsive(WidgetSizes)

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        // A widget update can start the process without MainActivity: the theme
        // choice lives in AppSettings' preferences.
        AppSettings.load(context)
        provideContent {
            val ctx = LocalContext.current
            // The face's counts in Latin digits like the app's (AppLocale.kt),
            // rebuilt when the language changes.
            val res = ctx.latinDigitResources()
            // A write or a stale mark ticks the revision, so a session that is still
            // running re-reads the file and today's date.
            val revision by WidgetSnapshotStore.revision.collectAsState()
            val theme by AppSettings.theme.collectAsState()
            val darkMode by AppSettings.darkMode.collectAsState()
            val dynamic by AppSettings.dynamicAccent.collectAsState()
            val face = remember(revision, res.configuration.locales) {
                widgetFace(WidgetSnapshotStore.read(ctx), todayEpochDay(), res)
            }
            val colors = remember(theme, darkMode, dynamic) { widgetColors(ctx, theme, darkMode, dynamic) }
            TodayWidgetContent(face, colors, widgetLayout(LocalSize.current))
        }
    }
}

/** How much of a [WidgetFace] a size shows. */
internal data class WidgetLayout(val maxLines: Int, val showWeek: Boolean, val maxActions: Int)

internal fun widgetLayout(size: DpSize): WidgetLayout = WidgetLayout(
    maxLines = when {
        size.height < ThreeLinesFrom -> 1
        size.height < SixLinesFrom -> 3
        else -> 6
    },
    showWeek = size.height >= WeekFrom,
    maxActions = if (size.width < TwoButtonsFrom) 1 else 2,
)

/** The app palette as Glance colours. */
internal data class WidgetColors(
    val ground: ColorProvider,
    val body: ColorProvider,
    val muted: ColorProvider,
    val accent: ColorProvider,
    val onAccent: ColorProvider,
    val danger: ColorProvider,
)

/**
 * [theme]'s palette, light and dark like the app: following the system mode
 * (day/night colour providers) unless [darkMode] forces one. With the system
 * accent on (Android 12+), the accent is the wallpaper colour through the same
 * contrast guard as [app.milestone.MilestoneTheme].
 */
internal fun widgetColors(ctx: Context, theme: AppTheme, darkMode: DarkMode, dynamic: Boolean): WidgetColors {
    val useDynamic = dynamic && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val light = if (useDynamic) theme.light.withDynamicAccent(dynamicLightColorScheme(ctx).primary) else theme.light
    val dark = if (useDynamic) theme.dark.withDynamicAccent(dynamicDarkColorScheme(ctx).primary) else theme.dark
    fun pick(role: (Palette) -> Color): ColorProvider = when (darkMode) {
        DarkMode.System -> DayNightColorProvider(role(light), role(dark))
        DarkMode.Light -> ColorProvider(role(light))
        DarkMode.Dark -> ColorProvider(role(dark))
    }
    return WidgetColors(
        ground = pick { it.bgTop },
        body = pick { it.onBgBody },
        muted = pick { it.onBgMuted },
        accent = pick { it.accent },
        onAccent = pick { it.bgTop },
        danger = pick { it.dangerText },
    )
}

@Composable
internal fun TodayWidgetContent(face: WidgetFace, colors: WidgetColors, layout: WidgetLayout) {
    val ctx = LocalContext.current
    val openToday = ctx.getString(R.string.widget_open_today_cd)
    Column(
        modifier = GlanceModifier
            .fillMaxSize()
            .appWidgetBackground()
            .background(colors.ground)
            .cornerRadius(16.dp)
            .padding(12.dp)
            .clickable(actionStartActivity(openIntent(ctx, OpenDest.Today)))
            .semantics { contentDescription = openToday },
    ) {
        Text(
            face.overline,
            style = TextStyle(color = colors.muted, fontSize = 12.sp, fontWeight = FontWeight.Medium),
            maxLines = 1,
        )
        Text(
            face.title,
            style = TextStyle(
                color = when (face.tone) {
                    WidgetTone.Paused -> colors.danger
                    WidgetTone.Muted -> colors.muted
                    WidgetTone.Normal -> colors.body
                },
                fontSize = 16.sp,
                fontWeight = FontWeight.Bold,
            ),
            maxLines = 2,
        )
        visibleLines(face, layout.maxLines, ctx.latinDigitResources()).forEach { line ->
            Text(line, style = TextStyle(color = colors.body, fontSize = 13.sp), maxLines = 2)
        }
        face.week?.takeIf { layout.showWeek }?.let {
            Spacer(GlanceModifier.height(4.dp))
            Text(it, style = TextStyle(color = colors.muted, fontSize = 12.sp), maxLines = 1)
        }
        Spacer(GlanceModifier.defaultWeight())
        val actions = face.actions.take(layout.maxActions)
        if (actions.isNotEmpty()) {
            Row(modifier = GlanceModifier.fillMaxWidth()) {
                actions.forEachIndexed { i, action ->
                    if (i > 0) Spacer(GlanceModifier.width(8.dp))
                    Button(
                        text = action.label,
                        onClick = actionStartActivity(openIntent(ctx, action.dest)),
                        modifier = GlanceModifier.defaultWeight(),
                        style = TextStyle(color = colors.onAccent, fontSize = 14.sp, fontWeight = FontWeight.Medium),
                        colors = ButtonDefaults.buttonColors(backgroundColor = colors.accent, contentColor = colors.onAccent),
                        maxLines = 1,
                    )
                }
            }
        }
    }
}
