package app.milestone.qa

import android.graphics.Bitmap
import android.view.View
import android.view.accessibility.AccessibilityManager
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.click
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import androidx.test.espresso.Espresso
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.Event
import app.milestone.R
import app.milestone.Core
import app.milestone.contrastRatio
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

/**
 * Accessibility audit over the REAL app and core: walks the main tabs (every
 * scroll page) and, by tapping every action on them, every sheet, dialog and
 * in-place panel they open, then audits what a screen reader gets from each
 * screen through the Compose semantics tree and the traversal order Compose hands
 * TalkBack (its accessibility delegate's traversal-before map, with touch
 * exploration on, as under TalkBack):
 *
 *  - every action has a label and a role; switches, checkboxes, radios and tabs
 *    expose their checked / selected state;
 *  - titles (`Type.Title`) and section overlines (`Type.Section`) are headings;
 *  - an image node has a description; no focusable node announces the same words
 *    twice;
 *  - custom controls expose a value and the action to change it (rating scale,
 *    keypad, week strip, trim thumbs);
 *  - traversal follows the visual order: never upward within a column, start to
 *    end within a row (right to left under RTL); the floating Log button and a
 *    snackbar are read after the list they ride over, and the Material time
 *    picker's clock face around the dial;
 *  - actions are at least 48dp (fully visible ones, [smallTargets]);
 *  - rendered text contrast: the pixels of each fully visible text line in a
 *    software capture, glyph vs ground, at least 4.5:1 (3:1 for large text, WCAG
 *    1.4.3), in every theme; screens with a sheet or dialog open, opening or closing
 *    are not sampled (Robolectric's software drawing does not composite them).
 *
 * The Material time picker's clock numbers are clickable with no role inside
 * library code ([MATERIAL_TIME_DIAL]); they are not reported.
 *
 * Configurations: font 1.0 / 2.0, light / dark, LTR / RTL (an Arabic locale). By default
 * the targeted tests and the regression pins run, plus one crawl: a hybrid athlete's
 * tabs and the sheets one tap away at font 2.0, dark, RTL. `A11Y_FULL=1` adds the
 * same crawl at font 1.0, light, LTR, the contrast pass over the 8 palettes and the
 * full crawl: every [Start] at all 8 configurations, two taps deep. Gradle caches a
 * passed test: add `--rerun` when only the environment changed.
 */
@RunWith(RobolectricTestRunner::class)
class AccessibilityAuditTest : UiProbeTest() {

    data class Config(val font: Float, val dark: Boolean, val rtl: Boolean) {
        override fun toString() = "font=$font ${if (dark) "dark" else "light"} ${if (rtl) "rtl" else "ltr"}"
    }

    data class Finding(val kind: String, val detail: String, val where: String, val config: String) {
        override fun toString() = "[$kind] $detail  @ $where ($config)"
    }

    private val findings = LinkedHashMap<String, Finding>()
    private var config = Config(1f, false, false)
    private var where = ""

    // ── Tests ───────────────────────────────────────────────────────────────

    @Test
    fun hybridAthlete_font2_dark_rtl() = audit(Start.Hybrid, Config(2f, dark = true, rtl = true), depth = 1)

    @Test
    fun hybridAthlete_font1_light_ltr() {
        assumeTrue("A11Y_FULL=1 runs the LTR crawl", FULL)
        audit(Start.Hybrid, Config(1f, dark = false, rtl = false), depth = 1)
    }

