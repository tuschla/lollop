package app.milestone.widget

import android.content.Context
import android.content.res.Resources
import android.os.Build
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.glance.ButtonDefaults
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.ImageProvider
import androidx.glance.LocalContext
import androidx.glance.LocalSize
import androidx.glance.action.Action
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.appWidgetBackground
import androidx.glance.appwidget.components.CircleIconButton
import androidx.glance.appwidget.components.FilledButton
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Column
import androidx.glance.layout.ColumnScope
import androidx.glance.layout.Row
import androidx.glance.layout.RowScope
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
import androidx.glance.text.TextAlign
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import app.milestone.ActionMinHeight
import app.milestone.AppRuntime
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DayMark
import app.milestone.OpenDest
import app.milestone.R
import app.milestone.RunSession
import app.milestone.Space
import app.milestone.Type
import app.milestone.appResources
import app.milestone.openIntent
import app.milestone.planDateLabel
import app.milestone.resolveSurfaceAccent
import app.milestone.todayEpochDay
import app.milestone.weekdayStripLabel
import kotlin.math.ceil
import kotlinx.datetime.LocalDate
import androidx.compose.ui.text.TextStyle as ComposeTextStyle
import androidx.compose.ui.text.font.FontWeight as ComposeFontWeight
import androidx.glance.color.ColorProvider as DayNightColorProvider

// ── Size steps and metrics (visual-design-system §10, "Home screen widget") ──

/**
 * From this width (three cells on any phone grid) the widget is "wide": the
 * primary labelled, Log beside it where both fit, "Today ·" in the overline
 * where it fits on its line.
 */
private val WideFrom = 200.dp

/** Below this height the stacked layout cannot hold a title, a line and a button. */
private val StackFrom = 130.dp

/**
 * A one-row widget narrower than this (two cells on five columns: the 130 dp
 * step) keeps [Space.Md] at its sides instead of [Space.Screen]: beside the
 * round button its text had ~57 dp, less than a title word ("Training" broke as
 * "Trai / ning p…" on a 145 dp 2×1).
 */
private val NarrowRowBelow = 160.dp

/** At most this many body lines: a session rarely has more items. */
private const val MaxBodyLines = 6

/**
 * A text's height in em of its font size, at font scale 1: TextView's font
 * padding (Glance's default `includeFontPadding`, Roboto's top and bottom beyond
 * its ascent and descent) once, then each line's ascent to descent. One line is
 * about 1.33 em, two 2.5 em.
 */
private const val FontPaddingEm = 0.156f
private const val LineEm = 1.171f

/** The height of a text in this style on [lines] lines at [fontScale]. */
private fun ComposeTextStyle.height(lines: Int, fontScale: Float): Dp =
    (fontSize.value * fontScale * (FontPaddingEm + LineEm * lines)).dp

/** A labelled button's icon (dp) and label size (sp): Glance's FilledButton. */
private const val LabelledIcon = 18
private const val ButtonLabelSp = 14f

/**
 * The size steps the host chooses from (at most 16, the platform's limit for a
 * sized RemoteViews map). A host shows the step that fits the real size closest
 * (squared distance in dp; the smallest step when none fits,
 * RemoteViews.findBestFitLayout), and the layout is composed for that step, so
 * what a cell has beyond its step stays empty: each step sits just under the
 * cell sizes it serves.
 *
 * Cells measured on the Pixel launcher (dp; 2 to 4 columns wide, 1 to 3 rows
 * tall): five columns on a 1080 × 1920 @ 420 dpi phone (Android 14) are
 * 145 / 221 / 297 wide and 90 / 193 / 296 tall; four columns on a 1080 × 2400
 * phone (Android 17) 179 / 276 / 373 wide and 95 / 202 / 308 tall. Other phone
 * launchers give two cells 130–170, three 200–260, four 280–360, a row 90–120.
 *
 * Widths: 130 (two cells on the narrowest grid), 160 (two cells on four
 * columns), 200 ([WideFrom], three cells), 272 (four cells, three on four
 * columns). Each width has a one-row step (72) and stacked steps for two rows
 * (184, and 200 where a four-column grid's rows have it: the run hero and the
 * date fit from 200) and three rows and more (288). The short-card step (140:
 * a title, a line and a button) keeps a widget 140–183 tall from picking a
 * one-row step closer in width; 160 needs none, 130 × 140 is closer than any
 * one-row step.
 */
