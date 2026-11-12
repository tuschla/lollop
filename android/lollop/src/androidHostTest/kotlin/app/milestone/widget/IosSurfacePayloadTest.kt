package app.milestone.widget

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.compose.ui.graphics.Color
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.DarkStatusColors
import app.milestone.JvmDefaultsRule
import app.milestone.LIVE_CHIP_MAX_CHARS
import app.milestone.R
import app.milestone.RestNextSet
import app.milestone.RestRun
import app.milestone.RunSurfacePhase
import app.milestone.RunSurfaceState
import app.milestone.StatusColors
import app.milestone.planDateLabel
import java.util.Locale
import kotlin.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * What the app hands the iOS widget extension and Live Activities
 * (widget/IosSurfacePayload.kt): the common widget and tile faces resolved down to
 * what the extension draws (line cuts, glyphs, colour roles, icons, the day it
 * holds), and the run and rest activity text, in the app's real resources.
 */
@RunWith(RobolectricTestRunner::class)
class IosSurfacePayloadTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val res get() = ctx.resources
    private val day = WidgetFixtures.DAY

    @get:Rule
    val jvmDefaults = JvmDefaultsRule()

    @Before
    fun englishUs() {
        Locale.setDefault(Locale.US)
    }

    private fun state(name: String) = WidgetFixtures.states(res).getValue(name)

    private fun payload(name: String, theme: AppTheme = AppTheme.Default, darkMode: DarkMode = DarkMode.System): IosWidgetPayload {
        val s = state(name)
        return iosWidgetPayload(s.snapshot, s.recording, theme, darkMode, res)
    }

    @Test
    fun theWidgetPayloadCarriesTheCommonFaces() {
        val p = payload("lift")
        // The fixture's lift day: four open lifts, Back Squat first, 1 of 4 sessions done this week.
        assertEquals("Heavy day", p.face.title)
        assertEquals(res.getString(R.string.widget_overline_day, res.getString(R.string.widget_overline_today), planDateLabel(day)), p.face.overline)
        assertEquals(planDateLabel(day), p.face.date)
        assertEquals("4 exercises · Back Squat first", p.face.status)
        assertEquals("Back Squat", p.face.brief)
        assertEquals(WidgetInk.Body, p.face.titleInk)
        assertEquals(listOf("log_set", "log"), p.face.actions.map { it.dest })
        assertEquals(listOf(SurfaceIcon.LogSet, SurfaceIcon.Log), p.face.actions.map { it.icon })
        val week = checkNotNull(p.face.week)
        assertEquals(res.getQuantityString(R.plurals.main_week_done_count, 4, 1, 4), week.text)
        assertEquals(1 to 4, week.done to week.planned)
        assertEquals(7, week.days.size)
        assertEquals(listOf("\u2713" to WidgetInk.Done, "\u25CF" to WidgetInk.Accent, "\u00A0" to WidgetInk.Faint), week.days.take(3).map { it.glyph to it.ink })
        assertEquals(listOf(false, true, false), week.days.take(3).map { it.today })
        assertTrue("two-letter weekdays: ${week.days}", week.days.all { it.label.length in 1..2 })
        // Any other day: "Open lollop to update" in the muted colour, Log only.
        assertEquals(res.getString(R.string.widget_open_to_update), p.staleFace.title)
        assertEquals(WidgetInk.Muted, p.staleFace.titleInk)
        assertNull(p.staleFace.week)
        assertEquals(listOf("log"), p.staleFace.actions.map { it.dest })
        // The control is the Quick Settings tile: Log set with the session's title.
        assertEquals(IosControlFace(res.getString(R.string.tile_log_set), "Heavy day", "log_set", "dumbbell"), p.control)
        assertEquals(IosControlFace(res.getString(R.string.tile_log), null, "log", "plus"), p.staleControl)
    }

    /** Every slot count, up to every line, cut as the Android widget cuts it. */
    @Test
    fun theBodyIsCutForEverySlotCount() {
        val body = payload("lift").face.body
        assertEquals(0, body.lead)
        val items = listOf("Back Squat" to "5×3 @\u00A0117.5\u00A0kg", "Deadlift" to "3×5", "Bench press" to "3×5", "Cable row" to "3×5")
            .map { (name, detail) -> res.getString(R.string.widget_item_line, name, detail) }
        fun more(n: Int) = res.getQuantityString(R.plurals.widget_more_items, n, n)
        // A count stands in for two or more cut items, never for one.
        assertEquals(
            listOf(emptyList(), items.take(1), listOf(items[0], more(3)), listOf(items[0], items[1], more(2)), items),
            body.fits,
        )
        // A finished session: its lead lines first, then the item.
        val done = payload("done").face
        assertEquals(2, done.body.lead)
        assertEquals(
            listOf(res.getString(R.string.widget_session_done), res.getString(R.string.widget_next, "Heavy volume day", planDateLabel(day + 2))),
            done.body.fits[2],
        )
        // The hero carries its own lines: "Session done" above it, the next session's items under it.
        val hero = checkNotNull(done.hero)
        assertFalse(hero.figure)
        assertEquals(1, hero.body.lead)
        assertEquals(res.getString(R.string.widget_session_done), hero.body.fits[1].single())
        assertTrue(payload("run").face.hero!!.figure)
    }

    /** A rest day's lone lead line ("Next: Heavy volume day, Thu, Oct 30") gives way
     *  to the short one ("Next: Thu") where it would wrap past two lines. */
    @Test
    fun aLongLoneLeadLineHasItsShortForm() {
        val rest = widgetFace(state("rest").snapshot, day, res)
        val short = checkNotNull(payload("rest").face.shortBody)
        // The measure the Android widget cuts by, so the extension decides as it does.
        assertEquals(textEm(rest.lead.single()), short.leadEm)
        // 2025-10-30, two days on, is a Thursday.
        assertEquals(listOf(res.getString(R.string.widget_next_day, "Thu")), short.body.fits[1])
        assertTrue(textEm(rest.brief!!) < short.leadEm)
        // A lead line that is its own short form has none.
        assertNull(payload("hold").face.shortBody)
        assertNull(payload("lift").face.shortBody)
    }

    /** The day the faces hold: the snapshot's local day in its zone, midnight to
     *  midnight (25 hours across the end of summer time); none once marked stale. */
    @Test
    fun theFacesHoldForTheSnapshotsLocalDay() {
        val current = checkNotNull(payload("lift").current)
        assertEquals("Europe/Berlin", current.zone)
        assertEquals(Instant.parse("2025-10-27T23:00:00Z").toEpochMilliseconds(), current.startMs)
        assertEquals(Instant.parse("2025-10-28T23:00:00Z").toEpochMilliseconds(), current.endMs)
        val dst = state("lift").snapshot.copy(epochDay = day - 2)
        val sunday = iosWidgetPayload(dst, false, AppTheme.Default, DarkMode.System, res).current!!
        assertEquals(25 * 3_600_000L, sunday.endMs - sunday.startMs)
        assertNull(payload("stale").current)
        assertEquals(payload("stale").staleFace, payload("stale").face)
    }

    @Test
    fun aRecordingRunLeadsWithItsTileFace() {
        val p = payload("recording", theme = AppTheme.Rose, darkMode = DarkMode.Dark)
        assertEquals(SurfaceIcon.TrackRun, p.face.actions.first().icon)
        assertEquals("track", p.face.actions.first().dest)
        assertEquals("track", p.staleFace.actions.first().dest)
        assertEquals("location.fill", p.control.symbol)
        assertEquals("location", payload("run").control.symbol)
    }

    /** The colours per system appearance: the theme's light and dark palettes, or
     *  the forced one for both. */
    @Test
    fun theColoursFollowTheAppsDarkMode() {
        val system = payload("lift", AppTheme.Iris, DarkMode.System)
        assertNotEquals(system.light, system.dark)
        assertEquals(AppTheme.Iris.dark.accent.hex(), system.dark.accent)
        assertEquals(AppTheme.Iris.light.accent.hex(), system.light.accent)
        // The done mark: the deep green on light grounds, the lighter one on dark.
        assertEquals(StatusColors().done.hex(), system.light.done)
        assertEquals(DarkStatusColors.done.hex(), system.dark.done)
        assertEquals(system.dark, payload("lift", AppTheme.Iris, DarkMode.Dark).light)
        assertEquals(system.light, payload("lift", AppTheme.Iris, DarkMode.Light).dark)
    }

    /** The colour the extension parses: `#AARRGGBB`, upper case, eight digits. */
    @Test
    fun coloursAreArgbHex() {
        assertEquals("#FF5A3FC0", Color(0xFF5A3FC0).hex())
        assertEquals("#0A0B0C0D", Color(0x0A0B0C0D).hex())
    }

    @Test
    fun theFallbackIsTheStaleFace() {
        val fallback = iosWidgetFallback(res)
        assertEquals(res.getString(R.string.widget_open_to_update), fallback.face.title)
        assertEquals(listOf(SurfaceIcon.Log), fallback.face.actions.map { it.icon })
        assertEquals(IosControlFace(res.getString(R.string.tile_log), null, "log", "plus"), fallback.control)
    }

    @Test
    fun theRunActivityShowsTheNotificationsTextAndFigures() {
        fun live(phase: RunSurfacePhase, hasTrack: Boolean = true, pace: Double = 5.75) =
            runLiveOf(RunSurfaceState(phase, movingSec = 1421, distanceKm = 4.12, paceMinPerKm = pace, miles = false, hasTrack = hasTrack), AppTheme.Iris, res)
        val recording = live(RunSurfacePhase.Recording)
        assertTrue(recording.recording)
        assertEquals("23:41", recording.time)
        assertEquals("23:41", recording.chip)
        assertEquals("4.12 km", recording.distance)
        assertEquals("5:45 /km", recording.pace)
        assertNull(recording.text)
        assertTrue(recording.canPause)
        assertFalse(recording.paused)
        assertEquals(AppTheme.Iris.dark.accent.hex(), recording.accentDark)
        assertNull(live(RunSurfacePhase.Recording, pace = Double.NaN).pace)
        // Before the track has two fixes: Android's hint line instead of the figures.
        val first = live(RunSurfacePhase.Recording, hasTrack = false)
        assertNull(first.distance)
        assertNotNull(first.text)
        // Paused: the figures without the pace, the chip says it.
        val paused = live(RunSurfacePhase.Paused)
        assertTrue(paused.paused)
        assertFalse(paused.recording)
        assertEquals("4.12 km", paused.distance)
        assertNull(paused.pace)
        assertTrue(paused.chip != paused.time)
        val lost = live(RunSurfacePhase.SignalLost)
        assertNull(lost.distance)
        assertNotNull(lost.text)
        // Before recording: no Pause/Resume.
        for (phase in listOf(RunSurfacePhase.Locating, RunSurfacePhase.Armed)) {
            val l = live(phase)
            assertFalse(phase.name, l.canPause)
            assertNotNull(phase.name, l.text)
            assertTrue("chip ${l.chip}", l.chip.length <= LIVE_CHIP_MAX_CHARS)
        }
        val mi = runLiveOf(RunSurfaceState(RunSurfacePhase.Recording, 600, 1.609344, 6.0, miles = true), AppTheme.Iris, res)
        assertEquals("1.00 mi", mi.distance)
    }

    @Test
    fun theRestActivityNamesTheExerciseAndTheNextSet() {
        val run = RestRun(startMillis = 1_000L, endAtMillis = 91_000L, exercise = "Back squat")
        val rest = restLiveOf(run, RestNextSet("Back squat", "Set 3 of 4 · 100 kg × 5"), AppTheme.Iris, res)
        assertTrue(rest.title, rest.title.contains("Back squat"))
        assertTrue(rest.next, rest.next!!.contains("Set 3 of 4"))
        assertEquals(1_000L, rest.startMs)
        assertEquals(91_000L, rest.endMs)
        assertEquals(AppTheme.Iris.light.accent.hex(), rest.accentLight)
        assertNull(restLiveOf(run, null, AppTheme.Iris, res).next)
    }
}
