package app.milestone

import android.content.Context
import android.content.res.Configuration
import android.content.res.Resources
import androidx.activity.ComponentActivity
import androidx.compose.foundation.ScrollState
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import app.milestone.qa.QaHarness
import app.milestone.qa.launchApp
import app.milestone.qa.selectTab
import app.milestone.qa.str
import androidx.test.core.app.ApplicationProvider
import java.time.LocalDate
import java.time.ZoneOffset
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * Numbers across device locales and both unit systems. The app's figures are
 * pinned to ASCII digits and a '.' decimal point ([trimDecimal], [displayFigure],
 * the keypad), so a screen never mixes decimal marks (RunCards.kt [displayFigure],
 * DEVIATIONS #45). These tests drive keypad entry → saved value, the display
 * helpers, export file names, the event wire, GPX/TCX import and case folding
 * under every locale below, and check that resource-formatted counts and
 * locale-formatted totals keep the same convention.
 */
@RunWith(RobolectricTestRunner::class)
class LocaleNumberTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val savedLocale: Locale = Locale.getDefault()

    @After
    fun restoreLocale() = Locale.setDefault(savedLocale)

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    /** A context whose resources format in [locale], as the app's do on a phone set
     *  to it: the configuration locale, with the app's Latin digits ([withLatinDigits]). */
    private fun ctxIn(locale: Locale): Context {
        val cfg = Configuration(ctx.resources.configuration).apply { setLocale(locale) }
        return ctx.createConfigurationContext(cfg).withLatinDigits()
    }

    private fun resIn(locale: Locale): Resources = ctxIn(locale).resources

    /** Runs [block] once per locale, with the JVM default set to it. */
    private fun eachLocale(block: (Locale) -> Unit) = Locales.forEach { l ->
        Locale.setDefault(l)
        block(l)
    }

    /** Every digit in [s] that is not 0–9 (Arabic-Indic, Persian, Devanagari, …). */
    private fun foreignDigits(s: String): String = s.filter { Character.isDigit(it) && it !in '0'..'9' }

    // ── Keypad entry → saved value ────────────────────────────────────────────

    @Test
    fun keypadEntry_savesTheSameValueInEveryLocaleAndUnit() = eachLocale { l ->
        fun type(keys: String): String = keys.fold("") { buf, k -> editNumericBuffer(buf, k, replaceAll = false) }
        assertEquals("$l", "102.5", type("102.5"))
        assertEquals("$l", 102.5, parseKeypadNumber(type("102.5"))!!, 0.0)
        assertEquals("$l: a leading point", "0.5", type(".5"))
        assertEquals("$l: a second point is ignored", "1.25", type("1.2.5"))
        // The buffer never holds a locale mark, so a comma never parses.
        assertNull("$l", parseKeypadNumber("102,5"))
        // A load typed in lb is saved as canonical kg on the wire grid, and shown back as typed.
        val kg = lbToKgWire(parseKeypadNumber(type("225"))!!)
        assertEquals("$l", 102.06, kg, 1e-9)
        assertEquals("$l", "225", weightFigure(kg, WeightUnit.Lb))
        assertEquals("$l", "102.5", trimWeightText("102.50"))
        assertEquals("$l", "102.5", fmtLosslessPrefill(102.5))
        // Distance: a mile buffer round-trips through canonical km.
        val km = displayToKm(parseKeypadNumber(type("3.1"))!!, DistanceUnit.Mi)
        assertEquals("$l", "3.1", distanceBufferText(km, DistanceUnit.Mi))
        assertEquals("$l", "12.5", distanceBufferText(12.5, DistanceUnit.Km))
        assertEquals("$l", "80.5", bodyweightBufferText(80.5, WeightUnit.Kg))
        // Duration digits typed on the keypad.
        val dur = "13000".fold("") { d, k -> editDurationDigits(d, k, replaceAll = false) }
        assertEquals("$l", "1:30:00", fmtDurationDigits(dur))
        assertEquals("$l", Triple(1, 30, 0), hmsDigits(dur))
        assertEquals("$l", "2615", appendRaceDigit("261", '5', fresh = false))
    }

    @Test
    fun keypadEntry_eventWireIsByteIdenticalInEveryLocale() {
        Locale.setDefault(Locale.US)
        val set = Event.LogSet("Back Squat", 102.5, 5, 8.5, 1_790_000_000L, 1_790_000_000_000L)
        val run = Event.LogRun(5.25, 27.5, 0.75, observedAt = 1_790_000_000L, entryId = 1L)
        val expectedSet = set.toJson().toString()
        val expectedRun = run.toJson().toString()
        assertTrue(expectedSet, expectedSet.contains("102.5") && expectedSet.contains("8.5"))
        eachLocale { l ->
            assertEquals("$l", expectedSet, set.toJson().toString())
            assertEquals("$l", expectedRun, run.toJson().toString())
        }
    }

    // ── Displayed figures ─────────────────────────────────────────────────────

    @Test
    fun displayFigures_useAsciiDigitsAndAPoint_inEveryLocaleAndUnit() = eachLocale { l ->
        assertEquals("$l", "102.5", trimDecimal(102.5))
        assertEquals("$l", "1234.5", trimDecimal(1234.45))
        assertEquals("$l", "-2.5", trimDecimal(-2.5))
        assertEquals("$l", "102.5", weightFigure(102.5, WeightUnit.Kg))
        assertEquals("$l", "226", weightFigure(102.5, WeightUnit.Lb))
        assertEquals("$l", "102.5", loadFigure(102.5, WeightUnit.Kg))
        assertEquals("$l", "8.05 km", runDistLabel(8.04672, DistanceUnit.Km))
        assertEquals("$l", "5.00 mi", runDistLabel(8.04672, DistanceUnit.Mi))
        assertEquals("$l", "0.50", displayFigure(0.5))
        assertEquals("$l", "5:30 /km", runPaceLabel(10.0, 55.0, DistanceUnit.Km))
        assertEquals("$l", "8:51 /mi", runPaceLabel(10.0, 55.0, DistanceUnit.Mi))
        assertEquals("$l", "6:00", formatPaceMinutes(5.999))
        assertEquals("$l", "45:30", fmtDurationDigits("4530"))
        // Dates are the device's words and order, never its digits.
        val day = LocalDate.of(2026, 3, 4).toEpochDay()
        listOf(planDateLabel(day), epochDayLabel(day, l), epochDayShortLabel(day, day, l), importDateLabel(1_772_600_000L, l, ZoneOffset.UTC))
            .forEach { assertEquals("$l: $it", "", foreignDigits(it)) }
    }

    // ── Exports ───────────────────────────────────────────────────────────────

    @Test
    fun exportFileNames_areLocaleIndependent() = eachLocale { l ->
        assertEquals("$l", "run-2026-03-04-0506.gpx", gpxFileName(1_772_600_760L, ZoneOffset.UTC))
        val data = dataExportFilename()
        assertTrue("$l: $data", Regex("""lollop-\d{4}-\d{2}-\d{2}\.ndjson""").matches(data) && foreignDigits(data).isEmpty())
    }

    // ── Import ────────────────────────────────────────────────────────────────

    private val gpx = """<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="t" xmlns="http://www.topografix.com/GPX/1/1" xmlns:gpxtpx="http://www.garmin.com/xmlschemas/TrackPointExtension/v1">
<trk><type>RUNNING</type><trkseg>
<trkpt lat="52.5200" lon="13.4050"><ele>34.5</ele><time>2026-03-04T05:06:00Z</time><hdop>1.5</hdop><extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>142.0</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions></trkpt>
<trkpt lat="52.5209" lon="13.4061"><ele>35.0</ele><time>2026-03-04T05:06:30+00:00</time><extensions><gpxtpx:TrackPointExtension><gpxtpx:hr>145</gpxtpx:hr></gpxtpx:TrackPointExtension></extensions></trkpt>
<trkpt lat="52.5218" lon="1.34072E1"><time>2026-03-04T07:07:00+0200</time></trkpt>
</trkseg></trk></gpx>"""

    private val tcx = """<?xml version="1.0" encoding="UTF-8"?>
<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2"><Activities><Activity Sport="Running"><Lap><Track>
<Trackpoint><Time>2026-03-04T05:06:00.000Z</Time><Position><LatitudeDegrees>52.52</LatitudeDegrees><LongitudeDegrees>13.405</LongitudeDegrees></Position><HeartRateBpm><Value>142</Value></HeartRateBpm></Trackpoint>
<Trackpoint><Time>2026-03-04T05:06:30.000Z</Time><Position><LatitudeDegrees>52.5209</LatitudeDegrees><LongitudeDegrees>13.4061</LongitudeDegrees></Position><HeartRateBpm><Value>145</Value></HeartRateBpm></Trackpoint>
</Track></Lap></Activity></Activities></TrainingCenterDatabase>"""

    @Test
    fun gpxAndTcxImport_readTheSameNumbersInEveryLocale() {
        Locale.setDefault(Locale.US)
        val gpxRuns = parseGpxRuns(gpx)
        val tcxRuns = parseTcxActivities(tcx)
        val fixes = gpxRuns.single().segments.single()
        assertEquals(listOf(52.52, 52.5209, 52.5218), fixes.map { it.lat })
        assertEquals(listOf(13.405, 13.4061, 13.4072), fixes.map { it.lon })
        assertEquals(listOf(1_772_600_760L, 1_772_600_790L, 1_772_600_820L), fixes.map { it.timeSec })
        assertEquals(listOf(142, 145, null), fixes.map { it.hrBpm })
        assertEquals(listOf(52.52, 52.5209), tcxRuns.single().segments.single().map { it.lat })
        assertEquals("1.5 hdop is metres, not a parse miss", true, fixes[0].accuracyM != null)
        eachLocale { l ->
            assertEquals("$l", gpxRuns, parseGpxRuns(gpx))
            assertEquals("$l", tcxRuns, parseTcxActivities(tcx))
            assertNull("$l: RUNNING is a run", nonRunSport("RUNNING"))
            assertEquals("$l", "cycling", nonRunSport("CYCLING"))
            assertEquals("$l", fixes.map { it.accuracyM }, parseGpxRuns(gpx).single().segments.single().map { it.accuracyM })
        }
    }

    // ── Case folding (tr-TR dotless i) ───────────────────────────────────────

    @Test
    fun caseFolding_ignoresTheTurkishDeviceLocale() {
        Locale.setDefault(Locale.forLanguageTag("tr-TR"))
        assertEquals("title", nameKey(" TITLE "))
        assertTrue(sameExercise("INCLINE BENCH PRESS", "Incline bench press"))
        assertTrue(isFloorLift("DEADLIFT"))
        assertEquals(listOf("Incline bench press"), quickPickCandidates("", listOf("Incline bench press"), listOf("INCLINE BENCH PRESS"), emptyList()))
        assertEquals("Incline bench press", resolveExerciseName("incline bench press", listOf("Incline bench press")))
        assertEquals("Incline press", exerciseDisplayName("incline press"))
        assertEquals("cycling", nonRunSport("BIKING"))
        assertEquals(DistanceUnit.Km, localeDistanceUnit(Locale.forLanguageTag("tr-TR")))
        assertEquals(WeightUnit.Kg, localeWeightUnit(Locale.forLanguageTag("tr-TR")))
    }

    /** A Turkish keyboard writes the plan's "Incline …" as "İncline …" (the search
     *  field auto-capitalises, SetEditor.kt `KeyboardCapitalization.Sentences`, and
     *  Turkish upper-cases i to İ) or, from the key where other layouts put i, as
     *  "Inclıne …". The quick-pick treats each as the plan item, and so does
     *  [nameKey] (both fold İ and ı to i), so the set is saved under the plan
     *  item's name and attributed to it. */
    @Test
    fun turkishKeyboardSpellings_resolveToTheKnownExercise() {
        val known = listOf("Incline bench press")
        val item = PrescriptionView(summary = "Incline bench press 3×8", exercise = "Incline bench press", sets = 3, reps_low = 8, reps_high = 8)
        for (typed in listOf("İncline bench press", "İNCLİNE BENCH PRESS", "Inclıne bench press", "ıncline bench press")) {
            assertTrue("quick-pick treats $typed as the known name", known.any { it.equals(typed, ignoreCase = true) })
            assertEquals(typed, "Incline bench press", resolveExerciseName(typed, known))
            assertNull(typed, newExerciseToAdd(typed, known))
            assertEquals(typed, item, planItemFor(listOf(item), typed))
        }
    }

    // ── Digits in resource-formatted text ─────────────────────────────────────

    private val squatSets = listOf(
        LiftResultView(exercise = "Back Squat", weight_kg = 102.5, reps = 5, observed_at = 1_790_000_000L),
        LiftResultView(exercise = "Back Squat", weight_kg = 102.5, reps = 5, observed_at = 1_790_000_180L),
        LiftResultView(exercise = "Back Squat", weight_kg = 102.5, reps = 5, observed_at = 1_790_000_360L),
    )

    @Test
    fun setSummaries_readTheSameInLatinDigitLocales() = Locales.filter { it.hasLatinDigits() }.forEach { l ->
        Locale.setDefault(l)
        val text = exerciseSetsSummary(resIn(l), squatSets, WeightUnit.Kg)
        assertEquals("$l", "3 × 5 @ 102.5 kg", text)
    }

    /** "3 × 5 @ 102.5 kg" on a phone set to Arabic (Egypt), Persian or Hindi with
     *  Devanagari digits: the counts go through a `%d` string resource, formatted in
     *  the app's locale with Latin digits ([withLatinDigits]), like the load
     *  ([loadFigure] → [trimDecimal], `Locale.US`). */
    @Test
    fun setSummaries_keepOneDigitScript_inEveryLocale() {
        val bad = Locales.mapNotNull { l ->
            Locale.setDefault(l)
            val text = exerciseSetsSummary(resIn(l), squatSets, WeightUnit.Kg)
            "$l → $text".takeIf { foreignDigits(text).isNotEmpty() }
        }
        assertEquals("set summaries mixing digit scripts", emptyList<String>(), bad)
    }

    /** The coach summary shared as text: its counts ("1 lifting session", "85
     *  active minutes", "effort 6/10") use the same digits as its distances,
     *  loads, paces and dates. */
    @Test
    fun coachSummary_keepsOneDigitScript_inEveryLocale() {
        val today = LocalDate.of(2026, 9, 23).toEpochDay()
        val noon = today * 86_400L + 43_200L
        val model = ViewModel(
            lifts = squatSets.map { it.copy(observed_at = noon) },
            runs = listOf(RunResultView(distance_km = 8.04672, duration_min = 40.0, observed_at = noon, session_rpe = 6)),
            activities = listOf(ActivityResultView(entry_id = 1, observed_at = noon, kind = ActivityKind.Sport, minutes = 45.0, vigorous = true)),
        )
        val bad = Locales.mapNotNull { l ->
            Locale.setDefault(l)
            val text = buildCoachSummary(resIn(l), model, today, ZoneOffset.UTC, WeightUnit.Kg, DistanceUnit.Km, weeks = 1)
            "$l → ${text.lines().filter { foreignDigits(it).isNotEmpty() }}".takeIf { foreignDigits(text).isNotEmpty() }
        }
        assertEquals("coach summary lines with non-ASCII digits", emptyList<String>(), bad)
    }

    /** The session reminder time goes through `java.text`
     *  ([android.text.format.DateFormat.getTimeFormat]) in the context's locale,
     *  which has Latin digits like every other clock time in the app
     *  ([localizedDayFormatter], `java.time`). */
    @Test
    fun reminderTime_usesTheAppsDigits_inEveryLocale() {
        val bad = Locales.mapNotNull { l ->
            Locale.setDefault(l)
            val text = reminderTimeText(ctxIn(l), 7 * 60 + 30)
            "$l → $text".takeIf { foreignDigits(text).isNotEmpty() }
        }
        assertEquals("reminder times with non-ASCII digits", emptyList<String>(), bad)
    }

    // ── Session volume: one decimal mark per sheet ───────────────────────────

    private fun showSessionSheet() {
        val day = LocalDate.of(2026, 9, 21)
        val at = day.atTime(12, 0).toEpochSecond(ZoneOffset.UTC)
        // 4 × 5 × 234.5 kg = 4690 kg lifted; each set row reads "234.5".
        val sets = (0 until 4).map { i ->
            LiftResultView(exercise = "Back Squat", weight_kg = 234.5, reps = 5, observed_at = at + i * 180L, entry_id = i + 1L, tonnage_kg = 1172.5)
        }
        val session = LiftSession(day.toEpochDay(), sets)
        rule.setContent {
            MilestoneTheme {
                LiftSessionSheet(
                    session = session,
                    model = ViewModel(lifts = sets),
                    e1rmSeries = emptyMap(),
                    weightUnit = WeightUnit.Kg,
                    nowMillis = (at + 3_600L) * 1000L,
                    scrollState = ScrollState(0),
                    onOpenSet = {},
                    onDismiss = {},
                    onEvent = {},
                )
            }
        }
        rule.waitForIdle()
    }

    /** Every text on screen, for assertions and failure messages. */
    private fun shownTexts(): List<String> =
        rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Text), useUnmergedTree = true).fetchSemanticsNodes()
            .flatMap { it.config[SemanticsProperties.Text] }.map { it.text }

    private fun volumeShown(figure: String): Boolean {
        val volume = ctx.getString(R.string.history_session_volume, figure, "kg")
        return shownTexts().any { volume in it }
    }

    @Test
    @Config(qualifiers = "en-rUS")
    fun sessionVolume_groupsThousandsWithAComma_inEnglish() {
        Locale.setDefault(Locale.US)
        showSessionSheet()
        assertTrue("4,690 kg lifted: ${shownTexts()}", volumeShown("4,690"))
    }

    /** On a German, Turkish or Brazilian phone the session sheet's volume groups
     *  thousands with ',' ("4,690 kg lifted") like the app's other figures, right
     *  above set rows of "234.5 kg": '.' is only ever a decimal point. */
    @Test
    @Config(qualifiers = "de-rDE")
    fun sessionVolume_neverUsesThePointAsAThousandsMark_inGerman() {
        Locale.setDefault(Locale.GERMANY)
        showSessionSheet()
        assertTrue("the set rows use '.' as the decimal point: ${shownTexts()}", shownTexts().any { "234.5" in it })
        assertTrue("4,690 kg lifted (not 4.690): ${shownTexts()}", volumeShown("4,690"))
    }

    private companion object {
        /** The locale sweep: Latin, comma-decimal, space-grouped, Arabic-Indic,
         *  Persian, Devanagari (the Android 14 numbering-system preference), CJK and
         *  Turkish dotless-i locales. */
        val Locales: List<Locale> = listOf(
            "en-US", "de-DE", "fr-FR", "fr-CH", "ar-EG", "fa-IR", "hi-IN", "hi-IN-u-nu-deva",
            "ja-JP", "tr-TR", "ru-RU", "pt-BR", "nb-NO",
        ).map(Locale::forLanguageTag)

        fun Locale.hasLatinDigits(): Boolean =
            java.text.DecimalFormatSymbols.getInstance(this).zeroDigit == '0'
    }
}