internal val WidgetSizes: Set<DpSize> = buildSet {
    fun steps(width: Int, vararg heights: Int) = heights.forEach { add(DpSize(width.dp, it.dp)) }
    steps(130, 72, 140, 184, 288)
    steps(160, 72, 200, 288)
    steps(200, 72, 140, 184, 288)
    steps(272, 72, 140, 184, 200, 288)
}

/**
 * The home screen widget: today's session and its items, the rest day and the
 * next session, or the pause, with the Quick Settings tile's action (and Log) as
 * its buttons. It reads [WidgetSnapshotStore], never the Rust core; [AndroidWidgets]
 * writes the snapshot and updates the widget whenever the app publishes a model.
 */
internal class TodayWidget : GlanceAppWidget() {
    override val sizeMode = SizeMode.Responsive(WidgetSizes)
    override val previewSizeMode = SizeMode.Responsive(WidgetSizes)

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        // A widget update can start the process without MainActivity.
        AppRuntime.start(context)
        provideContent {
            val ctx = LocalContext.current
            // The face's counts in Latin digits like the app's (AppLocale.kt),
            // rebuilt when the language changes.
            val res = ctx.appResources()
            // A write or a stale mark ticks the revision, so a session that is still
            // running re-reads the file and today's date.
            val revision by WidgetSnapshotStore.revision.collectAsState()
            val recording by RunSession.tracking.collectAsState()
            val theme by AppSettings.theme.collectAsState()
            val darkMode by AppSettings.darkMode.collectAsState()
            val dynamic by AppSettings.dynamicAccent.collectAsState()
            val face = remember(revision, recording, res.configuration.locales) {
                widgetFace(WidgetSnapshotStore.read(ctx), todayEpochDay(), res, recording)
            }
            val colors = remember(theme, darkMode, dynamic) { widgetColors(ctx, theme, darkMode, dynamic) }
            TodayWidgetContent(face, colors)
        }
    }

    /** The widget picker's preview (Android 15+, published by [AndroidWidgets]): a
     *  sample lift day in the app's current theme. */
    override suspend fun providePreview(context: Context, widgetCategory: Int) {
        AppRuntime.start(context)
        val res = context.appResources()
        val face = widgetFace(previewSnapshot(res, todayEpochDay()), todayEpochDay(), res)
        val colors = widgetColors(context, AppSettings.theme.value, AppSettings.darkMode.value, AppSettings.dynamicAccent.value)
        provideContent { TodayWidgetContent(face, colors) }
    }
}

private val PreviewSessionDays = setOf(0, 2, 4, 5)

/** The picker preview's sample day: [today] is its lift day, in a plan week of
 *  Monday, Wednesday, Friday and Saturday sessions (and today). */
internal fun previewSnapshot(res: Resources, today: Long): WidgetSnapshot {
    val monday = today - LocalDate.fromEpochDays(today).dayOfWeek.ordinal
    val week = (0 until 7).map { i ->
        val day = monday + i
        val mark = when {
            day == today -> DayMark.Next
            i !in PreviewSessionDays -> DayMark.Rest
            day < today -> DayMark.Done
            else -> DayMark.Planned
        }
        WidgetWeekDay(day, mark)
    }
    fun lift(name: Int, detail: Int) = WidgetItem(WidgetItemKind.Lift, res.getString(name), res.getString(detail))
    return WidgetSnapshot(
        epochDay = today,
        kind = WidgetDayKind.Lift,
        title = res.getString(R.string.widget_preview_title),
        items = listOf(
            lift(R.string.widget_preview_lift_1, R.string.widget_preview_lift_1_detail),
            lift(R.string.widget_preview_lift_2, R.string.widget_preview_lift_2_detail),
            lift(R.string.widget_preview_lift_3, R.string.widget_preview_lift_3_detail),
        ),
        weekDone = week.count { it.mark == DayMark.Done },
        weekPlanned = week.count { it.mark != DayMark.Rest && it.mark != DayMark.None },
        week = week,
    )
}

/** The widget's two layouts: one row (a 2×1 or 4×1), or the stacked card. */
internal enum class WidgetShape {
    /** Title and one status line beside the action button(s). */
    Line,

    /** Overline, title, body lines, the week strip and the action row, as room allows. */
    Stack,
}

