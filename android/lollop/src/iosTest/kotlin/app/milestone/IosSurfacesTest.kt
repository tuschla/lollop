package app.milestone

import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetWeekDay
import app.milestone.widget.WidgetWeekMark
import app.milestone.widget.widgetFace
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * What the app hands the Swift widget extension and Live Activities (IosSurfaces.kt):
 * the common widget faces and tile face in the payload, the JSON keys the
 * extension decodes (iosApp/LollopWidgets/WidgetModel.swift, LollopActivity.swift),
 * and the run and rest activity text, in the app's real resources.
 */
class IosSurfacesTest {
    private val res = systemResources()
    private val day = 20_390L

    private fun lift(name: String, detail: String, done: Boolean = false) = WidgetItem(WidgetItemKind.Lift, name, detail, done)

    private val week = (0 until 7).map { i ->
        WidgetWeekDay(day - 1 + i, if (i == 0) WidgetWeekMark.Done else if (i == 1) WidgetWeekMark.Next else WidgetWeekMark.Planned)
    }

    private val liftDay = WidgetSnapshot(
        epochDay = day,
        kind = WidgetDayKind.Lift,
        title = "Heavy day",
        items = listOf(lift("Back Squat", "5×3 @ 117.5 kg"), lift("Deadlift", "3×5"), lift("Bench press", "3×5"), lift("Cable row", "3×5")),
        weekDone = 1,
        weekPlanned = 4,
        zone = "Europe/Berlin",
        week = week,
    )

    @Test
    fun theWidgetPayloadCarriesTheCommonFaces() {
        val p = iosWidgetPayload(liftDay, recording = false, theme = AppTheme.Paper, darkMode = DarkMode.System, res = res)
        val face = widgetFace(liftDay, day, res)
        assertEquals(face.title, p.face.title)
        assertEquals(face.overline, p.face.overline)
        assertEquals(face.items, p.face.items)
        assertEquals(face.week, p.face.week)
        assertEquals(planDateLabel(day), p.face.date)
        assertEquals(listOf("log_set", "log"), p.face.actions.map { it.dest })
        assertEquals(listOf("log_set", "log"), p.face.actions.map { it.face })
        assertEquals(7, p.face.days.size)
        assertEquals(listOf("done", "next"), p.face.days.take(2).map { it.mark })
        assertEquals(listOf(false, true, false), p.face.days.take(3).map { it.today })
        assertTrue(p.face.days.all { it.label.length in 1..2 }, "two-letter weekdays: ${p.face.days}")
        // Any other day: "Open lollop to update" with Log only.
        assertEquals(widgetFace(null, day, res).title, p.staleFace.title)
        assertEquals("muted", p.staleFace.tone)
        assertEquals(listOf("log"), p.staleFace.actions.map { it.dest })
        // The control is the Quick Settings tile: Log set with the session's title.
        assertEquals("log_set", p.control.dest)
        assertEquals(res.getString(TileFace.LogSet.label), p.control.label)
        assertEquals("Heavy day", p.control.subtitle)
        assertEquals("log", p.staleControl.dest)
        assertNull(p.staleControl.subtitle)
        // "N more items" for every count a cut list can show.
        assertEquals(4, p.moreItems.size)
        assertTrue(p.moreItems[2].startsWith("3"), p.moreItems[2])
        // Colours as #AARRGGBB.
        assertEquals("system", p.palette.mode)
        assertTrue(Regex("#[0-9A-F]{8}").matches(p.palette.light.accent), p.palette.light.accent)
        assertEquals(AppTheme.Paper.dark.bgTop.hex(), p.palette.dark.ground)
    }

    @Test
    fun aRecordingRunLeadsWithItsTileFace() {
        val p = iosWidgetPayload(liftDay.copy(kind = WidgetDayKind.Rest, items = emptyList(), title = ""), recording = true, theme = AppTheme.Rose, darkMode = DarkMode.Dark, res = res)
        assertEquals("recording", p.face.actions.first().face)
        assertEquals("track", p.face.actions.first().dest)
        assertEquals("track", p.staleFace.actions.first().dest)
        assertTrue(p.control.active)
        assertEquals("dark", p.palette.mode)
    }