/**
 * The real app over the real core on phones set to other locales: Today, History
 * and Profile with a seeded lift + run history, every visible text collected.
 * Comma-decimal locales never get a decimal comma; the default sweep is de-DE,
 * tr-TR and fr-CH, `LOCALE_SWEEP_ALL=1` adds fr-FR, ru-RU, pt-BR, nb-NO and ja-JP.
 */
@RunWith(RobolectricTestRunner::class)
class LocaleNumberAppTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val savedLocale: Locale = Locale.getDefault()

    @After
    fun tearDown() {
        QaHarness.teardown()
        Locale.setDefault(savedLocale)
    }

    private fun history(): List<Event> {
        val events = mutableListOf<Event>(QaHarness.hybridTenKProfile())
        for (d in 1..3) {
            val at = QaHarness.daysAgoAtMidday(d * 2)
            events += (0 until 3).map { i -> Event.LogSet("Back Squat", 102.5, 5, 8.5, at + i * 180L, (at + i * 180L) * 1000) }
        }
        events += QaHarness.gpsTenK(QaHarness.daysAgoAtMidday(1), sessionRpe = 6)
        val runAt = QaHarness.daysAgoAtMidday(3)
        events += Event.LogRun(5.25, 28.75, 0.0, observedAt = runAt, entryId = runAt * 1000)
        return events
    }

    /** Every text shown on Today, History and Profile under [qualifiers], on a
     *  3000dp-tall window so the lazy lists compose the seeded history. */
    private fun appTexts(qualifiers: String, locale: Locale): List<String> {
        QaHarness.teardown()
        RuntimeEnvironment.setQualifiers("$qualifiers-w411dp-h3000dp")
        Locale.setDefault(locale)
        QaHarness.reset(ctx)
        QaHarness.seedLog(ctx, history())
        rule.launchApp()
        fun texts() = rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsProperties.Text), useUnmergedTree = true)
            .fetchSemanticsNodes().flatMap { it.config[SemanticsProperties.Text] }.map { it.text }
        val out = texts().toMutableList()
        for (tab in listOf(R.string.main_dest_history, R.string.main_dest_profile)) {
            rule.selectTab(str(tab))
            out += texts()
        }
        return out
    }

    /** "1,5" / "12,25": a comma followed by one or two digits, never a thousands group. */
    private val decimalComma = Regex("""(?<![\d,])\d+,\d{1,2}(?![\d])""")

    /** A figure with a decimal point ("4.6", "10.1 km"). */
    private val decimalPoint = Regex("""\d\.\d""")

    @Test
    fun commaDecimalLocales_showEveryFigureWithAPoint() {
        val sweep = buildList {
            add("de-rDE" to Locale.GERMANY)
            add("tr-rTR" to Locale.forLanguageTag("tr-TR"))
            add("fr-rCH" to Locale.forLanguageTag("fr-CH"))
            if (System.getenv("LOCALE_SWEEP_ALL") == "1") {
                listOf("fr-FR", "ru-RU", "pt-BR", "nb-NO", "ja-JP").forEach { add(it.replace("-", "-r") to Locale.forLanguageTag(it)) }
            }
        }
        val english = appTexts("en-rUS", Locale.US)
        assertTrue("the seeded history shows decimal figures: $english", english.count { decimalPoint.containsMatchIn(it) } >= 3)
        for ((q, l) in sweep) {
            val texts = appTexts(q, l)
            assertTrue("$l: decimal figures are on screen: $texts", texts.count { decimalPoint.containsMatchIn(it) } >= 3)
            val commas = texts.filter { decimalComma.containsMatchIn(it) }
            assertEquals("$l: texts with a decimal comma", emptyList<String>(), commas)
            val foreign = texts.filter { t -> t.any { Character.isDigit(it) && it !in '0'..'9' } }
            assertEquals("$l: texts with non-ASCII digits", emptyList<String>(), foreign)
        }
    }

    /** Arabic (Egypt) and Persian: counts and plural strings use the same ASCII
     *  digits as the app's figures. */
    @Test
    fun arabicIndicLocales_keepOneDigitScriptOnEveryScreen() {
        for ((q, l) in listOf("ar-rEG" to Locale.forLanguageTag("ar-EG"), "fa-rIR" to Locale.forLanguageTag("fa-IR"))) {
            val texts = appTexts(q, l)
            assertTrue("$l: decimal figures are on screen: $texts", texts.count { decimalPoint.containsMatchIn(it) } >= 3)
            val foreign = texts.filter { t -> t.any { Character.isDigit(it) && it !in '0'..'9' } }
            assertEquals("$l: texts with non-ASCII digits", emptyList<String>(), foreign)
        }
    }
}