/**
 * What a size shows of a face. [status] is the one-row layout's status line
 * (on [statusLines] lines), only one that fits whole; [bodyHeight] the stacked
 * card's room for body lines ([bodyLines] fills it); [textWidth] the width text
 * wraps at.
 */
internal data class WidgetLayout(
    val shape: WidgetShape,
    val wide: Boolean,
    val overline: Boolean,
    /** The overline as "Today · Wed, Oct 28" (a wide card where it fits on its line), else the date alone. */
    val todayOverline: Boolean,
    /** The side padding: [Space.Screen], [Space.Md] on a narrow one-row widget. */
    val edge: Dp,
    val titleLines: Int,
    /** The stacked title in [Type.Display]: a card whose whole content is its title and a line. */
    val bigTitle: Boolean,
    /** The [Space.Sm] gap under the stacked title (left out only so the line under it fits whole). */
    val titleGap: Boolean,
    val status: String?,
    val statusLines: Int,
    /** The face's [WidgetFace.hero] block, with its own lead and item lines. */
    val hero: Boolean,
    val bodyHeight: Dp,
    /** The week block above the actions: the week count (on [weekLines] lines)… */
    val week: Boolean,
    val weekLines: Int,
    /** …and the strip of days, where the face has them and each day's label fits its column. */
    val weekDays: Boolean,
    val maxActions: Int,
    val labelled: Boolean,
    val textWidth: Dp,
    val fontScale: Float,
)

/** How many lines [text] takes in [style] at [width] (1 or 2: the widget never wraps more). */
private fun linesOf(text: String, style: ComposeTextStyle, width: Dp, fontScale: Float): Int =
    wrappedLines(text, style, width, fontScale).coerceIn(1, 2)

/** How many lines [text] wraps to in [style] at [width], however many. */
private fun wrappedLines(text: String, style: ComposeTextStyle, width: Dp, fontScale: Float): Int =
    ceil(textEm(text) * style.fontSize.value * fontScale / width.value.coerceAtLeast(1f)).toInt().coerceAtLeast(1)

/** The width of a labelled button: its padding, icon and gap, and the label in its 14 sp. */
private fun labelledWidth(label: String, fontScale: Float): Dp =
    (Space.Screen * 2 + LabelledIcon + Space.Md).dp + (textEm(label) * ButtonLabelSp * fontScale).dp

/** Whether each weekday label of [days] fits a strip column of [column], with a [Space.Sm] margin each side. */
private fun dayLabelsFit(days: List<WidgetWeekDay>, column: Dp, fontScale: Float): Boolean =
    days.all { (textEm(weekdayStripLabel(LocalDate.fromEpochDays(it.epochDay).dayOfWeek)) * Type.Overline.fontSize.value * fontScale).dp + (Space.Sm * 2).dp <= column }

/**
 * The layout for [face] at [size] and the system [fontScale]. The richest one
 * whose parts all fit: a widget too short for a title, a line and a button above
 * one another is one row. The stacked card spends height on, in order, the
 * action row, one title line, one body line, a second title line when the title
 * wraps, the overline, the hero block, the week block (the count, and the strip of
 * days when its labels fit the width) while the hero or two body lines still fit,
 * then body lines. A card whose content is only its title and a line sets the
 * title in [Type.Display] where it fits on one line (two lines of it would shout).
 * Sizes in dp; text heights scale with [fontScale], so a larger font drops parts
 * instead of clipping them.
 */