    @Test
    fun theJsonHasTheKeysTheExtensionDecodes() {
        val json = surfaceJson.encodeToString(
            IosWidgetPayload.serializer(),
            iosWidgetPayload(liftDay, recording = false, theme = AppTheme.Iris, darkMode = DarkMode.Light, res = res),
        )
        val o = Json.parseToJsonElement(json).jsonObject
        for (key in listOf("version", "snapshot", "recording", "face", "staleFace", "moreItems", "control", "staleControl", "palette")) {
            assertNotNull(o[key], "payload key $key")
        }
        val snapshot = o["snapshot"]!!.jsonObject
        for (key in listOf("epochDay", "stale", "kind", "weekDone", "weekPlanned", "zone")) assertNotNull(snapshot[key], "snapshot key $key")
        assertEquals("Lift", snapshot["kind"]!!.jsonPrimitive.content)
        val face = o["face"] as JsonObject
        for (key in listOf("overline", "date", "title", "tone", "status", "brief", "lead", "items", "hero", "heroLead", "heroItems", "week", "days", "actions")) {
            assertTrue(key in face, "face key $key")
        }
        assertEquals("log_set", face["actions"]!!.jsonArray[0].jsonObject["dest"]!!.jsonPrimitive.content)
    }

    @Test
    fun theRunActivityShowsTheNotificationsTextAndFigures() {
        fun live(phase: RunActivityPhase, hasTrack: Boolean = true, pace: Double = 5.75) =
            runLiveOf(RunActivityState(phase, movingSec = 1421, distanceKm = 4.12, paceMinPerKm = pace, miles = false, hasTrack = hasTrack), res)
        val recording = live(RunActivityPhase.Recording)
        assertEquals("recording", recording.phase)
        assertEquals("23:41", recording.time)
        assertEquals("23:41", recording.chip)
        assertEquals("4.12 km", recording.distance)
        assertEquals("5:45 /km", recording.pace)
        assertNull(recording.text)
        assertTrue(recording.canPause)
        assertFalse(recording.paused)
        // No pace yet: the figures without one.
        assertNull(live(RunActivityPhase.Recording, pace = Double.NaN).pace)
        // Before the track has two fixes: Android's hint line instead of the figures.
        val first = live(RunActivityPhase.Recording, hasTrack = false)
        assertNull(first.distance)
        assertNotNull(first.text)
        // Paused: the figures without the pace, the chip says it.
        val paused = live(RunActivityPhase.Paused)
        assertTrue(paused.paused)
        assertEquals("4.12 km", paused.distance)
        assertNull(paused.pace)
        assertTrue(paused.chip != paused.time)
        // Signal lost: the warning line, no figures.
        val lost = live(RunActivityPhase.SignalLost)
        assertNull(lost.distance)
        assertNotNull(lost.text)
        // Before recording: no Pause/Resume.
        for (phase in listOf(RunActivityPhase.Locating, RunActivityPhase.Armed)) {
            val l = live(phase)
            assertFalse(l.canPause, phase.name)
            assertNotNull(l.text, phase.name)
            assertTrue(l.chip.length <= 7, "chip ${l.chip}")
        }
        // Miles.
        val mi = runLiveOf(RunActivityState(RunActivityPhase.Recording, 600, 1.609344, 6.0, miles = true), res)
        assertEquals("1.00 mi", mi.distance)
    }

    @Test
    fun theRestActivityNamesTheExerciseAndTheNextSet() {
        val run = RestRun(startMillis = 1_000L, endAtMillis = 91_000L, exercise = "Back squat")
        val rest = restLiveOf(run, RestNextSet("Back squat", "Set 3 of 4 · 100 kg × 5"), res)
        assertTrue(rest.title.contains("Back squat"), rest.title)
        assertNotNull(rest.next)
        assertTrue(rest.next!!.contains("Set 3 of 4"), rest.next)
        assertEquals(1_000L, rest.startMs)
        assertEquals(91_000L, rest.endMs)
        assertNull(restLiveOf(run, null, res).next)
    }
}