    /** Every palette (and Paper with the system accent, a fresh install's default),
     *  light and dark: every page of each tab, sampled from captures. */
    @Test
    fun renderedTextContrast_everyTheme() {
        assumeTrue("A11Y_FULL=1 runs the contrast pass", FULL)
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Hybrid, config)
        val palettes = AppTheme.entries.map { it to false } + (AppTheme.Paper to true)
        for ((theme, dynamic) in palettes) for (dark in listOf(false, true)) {
            AppSettings.theme.value = theme
            AppSettings.dynamicAccent.value = dynamic
            AppSettings.darkMode.value = if (dark) DarkMode.Dark else DarkMode.Light
            config = config.copy(dark = dark)
            val label = "$theme${if (dynamic) "+system accent" else ""} ${if (dark) "dark" else "light"}"
            for (tab in TABS.map { str(it) }) {
                if (!goHome(tab)) continue
                scrollMainToTop()
                var page = 0
                while (true) {
                    where = "$tab p$page"
                    auditContrast(label)
                    if (page + 1 >= MAX_PAGES || !scrollMainBy(page + 1)) break
                    page++
                }
            }
        }
        assertNoFindings()
    }

    // ── Custom controls: value and action exposed ─────────────────────────────

    /** The set loop's rating tracks (effort, reps left) and its number keypad. */
    @Test
    fun customControls_setLoopScalesAndKeypad() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Lifter, config)
        val ex = mergedNodes().mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
            .first { it.startsWith("plan-item:") }.removePrefix("plan-item:")
        rule.openLoopFor(ex)
        settle()
        where = "set loop"
        auditNow()
        // Reps and reps-left rows: pick-one groups whose cells read their state and label.
        for (tag in listOf("reps-scale", "effort-scale")) {
            val group = unmergedNodes().firstOrNull { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
            assertTrue("the loop shows $tag", group != null)
            val cells = mergedNodes().filter { n ->
                n.config.getOrNull(SemanticsActions.OnClick) != null && n.config.getOrNull(SemanticsProperties.Role) == Role.RadioButton &&
                    generateSequence(n) { it.parent }.any { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
            }
            assertTrue("$tag has radio cells", cells.isNotEmpty())
            node(cells.first()).performSemanticsAction(SemanticsActions.OnClick)
            settle()
            val picked = mergedNodes().filter { n ->
                n.config.getOrNull(SemanticsProperties.Selected) == true &&
                    generateSequence(n) { it.parent }.any { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
            }
            assertEquals("$tag: the tapped cell reads selected", listOf(nameOf(cells.first())), picked.map { nameOf(it) })
        }
        // The keypad: every key a labelled button; the non-digit keys say what they do.
        tapLabelContaining(str(R.string.log_field_weight))
        settle()
        where = "set loop keypad"
        auditNow()
        val keys = mergedNodes().filter { n ->
            n.config.getOrNull(SemanticsActions.OnClick) != null &&
                generateSequence(n) { it.parent }.any { it.config.getOrNull(SemanticsProperties.TestTag) == "numeric-keypad" }
        }
        val labels = keys.map { spoken(it).joinToString(", ") }
        for (d in 0..9) assertTrue("digit $d key in $labels", labels.any { it == "$d" })
        assertTrue("decimal key named in $labels", labels.any { it.contains(str(R.string.ui_cd_decimal_point)) })
        assertTrue("delete key named in $labels", labels.any { it.contains(str(R.string.ui_cd_delete_digit)) })
        assertNoFindings()
    }

    /** Week strip, activity heatmap, distance charts, route map and the trim thumbs. */
    @Test
    fun customControls_weekStripHeatmapChartsRouteAndTrim() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Hybrid, config, extra = listOf(QaHarness.gpsTenK(QaHarness.daysAgoAtMidday(2) - 7_200L, sessionRpe = 6)))
        // Week strip: seven day tabs, each naming its weekday, one selectable at a time.
        val days = mergedNodes().filter { n ->
            n.config.getOrNull(SemanticsProperties.Role) == Role.Tab && generateSequence(n) { it.parent }.any { it.config.getOrNull(SemanticsProperties.TestTag) == "this-week" }
        }
        assertEquals("seven day tabs: ${days.map { nameOf(it) }}", 7, days.size)
        val weekdays = java.time.DayOfWeek.entries.map { it.getDisplayName(java.time.format.TextStyle.FULL, java.util.Locale.US) }
        days.zip(weekdays).forEach { (n, wd) ->
            assertTrue("day tab names $wd: ${nameOf(n)}", cds(n).any { it.contains(wd) })
        }
        node(days[3]).performSemanticsAction(SemanticsActions.OnClick)
        settle()
        val picked = mergedNodes().filter { it.config.getOrNull(SemanticsProperties.Role) == Role.Tab && it.config.getOrNull(SemanticsProperties.Selected) == true && cds(it).any { c -> c.contains(weekdays[3]) } }
        assertEquals("the tapped day reads selected", 1, picked.size)
        where = "Today, Thursday open"
        auditNow()

        // Morning check-in: each rating track is one adjustable node with a spoken value.
        tapTag("log-fab")
        settle()
        tapLabelContaining(str(R.string.main_morning_checkin_title))
        settle()
        where = "morning check-in"
        auditNow()

        // History: every logged heatmap day names itself, with an open action.
        goHome(str(R.string.main_dest_history))
        val cells = unmergedNodes().filter { it.config.getOrNull(SemanticsActions.CustomActions)?.any { a -> a.label == str(R.string.history_heatmap_open_day) } == true }
        assertTrue("logged heatmap days offer \"Open this day\"", cells.isNotEmpty())
        assertTrue("each logged day names itself", cells.all { cds(it).any { c -> c.isNotBlank() } })

        // Run detail: the route map describes the route.
        val runTitle = str(R.string.history_run_weekday, "Saturday")
        scrollMainToText(runTitle)
        tapLabelContaining(runTitle)
        settle()
        where = "run detail (GPS)"
        auditNow()
        val mapPrefix = str(R.string.runmap_cd).substringBefore("%")
        assertTrue("route map description", unmergedNodes(roots().last()).any { n -> cds(n).any { it.startsWith(mapPrefix) } })

        // Trim: two adjustable thumbs, each named and saying what it removes.
        val trim = flatten(roots().last()).first { it.config.getOrNull(SemanticsProperties.TestTag) == "run-trim" }
        node(trim).performSemanticsAction(SemanticsActions.OnClick)
        settle()
        where = "trim sheet"
        auditNow()
        val thumbs = mergedNodes(roots().last()).filter { it.config.getOrNull(SemanticsActions.SetProgress) != null }
        assertEquals("two trim thumbs", 2, thumbs.size)
        val names = listOf(str(R.string.trim_start_thumb), str(R.string.trim_end_thumb))
        thumbs.forEach { t ->
            assertTrue("thumb named: ${nameOf(t)}", cds(t).any { c -> names.any { c.contains(it) } })
            assertTrue("thumb has a range", t.config.getOrNull(SemanticsProperties.ProgressBarRangeInfo) != null)
        }
        assertNoFindings()
    }

    /** A snackbar and the loop's "Logged … · Undo" line are polite live regions; the
     *  running rest countdown is not (it would speak every second). */
    @Test
    fun liveRegions_snackbarUndoLineAndRest() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Lifter, config)
        app.milestone.UiMessages.post(app.milestone.UiMessage(text = "Set updated", actionLabel = "Undo", onAction = {}))
        rule.waitUntil(10_000) { unmergedNodes().any { n -> texts(n).contains("Set updated") } }
        val snack = unmergedNodes().first { n -> texts(n).contains("Set updated") }
        assertTrue("the snackbar is a live region", generateSequence(snack) { it.parent }.any { it.config.contains(SemanticsProperties.LiveRegion) })

        val ex = mergedNodes().mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
            .first { it.startsWith("plan-item:") }.removePrefix("plan-item:")
        rule.openLoopFor(ex)
        settle()
        val sets = Core.currentView().lifts.size
        val primary = flatten(roots().last()).first { it.config.getOrNull(SemanticsProperties.TestTag) == "editor-primary" }
        node(primary).performSemanticsAction(SemanticsActions.OnClick)
        awaitCoreLifts(sets + 1)
        // The countdown on Today's card or in the loop ("Rest: 2:59 left").
        val restWord = strPrefix(R.string.log_rest_left_cd).substringBefore(' ')
        val clock = Regex("""\d+:\d\d""")
        val rest = roots().flatMap { allUnmerged(it) }.firstOrNull { n -> spoken(n).any { it.startsWith(restWord) && clock.containsMatchIn(it) } }
        if (rest == null) fail("a rest readout runs after a logged set")
        assertTrue("the running rest countdown is not a live region", rest!!.config.getOrNull(SemanticsProperties.LiveRegion) == null)
        val undo = str(R.string.log_undo)
        val line = unmergedNodes(roots().last()).firstOrNull { it.config.contains(SemanticsProperties.LiveRegion) && texts(it).isNotEmpty() }
        assertTrue("the loop's \"Logged … · $undo\" line is a live region", line != null)
    }

    // ── Regression pins for fixed findings ────────────────────────────────────

    @Test
    fun rtl_bottomNavigation_isReadStartToEnd() {
        config = Config(1f, dark = false, rtl = true)
        launch(Start.Hybrid, config)
        val w = windows().first()
        val names = TABS.map { str(it) }
        val tabs = flatten(w.merged).filter { it.config.getOrNull(SemanticsProperties.Role) == Role.Tab && labelOf(it) in names }
        val byX = tabs.sortedByDescending { it.boundsInRoot.right }.map { labelOf(it) }
        assertEquals("the RTL layout puts Today at the right", names, byX)
        val read = w.order.mapNotNull { id -> tabs.firstOrNull { it.id == id } }.map { labelOf(it) }
        assertEquals("TalkBack's order of the tabs under RTL (right to left)", names, read)
    }

    @Test
    fun sectionOverlinesAndSheetTitles_areHeadings() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Hybrid, config)
        goHome(str(R.string.main_dest_profile))
        val overlines = listOf(R.string.main_overline_calculators, R.string.main_overline_units).map { str(it).uppercase(java.util.Locale.US) }
        val missing = ArrayList<String>()
        for (o in overlines) {
            val n = unmergedNodes().firstOrNull { texts(it).contains(o) } ?: run { missing += "$o (not on screen)"; null } ?: continue
            if (generateSequence(n) { it.parent }.none { it.config.contains(SemanticsProperties.Heading) }) missing += "overline \"$o\""
        }
        tapLabelContaining(str(R.string.main_tool_race_title))
        settle()
        val title = unmergedNodes(roots().last()).firstOrNull { texts(it).contains(str(R.string.main_tool_race_title)) }
        if (title == null || generateSequence(title) { it.parent }.none { it.config.contains(SemanticsProperties.Heading) }) {
            missing += "sheet title \"${str(R.string.main_tool_race_title)}\""
        }
        assertTrue("not headings: $missing", missing.isEmpty())
    }

    @Test
    fun glossarySearch_namesItselfOnce() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Hybrid, config)
        goHome(str(R.string.main_dest_history))
        val explain = str(R.string.main_explain, "").trim()
        tapLabelContaining(explain)
        settle()
        val field = mergedNodes(roots().last()).first { it.config.getOrNull(SemanticsProperties.TestTag) == "glossary-search" }
        val words = spoken(field)
        assertEquals("the empty search field's spoken words: $words", 1, words.count { it == str(R.string.main_glossary_search_hint) })
    }

    @Test
    fun historyCardsAndProfileRows_haveRoles() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Hybrid, config)
        goHome(str(R.string.main_dest_history))
        val runTitle = str(R.string.history_run_weekday, "Sunday")
        scrollMainToText(runTitle)
        val bare = ArrayList<String>()
        val cards = mergedNodes().filter { it.config.getOrNull(SemanticsActions.OnClick) != null && spoken(it).firstOrNull() == runTitle }
        assertTrue("History shows a clickable \"$runTitle\" card", cards.isNotEmpty())
        cards.forEach { if (it.config.getOrNull(SemanticsProperties.Role) == null) bare += nameOf(it) }
        goHome(str(R.string.main_dest_profile))
        rule.openPlanAnswers()
        settle()
        scrollMainToText(str(R.string.setup_row_lift_experience))
        val rows = mergedNodes().filter { it.config.getOrNull(SemanticsActions.OnClick) != null && spoken(it).firstOrNull() == str(R.string.setup_row_lift_experience) }
        assertTrue("the plan answers show a clickable \"${str(R.string.setup_row_lift_experience)}\" row", rows.isNotEmpty())
        rows.forEach { if (it.config.getOrNull(SemanticsProperties.Role) == null) bare += nameOf(it) }
        assertTrue("clickable with no role: $bare", bare.isEmpty())
    }

    /** One node per week the chart shows, each in TalkBack's order after the summary. */
    @Test
    fun weeklyDistanceChart_weekFiguresReachableWithoutTouch() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Runner, config)
        goHome(str(R.string.main_dest_history))
        val prefix = strPrefix(R.string.history_weekly_desc).take(12)
        scrollMainToText(prefix)
        val weeks = checkNotNull(Core.currentView().run_progress).weekly.size
        val w = windows().first()
        val nodes = flatten(w.unmerged).filter { it.placedVisible() }
        val summary = nodes.first { n -> cds(n).any { it.startsWith(prefix) } }
        val weekPrefix = strPrefix(R.string.history_weekly_bar_value)
        val perWeek = nodes.filter { n -> cds(n).any { it.startsWith(weekPrefix) } }
        assertEquals("one node per week shown: ${perWeek.map { cds(it) }}", weeks, perWeek.size)
        val at = w.order.indexOf(summary.id)
        val weekAt = perWeek.map { w.order.indexOf(it.id) }
        assertTrue(
            "the summary \"${short(cds(summary).first())}\" is read before every week: summary at $at, weeks at $weekAt in TalkBack's order",
            at >= 0 && weekAt.all { it > at },
        )
    }

    /** History's "Last 7 days" tiles at font 2.0, LTR and RTL: tile by tile from the
     *  start edge, though "sessions" wraps to more lines than "running". */
    @Test
    fun historyTiles_readTileByTile_atFont2() {
        val sessions = setOf(1, 4).map { ctx.resources.getQuantityString(R.plurals.history_stat_sessions, it) }
        val tonnage = str(R.string.main_stat_tonnage)
        val running = str(R.string.main_stat_running)
        for (rtl in listOf(false, true)) {
            freshProcess()
            config = Config(2f, dark = false, rtl = rtl)
            launch(Start.MilesLb, config)
            goHome(str(R.string.main_dest_history))
            scrollMainToTop()
            where = "History p0"
            val w = windows().first()
            val byId = flatten(w.merged).associateBy { it.id }
            val read = w.order.mapNotNull { byId[it] }.map { spoken(it).joinToString(" ") }
                .mapNotNull { s -> if (s in sessions) "sessions" else s.takeIf { it == tonnage || it == running } }
            assertEquals("tile labels in TalkBack's order ($config)", listOf("sessions", tonnage, running), read)
            auditTraversal(w)
        }
        assertNoFindings()
    }

    /** A safety card names itself once: its SAFETY chip, not the mark tile as well. */
    @Test
    fun safetyCard_saysSafetyOnce() {
        config = Config(1f, dark = false, rtl = false)
        launch(Start.Pregnancy, config)
        where = "Today p0"
        val chip = str(R.string.main_chip_safety)
        assertTrue("a safety card on Today", mergedNodes(roots().first()).any { n -> texts(n).any { it.equals(chip, ignoreCase = true) } })
        auditSemantics(windows().first())
        assertNoFindings()
    }

    // ── Small node helpers for the targeted tests ─────────────────────────────

    private fun mergedNodes(root: SemanticsNode = roots().last()): List<SemanticsNode> = flatten(root).filter { it.placedVisible() }

    private fun unmergedNodes(root: SemanticsNode = roots().last()): List<SemanticsNode> = allUnmerged(root).filter { it.placedVisible() }

    private fun tapTag(tag: String) {
        val n = allUnmerged(roots().last()).first { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
        val target = generateSequence(n) { it.parent }.first { it.config.getOrNull(SemanticsActions.OnClick) != null }
        rule.onAllNodes(SemanticsMatcher("node #${target.id}") { it.id == target.id && it.boundsInRoot == target.boundsInRoot }, useUnmergedTree = true)[0]
            .performSemanticsAction(SemanticsActions.OnClick)
    }

    private fun tapLabelContaining(text: String) {
        val n = flatten(roots().last()).first { it.isVisible() && it.config.getOrNull(SemanticsActions.OnClick) != null && spoken(it).any { s -> s.contains(text) } }
        node(n).performSemanticsAction(SemanticsActions.OnClick)
    }

    /** Scroll the main list until a node speaking [text] is on screen. */
    private fun scrollMainToText(text: String) {
        scrollMainToTop()
        var page = 0
        while (unmergedNodes(roots().first()).none { n -> spoken(n).any { it.contains(text) } } && page < MAX_PAGES) {
            if (!scrollMainBy(++page)) break
        }
    }

    @Test
    fun fullCrawl_everyStartEveryConfig() {
        assumeTrue("A11Y_FULL=1 runs the full crawl", FULL)
        for (s in Start.entries) for (c in CONFIGS) {
            freshProcess()
            crawlOnce(s, c, depth = 2)
        }
        assertNoFindings()
    }

    // ── Driver ──────────────────────────────────────────────────────────────

    private fun audit(start: Start, c: Config, depth: Int) {
        crawlOnce(start, c, depth)
        assertNoFindings()
    }

    private fun crawlOnce(start: Start, c: Config, depth: Int) {
        config = c
        launch(start, c)
        val tabs = if (start == Start.FreshInstall) listOf<String?>(null) else TABS.map { str(it) }
        for (tab in tabs) {
            if (tab != null && !goHome(tab)) fail("the crawl cannot reach the $tab tab")
            crawlScreen(tab, depth)
        }
    }

    private fun launch(start: Start, c: Config, extra: List<Event> = emptyList()) {
        val am = ctx.getSystemService(AccessibilityManager::class.java)
        shadowOf(am).setEnabled(true)
        shadowOf(am).setTouchExplorationEnabled(true)
        // RTL comes from an Arabic locale (the app has no Arabic strings, so the copy
        // stays English, as on such a phone): a bare `ldrtl` qualifier does not
        // survive setFontScale's configuration update and the activity lays out LTR.
        if (c.rtl) RuntimeEnvironment.setQualifiers("+ar-rEG")
        RuntimeEnvironment.setFontScale(c.font)
        AppSettings.darkMode.value = if (c.dark) DarkMode.Dark else DarkMode.Light
        if (start == Start.MilesLb) AppSettings.setDistanceUnitOverride(ctx, app.milestone.DistanceUnitOverride.Mi)
        (seedEvents(start).orEmpty() + extra).takeIf { it.isNotEmpty() }?.let { QaHarness.seedLog(ctx, it) }
        this.start = start
        if (start == Start.FreshInstall) rule.launchAppExpectingSetup() else rule.launchApp()
        settle()
        launchSettings = Triple(AppSettings.theme.value, AppSettings.dynamicAccent.value, AppSettings.darkMode.value)
    }

    private var launchSettings: Triple<AppTheme, Boolean, DarkMode>? = null

    /** [MATERIAL_SHEET_CHROME] in the current locale. */
    private val materialSheetChrome: Set<String>
        get() = MATERIAL_SHEET_CHROME.mapNotNull(::materialString).toSet()

    /** The Material time picker's clock-face numbers ("7 o'clock", "35 minutes") in the
     *  current locale: [MATERIAL_TIME_DIAL] with its number as any digits. */
    private val materialTimeDial: List<Regex>
        get() = MATERIAL_TIME_DIAL.mapNotNull(::materialString).map { f ->
            Regex(f.split("%1\$d").joinToString("""\p{Nd}+""") { Regex.escape(it) })
        }

    private fun materialString(name: String): String? =
        listOf(ctx.packageName, "app.milestone", "androidx.compose.material3").firstNotNullOfOrNull { pkg ->
            ctx.resources.getIdentifier(name, "string", pkg).takeIf { it != 0 }
        }?.let { ctx.getString(it) }

    /** A number on the Material time picker's clock face (library code: see [MATERIAL_TIME_DIAL]). */
    private fun isTimeDialNumber(n: SemanticsNode, dial: List<Regex> = materialTimeDial): Boolean =
        spoken(n).singleOrNull()?.let { s -> dial.any { it.matches(s) } } == true

    private lateinit var start: Start

    /** Audit the current screen page by page and tap each action on each page. */
    private fun crawlScreen(tab: String?, depth: Int) {
        val base = tab ?: "setup"
        scrollMainToTop()
        var page = 0
        val tapped = HashSet<String>()
        while (page < MAX_PAGES) {
            where = "$base p$page"
            auditNow()
            if (depth >= 1) {
                for (label in actionLabels(roots().first(), skipTabs = true)) {
                    if (!tapped.add(label)) continue
                    tapAndAudit(tab, page, label, depth)
                }
            }
            if (!scrollMainBy(page + 1)) break
            page++
        }
    }

    /** Tap [label] on page [page] of [tab]; audit what it opened; restore. */
    private fun tapAndAudit(tab: String?, page: Int, label: String, depth: Int) {
        if (label in SKIP_TAPS.map { str(it) }) return
        if (!bringIntoView(tab, page, label)) return
        val before = fingerprint()
        val windows = roots().size
        if (!guard("tap $label") { tapLabel(roots().first(), label) }) return
        settle()
        // A picked swatch or mode can leave the texts as they were: restore anyway.
        if (fingerprint() == before) return restoreSettings()
        where = "${tab ?: "setup"} p$page > $label"
        auditNow()
        val opened = roots().size > windows
        if (opened && depth >= 2) crawlOverlay(where)
        if (!opened && (tab == null || selectedTab() == tab) && roots().size == 1) {
            // An in-place panel (a disclosure, a day of the week strip): tap it again to fold.
            guard("refold $label") { tapLabel(roots().first(), label) }
            settle()
        }
        if (tab != null) goHome(tab)
        restoreSettings()
    }

    /** The crawl taps the theme, dark-mode and unit pickers too; put the
     *  configuration's settings back so every later screen is audited in it. */
    private fun restoreSettings() {
        val (theme, dynamic, dark) = launchSettings ?: return
        if (AppSettings.theme.value == theme && AppSettings.dynamicAccent.value == dynamic && AppSettings.darkMode.value == dark) return
        AppSettings.theme.value = theme
        AppSettings.dynamicAccent.value = dynamic
        AppSettings.darkMode.value = dark
        settle()
    }

    /** One level deeper inside the open overlay (sheet or dialog). */
    private fun crawlOverlay(path: String) {
        val depthWindows = roots().size
        val tapped = HashSet<String>()
        repeat(MAX_OVERLAY_PAGES) { page ->
            for (label in actionLabels(roots().last(), skipTabs = false)) {
                if (!tapped.add(label) || label in OVERLAY_SKIP.map { str(it) }) continue
                val top = roots().last()
                val before = fingerprint()
                if (!guard("tap $label") { tapLabel(top, label) }) continue
                settle()
                if (fingerprint() == before) continue
                where = "$path > $label"
                auditNow()
                // Back to the overlay we came from.
                var n = 0
                while (roots().size > depthWindows && n++ < 3) {
                    guard("back") { Espresso.pressBackUnconditionally() }
                    settle()
                }
                if (roots().size < depthWindows) return
            }
            val top = roots().last()
            val scroller = flatten(top).firstOrNull { it.config.getOrNull(SemanticsActions.ScrollBy) != null && it.isVisible() } ?: return
            val ok = guard("scroll overlay") {
                node(scroller).performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, scroller.boundsInRoot.height * 0.8f) }
            }
            if (!ok) return
            settle()
            where = "$path (scrolled ${page + 1})"
            auditNow()
        }
    }

    /** Back to [tab] on the main scaffold with nothing open; relaunch if Back cannot. */
    private fun goHome(tab: String): Boolean {
        var n = 0
        while ((roots().size > 1 || selectedTab() == null) && n++ < 5) {
            guard("back") { Espresso.pressBackUnconditionally() }
            relaunchIfLeft()
            settle()
        }
        if (roots().size > 1 || selectedTab() == null) {
            relaunch()
        }
        if (selectedTab() != tab) {
            guard("tab $tab") {
                rule.onAllNodes(SemanticsMatcher("tab $tab") { n -> n.config.getOrNull(SemanticsProperties.Role) == Role.Tab && labelOf(n) == tab })[0]
                    .performSemanticsAction(SemanticsActions.OnClick)
            }
            settle()
        }
        return selectedTab() == tab
    }

    private fun relaunch() {
        runCatching { QaHarness.teardown() }
        rule.launchApp()
        settle()
    }

    // ── Screen helpers ──────────────────────────────────────────────────────

    override fun fail(msg: String): Nothing = super.fail("$msg\n@ $where ($config)")

    /** Labels of the enabled, visible actions in [root], top to bottom. */
    private fun actionLabels(root: SemanticsNode, skipTabs: Boolean): List<String> = flatten(root)
        .filter { n ->
            n.isVisible() && n.config.getOrNull(SemanticsActions.OnClick) != null &&
                n.config.getOrNull(SemanticsProperties.Disabled) == null &&
                !(skipTabs && n.config.getOrNull(SemanticsProperties.Role) == Role.Tab && labelOf(n) in TABS.map { str(it) })
        }
        .sortedWith(compareBy({ it.boundsInRoot.top }, { it.boundsInRoot.left }))
        .map { labelOf(it) }
        .filter { it.isNotBlank() }
        .distinct()

    private fun tapLabel(root: SemanticsNode, label: String) {
        val n = flatten(root).first { it.isVisible() && it.config.getOrNull(SemanticsActions.OnClick) != null && labelOf(it) == label }
        node(n).performSemanticsAction(SemanticsActions.OnClick)
    }

    private fun mainScroller(): SemanticsNode? = roots().firstOrNull()?.let { r ->
        flatten(r).filter { it.isVisible() && it.config.getOrNull(SemanticsActions.ScrollBy) != null }
            .maxByOrNull { it.boundsInRoot.height * it.boundsInRoot.width }
    }

    private fun scrollMainToTop() {
        val s = mainScroller() ?: return
        repeat(30) {
            val range = s.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) ?: return
            if (range.value() <= 0f) return
            guard("scroll up") { node(s).performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, -100_000f) } }
            idle()
            val again = mainScroller() ?: return
            if ((again.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.value() ?: 0f) <= 0f) return
        }
    }

    /** Scroll the main list to page [page] from the top; false past the end. */
    private fun scrollMainBy(page: Int): Boolean {
        scrollMainToTop()
        repeat(page) {
            val s = mainScroller() ?: return false
            val range = s.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) ?: return false
            if (range.value() >= range.maxValue() - 1f && !range.reverseScrolling) {
                // A lazy list reports a growing max; one more step past a settled end is the end.
                if (range.maxValue() <= 0f || range.value() >= range.maxValue()) return false
            }
            val v0 = range.value()
            guard("scroll") { node(s).performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, s.boundsInRoot.height * 0.8f) } }
            idle()
            val v1 = mainScroller()?.config?.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.value() ?: v0
            if (v1 <= v0 + 1f) return false
        }
        settle()
        return true
    }

    private fun bringIntoView(tab: String?, page: Int, label: String): Boolean {
        fun visible() = flatten(roots().first()).any { it.isVisible() && it.config.getOrNull(SemanticsActions.OnClick) != null && labelOf(it) == label }
        if (roots().size == 1 && visible()) return true
        if (tab != null && !goHome(tab)) return false
        if (page > 0) scrollMainBy(page) else scrollMainToTop()
        return visible()
    }

    // ── Audit ───────────────────────────────────────────────────────────────

    private class Win(val index: Int, val view: ViewRootForTest, val merged: SemanticsNode, val unmerged: SemanticsNode, val order: List<Int>)

    private fun windows(): List<Win> = composeRoots().mapIndexed { i, r ->
        r.forceAccessibilityForTesting(true)
        val merged = r.semanticsOwner.rootSemanticsNode
        val unmerged = r.semanticsOwner.unmergedRootSemanticsNode
        // Building any node info makes the delegate (re)compute its traversal map.
        val provider = r.view.accessibilityNodeProvider
        flatten(unmerged).drop(1).firstOrNull()?.let { provider?.createAccessibilityNodeInfo(it.id) }
        Win(i, r, merged, unmerged, traversal(r))
    }

    private fun report(kind: String, detail: String) {
        val f = Finding(kind, detail, where, "${if (::start.isInitialized) "$start " else ""}$config")
        // One entry per kind + detail: the same widget seen again elsewhere is the same finding.
        findings.putIfAbsent("$kind|$detail", f)
    }

    private fun auditNow() {
        // The top window only: what is under a modal sheet or dialog is out of reach.
        val w = windows().lastOrNull() ?: fail("no Compose window to audit")
        if (w.order.isEmpty()) fail("TalkBack's traversal order of the top window is empty")
        auditSemantics(w)
        auditTraversal(w)
        smallTargets().forEach { report("small-touch-target", it) }
        // In the crawl's own theme (the seeded user's).
        auditContrast(
            "${AppSettings.theme.value}${if (AppSettings.dynamicAccent.value) "+system accent" else ""} " +
                if (AppSettings.darkMode.value == DarkMode.Dark) "dark" else "light",
        )
    }

    private fun SemanticsNode.hidden(): Boolean = generateSequence(this) { it.parent }.any {
        it.config.contains(SemanticsProperties.HideFromAccessibility)
    }

    private fun SemanticsNode.placedVisible(): Boolean = layoutInfo.isPlaced && isVisible() && !hidden()

    private fun texts(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text }
    private fun cds(n: SemanticsNode): List<String> = n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()

    /** The words a screen reader speaks for a focusable merged node. */
    private fun spoken(n: SemanticsNode): List<String> =
        (cds(n) + texts(n) + listOfNotNull(n.config.getOrNull(SemanticsProperties.EditableText)?.text)).filter { it.isNotBlank() }

    private fun short(s: String) = s.replace("\n", " ").let { if (it.length > 60) it.take(60) + "…" else it }

    private fun nameOf(n: SemanticsNode): String = spoken(n).joinToString(", ").ifBlank {
        n.config.getOrNull(SemanticsProperties.TestTag) ?: "<${n.config.getOrNull(SemanticsProperties.Role) ?: "node"} ${n.boundsInRoot.dpString()}>"
    }.let(::short)

    private fun Rect.dpString(): String {
        val d = rule.density.density
        return "(${(left / d).toInt()},${(top / d).toInt()} ${(width / d).toInt()}x${(height / d).toInt()}dp)"
    }

    private fun auditSemantics(w: Win) {
        val merged = flatten(w.merged).drop(1).filter { it.placedVisible() }
        val dial = materialTimeDial
        for (n in merged) {
            val c = n.config
            val role = c.getOrNull(SemanticsProperties.Role)
            val click = c.getOrNull(SemanticsActions.OnClick) != null
            val name = nameOf(n)
            if (click && spoken(n).isEmpty()) report("unlabeled-action", "action ${n.boundsInRoot.dpString()} has no label")
            if (click && role == null && c.getOrNull(SemanticsActions.SetText) == null && name !in materialSheetChrome && !isTimeDialNumber(n, dial)) {
                val kind = if (c.getOrNull(SemanticsProperties.StateDescription) in setOf(str(R.string.ui_cd_expanded), str(R.string.ui_cd_collapsed))) " (expandable row)" else ""
                report("action-without-role", "\"$name\"$kind is clickable with no role (not announced as a button/tab/switch)")
            }
            when (role) {
                Role.Switch, Role.Checkbox -> if (c.getOrNull(SemanticsProperties.ToggleableState) == null) {
                    report("toggle-without-state", "$role \"$name\" exposes no checked state")
                }
                Role.RadioButton -> if (c.getOrNull(SemanticsProperties.Selected) == null && c.getOrNull(SemanticsProperties.ToggleableState) == null) {
                    report("toggle-without-state", "radio \"$name\" exposes no selected state")
                }
                Role.Tab -> if (c.getOrNull(SemanticsProperties.Selected) == null) report("toggle-without-state", "tab \"$name\" exposes no selected state")
                Role.Image -> if (cds(n).isEmpty()) report("image-without-description", "image ${n.boundsInRoot.dpString()} has no description")
                else -> {}
            }
            if (c.getOrNull(SemanticsProperties.ToggleableState) != null && role == null) {
                report("state-without-role", "\"$name\" has a checked state but no role (switch/checkbox)")
            }
            // A focusable node (merging, or a leaf that speaks) announcing a phrase twice.
            if (c.isMergingSemanticsOfDescendants || n.children.isEmpty()) {
                val words = spoken(n).map { it.trim().lowercase() }.filter { it.length > 1 }
                val dupes = words.groupingBy { it }.eachCount().filterValues { it > 1 }.keys
                if (dupes.isNotEmpty()) report("duplicate-announcement", "\"$name\" announces ${dupes.joinToString { "\"$it\"" }} twice")
            }
            auditCustomControl(n)
        }
        // Headings: titles and section overlines outside an action. A title-size
        // figure (tabular digits: the trim readout, a record value) is a value, and the
        // brand wordmark in the top bar is the app's name on every tab, not a title.
        val unmerged = flatten(w.unmerged).drop(1).filter { it.placedVisible() }
        for (n in unmerged) {
            val layout = layoutOf(n) ?: continue
            if (layout.layoutInput.text.text == BRAND || layout.layoutInput.style.fontFeatureSettings?.contains("tnum") == true) continue
            val style = textStyleOf(layout)
            val kind = when {
                style.first.isSp && abs(style.first.value - 20f) < 0.1f && style.second >= FontWeight.Bold && !style.third -> "title"
                style.first.isSp && abs(style.first.value - 13f) < 0.1f && style.second >= FontWeight.Bold && style.third -> "section overline"
                else -> null
            } ?: continue
            val chain = generateSequence(n) { it.parent }.toList()
            if (chain.any { it.config.getOrNull(SemanticsActions.OnClick) != null || it.config.getOrNull(SemanticsActions.SetText) != null }) continue
            if (chain.none { it.config.contains(SemanticsProperties.Heading) }) {
                report("missing-heading", "$kind \"${short(texts(n).joinToString(" "))}\" is not a heading")
            }
        }
    }

    /** fontSize, weight, letter-spaced of a laid-out text. */
    private fun textStyleOf(layout: TextLayoutResult): Triple<TextUnit, FontWeight, Boolean> {
        val s = layout.layoutInput.style
        val spaced = s.letterSpacing.isSp && s.letterSpacing.value >= 1f
        return Triple(s.fontSize, s.fontWeight ?: FontWeight.Normal, spaced)
    }

    private fun layoutOf(n: SemanticsNode): TextLayoutResult? {
        val get = n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action ?: return null
        val out = ArrayList<TextLayoutResult>()
        return if (runCatching { get(out) }.getOrDefault(false)) out.firstOrNull() else null
    }

    private fun auditCustomControl(n: SemanticsNode) {
        val c = n.config
        val tag = c.getOrNull(SemanticsProperties.TestTag)
        val name = nameOf(n)
        // A rating track (RatingScale: run effort, pain, check-in scales): one adjustable node.
        if (c.getOrNull(SemanticsProperties.ProgressBarRangeInfo) != null && c.isMergingSemanticsOfDescendants) {
            if (c.getOrNull(SemanticsActions.SetProgress) == null) report("custom-control", "adjustable \"$name\" has no set-progress action")
        }
        // A pick-one group (scale rows, effort cells, chips): every option says whether it is picked.
        if (c.contains(SemanticsProperties.SelectableGroup)) {
            flatten(n).drop(1).filter { it.placedVisible() && it.config.getOrNull(SemanticsActions.OnClick) != null }.forEach { o ->
                if (o.config.getOrNull(SemanticsProperties.Selected) == null && o.config.getOrNull(SemanticsProperties.ToggleableState) == null) {
                    report("toggle-without-state", "option \"${nameOf(o)}\" of a pick-one group exposes no selected state")
                }
            }
        }
        // Keypad keys: each a labelled button.
        if (c.getOrNull(SemanticsActions.OnClick) != null && generateSequence(n) { it.parent }.any { it.config.getOrNull(SemanticsProperties.TestTag) == "numeric-keypad" }) {
            if (c.getOrNull(SemanticsProperties.Role) != Role.Button) report("custom-control", "keypad key \"$name\" is not a button")
        }
        // Anything with a range must say its value.
        c.getOrNull(SemanticsProperties.ProgressBarRangeInfo)?.let {
            if (c.getOrNull(SemanticsProperties.StateDescription).isNullOrBlank() && c.getOrNull(SemanticsActions.SetProgress) != null) {
                report("custom-control", "adjustable \"$name\" has no spoken value")
            }
        }
    }

    /** Traversal: never upward within a column, start-to-end within a row. */
    private fun auditTraversal(w: Win) {
        val byId = flatten(w.merged).associateBy { it.id }
        val seq = w.order.mapNotNull { byId[it] }.filter { n ->
            n.placedVisible() && (n.config.isMergingSemanticsOfDescendants || spoken(n).isNotEmpty() || n.config.getOrNull(SemanticsActions.OnClick) != null) &&
                generateSequence(n.parent) { it.parent }.none { it.config.isMergingSemanticsOfDescendants }
        }
        val rtl = config.rtl
        val slack = 4f * rule.density.density
        // Floating layers ride over the list and are read after it (Scaffold order):
        // the Log button and a snackbar (its host: a live region with a dismiss
        // action). No place in a linear order is "visual" for a layer with list rows
        // both above and below it; a snackbar announces itself as a live region.
        fun floating(n: SemanticsNode) = generateSequence(n) { it.parent }.any {
            it.config.getOrNull(SemanticsProperties.TestTag) == "log-fab" ||
                (it.config.getOrNull(SemanticsActions.Dismiss) != null && it.config.contains(SemanticsProperties.LiveRegion))
        }
        val dial = materialTimeDial
        for ((a, b) in seq.zipWithNext()) {
            if (floating(a) || floating(b)) continue
            // The Material clock face is read clockwise around the dial, not by rows.
            if (isTimeDialNumber(a, dial) && isTimeDialNumber(b, dial)) continue
            val ra = a.boundsInRoot
            val rb = b.boundsInRoot
            val hOverlap = min(ra.right, rb.right) - max(ra.left, rb.left)
            val vOverlap = min(ra.bottom, rb.bottom) - max(ra.top, rb.top)
            if (rb.bottom <= ra.top - slack && hOverlap > slack) {
                report("traversal-order", "\"${nameOf(b)}\" ${rb.dpString()} is read after \"${nameOf(a)}\" ${ra.dpString()} below it")
            } else if (vOverlap > 0.5f * min(ra.height, rb.height) && hOverlap <= slack && rb.top <= ra.top + slack) {
                // Same row, and not a lower line: the later node must sit further toward the end.
                val backwards = if (rtl) rb.left >= ra.right - slack else rb.right <= ra.left + slack
                if (backwards) {
                    report(
                        "traversal-order",
                        "\"${nameOf(b)}\" ${rb.dpString()} is read after \"${nameOf(a)}\" ${ra.dpString()} though it sits before it in the ${if (rtl) "RTL" else "LTR"} row",
                    )
                }
            }
        }
    }

    // ── Contrast ────────────────────────────────────────────────────────────

    /** Rendered contrast of every fully visible text line in the top window (what is
     *  under a sheet's scrim is not readable content). */
    private fun auditContrast(label: String) {
        // Software drawing does not composite a sheet's dialog window under Robolectric
        // (its capture shows the activity), so only a screen with no overlay is sampled.
        if (roots().size != 1 || roots().flatMap { flatten(it) }.any { labelOf(it) in materialSheetChrome }) return
        // A sheet or dialog still opening or closing has its window shown while its
        // Compose root is not (yet, or any more) registered, and its scrim draws over
        // the activity: wait for its animation to settle, else do not sample.
        var waits = 0
        while (shownWindows().size > 1 && waits++ < OVERLAY_SETTLE_STEPS) {
            rule.mainClock.advanceTimeBy(OVERLAY_SETTLE_STEP_MS)
            rule.waitForIdle()
        }
        if (shownWindows().size > 1 || roots().size != 1) return
        val bmp = screenBitmap() ?: return
        // A window that came up while the capture idled is not sampled.
        roots().singleOrNull()?.takeIf { shownWindows().size == 1 }?.let { r ->
            val offset = r.positionOnScreen
            val unmerged = allUnmerged(r)
            for (n in unmerged) {
                if (!n.placedVisible()) continue
                if (generateSequence(n) { it.parent }.any { it.config.contains(SemanticsProperties.Disabled) }) continue
                val layout = layoutOf(n) ?: continue
                val text = layout.layoutInput.text.text
                if (text.isBlank() || text == BRAND) continue
                val clip = n.boundsInRoot.translate(offset)
                val origin = n.positionInRoot + offset
                val style = layout.layoutInput.style
                val sp = if (style.fontSize.isSp) style.fontSize.value else 14f
                val px = sp * config.font
                val bold = (style.fontWeight ?: FontWeight.Normal) >= FontWeight.Bold
                val large = px >= 24f || (bold && px >= 18.66f)
                val need = if (large) 3.0 else 4.5
                for (line in 0 until layout.lineCount) {
                    // The node's whole width at the line's height: a line's own left/right
                    // can sit off the drawn glyphs (aligned or bidi text), the ground is the same.
                    val t = origin.y + layout.getLineTop(line)
                    val b = origin.y + layout.getLineBottom(line)
                    // A line the list's edge cuts (under the app bar or the nav bar) shows
                    // only glyph tips: not readable here, it is sampled on the page that
                    // shows it whole.
                    if (t < clip.top - 1f || b > clip.bottom + 1f) continue
                    val box = Rect(clip.left, max(t, clip.top), clip.right, min(b, clip.bottom))
                    if (box.width < 4f || box.height < 4f) continue
                    val ratio = lineContrast(bmp, box) ?: continue
                    if (ratio.first + 0.05 < need) {
                        val lineText = text.substring(layout.getLineStart(line), layout.getLineEnd(line)).trim()
                        report(
                            "text-contrast",
                            "\"${short(lineText)}\" ${"%.2f".format(ratio.first)}:1 < $need " +
                                "(glyph #${Integer.toHexString(ratio.second).takeLast(6)} on #${Integer.toHexString(ratio.third).takeLast(6)}, ${sp}sp${if (bold) " bold" else ""}) in $label",
                        )
                    }
                }
            }
        }
    }

    /** The shown windows (the activity's, each dialog's and sheet's), in the order
     *  they were added. */
    private fun shownWindows(): List<Pair<View, android.view.WindowManager.LayoutParams?>> {
        val params = windowParams()
        return windowViews().withIndex().filter { (_, v) -> v.isShown && v.width > 0 }.map { (i, v) -> v to params.getOrNull(i) }
    }

    /** Every window drawn in software, in the order it was added (its z-order), onto
     *  one screen-sized bitmap. */
    private fun screenBitmap(): Bitmap? {
        rule.waitForIdle()
        // Window z-order: by type (the activity's base window under its dialogs), then add order.
        val views = shownWindows().withIndex()
            .sortedWith(compareBy({ it.value.second?.type ?: 0 }, { it.index }))
            .map { it.value.first }
        val base = views.firstOrNull() ?: return null
        val dm = base.resources.displayMetrics
        val out = Bitmap.createBitmap(max(dm.widthPixels, base.width), max(dm.heightPixels, base.height), Bitmap.Config.ARGB_8888)
        val canvas = android.graphics.Canvas(out)
        for (v in views) {
            val at = IntArray(2).also { v.getLocationOnScreen(it) }
            canvas.save()
            canvas.translate(at[0].toFloat(), at[1].toFloat())
            v.draw(canvas)
            canvas.restore()
        }
        return out
    }

    /** (contrast, glyph colour, ground colour) of the pixels in [box], or null for a
     *  busy ground (no colour covers 35% of the box: a map, a gradient). */
    private fun lineContrast(bmp: Bitmap, box: Rect): Triple<Double, Int, Int>? {
        val x0 = box.left.toInt().coerceIn(0, bmp.width - 1)
        val x1 = box.right.toInt().coerceIn(x0 + 1, bmp.width)
        val y0 = box.top.toInt().coerceIn(0, bmp.height - 1)
        val y1 = box.bottom.toInt().coerceIn(y0 + 1, bmp.height)
        val counts = HashMap<Int, Int>()
        for (y in y0 until y1) for (x in x0 until x1) {
            val p = bmp.getPixel(x, y) or (0xFF shl 24)
            counts[p] = (counts[p] ?: 0) + 1
        }
        val total = (x1 - x0) * (y1 - y0)
        val (ground, gCount) = counts.maxByOrNull { it.value }?.toPair() ?: return null
        if (gCount < total * 0.35) return null
        val glyph = counts.filter { it.key != ground && it.value >= 3 }.keys.maxByOrNull { contrastRatio(Color(it), Color(ground)) } ?: return null
        return Triple(contrastRatio(Color(glyph), Color(ground)), glyph, ground)
    }

    // ── Traversal map + windows (reflection into the Compose accessibility delegate) ──

    private fun delegateOf(root: ViewRootForTest): Any {
        val view = root.view
        val f = generateSequence<Class<*>>(view.javaClass) { it.superclass }.firstNotNullOf { c ->
            runCatching { c.getDeclaredField("composeAccessibilityDelegate") }.getOrNull()
        }.apply { isAccessible = true }
        return f.get(view)!!
    }

    /** The node ids in the order Compose tells TalkBack to visit them. */
    private fun traversal(root: ViewRootForTest): List<Int> {
        val delegate = delegateOf(root)
        val map = delegate.javaClass.getMethod("getIdToBeforeMap\$ui").invoke(delegate)
        val get = map.javaClass.getMethod("getOrDefault", Int::class.javaPrimitiveType, Int::class.javaPrimitiveType)
        val next = HashMap<Int, Int>()
        val targets = HashSet<Int>()
        for (n in flatten(root.semanticsOwner.unmergedRootSemanticsNode)) {
            val v = get.invoke(map, n.id, Int.MIN_VALUE) as Int
            if (v != Int.MIN_VALUE) {
                next[n.id] = v
                targets += v
            }
        }
        val head = next.keys.firstOrNull { it !in targets } ?: return emptyList()
        val out = ArrayList<Int>()
        var cur: Int? = head
        val seen = HashSet<Int>()
        while (cur != null && seen.add(cur)) {
            out += cur
            cur = next[cur]
        }
        return out
    }

    private fun composeRoots(): List<ViewRootForTest> =
        windowViews().flatMap { w -> w.descendants().filter { it is ViewRootForTest && it.isShown }.map { it as ViewRootForTest }.toList() }

    // ── Verdict ─────────────────────────────────────────────────────────────

    private fun assertNoFindings() {
        if (findings.isNotEmpty()) throw AssertionError("${findings.size} accessibility findings:\n" + findings.values.take(40).joinToString("\n"))
    }

    companion object {
        private val FULL = System.getenv("A11Y_FULL") == "1"
        private const val MAX_PAGES = 12
        private const val MAX_OVERLAY_PAGES = 3
        private const val BRAND = "lollop"

        /** Up to 2 s of virtual time for an opening or closing overlay to settle
         *  before a contrast sample. */
        private const val OVERLAY_SETTLE_STEPS = 20
        private const val OVERLAY_SETTLE_STEP_MS = 100L

        private val TABS = listOf(R.string.main_dest_today, R.string.main_dest_history, R.string.main_dest_profile)

        val CONFIGS = listOf(1f, 2f).flatMap { f -> listOf(false, true).flatMap { d -> listOf(false, true).map { r -> Config(f, d, r) } } }

        /** Taps that leave the app's UI (system pickers, share sheets) or wipe the
         *  state the crawl stands on. */
        private val SKIP_TAPS = listOf(R.string.main_clear_all_data)
        private val OVERLAY_SKIP = listOf(R.string.main_cancel)

        /** Material 3's own sheet scrim and drag handle (library chrome, not app code). */
        private val MATERIAL_SHEET_CHROME = listOf(
            "m3c_bottom_sheet_dismiss_description", "m3c_bottom_sheet_drag_handle_description", "m3c_bottom_sheet_pane_title", "close_sheet",
        )

        /** The numbers on Material 3's time picker clock face (library code). Each is a
         *  clickable, selectable node with no role, set inside `TimePicker`'s private
         *  `ClockText`; a modifier on `TimePicker` cannot add a role to its children, so
         *  this is a platform finding, not one the app can fix. Read around the dial. */
        private val MATERIAL_TIME_DIAL = listOf(
            "m3c_time_picker_hour_suffix", "m3c_time_picker_hour_24h_suffix", "m3c_time_picker_minute_suffix",
        )
    }
}