internal fun widgetLayout(size: DpSize, fontScale: Float, face: WidgetFace): WidgetLayout {
    val wide = size.width >= WideFrom
    val caption = Type.Caption.height(1, fontScale)
    val actionRow = if (face.actions.isEmpty()) 0.dp else ActionMinHeight + Space.Md.dp
    val titleH = Type.Title.height(1, fontScale)
    val stackMin = Space.Screen.dp * 2 + actionRow + titleH + Space.Sm.dp + caption
    if (size.height < StackFrom || size.height < stackMin) {
        // The richest row whose title and status line fit: both actions before one
        // (Log is the second), each with the primary labelled (wide rows) before
        // round; a 3×1 lift day at font 1.3 kept "Heavy day" alone beside a
        // labelled "Log set". The title and its line come first.
        val one = face.actions.size.coerceAtMost(1)
        for (n in face.actions.size.coerceAtMost(2) downTo one) for (labelled in listOfNotNull(true.takeIf { wide }, false)) {
            lineLayout(size, fontScale, face, n, labelled).let { if (it.status != null) return it }
        }
        return lineLayout(size, fontScale, face, one, labelled = wide)
    }
    val textWidth = size.width - Space.Screen.dp * 2
    // The title, and the Space.Sm gap under it (the Today card's title gap).
    var avail = size.height - Space.Screen.dp * 2 - actionRow - titleH - Space.Sm.dp
    // A wrapped title keeps its second line before the overline: the title is the key fact.
    val titleLine = Type.Title.height(2, fontScale) - titleH
    val secondTitle = linesOf(face.title, Type.Title, textWidth, fontScale) > 1 && avail - titleLine >= caption
    if (secondTitle) avail -= titleLine
    val overlineH = Type.Overline.height(1, fontScale) + Space.Sm.dp
    val overline = avail - overlineH >= caption
    if (overline) avail -= overlineH
    val todayOverline = wide && wrappedLines(face.overline.uppercase(), Type.Overline, textWidth, fontScale) == 1
    // The hero: a figure on one line in Display, or a session name, then its note.
    val heroH = face.hero?.let { h ->
        val main = if (h.figure) {
            Type.Display.height(1, fontScale).takeIf { wrappedLines(h.text, Type.Display, textWidth, fontScale) == 1 }
        } else {
            Type.BodyBold.height(linesOf(h.text, Type.BodyBold, textWidth, fontScale), fontScale)
        }
        main?.plus(h.note?.let { Type.Caption.height(linesOf(it, Type.Caption, textWidth, fontScale), fontScale) } ?: 0.dp)
    }
    val hero = heroH != null && avail - heroH >= caption * face.heroLead.size
    if (hero && heroH != null) avail -= heroH
    // The week block while the content keeps its hero, or two body lines.
    val weekText = face.week
    val weekLines = weekText?.let { linesOf(it, Type.Caption, textWidth, fontScale) } ?: 0
    val countH = Type.Caption.height(weekLines, fontScale) + Space.Md.dp
    val stripH = Type.Overline.height(1, fontScale) + Type.Chip.height(1, fontScale) + Space.Sm.dp * 3
    val keep = caption * (if (hero) face.heroLead.size else minOf(2, face.lead.size + face.items.size))
    val weekDays = weekText != null && face.days.size == 7 && dayLabelsFit(face.days, textWidth / 7, fontScale) &&
        avail - countH - stripH >= keep
    val week = weekText != null && (weekDays || avail - countH >= keep)
    if (week) avail -= countH + if (weekDays) stripH else 0.dp
    // Only a title and its line: the title large, where it fits on one line (two
    // lines of it would shout, a hold's in danger red most of all).
    var titleLines = if (secondTitle) 2 else 1
    val sparse = !hero && !week && face.items.isEmpty()
    val bigExtra = Type.Display.height(1, fontScale) - Type.Title.height(titleLines, fontScale)
    val leadH = face.lead.fold(0.dp) { acc, l -> acc + Type.Caption.height(linesOf(l, Type.Caption, textWidth, fontScale), fontScale) }
    val bigTitle = sparse && wrappedLines(face.title, Type.Display, textWidth, fontScale) == 1 && avail - bigExtra >= leadH
    if (bigTitle) {
        avail -= bigExtra
        titleLines = 1
    }
    // The title's gap gives way before the lead line under it would be cut.
    val titleGap = !(sparse && avail < leadH && avail + Space.Sm.dp >= leadH)
    if (!titleGap) avail += Space.Sm.dp
    return WidgetLayout(
        shape = WidgetShape.Stack,
        wide = wide,
        overline = overline,
        todayOverline = todayOverline,
        edge = Space.Screen.dp,
        titleLines = titleLines,
        bigTitle = bigTitle,
        titleGap = titleGap,
        status = null,
        statusLines = 0,
        hero = hero,
        bodyHeight = avail,
        week = week,
        weekLines = weekLines,
        weekDays = weekDays,
        maxActions = if (wide && face.actions.size > 1 && stackRowFits(face, textWidth, fontScale)) 2 else face.actions.size.coerceAtMost(1),
        labelled = true,
        textWidth = textWidth,
        fontScale = fontScale,
    )
}

/**
 * Whether Log and the labelled primary fit the stacked card's action row of
 * [width]: a round button and its [Space.Md] gap, then the primary ("Run ·
 * recording" or a large font leaves the primary alone on a three-cell card).
 */
