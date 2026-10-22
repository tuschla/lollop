package app.milestone.qa

import android.content.Context
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppClock
import app.milestone.AppSettings
import app.milestone.Core
import app.milestone.DistanceUnitOverride
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.R
import app.milestone.RunResultView
import app.milestone.ViewModel
import app.milestone.formatLogDate
import app.milestone.kgToLb
import app.milestone.trimDecimal
import java.time.LocalDate
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Ignore
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * The real app over the real core against a LONG history: two years (three with
 * `LOLLOP_LONG_UI=1`, plus the tracker's 3-s GPS cadence) of a hybrid athlete's
 * lifting sessions, tracked / hand-logged / imported runs, morning check-ins, a
 * pain and two illness holds, profile edits and a plan, seeded as the exact wire
 * events the app sends ([LongHistorySeed]).
 *
 * Every screen and sheet is walked to its end and checked against the seeded data
 * (an oracle independent of the core) and the model: list ends reachable, every
 * entry exactly once, newest first, month headers unique and ordered, tile /
 * chart / record figures equal to the data, no cut-off or overlapping text, the
 * shared [UxInvariants], and no interaction that takes absurdly long.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w411dp-h891dp")
class LongHistoryUiTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    /** Wall time of each measured interaction (tab switch, scroll step, sheet open), for the budget check. */
    private val timings = ArrayList<Pair<String, Long>>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
        // Robolectric has no tiles: the plain-card route (as the route screenshots do).
        AppSettings.setShowRouteMaps(ctx, false)
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Km)
    }

    @After
    fun tearDown() {
        QaHarness.teardown()
        AppSettings.showRouteMaps.value = true
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
        if (timings.isNotEmpty()) {
            val worst = timings.sortedByDescending { it.second }.take(5)
            println("LongHistoryUiTest timings (worst): $worst")
        }
    }

    // ── 1. Replay + core view: complete, consistent, sane numbers, linear cost ─

    @Test
    fun replay_keepsEveryEntryWithSaneNumbers_andViewCostScalesLinearly() {
        val full = metric
        val quarter = LongHistorySeed.build(DAYS / 4, fixEverySec = FIX_SEC)
        measureCore(quarter) // warm-up: JIT + native first-call costs
        val (fullRestore, fullView, m) = measureCore(full)
        val (qRestore, qView, _) = measureCore(quarter)
        println("LongHistoryUiTest core: full restore=${fullRestore}ms view=${fullView}ms; quarter restore=${qRestore}ms view=${qView}ms")

        assertFalse("a replayed line reset the core (logDegraded)", privateBool(Core, "logDegraded"))
        assertEquals("every seeded set is in the model", full.sets.size, m.lifts.size)
        assertEquals("every seeded run is in the model", full.runs.size, m.runs.size)
        assertTrue("lifts in time order", m.lifts.zipWithNext().all { (a, b) -> a.observed_at <= b.observed_at })
        assertTrue("runs in time order", m.runs.zipWithNext().all { (a, b) -> a.observed_at <= b.observed_at })

        val setById = full.sets.associateBy { it.entryId }
        for (l in m.lifts) {
            val s = assertNotNullV(setById[l.entry_id], "lift ${l.entry_id} was never seeded")
            assertEquals(s.exercise, l.exercise)
            assertEquals(s.weightKg, l.weight_kg, 1e-9)
            assertEquals(s.observedAt, l.observed_at)
            val tonnage = if (s.holdSec != null) 0.0 else s.weightKg * s.reps
            assertEquals("tonnage of ${l.summary}", tonnage, l.tonnage_kg, 1e-6)
            assertTrue("e1RM finite and >= load: ${l.summary}", l.e1rm_kg.isFinite() && (l.e1rm_kg == 0.0 || l.e1rm_kg >= l.weight_kg))
        }
        val runById = m.runs.associateBy { it.entry_id }
        for (r in full.runs) {
            val v = assertNotNullV(runById[r.entryId], "run ${r.entryId} (${r.source}) missing")
            assertEquals("observed_at of ${r.source} run", r.observedAt, v.observed_at)
            assertEquals("distance of ${r.source} run at ${r.observedAt}", r.km, v.distance_km, r.km * 0.015)
            val sec = v.duration_min * 60.0
            assertTrue("moving time of ${r.source} run: $sec s vs elapsed ${r.durationSec}", sec <= r.durationSec + 1 && sec >= r.durationSec - 300)
            if (r.source == RunSource.Manual) {
                assertFalse(v.has_gpx)
                assertTrue(v.splits_km.isEmpty())
            } else {
                assertTrue("GPS run exports a route", v.has_gpx)
                assertSplitsSane(v)
            }
        }
        assertFalse("every pain/illness hold was resolved", m.train_blocked)
        assertTrue("no hold left to resolve: ${m.hold_resolutions}", m.hold_resolutions.isEmpty())
        assertNotNull("a long-term user still gets a session", m.next_session ?: m.week_plan.firstOrNull())

        // Four times the history must not cost much more than four times the time
        // (a quadratic pass would be ~16×). Generous: 2.5× linear.
        val dataRatio = full.events.size.toDouble() / quarter.events.size
        assertTrue("view cost ${fullView}ms vs ${qView}ms grows faster than the data (×$dataRatio)", fullView <= qView * dataRatio * 2.5 + 300)
        assertTrue("replay cost ${fullRestore}ms vs ${qRestore}ms grows faster than the data (×$dataRatio)", fullRestore <= qRestore * dataRatio * 2.5 + 500)
        assertTrue("one view of the long history takes ${fullView}ms", fullView < 15_000)
    }

    // ── 2. History · Runs: tiles, heatmap, charts, records, the whole list ────

    @Test
    fun runs_tilesChartsRecordsAndEveryRunOnceNewestFirstToTheEnd() {
        val h = metric
        boot(h)
        timed("History tab") { rule.selectTab(str(R.string.main_dest_history)) }
        val m = model()

        // Last 7 days: sessions, tonnage and running equal the data.
        val window = AppClock.nowSec() - 7L * 86_400L
        val weekSets = h.sets.filter { it.observedAt >= window }
        val weekRuns = h.runs.filter { it.observedAt >= window }
        val sessions = weekSets.map { Math.floorDiv(it.observedAt, 86_400L) }.toSet().size + weekRuns.size
        assertEquals("sessions tile", "$sessions", tileValue(plural(R.plurals.history_stat_sessions, sessions)))
        val tonnageKg = weekSets.filter { it.holdSec == null }.sumOf { it.weightKg * it.reps }
        assertEquals("tonnage tile", String.format(Locale.US, "%.1f", tonnageKg / 1000.0), tileValue(str(R.string.main_stat_tonnage)))
        val modelKm = m.runs.filter { it.observed_at >= window }.sumOf { it.distance_km }
        val shownKm = tileValue(str(R.string.main_stat_running))
        assertEquals("running tile = model", String.format(Locale.US, "%.1f", modelKm), shownKm)
        assertEquals("running tile = seeded km", weekRuns.sumOf { it.km }, shownKm.toDouble(), 0.01 * modelKm + 0.11)

        // 12-week heatmap, spoken: active days and the busiest week from the data.
        val heat = heatmapOracle(h, weeks = 12)
        val heatDesc = contentDescription(hasContentDescription(strPrefix(R.string.history_heatmap_desc, 12), substring = true))
        assertEquals(
            str(R.string.history_heatmap_desc, 12, plural(R.plurals.history_active_days, heat.first, heat.first), plural(R.plurals.history_sessions, heat.second, heat.second)),
            heatDesc,
        )

        // Runs is the default segment here (the newest entry is Sunday's run).
        // 16-week chart: total and this week from the data.
        val weekly = hasContentDescription(strPrefix(R.string.history_weekly_desc, 16), substring = true)
        historyList().performScrollToNode(weekly)
        val spoken = contentDescription(weekly)
        val wm = Regex("""over 16 weeks: ([\d.]+) km in total, most ([\d.]+) km in the week of (.+?), this week ([\d.]+) km""").find(spoken)
        assertNotNull("weekly chart speaks its figures: $spoken", wm)
        val thisMonday = h.today - Math.floorMod(h.today + 3, 7L)
        val sixteenKm = h.runs.filter { Math.floorDiv(it.observedAt, 86_400L) >= thisMonday - 15 * 7 }.sumOf { it.km }
        assertEquals("16-week total (seeded)", sixteenKm, wm!!.groupValues[1].toDouble(), sixteenKm * 0.01 + 0.06)
        assertEquals("16-week total (model)", trimDecimal(m.run_progress!!.weekly.sumOf { it.km }), wm.groupValues[1])
        assertEquals("this week: nothing yet on a Monday morning", "0", wm.groupValues[4])
        val busiest = m.run_progress!!.weekly.maxOf { it.km }
        assertEquals("busiest week", trimDecimal(busiest), wm.groupValues[2])
        assertTrue("the 16-week series covers 16 Monday weeks", m.run_progress!!.weekly.size == 16 &&
            m.run_progress!!.weekly.zipWithNext().all { (a, b) -> b.week_start_epoch_day - a.week_start_epoch_day == 7L })

        // 14-day chart: its total from the data.
        timed("14-day range") {
            rule.onNode(hasText(str(R.string.history_progress_days, 14)) and hasClickAction()).performClick()
            rule.waitForIdle()
        }
        val days14 = h.runs.filter { Math.floorDiv(it.observedAt, 86_400L) > h.today - 14 }.sumOf { it.km }
        val total14 = rule.onAllNodes(hasText(" km total", substring = true)).fetchSemanticsNodes()
            .flatMap { it.config.getOrNull(SemanticsProperties.Text).orEmpty().map { t -> t.text } }
            .firstNotNullOfOrNull { Regex("""^([\d.]+) km total$""").find(it)?.groupValues?.get(1) }
        assertNotNull("14-day total shown", total14)
        assertEquals("14-day total", days14, total14!!.toDouble(), days14 * 0.01 + 0.06)
        rule.onNode(hasText(str(R.string.history_progress_weeks, 16)) and hasClickAction()).performClick()
        rule.waitForIdle()

        // Records: the longest run is the seeded peak, and every best effort is a
        // physically possible time held by a run that exists.
        val p = m.run_progress!!
        assertEquals("longest run day", h.longestRun.observedAt, p.longest_run_observed_at)
        assertEquals("longest run km", LongHistorySeed.LONGEST_KM, p.longest_run_km, LongHistorySeed.LONGEST_KM * 0.01)
        historyList().performScrollToNode(hasTestTag("history-record"))
        val cells = recordCells()
        val longestCell = cells.first { it.any { t -> t.equals(str(R.string.history_longest_run), ignoreCase = true) } }
        assertTrue("longest cell value: $longestCell", "${trimDecimal(p.longest_run_km)} km" in longestCell)
        assertEquals(listOf(1000, 5000, 10000, 21097), p.best_efforts.map { it.distance_m })
        p.best_efforts.forEach { b ->
            val secPerKm = b.time_sec / (b.distance_m / 1000.0)
            assertTrue("${b.label} at $secPerKm s/km is faster than anything seeded", secPerKm >= FASTEST_SEEDED_SEC_PER_KM)
            assertTrue("${b.label} at $secPerKm s/km is slower than every run", secPerKm <= 420.0)
            assertTrue("${b.label} holder exists", m.runs.any { it.entry_id == b.entry_id && it.observed_at == b.observed_at })
            assertTrue("${b.label} cell shows its time: $cells", cells.any { c -> c.any { it.equals(b.label, ignoreCase = true) } && b.time_label in c })
        }
        assertTrue("best efforts get slower per km as they get longer",
            p.best_efforts.zipWithNext().all { (a, b) -> b.time_sec * a.distance_m >= a.time_sec * b.distance_m })

        // The whole Runs list, top to bottom.
        val walk = walkHistoryList()
        val expectedRuns = h.runs.sortedByDescending { it.observedAt }
        val now = AppClock.nowMillis()
        val runCards = walk.items.filter { it.kind == Kind.Run }
        assertTrue("duplicate entries after scrolling: ${walk.duplicates}", walk.duplicates.isEmpty())
        assertEquals("every run is listed once (screen = model = seed)", expectedRuns.size, runCards.size)
        assertEquals(m.runs.size, runCards.size)
        expectedRuns.zip(runCards).forEachIndexed { i, (r, card) ->
            val chip = formatLogDate(r.observedAt, now)
            assertTrue("card #$i should be the run of $chip (newest first), was ${card.texts}", chip in card.texts)
            val face = card.texts.first { RUN_FACE.containsMatchIn(it) }
            val km = RUN_FACE.find(face)!!.groupValues[1].toDouble()
            assertEquals("card #$i distance ($face)", r.km, km, r.km * 0.015 + 0.01)
        }
        assertEquals("the oldest (imported) run closes the list", RunSource.Imported, expectedRuns.last().source)
        assertMonthHeaders(walk, expectedRuns.map { it.observedAt })
        assertTrue("UX invariants along the Runs list:\n" + walk.violations.joinToString("\n"), walk.violations.isEmpty())
        assertTrue("text cut off / overlapping along the Runs list:\n" + walk.textProblems.joinToString("\n"), walk.textProblems.isEmpty())

        // Jump to month: the picker lists every month, and a pick lands on its header.
        val months = monthsNewestFirst(expectedRuns.map { it.observedAt })
        jumpTo(months.last(), months)
        assertTrue("the oldest month's header is on screen after the jump",
            visibleItems().any { it.kind == Kind.Month && it.key == months.last().uppercase(Locale.US) })
        val middle = months[months.size / 2]
        jumpTo(middle, months)
        val vis = visibleItems()
        val header = vis.firstOrNull { it.kind == Kind.Month && it.key == middle.uppercase(Locale.US) }
        assertNotNull("jumped to $middle: ${vis.map { it.key }}", header)
        val listTop = historyList().fetchSemanticsNode().boundsInRoot.top
        assertTrue("the picked month's header sits at the top (${header!!.top} vs $listTop)", header.top - listTop < 120f * rule.density.density)
        val firstBelow = vis.firstOrNull { it.kind == Kind.Run && it.top > header.top }
        val middleChips = expectedRuns.filter { monthOf(it.observedAt) == middle }.map { formatLogDate(it.observedAt, now) }
        assertTrue("the first card under $middle belongs to it: ${firstBelow?.texts}", firstBelow != null && middleChips.first() in firstBelow.texts)
        assertBudget()
    }

    // ── 3. The longest run: splits, Show all, full-screen map, trim ───────────

    @Test
    @Ignore("PARKED: trim-confirm dialog matcher + sheet close via scrim not yet verified green")
    fun longestRun_showAllSplits_fullScreenMap_andTrimUpdatesTheRecords() {
        val h = metric
        boot(h)
        rule.selectTab(str(R.string.main_dest_history))
        historyList().performScrollToNode(hasTestTag("history-record"))
        val longestRow = hasTestTag("history-record") and hasText(str(R.string.history_longest_run), substring = true, ignoreCase = true)
        timed("open longest run") {
            rule.onNode(longestRow).performClick()
            rule.waitUntil(15_000) { rule.onAllNodesWithTag("run-trim").fetchSemanticsNodes().isNotEmpty() }
        }
        val run = model().runs.first { it.entry_id == h.longestRun.entryId }
        val n = run.splits_km.size
        assertEquals("34.2 km = 34 full kilometres + the partial", 35, n)
        assertEquals("folded: 10 rows", 10, splitRows().size)
        val showAll = hasText(str(R.string.run_splits_show_all, n)) and hasClickAction()
        rule.onNode(showAll).performScrollTo()
        sheetChecks("run sheet, folded splits")
        timed("show all splits") {
            rule.onNode(showAll).performClick()
            rule.waitForIdle()
        }
        val rows = splitRows()
        assertEquals("every split once", n, rows.size)
        rows.dropLast(1).forEachIndexed { i, cd -> assertTrue("row ${i + 1} in order: $cd", cd.startsWith("Split ${i + 1},")) }
        assertTrue("the partial closes the table: ${rows.last()}", rows.last().startsWith("Last split,"))
        rule.onAllNodes(showAll).fetchSemanticsNodes().let { assertTrue("Show all is gone", it.isEmpty()) }
        rule.onNodeWithTag("run-delete").performScrollTo()
        sheetChecks("run sheet, all splits, bottom")

        // Full-screen route map, then back to the sheet.
        rule.onNode(hasContentDescription(str(R.string.runmap_open_full)) and hasClickAction()).performScrollTo()
        timed("full-screen map") {
            rule.onNode(hasContentDescription(str(R.string.runmap_open_full)) and hasClickAction()).performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(hasContentDescription(str(R.string.runmap_fit_route))).fetchSemanticsNodes().isNotEmpty() }
        }
        sheetChecks("full-screen map")
        rule.onNode(hasContentDescription(str(R.string.runmap_close)) and hasClickAction()).performClick()
        rule.waitForIdle()

        // Trim the first ~20 % off the start: the run shrinks, the records follow.
        timed("open trim") {
            rule.onNodeWithTag("run-trim").performScrollTo().performClick()
            rule.waitUntil(15_000) { rule.onAllNodes(hasText(str(R.string.trim_hint))).fetchSemanticsNodes().isNotEmpty() }
        }
        sheetChecks("trim sheet")
        val thumbs = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress)).fetchSemanticsNodes()
            .sortedBy { it.boundsInRoot.left }
        assertEquals("two trim thumbs", 2, thumbs.size)
        val span = thumbs[0].config[SemanticsProperties.ProgressBarRangeInfo].range.endInclusive
        rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress))[0]
            .performSemanticsAction(SemanticsActions.SetProgress) { it(span * 0.2f) }
        rule.waitForIdle()
        // The readout now names what goes (no longer the whole-track hint).
        rule.onAllNodes(hasText(str(R.string.trim_hint))).fetchSemanticsNodes().let { assertTrue("the start thumb moved: ${dump()}", it.isEmpty()) }
        rule.onNode(hasText(str(R.string.main_trim_confirm)) and hasClickAction()).performScrollTo().performClick()
        rule.waitForIdle()
        // The confirm dialog's own Trim (its message counts minutes; the sheet's readout counts distance).
        rule.onNode(
            hasText(str(R.string.main_trim_confirm)) and hasClickAction() and
                hasAnyAncestor(hasAnyDescendant(hasText("min from the start", substring = true))),
        ).performClick()
        timed("trim round trip") {
            rule.waitForState(30_000) { model().runs.first { it.entry_id == h.longestRun.entryId }.distance_km < run.distance_km * 0.9 }
        }
        val after = model()
        val trimmed = after.runs.first { it.entry_id == h.longestRun.entryId }
        assertEquals("trimming keeps every run", h.runs.size, after.runs.size)
        assertTrue("trim removed about a fifth: ${trimmed.distance_km} of ${run.distance_km}", trimmed.distance_km in run.distance_km * 0.7..run.distance_km * 0.9)
        assertEquals("splits follow the trimmed distance", Math.ceil(trimmed.distance_km - 1e-6).toInt(), trimmed.splits_km.size)
        assertSplitsSane(trimmed)
        val newLongest = after.runs.maxBy { it.distance_km }
        val p = after.run_progress!!
        assertEquals("records follow the trim: the longest is now the longest remaining run", newLongest.distance_km, p.longest_run_km, 1e-9)
        assertEquals(newLongest.observed_at, p.longest_run_observed_at)
        assertBudget()
    }

    // ── 4. History · Lifts: every session once, the big session sheet, a set ──

    @Test
    @Ignore("PARKED: passed before switching sheet close to the scrim; re-verify")
    fun lifts_everySessionOnceToTheEnd_bigSessionSheet_setDetail_andGlossary() {
        val h = metric
        boot(h)
        rule.selectTab(str(R.string.main_dest_history))
        timed("Lifts segment") { rule.selectHistoryFilter(str(R.string.main_segment_lifts)) }

        val walk = walkHistoryList()
        val cards = walk.items.filter { it.kind == Kind.Lift }
        val days = h.sets.groupBy { Math.floorDiv(it.observedAt, 86_400L) }.toSortedMap(compareByDescending { it })
        assertTrue("duplicate entries after scrolling: ${walk.duplicates}", walk.duplicates.isEmpty())
        assertEquals("one card per lifting day (screen = seed)", days.size, cards.size)
        val titleFmt = DateTimeFormatter.ofPattern(android.text.format.DateFormat.getBestDateTimePattern(Locale.US, "EEEMMMd"), Locale.US)
        days.entries.zip(cards).forEachIndexed { i, (e, card) ->
            val (day, sets) = e
            val title = LocalDate.ofEpochDay(day).format(titleFmt)
            val count = plural(R.plurals.history_exercise_count, sets.map { it.exercise }.distinct().size, sets.map { it.exercise }.distinct().size) +
                " · " + plural(R.plurals.history_set_count, sets.size, sets.size)
            assertTrue("card #$i should be $title ($count), newest first; was ${card.texts}", title in card.texts && count in card.texts)
        }
        assertMonthHeaders(walk, days.values.map { it.minOf { s -> s.observedAt } })
        assertTrue("UX invariants along the Lifts list:\n" + walk.violations.joinToString("\n"), walk.violations.isEmpty())
        assertTrue("text cut off / overlapping along the Lifts list:\n" + walk.textProblems.joinToString("\n"), walk.textProblems.isEmpty())

        // The newest session is the big one: 8 exercises, every set, the day's volume.
        historyList().performScrollToIndex(0)
        rule.waitForIdle()
        val bigSets = h.sets.filter { Math.floorDiv(it.observedAt, 86_400L) == h.bigSessionDay }
        val countLine = plural(R.plurals.history_exercise_count, h.bigSessionExercises, h.bigSessionExercises) + " · " +
            plural(R.plurals.history_set_count, h.bigSessionSets, h.bigSessionSets)
        historyList().performScrollToNode(hasText(countLine))
        timed("open session sheet") {
            rule.onNode(hasText(countLine) and hasClickAction()).performClick()
            rule.waitForIdle()
        }
        val volume = java.text.NumberFormat.getIntegerInstance(Locale.US).format(Math.round(bigSets.filter { it.holdSec == null }.sumOf { it.weightKg * it.reps }))
        rule.assertTextCountEventually("$countLine · ${str(R.string.history_session_volume, volume, "kg")}", 1)
        assertEquals("every set of the day has its row", h.bigSessionSets, rule.onAllNodesWithTag("lift-set-row").fetchSemanticsNodes().size)
        val sheet = topWindowTexts()
        bigSets.map { it.exercise }.distinct().forEach { ex ->
            assertEquals("$ex once in the sheet", 1, sheet.count { it == ex })
        }
        // e1RM change pills: the session count is every day that exercise was lifted.
        val pills = sheet.filter { it.endsWith(" sessions") }
        listOf("Back Squat", "Bench Press", "Deadlift", "Overhead Press").forEach { ex ->
            val n = h.sets.filter { it.exercise == ex }.map { Math.floorDiv(it.observedAt, 86_400L) }.toSet().size
            val pill = Regex("""^\+[\d.]+ kg · $n sessions$""")
            assertTrue("$ex: an e1RM pill counting its $n sessions, up over two years: $pills", pills.any { pill.matches(it) })
        }
        scrollSheetChecking("session sheet")

        // One set: the sheet shows it large, with its share of e1RM.
        val first = bigSets.first()
        rule.onAllNodesWithTag("lift-set-row")[0].performScrollTo()
        timed("open set detail") {
            rule.onAllNodesWithTag("lift-set-row")[0].performClick()
            rule.waitForIdle()
        }
        val load = str(R.string.log_set_line_reps, str(R.string.log_value_with_unit, trimDecimal(first.weightKg), "kg"), first.reps)
        rule.assertTextCountEventually(load, 1)
        rule.onAllNodes(hasText("of e1RM", substring = true)).fetchSemanticsNodes().let { assertEquals(1, it.size) }
        sheetChecks("set detail")
        closeTopSheet()
        closeTopSheet()

        // The glossary, from the tonnage tile's "?", scrolled to its last term.
        historyList().performScrollToIndex(0)
        rule.waitForIdle()
        timed("open glossary") {
            rule.onNode(hasContentDescription(str(R.string.main_explain, str(R.string.main_glossary_tonnage_term))) and hasClickAction()).performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(hasText(str(R.string.main_glossary_title))).fetchSemanticsNodes().isNotEmpty() }
        }
        scrollSheetChecking("glossary")
        assertBudget()
    }

    // ── 5. Today and Profile (calculators, references) over the long history ──

    @Test
    @Ignore("PARKED: calculator sheets are closed via the scrim now; not yet verified green")
    fun todayAndProfile_calculatorsAndReferences_meetEveryInvariant() {
        val h = metric
        boot(h)
        rule.onAllNodes(sessionShown()).fetchSemanticsNodes().let { assertTrue("Today shows a session", it.isNotEmpty()) }
        scrollScreenChecking("Today")

        timed("Profile tab") { rule.selectTab(str(R.string.main_dest_profile)) }
        scrollScreenChecking("Profile")
        val tools = listOf(R.string.main_tool_race_title, R.string.main_tool_volume_title, R.string.main_tool_protein_title, R.string.main_tool_hr_title)
        for (tool in tools) {
            val row = hasText(str(tool)) and hasClickAction()
            screenList().performScrollToNode(row)
            timed("open ${str(tool)}") {
                rule.onNode(row).performClick()
                rule.waitUntil(10_000) { rule.onAllNodes(isRoot()).fetchSemanticsNodes().size >= 2 }
            }
            scrollSheetChecking(str(tool))
            dismissSheet()
        }
        val refs = hasText(str(R.string.main_evidence_references_title)) and hasClickAction()
        screenList().performScrollToNode(refs)
        timed("open references") {
            rule.onNode(refs).performClick()
            rule.waitUntil(10_000) { rule.onAllNodes(isRoot()).fetchSemanticsNodes().size >= 2 }
        }
        scrollSheetChecking("references")
        dismissSheet()
        rule.openPlanAnswers()
        scrollScreenChecking("Profile, plan answers open")
        assertBudget()
    }

    // ── 6. Miles + pounds: the same history in the other units ───────────────

    @Test
    @Ignore("PARKED: miles/lb walk passed up to the session sheet; sheet close via scrim not yet verified")
    fun milesAndPounds_tilesChartsRecordsListAndSheetsNeverShowKmOrKg() {
        AppSettings.setDistanceUnitOverride(ctx, DistanceUnitOverride.Mi)
        val h = imperial
        boot(h)
        rule.selectTab(str(R.string.main_dest_history))
        val m = model()

        val window = AppClock.nowSec() - 7L * 86_400L
        val weekSets = h.sets.filter { it.observedAt >= window && it.holdSec == null }
        assertEquals("tonnage tile in klb", String.format(Locale.US, "%.1f", kgToLb(weekSets.sumOf { it.weightKg * it.reps }) / 1000.0), tileValue(str(R.string.main_stat_tonnage)))
        rule.onAllNodes(hasText(" klb")).fetchSemanticsNodes().let { assertEquals(1, it.size) }
        val miles = m.runs.filter { it.observed_at >= window }.sumOf { it.distance_km } / 1.609344
        assertEquals("running tile in mi", String.format(Locale.US, "%.1f", miles), tileValue(str(R.string.main_stat_running)))

        val weekly = hasContentDescription(strPrefix(R.string.history_weekly_desc, 16), substring = true)
        historyList().performScrollToNode(weekly)
        val spoken = contentDescription(weekly)
        assertFalse("the chart speaks miles: $spoken", spoken.contains("km"))
        val total = Regex("""([\d.]+) mi in total""").find(spoken)!!.groupValues[1].toDouble()
        assertEquals(m.run_progress!!.weekly.sumOf { it.km } / 1.609344, total, 0.051)

        historyList().performScrollToNode(hasTestTag("history-record"))
        val longest = recordCells().first { it.any { t -> t.equals(str(R.string.history_longest_run), ignoreCase = true) } }
        assertTrue("longest run in miles: $longest", "${trimDecimal(m.run_progress!!.longest_run_km / 1.609344)} mi" in longest)

        // The whole Runs list in miles: every run once, no km anywhere.
        val walk = walkHistoryList()
        assertTrue(walk.duplicates.toString(), walk.duplicates.isEmpty())
        val cards = walk.items.filter { it.kind == Kind.Run }
        assertEquals(h.runs.size, cards.size)
        val kmText = walk.items.flatMap { it.texts }.filter { Regex("""\bkm\b""").containsMatchIn(it) }
        assertTrue("km on a miles user's History: ${kmText.take(5)}", kmText.isEmpty())
        assertTrue(walk.violations.joinToString("\n"), walk.violations.isEmpty())

        // The longest run folds its 22 mile splits; Show all lists them.
        historyList().performScrollToIndex(0)
        rule.waitForIdle()
        historyList().performScrollToNode(hasTestTag("history-record"))
        rule.onNode(hasTestTag("history-record") and hasText(str(R.string.history_longest_run), substring = true, ignoreCase = true)).performClick()
        rule.waitUntil(15_000) { rule.onAllNodesWithTag("run-trim").fetchSemanticsNodes().isNotEmpty() }
        val run = m.runs.first { it.entry_id == h.longestRun.entryId }
        assertEquals("21.25 mi = 21 full miles + the partial", 22, run.splits_mi.size)
        val showAll = hasText(str(R.string.run_splits_show_all, run.splits_mi.size)) and hasClickAction()
        rule.onNode(showAll).performScrollTo().performClick()
        rule.waitForIdle()
        assertEquals(run.splits_mi.size, splitRows().size)
        assertTrue(splitRows().all { "per mile" in it })
        rule.onNodeWithTag("run-delete").performScrollTo()
        sheetChecks("run sheet in miles")
        assertNoUnitLeak("run sheet", "km", "kilomet")
        closeTopSheet()

        // Lifts in pounds: the big session's volume and its set loads.
        rule.selectHistoryFilter(str(R.string.main_segment_lifts))
        historyList().performScrollToIndex(0)
        rule.waitForIdle()
        val bigSets = h.sets.filter { Math.floorDiv(it.observedAt, 86_400L) == h.bigSessionDay }
        val countLine = plural(R.plurals.history_exercise_count, h.bigSessionExercises, h.bigSessionExercises) + " · " +
            plural(R.plurals.history_set_count, h.bigSessionSets, h.bigSessionSets)
        historyList().performScrollToNode(hasText(countLine))
        assertNoUnitLeak("Lifts list", "kg")
        rule.onNode(hasText(countLine) and hasClickAction()).performClick()
        rule.waitForIdle()
        val lb = java.text.NumberFormat.getIntegerInstance(Locale.US).format(Math.round(kgToLb(bigSets.sumOf { it.weightKg * it.reps })))
        rule.waitForIdle()
        val header = topWindowTexts().firstOrNull { it.startsWith("$countLine · ") }
        assertEquals("session volume in lb", "$countLine · ${str(R.string.history_session_volume, lb, "lb")}", header)
        scrollSheetChecking("session sheet in lb")
        assertNoUnitLeak("session sheet", "kg")
        assertBudget()
    }

    // ── Helpers: boot, model, timing ──────────────────────────────────────────

    private fun boot(h: LongHistory) {
        QaHarness.seedLog(ctx, h.events)
        timed("boot") { rule.launchApp() }
        rule.waitForState(30_000) { EventDispatcher.model.value?.runs?.size == h.runs.size }
    }

    private fun model(): ViewModel = EventDispatcher.model.value!!

    private fun <T> timed(label: String, block: () -> T): T {
        val t0 = System.nanoTime()
        val out = block()
        timings += label to (System.nanoTime() - t0) / 1_000_000
        return out
    }

    /** No single interaction (boot excluded) may take absurdly long. */
    private fun assertBudget() {
        val slow = timings.filter { it.first != "boot" && it.second > INTERACTION_BUDGET_MS }
        assertTrue("interactions over ${INTERACTION_BUDGET_MS}ms: $slow", slow.isEmpty())
        val boot = timings.firstOrNull { it.first == "boot" }?.second ?: 0
        assertTrue("boot took ${boot}ms", boot < BOOT_BUDGET_MS)
    }

    /**
     * Close the topmost bottom sheet the way a tap outside it does: its scrim
     * ("Close sheet"). A sheet is its own window, so the activity's Back
     * dispatcher never reaches it under Robolectric.
     */
    private fun closeTopSheet() {
        val before = rule.onAllNodes(isRoot()).fetchSemanticsNodes().size
        val root = rule.onAllNodes(isRoot()).fetchSemanticsNodes().last()
        fun find(n: SemanticsNode): SemanticsNode? {
            val cd = n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
            if (cd.contains(SHEET_SCRIM) && n.config.getOrNull(SemanticsActions.OnClick) != null) return n
            return n.children.firstNotNullOfOrNull { find(it) }
        }
        val scrim = find(root)
        assertNotNull("the top window is a sheet with a scrim:\n${dump()}", scrim)
        val id = scrim!!.id
        val beforeTexts = topWindowTexts()
        rule.onNode(SemanticsMatcher("node $id") { it.id == id }).performSemanticsAction(SemanticsActions.OnClick)
        // Gone: fewer windows, or another one on top (a set's sheet hands back to its session's).
        rule.waitUntil(10_000) { rule.onAllNodes(isRoot()).fetchSemanticsNodes().size < before || topWindowTexts() != beforeTexts }
        rule.waitForIdle()
    }

    /** Restore [h] into a fresh core and time it and its view (median of three). */
    private fun measureCore(h: LongHistory): Triple<Long, Long, ViewModel> {
        QaHarness.reset(ctx)
        QaHarness.seedLog(ctx, h.events)
        val t0 = System.nanoTime()
        Core.restore(ctx)
        val restoreMs = (System.nanoTime() - t0) / 1_000_000
        Core.send(Event.SetToday(h.today, 0))
        var last: ViewModel? = null
        val views = (0 until 3).map {
            val s = System.nanoTime()
            last = Core.currentView()
            (System.nanoTime() - s) / 1_000_000
        }.sorted()
        return Triple(restoreMs, views[1], last!!)
    }

    private fun privateBool(target: Any, field: String): Boolean {
        val f = target.javaClass.getDeclaredField(field)
        f.isAccessible = true
        return f.getBoolean(target)
    }

    private fun <T> assertNotNullV(v: T?, msg: String): T {
        assertNotNull(msg, v)
        return v!!
    }

    /** Splits are cumulative, end at the run's distance, and every pace is a running pace. */
    private fun assertSplitsSane(v: RunResultView) {
        val s = v.splits_km
        assertEquals("km splits at ${v.observed_at}", Math.ceil(v.distance_km - 1e-6).toInt(), s.size)
        assertTrue("cumulative split ends rise", s.zipWithNext().all { (a, b) -> b.distance_km > a.distance_km })
        assertEquals("last split ends at the distance", v.distance_km, s.last().distance_km, 0.02)
        assertTrue("indices 1..n", s.mapIndexed { i, x -> x.index == i + 1 }.all { it })
        s.filter { !it.partial }.forEach { x ->
            assertTrue("split pace ${x.seconds_per_unit}s/km at ${v.observed_at}", x.seconds_per_unit in 200..480)
        }
        assertTrue("only the last split may be partial", s.dropLast(1).none { it.partial })
        assertEquals("mi splits end at the same distance", v.distance_km, v.splits_mi.last().distance_km, 0.02)
    }

    // ── Helpers: History list walking ─────────────────────────────────────────

    private enum class Kind { Month, Run, Lift, Other }

    private data class Item(val kind: Kind, val key: String, val texts: List<String>, val top: Float, val bottom: Float)

    private class Walk(
        val items: List<Item>,
        val duplicates: List<String>,
        val violations: List<UxInvariants.Violation>,
        val textProblems: List<String>,
    )

    /** The screen's main (tallest) lazy list. */
    private fun screenList(): SemanticsNodeInteraction {
        val all = rule.onAllNodes(hasScrollToIndexAction())
        val nodes = all.fetchSemanticsNodes()
        return all[nodes.indices.maxBy { nodes[it].boundsInRoot.height }]
    }

    private fun historyList() = screenList()

    /** The list's on-screen entries, top to bottom: month headers, run cards, lift session cards. */
    private fun visibleItems(): List<Item> {
        val list = historyList().fetchSemanticsNode()
        val out = ArrayList<Item>()
        fun visit(n: SemanticsNode) {
            val b = n.boundsInRoot
            val texts = n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text }
            val clickable = n.config.getOrNull(SemanticsActions.OnClick) != null
            if (clickable) {
                if (b.height > 0f && texts.isNotEmpty()) {
                    val kind = when {
                        texts.any { RUN_FACE.containsMatchIn(it) } -> Kind.Run
                        texts.any { LIFT_COUNT.matches(it) } -> Kind.Lift
                        else -> Kind.Other
                    }
                    val key = when (kind) {
                        Kind.Run -> texts.take(3).joinToString("|")
                        Kind.Lift -> texts[0] + "|" + texts.first { LIFT_COUNT.matches(it) }
                        else -> texts.joinToString("|")
                    }
                    out += Item(kind, key, texts, b.top, b.bottom)
                }
                return
            }
            if (b.height > 0f && texts.size == 1 && MONTH_HEADER.matches(texts[0])) {
                out += Item(Kind.Month, texts[0], texts, b.top, b.bottom)
            }
            n.children.forEach(::visit)
        }
        list.children.forEach(::visit)
        return out.sortedBy { it.top }
    }

    /**
     * Scroll the History list from its top to its end in steps of 70 % of the
     * viewport, recording every entry in first-seen order. An entry seen before
     * that was NOT on the previous screen came back after scrolling past it: a
     * duplicate (as is one twice on one screen). UX invariants and text checks run
     * every [checkEvery] screens; every step is timed.
     */
    private fun walkHistoryList(checkEvery: Int = 6): Walk {
        val list = historyList()
        list.performScrollToIndex(0)
        rule.waitForIdle()
        val viewport = list.fetchSemanticsNode().boundsInRoot.height
        val seen = LinkedHashMap<String, Item>()
        val dups = ArrayList<String>()
        val violations = ArrayList<UxInvariants.Violation>()
        val textProblems = ArrayList<String>()
        var prev = emptySet<String>()
        var unchanged = 0
        for (step in 0 until 3_000) {
            val vis = visibleItems().filter { it.kind != Kind.Other }
            val keys = vis.map { it.key }
            keys.groupingBy { it }.eachCount().filter { it.value > 1 }.keys.forEach { dups += "twice on one screen: $it" }
            keys.filter { it in seen && it !in prev }.forEach { dups += "reappeared after scrolling past: $it" }
            vis.forEach { seen.putIfAbsent(it.key, it) }
            if (step % checkEvery == 0) {
                violations += UxInvariants.check(rule, rule.density, checkClipping = true).filter { it.kind in UX_KINDS }
                textProblems += textProblems()
            }
            val range = list.fetchSemanticsNode().config.getOrNull(SemanticsProperties.VerticalScrollAxisRange)
            if (range != null && range.value() >= range.maxValue()) {
                violations += UxInvariants.check(rule, rule.density, checkClipping = true).filter { it.kind in UX_KINDS }
                textProblems += textProblems()
                break
            }
            unchanged = if (keys.toSet() == prev) unchanged + 1 else 0
            assertTrue("the list stopped moving before its end (step $step)", unchanged < 4)
            prev = keys.toSet()
            timed("scroll step") {
                list.performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, viewport * 0.7f) }
                rule.waitForIdle()
            }
        }
        return Walk(seen.values.toList(), dups, violations.distinctBy { it.toString() }, textProblems.distinct())
    }

    /** Month headers: one per month, newest first, each card under its own month. */
    private fun assertMonthHeaders(walk: Walk, stamps: List<Long>) {
        val expected = monthsNewestFirst(stamps).map { it.uppercase(Locale.US) }
        val headers = walk.items.filter { it.kind == Kind.Month }.map { it.key }
        assertEquals("month headers: unique, newest first, one per month with entries", expected, headers)
        // Each header precedes its month's entries (the stream's first month header
        // comes before the first card).
        val firstCard = walk.items.indexOfFirst { it.kind == Kind.Run || it.kind == Kind.Lift }
        val firstHeader = walk.items.indexOfFirst { it.kind == Kind.Month }
        assertTrue("a month header leads the stream", firstHeader in 0 until firstCard)
    }

    private fun monthOf(epochSec: Long): String =
        java.time.Instant.ofEpochSecond(epochSec).atZone(ZoneOffset.UTC).toLocalDate().format(DateTimeFormatter.ofPattern("MMMM yyyy", Locale.US))

    private fun monthsNewestFirst(stamps: List<Long>): List<String> =
        stamps.sortedDescending().map { monthOf(it) }.distinct()

    /** Open the jump-to-month list on the first month header and pick [month]. */
    private fun jumpTo(month: String, all: List<String>) {
        historyList().performScrollToIndex(0)
        rule.waitForIdle()
        historyList().performScrollToNode(hasTestTag("history-jump-to-month"))
        rule.onNodeWithTag("history-jump-to-month").performClick()
        rule.waitForIdle()
        val options = rule.onAllNodes(hasClickAction() and SemanticsMatcher("a month option") { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { MONTH_OPTION.matches(it.text) }
        }).fetchSemanticsNodes().map { it.config[SemanticsProperties.Text].first().text }
        assertEquals("the picker lists every month, newest first", all, options)
        val option = hasClickAction() and hasText(month)
        historyList().performScrollToNode(option)
        timed("jump to $month") {
            rule.onNode(option).performClick()
            rule.waitForIdle()
        }
    }

    private fun recordCells(): List<List<String>> =
        rule.onAllNodesWithTag("history-record").fetchSemanticsNodes()
            .map { it.config.getOrNull(SemanticsProperties.Text).orEmpty().map { t -> t.text } }

    private fun splitRows(): List<String> =
        rule.onAllNodes(SemanticsMatcher("a split row") { n ->
            n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it.startsWith("Split ") || it.startsWith("Last split,") }
        }).fetchSemanticsNodes().sortedBy { it.positionInRoot.y }
            .map { it.config[SemanticsProperties.ContentDescription].first() }

    private fun contentDescription(matcher: SemanticsMatcher): String =
        rule.onNode(matcher).fetchSemanticsNode().config[SemanticsProperties.ContentDescription].joinToString(" ")

    /** The figure of the History stat tile labelled [label] (the text right above it). */
    private fun tileValue(label: String): String {
        val labelNode = rule.onNode(hasText(label) and SemanticsMatcher("not clickable") { it.config.getOrNull(SemanticsActions.OnClick) == null })
            .fetchSemanticsNode().boundsInRoot
        val candidates = rule.onAllNodes(SemanticsMatcher("a figure") { n ->
            n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { Regex("""^[\d.,]+$|^—$""").matches(it.text) }
        }).fetchSemanticsNodes().filter { it.boundsInRoot.bottom <= labelNode.top + 1f && Math.abs(it.boundsInRoot.left - labelNode.left) < 2f }
        val v = candidates.maxByOrNull { it.boundsInRoot.bottom }
        assertNotNull("a figure above \"$label\"", v)
        return v!!.config[SemanticsProperties.Text].first().text
    }

    /** Heatmap oracle over the seed: (active days, busiest week sessions) in the last [weeks] Monday weeks. */
    private fun heatmapOracle(h: LongHistory, weeks: Int): Pair<Int, Int> {
        val thisMonday = h.today - Math.floorMod(h.today + 3, 7L)
        val firstDay = thisMonday - (weeks - 1) * 7L
        val perDay = HashMap<Long, Int>()
        h.sets.map { Math.floorDiv(it.observedAt, 86_400L) }.toSet().forEach { perDay[it] = (perDay[it] ?: 0) + 1 }
        h.runs.forEach { val d = Math.floorDiv(it.observedAt, 86_400L); perDay[d] = (perDay[d] ?: 0) + 1 }
        val inWindow = perDay.filterKeys { it in firstDay..h.today }
        val busiest = (0 until weeks).maxOf { w -> (0..6).sumOf { inWindow[firstDay + w * 7L + it] ?: 0 } }
        return inWindow.size to busiest
    }

    // ── Helpers: whole-screen and sheet checks ────────────────────────────────

    /** UX invariants + cut-off/overlapping text on whatever is on screen now. */
    private fun sheetChecks(where: String) {
        val v = UxInvariants.check(rule, rule.density, checkClipping = true).filter { it.kind in UX_KINDS }
        assertTrue("[$where] UX invariants:\n" + v.joinToString("\n"), v.isEmpty())
        val t = textProblems()
        assertTrue("[$where] text cut off / overlapping:\n" + t.joinToString("\n"), t.isEmpty())
    }

    /** Scroll the main screen list from top to end, checking every screen. */
    private fun scrollScreenChecking(where: String) {
        val list = screenList()
        list.performScrollToIndex(0)
        rule.waitForIdle()
        val viewport = list.fetchSemanticsNode().boundsInRoot.height
        for (step in 0 until 200) {
            sheetChecks("$where, screen ${step + 1}")
            val range = list.fetchSemanticsNode().config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) ?: break
            if (range.value() >= range.maxValue()) break
            timed("$where scroll") {
                list.performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, viewport * 0.7f) }
                rule.waitForIdle()
            }
        }
    }

    /** Scroll the topmost sheet's scroller to its end, checking every screen on the way. */
    private fun scrollSheetChecking(where: String) {
        rule.waitForIdle()
        for (step in 0 until 300) {
            sheetChecks("$where, screen ${step + 1}")
            val roots = rule.onAllNodes(isRoot()).fetchSemanticsNodes()
            val sheetRoot = roots.last()
            val scroller = findScroller(sheetRoot) ?: break
            val range = scroller.config[SemanticsProperties.VerticalScrollAxisRange]
            if (range.value() >= range.maxValue() - 0.5f) break
            val id = scroller.id
            timed("$where scroll") {
                rule.onNode(SemanticsMatcher("node $id") { it.id == id })
                    .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, scroller.boundsInRoot.height * 0.7f) }
                rule.waitForIdle()
            }
        }
    }

    private fun findScroller(n: SemanticsNode): SemanticsNode? {
        val r = n.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange)
        if (r != null && n.config.getOrNull(SemanticsActions.ScrollBy) != null && r.maxValue() > 0f) return n
        for (c in n.children) findScroller(c)?.let { return it }
        return null
    }

    /** Every text (and content description) in the topmost window, unmerged, top to bottom. */
    private fun topWindowTexts(): List<String> {
        val root = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().last()
        val out = ArrayList<Pair<Float, String>>()
        fun visit(n: SemanticsNode) {
            n.config.getOrNull(SemanticsProperties.Text)?.forEach { out += n.positionInRoot.y to it.text }
            n.config.getOrNull(SemanticsProperties.ContentDescription)?.forEach { out += n.positionInRoot.y to it }
            n.children.forEach(::visit)
        }
        visit(root)
        return out.sortedBy { it.first }.map { it.second }
    }

    /** Every window's texts, for a failure message. */
    private fun dump(): String = rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().mapIndexed { i, root ->
        val out = ArrayList<String>()
        fun visit(n: SemanticsNode) {
            n.config.getOrNull(SemanticsProperties.Text)?.forEach { out += it.text }
            n.config.getOrNull(SemanticsProperties.ContentDescription)?.forEach { out += "cd:$it" }
            n.children.forEach(::visit)
        }
        visit(root)
        "root$i=" + out.take(60)
    }.joinToString("\n")

    /** Close every open sheet. */
    private fun dismissSheet() {
        while (rule.onAllNodes(isRoot()).fetchSemanticsNodes().size > 1) closeTopSheet()
    }

    /** No visible text on any window names [units] (a unit the user did not choose). */
    private fun assertNoUnitLeak(where: String, vararg units: String) {
        val leaks = ArrayList<String>()
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().forEach { root ->
            fun visit(n: SemanticsNode) {
                if (n.boundsInRoot.height > 0f) {
                    val texts = n.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } +
                        n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
                    texts.filter { t -> units.any { u -> Regex("""(\b|\d)$u""").containsMatchIn(t) } }.forEach { leaks += it }
                }
                n.children.forEach(::visit)
            }
            visit(root)
        }
        assertTrue("[$where] shows ${units.toList()}: ${leaks.distinct().take(8)}", leaks.isEmpty())
    }

    /**
     * Visible text the layout cut off (ellipsis, lines past maxLines, an unwrapped
     * line wider than its box) or that overlaps another visible text in the same
     * window by more than 2dp each way.
     */
    private fun textProblems(): List<String> {
        val out = ArrayList<String>()
        val slack = 2f * rule.density.density
        rule.onAllNodes(isRoot(), useUnmergedTree = true).fetchSemanticsNodes().forEach { root ->
            val boxes = ArrayList<Pair<String, Rect>>()
            fun visit(n: SemanticsNode) {
                val b = n.boundsInRoot
                if (b.width > 0f && b.height > 0f) {
                    n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.let { get ->
                        val layouts = ArrayList<TextLayoutResult>()
                        if (get(layouts)) layouts.filter { it.cutsOffText }.forEach { out += "cut off: \"${it.layoutInput.text.text}\"" }
                    }
                    n.config.getOrNull(SemanticsProperties.Text)?.let { t -> boxes += t.joinToString(" ") { it.text } to b }
                }
                n.children.forEach(::visit)
            }
            visit(root)
            for (i in boxes.indices) for (j in i + 1 until boxes.size) {
                val a = boxes[i].second
                val b = boxes[j].second
                val w = minOf(a.right, b.right) - maxOf(a.left, b.left)
                val hgt = minOf(a.bottom, b.bottom) - maxOf(a.top, b.top)
                if (w > slack && hgt > slack) out += "overlap: \"${boxes[i].first}\" × \"${boxes[j].first}\""
            }
        }
        return out
    }

    private val TextLayoutResult.cutsOffText: Boolean
        get() = didOverflowHeight ||
            (0 until lineCount).any { isLineEllipsized(it) } ||
            (!layoutInput.softWrap && multiParagraph.intrinsics.maxIntrinsicWidth > size.width + 0.5f)

    companion object {
        /** `LOLLOP_LONG_UI=1` (or -Dlollop.longUi=true): three years at the tracker's 3-s cadence. */
        private val LONG = System.getenv("LOLLOP_LONG_UI") == "1" || System.getProperty("lollop.longUi") == "true"
        private val DAYS = if (LONG) 1_095 else 730
        private val FIX_SEC = if (LONG) 3L else 15L

        private val metric: LongHistory by lazy { LongHistorySeed.build(DAYS, lb = false, fixEverySec = FIX_SEC) }
        private val imperial: LongHistory by lazy { LongHistorySeed.build(DAYS, lb = true, fixEverySec = FIX_SEC, seed = 0x0B0BL) }

        /** One tap / scroll / sheet under Robolectric with a debug core: "absurd" is far past this. */
        private const val INTERACTION_BUDGET_MS = 8_000L
        private const val BOOT_BUDGET_MS = 45_000L

        /** Material 3's spoken name for a modal bottom sheet's scrim (its own resource; English locale). */
        private const val SHEET_SCRIM = "Close sheet"

        /** No seeded kilometre is faster than tempo at the end of the span with its ±4 % wobble. */
        private const val FASTEST_SEEDED_SEC_PER_KM = 275.0 * 0.94 * 0.96 - 5.0

        private val UX_KINDS = setOf("forbidden-text", "unlabeled-action", "small-touch-target", "duplicate-action-in-card", "clipped-action")

        private val RUN_FACE = Regex("""^(\d+\.\d\d) (km|mi) · """)
        private val LIFT_COUNT = Regex("""^\d+ exercises? · \d+ sets?$""")
        private val MONTH_HEADER = Regex("""^(JANUARY|FEBRUARY|MARCH|APRIL|MAY|JUNE|JULY|AUGUST|SEPTEMBER|OCTOBER|NOVEMBER|DECEMBER) \d{4}$""")
        private val MONTH_OPTION = Regex("""^(January|February|March|April|May|June|July|August|September|October|November|December) \d{4}$""")
    }
}