private fun stackRowFits(face: WidgetFace, width: Dp, fontScale: Float): Boolean =
    ActionMinHeight + Space.Md.dp + labelledWidth(face.actions.first().label, fontScale) <= width

/** The one-row layout ([WidgetShape.Line]) at [size] with [actions] buttons, the primary [labelled] or round. */
private fun lineLayout(size: DpSize, fontScale: Float, face: WidgetFace, actions: Int, labelled: Boolean): WidgetLayout {
    val wide = size.width >= WideFrom
    // The text column: the row less its padding and the action(s), a round
    // button each or the labelled primary, each after a Space.Md gap.
    val round = ActionMinHeight + Space.Md.dp
    val actionsWidth = when {
        actions == 0 -> 0.dp
        !labelled -> round * actions
        else -> labelledWidth(face.actions.first().label, fontScale) + Space.Md.dp + round * (actions - 1)
    }
    val edge = if (size.width < NarrowRowBelow) Space.Md.dp else Space.Screen.dp
    val textWidth = size.width - edge * 2 - actionsWidth
    // The row centres its text in a launcher row taller than the step (a 2×1 of
    // ~95 dp shows the 72 dp step), so the text keeps only a Space.Sm margin.
    val text = size.height - Space.Sm.dp * 2
    // A title that wraps keeps its second line (it is the key fact); the status
    // line gets what is left.
    val titleLines = if (linesOf(face.title, Type.BodyBold, textWidth, fontScale) > 1 && text >= Type.BodyBold.height(2, fontScale)) 2 else 1
    val room = text - Type.BodyBold.height(titleLines, fontScale)
    // The full status where the row is wide, else the short one; one that would
    // be cut is left out rather than ellipsized.
    val status = listOfNotNull(face.status.takeIf { wide }, face.brief).firstOrNull {
        val lines = wrappedLines(it, Type.Caption, textWidth, fontScale)
        lines <= 2 && Type.Caption.height(lines, fontScale) <= room
    }
    return WidgetLayout(
        shape = WidgetShape.Line,
        wide = wide,
        overline = false,
        todayOverline = false,
        edge = edge,
        titleLines = titleLines,
        bigTitle = false,
        titleGap = false,
        status = status,
        statusLines = status?.let { linesOf(it, Type.Caption, textWidth, fontScale) } ?: 0,
        hero = false,
        bodyHeight = 0.dp,
        week = false,
        weekLines = 0,
        weekDays = false,
        maxActions = actions,
        labelled = labelled,
        textWidth = textWidth,
        fontScale = fontScale,
    )
}

/** One body line and the lines it may wrap to; [large] in [Type.Body], else [Type.Caption]. */
internal data class BodyLine(val text: String, val maxLines: Int, val large: Boolean = false)

/**
 * The body lines of [face] that fit [layout]'s [WidgetLayout.bodyHeight]: every
 * line in [Type.Body] where they all fit so (a big card reads like the Today
 * card's rows rather than leaving room unused), else the most of [visibleLines]
 * (lead lines, then items, then "N more items"; the hero's own where the layout
 * shows it) in [Type.Caption] whose estimated heights all fit; without a hero, at
 * least the first line, on one line, when one line fits.
 */
internal fun bodyLines(face: WidgetFace, layout: WidgetLayout, res: Resources): List<BodyLine> {
    if (layout.shape != WidgetShape.Stack) return emptyList()
    val fs = layout.fontScale
    // A lone lead line too long for two lines ("Next: Heavy volume day, Thu, Oct 30"
    // at a large font) gives way to the short one ("Next: Thu").
    val lead = face.shortLead?.takeIf { wrappedLines(face.lead.single(), Type.Caption, layout.textWidth, fs) > 2 }
        ?.let { listOf(it) } ?: face.lead
    val src = if (layout.hero) face.copy(lead = face.heroLead, items = face.heroItems) else face.copy(lead = lead)
    fun List<BodyLine>.height() = fold(0.dp) { acc, l -> acc + (if (l.large) Type.Body else Type.Caption).height(l.maxLines, fs) }
    val all = src.lead.size + src.items.size
    if (all in 1..MaxBodyLines) {
        val large = visibleLines(src.lead, src.items, all, res).map { BodyLine(it, wrappedLines(it, Type.Body, layout.textWidth, fs), large = true) }
        if (large.all { it.maxLines <= 2 } && large.height() <= layout.bodyHeight) return large
    }
    for (n in minOf(all, MaxBodyLines) downTo 1) {
        val lines = visibleLines(src.lead, src.items, n, res).map { BodyLine(it, linesOf(it, Type.Caption, layout.textWidth, fs)) }
        if (lines.height() <= layout.bodyHeight) return lines
    }
    // A hero's lines show whole or not at all; a list keeps its first line, cut to one.
    return if (!layout.hero && all > 0 && Type.Caption.height(1, fs) <= layout.bodyHeight) {
        listOf(BodyLine(visibleLines(src.lead, src.items, 1, res).first(), 1))
    } else {
        emptyList()
    }
}

/** The app palette as Glance colours, one per [WidgetInk]. */
internal class WidgetColors(private val inks: Map<WidgetInk, ColorProvider>) {
    operator fun get(ink: WidgetInk): ColorProvider = inks.getValue(ink)

    val ground: ColorProvider get() = this[WidgetInk.Ground]
    val body: ColorProvider get() = this[WidgetInk.Body]
    val muted: ColorProvider get() = this[WidgetInk.Muted]
    val faint: ColorProvider get() = this[WidgetInk.Faint]
    val accent: ColorProvider get() = this[WidgetInk.Accent]
    val onAccent: ColorProvider get() = this[WidgetInk.OnAccent]
    val accentTint: ColorProvider get() = this[WidgetInk.AccentTint]
}

/**
 * [theme]'s palette, light and dark like the app: following the system mode
 * (day/night colour providers) unless [darkMode] forces one. The accent as
 * [resolveSurfaceAccent] gives it ([dynamic]: the system accent); the done mark
 * stays the theme's.
 */
internal fun widgetColors(ctx: Context, theme: AppTheme, darkMode: DarkMode, dynamic: Boolean): WidgetColors {
    val light = resolveSurfaceAccent(ctx, theme.light, night = false, dynamic = dynamic)
    val dark = resolveSurfaceAccent(ctx, theme.dark, night = true, dynamic = dynamic)
    fun pick(ink: WidgetInk): ColorProvider = when (darkMode) {
        DarkMode.System -> DayNightColorProvider(light.ink(ink, dark = false), dark.ink(ink, dark = true))
        DarkMode.Light -> ColorProvider(light.ink(ink, dark = false))
        DarkMode.Dark -> ColorProvider(dark.ink(ink, dark = true))
    }
    return WidgetColors(WidgetInk.entries.associateWith(::pick))
}

/** A [Type] token as a Glance text style in [color]. */
private fun ComposeTextStyle.glance(color: ColorProvider, align: TextAlign? = null) = TextStyle(
    color = color,
    fontSize = fontSize,
    fontWeight = when {
        (fontWeight ?: ComposeFontWeight.Normal) >= ComposeFontWeight.SemiBold -> FontWeight.Bold
        fontWeight == ComposeFontWeight.Medium -> FontWeight.Medium
        else -> FontWeight.Normal
    },
    textAlign = align,
)

private fun WidgetFace.titleColor(colors: WidgetColors) = colors[tone.ink]

@Composable
internal fun TodayWidgetContent(face: WidgetFace, colors: WidgetColors) {
    val ctx = LocalContext.current
    val layout = widgetLayout(LocalSize.current, ctx.resources.configuration.fontScale, face)
    val openToday = ctx.appResources().getString(R.string.widget_open_today_cd)
    val root = GlanceModifier
        .fillMaxSize()
        .appWidgetBackground()
        .background(colors.ground)
        .then(
            // The launcher's own widget corner (Android 12+; older hosts draw square corners).
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                GlanceModifier.cornerRadius(android.R.dimen.system_app_widget_background_radius)
            } else {
                GlanceModifier
            },
        )
        .clickable(actionStartActivity(openIntent(ctx, OpenDest.Today)))
        .semantics { contentDescription = openToday }
    when (layout.shape) {
        WidgetShape.Line -> LineLayout(face, colors, layout, root)
        WidgetShape.Stack -> StackLayout(face, colors, layout, root)
    }
}

/** One row: title over its status line, then the action(s) at the end. */
@Composable
private fun LineLayout(face: WidgetFace, colors: WidgetColors, layout: WidgetLayout, modifier: GlanceModifier) {
    Row(
        modifier = modifier.padding(horizontal = layout.edge, vertical = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = GlanceModifier.defaultWeight()) {
            Text(face.title, style = Type.BodyBold.glance(face.titleColor(colors)), maxLines = layout.titleLines)
            layout.status?.let {
                Text(it, style = Type.Caption.glance(colors.muted), maxLines = layout.statusLines)
            }
        }
        Actions(face, colors, layout)
    }
}

/**
 * The stacked card, composed for what it holds:
 * - a list (a lift day's items): overline, title and lines from the top, the week
 *   block and actions at the bottom;
 * - a hero (a run's distance, the next session): the hero block centred in the
 *   room between the title and the week block;
 * - only a title and its line (a hold, no plan, open to update): the overline at
 *   the top, the title (large where it fits) and its line set on the actions.
 * Every block is its own column: a Glance column keeps at most 10 children and
 * drops the rest, which would take the actions.
 */
@Composable
private fun StackLayout(face: WidgetFace, colors: WidgetColors, layout: WidgetLayout, modifier: GlanceModifier) {
    val ctx = LocalContext.current
    val res = ctx.appResources()
    val lines = bodyLines(face, layout, res)
    // The lead lines (session done, the next session, the pause pointer) read muted;
    // items are what to do and read in body colour.
    val leadCount = (if (layout.hero) face.heroLead.size else face.lead.size).coerceAtMost(lines.size)
    val poster = !layout.hero && !layout.week && face.items.isEmpty()
    val title = @Composable {
        Text(
            face.title,
            style = (if (layout.bigTitle) Type.Display else Type.Title).glance(face.titleColor(colors)),
            maxLines = layout.titleLines,
        )
        if (layout.titleGap) Spacer(GlanceModifier.height(Space.Sm.dp))
    }
    val line = @Composable { l: BodyLine, muted: Boolean ->
        Text(l.text, style = (if (l.large) Type.Body else Type.Caption).glance(if (muted) colors.muted else colors.body), maxLines = l.maxLines)
    }
    Column(modifier = modifier.padding(Space.Screen.dp)) {
        Column(modifier = GlanceModifier.fillMaxWidth()) {
            if (layout.overline) {
                val date = face.day?.let { planDateLabel(it) }
                // Narrow (or the full line would not fit): the date alone; it is the day the content is for.
                val text = if (layout.todayOverline || date == null) face.overline else date
                Text(text.uppercase(), style = Type.Overline.glance(colors.accent), maxLines = 1)
                Spacer(GlanceModifier.height(Space.Sm.dp))
            }
            if (!poster) title()
        }
        when {
            poster -> {
                Spacer(GlanceModifier.defaultWeight())
                Column(modifier = GlanceModifier.fillMaxWidth()) {
                    title()
                    lines.forEach { line(it, true) }
                }
            }
            layout.hero -> {
                val hero = face.hero!!
                Spacer(GlanceModifier.defaultWeight())
                Column(modifier = GlanceModifier.fillMaxWidth()) {
                    lines.take(leadCount).forEach { line(it, true) }
                    Text(
                        hero.text,
                        style = (if (hero.figure) Type.Display else Type.BodyBold).glance(colors.body),
                        maxLines = if (hero.figure) 1 else 2,
                    )
                    hero.note?.let { Text(it, style = Type.Caption.glance(colors.muted), maxLines = 2) }
                    lines.drop(leadCount).forEach { line(it, false) }
                }
                Spacer(GlanceModifier.defaultWeight())
            }
            else -> {
                Column(modifier = GlanceModifier.fillMaxWidth()) {
                    lines.forEachIndexed { i, l -> line(l, i < leadCount) }
                }
                Spacer(GlanceModifier.defaultWeight())
            }
        }
        if (layout.week) {
            Spacer(GlanceModifier.height(Space.Md.dp))
            Column(modifier = GlanceModifier.fillMaxWidth()) { WeekStrip(face, colors, layout) }
        }
        if (layout.maxActions > 0) {
            Spacer(GlanceModifier.height(Space.Md.dp))
            Row(modifier = GlanceModifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                if (layout.wide) Spacer(GlanceModifier.defaultWeight())
                Actions(face, colors, layout, fill = !layout.wide)
            }
        }
    }
}

/** The icon's drawable (24 dp), also the source of the iOS extension's image set. */
internal val SurfaceIcon.drawable: Int
    get() = when (this) {
        SurfaceIcon.TrackRun -> R.drawable.ic_content_track_run
        SurfaceIcon.LogSet -> R.drawable.ic_content_set_dumbbell
        SurfaceIcon.Log -> R.drawable.ic_action_log_plus
        SurfaceIcon.Mark -> R.drawable.ic_stat_mark
        SurfaceIcon.Pause -> R.drawable.ic_track_pause
        SurfaceIcon.Play -> R.drawable.ic_track_play
        SurfaceIcon.Stop -> R.drawable.ic_track_stop
    }

/** The button's icon: its tile face's. */
private val WidgetAction.icon: Int get() = face.icon.drawable

/**
 * The action(s): the secondary one (Log) as a tinted round icon button, the
 * primary as an accent button with its label, or a round icon button where the
 * label does not fit (a 2×1). Both open their screen through [openIntent].
 */
@Composable
private fun RowScope.Actions(face: WidgetFace, colors: WidgetColors, layout: WidgetLayout, fill: Boolean = false) {
    val ctx = LocalContext.current
    val actions = face.actions.take(layout.maxActions)
    if (actions.isEmpty()) return
    fun open(a: WidgetAction): Action = actionStartActivity(openIntent(ctx, a.dest))
    // Space.Md from the text in the one-row layout, and between the two actions.
    if (layout.shape == WidgetShape.Line) Spacer(GlanceModifier.width(Space.Md.dp))
    actions.drop(1).forEach { a ->
        CircleIconButton(
            imageProvider = ImageProvider(a.icon),
            contentDescription = a.label,
            onClick = open(a),
            backgroundColor = colors.accentTint,
            contentColor = colors.accent,
        )
        Spacer(GlanceModifier.width(Space.Md.dp))
    }
    val primary = actions.first()
    if (layout.labelled) {
        // A full-width button too narrow for icon and label drops the icon, never the label.
        val icon = !fill || labelledWidth(primary.label, layout.fontScale) <= layout.textWidth
        FilledButton(
            text = primary.label,
            onClick = open(primary),
            modifier = if (fill) GlanceModifier.defaultWeight() else GlanceModifier,
            icon = if (icon) ImageProvider(primary.icon) else null,
            colors = ButtonDefaults.buttonColors(backgroundColor = colors.accent, contentColor = colors.onAccent),
            maxLines = 1,
        )
    } else {
        CircleIconButton(
            imageProvider = ImageProvider(primary.icon),
            contentDescription = primary.label,
            onClick = open(primary),
            backgroundColor = colors.accent,
            contentColor = colors.onAccent,
        )
    }
}

/**
 * The week block: the week count ("1 of 4 sessions done"), then, where the layout
 * has room ([WidgetLayout.weekDays]), the plan week Monday to Sunday, each day's
 * weekday over its mark (✓ trained, ● the next session, ○ planned, a muted ○
 * missed, nothing on a rest day, – before the plan), today's label in `accent` on
 * an accent tint. The strip is spoken as the count, which it draws.
 */
@Composable
private fun ColumnScope.WeekStrip(face: WidgetFace, colors: WidgetColors, layout: WidgetLayout) {
    val today = face.day
    Text(face.week.orEmpty(), style = Type.Caption.glance(colors.muted), maxLines = layout.weekLines)
    if (!layout.weekDays) return
    Spacer(GlanceModifier.height(Space.Sm.dp))
    Row(
        modifier = GlanceModifier.fillMaxWidth().semantics { contentDescription = face.week.orEmpty() },
    ) {
        face.days.forEach { d ->
            val isToday = d.epochDay == today
            val (glyph, color) = d.mark.glyph.let { it.text to colors[it.ink] }
            Column(
                modifier = GlanceModifier
                    .defaultWeight()
                    .then(
                        if (isToday) {
                            GlanceModifier.background(colors.accentTint).cornerRadius(Space.Md.dp)
                        } else {
                            GlanceModifier
                        },
                    )
                    .padding(vertical = Space.Sm.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    weekdayStripLabel(LocalDate.fromEpochDays(d.epochDay).dayOfWeek),
                    style = (if (isToday) Type.Overline else Type.Overline.copy(fontWeight = ComposeFontWeight.Normal))
                        .glance(if (isToday) colors.accent else colors.muted, TextAlign.Center),
                    maxLines = 1,
                )
                Text(glyph, style = Type.Chip.glance(color, TextAlign.Center), maxLines = 1)
            }
        }
    }
}
