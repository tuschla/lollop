@file:OptIn(ExperimentalForeignApi::class, ObsoleteWorkersApi::class)

package app.milestone.core

import app.milestone.core.cinterop.lollo_panic_for_test
import kotlin.native.concurrent.ObsoleteWorkersApi
import kotlin.native.concurrent.TransferMode
import kotlin.native.concurrent.Worker
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.double
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long

private const val VIEW_FIXTURES = "shared/tests/fixtures/view_snapshot"
private const val COMPACTION_FIXTURES = "shared/tests/fixtures/compaction"
private const val SOAK_RUN = "shared/tests/fixtures/device_tracks/soak-e37-20min.events.ndjson"

/** The first [type] line of the core's hybrid snapshot log: an event as the shell writes it. */
private fun fixtureEvent(type: String): String =
    readLogLines("$VIEW_FIXTURES/hybrid.events.ndjson").first { it.startsWith("{\"$type\"") }

private val PROFILE = fixtureEvent("SetProfile")
private val LOG_SET = json(fixtureEvent("LogSet")).jsonObject.getValue("LogSet").jsonObject

private fun setEvent(entryId: Int): String =
    JsonObject(mapOf("LogSet" to JsonObject(LOG_SET + ("entry_id" to JsonPrimitive(entryId))))).toString()

/** The core applied the event: it answered the effect-request array ([CoreBridge.update]). */
private fun accepted(answer: ByteArray?): Boolean = answer != null && answer.isNotEmpty() && json(answer) is JsonArray

private fun json(bytes: ByteArray?): JsonElement = json(bytes!!.decodeToString())

private fun json(text: String): JsonElement = Json.parseToJsonElement(text)

/** The GPX export's input for the run `(entryId, observedAt)`. */
private fun runRef(entryId: Long, observedAt: Long): ByteArray =
    """{"entry_id":$entryId,"observed_at":$observedAt}""".encodeToByteArray()

private fun JsonElement.errorKind(): String? =
    (this as? JsonObject)?.get("error")?.jsonObject?.get("kind")?.jsonPrimitive?.content

/**
 * Drives the Rust core through the C ABI exactly as the iOS shell will: the
 * core's own golden fixtures (view snapshots, compaction verdicts, a recorded
 * device run) must come back through Kotlin/Native unchanged.
 */
class BridgeTest {
    private val core: CoreBridge = NativeCore

    @BeforeTest
    fun freshCore() {
        core.reset()
    }

    private fun replay(lines: List<String>) = lines.forEach { line ->
        assertTrue(accepted(core.update(line.encodeToByteArray())), "replayed line accepted: ${line.take(80)}")
    }

    @Test
    fun snapshotLogsReplayToTheGoldenView() {
        val cases = listRepoDir(VIEW_FIXTURES).mapNotNull { it.removeSuffix(".events.ndjson").takeIf { c -> c != it } }
        assertTrue(cases.isNotEmpty(), "view snapshot fixtures found")
        for (case in cases) {
            core.reset()
            replay(readLogLines("$VIEW_FIXTURES/$case.events.ndjson"))
            val golden = json(readRepoFile("$VIEW_FIXTURES/$case.view.json"))
            assertEquals(golden, json(core.view()), "view of $case matches the core's snapshot")
        }
    }

    @Test
    fun replayAfterResetRebuildsTheSameViewBytes() {
        val log = readLogLines("$VIEW_FIXTURES/hybrid.events.ndjson")
        replay(log)
        val first = core.view()!!
        core.reset()
        assertNotEquals(json(first), json(core.view()), "reset empties the core")
        replay(log)
        assertContentEquals(first, core.view(), "a replayed log renders byte-identical JSON")
    }

    @Test
    fun aMalformedEventIsDroppedAndTheViewIsUnchanged() {
        replay(listOf(PROFILE))
        val before = core.view()!!
        for (line in listOf("""{"NoSuchEvent":1}""", "{\"SetPro", "")) {
            val answer = core.update(line.encodeToByteArray())
            assertEquals(0, answer!!.size, "dropped event answers no bytes: $line")
        }
        assertContentEquals(before, core.view())
    }

    @Test
    fun anUpdatePanicResetsTheCoreAndReplayRestoresIt() {
        val log = listOf(PROFILE, setEvent(1), setEvent(2))
        replay(log)
        val before = core.view()!!
        val answer = json(lollo_panic_for_test(true).take())
        assertEquals("panic_reset", answer.errorKind(), "update-path panic: $answer")
        assertEquals("update", answer.jsonObject["error"]!!.jsonObject["context"]!!.jsonPrimitive.content)
        // The core was replaced: the sets are gone until the shell replays.
        assertEquals(0, json(core.view()).jsonObject["lifts"]!!.jsonArray.size)
        replay(log)
        assertContentEquals(before, core.view(), "replay after panic_reset restores the view")
    }

    @Test
    fun aViewPanicKeepsTheCore() {
        replay(listOf(PROFILE, setEvent(1)))
        val before = core.view()!!
        val answer = json(lollo_panic_for_test(false).take())
        assertEquals("panic", answer.errorKind(), "view-path panic: $answer")
        assertContentEquals(before, core.view(), "a view panic leaves the model intact")
    }

    @Test
    fun compactLogMatchesTheGoldenCompactions() {
        val cases = listRepoDir(COMPACTION_FIXTURES).mapNotNull { it.removeSuffix(".in.ndjson").takeIf { c -> c != it } }
        assertTrue(cases.size > 10, "compaction fixtures found")
        for (case in cases) {
            val lines = readLogLines("$COMPACTION_FIXTURES/$case.in.ndjson")
            val verdict = json(core.compactLog(lines.toTypedArray())).jsonObject
            val gone = (verdict["dropped"]!!.jsonArray + verdict["torn"]!!.jsonArray).map { it.jsonPrimitive.int }.toSet()
            val kept = lines.filterIndexed { i, _ -> i !in gone }.map { json(it) }
            val expected = readLogLines("$COMPACTION_FIXTURES/$case.expected.ndjson").map { json(it) }
            assertEquals(expected, kept, "compaction of $case")
        }
        val torn = json(core.compactLog(arrayOf(PROFILE, "{\"SetPro"))).jsonObject
        assertEquals(listOf(1), torn["torn"]!!.jsonArray.map { it.jsonPrimitive.int })
    }

    @Test
    fun aRecordedRunExportsGpxAndPreviewsItsOfRecordFigures() {
        val line = readLogLines(SOAK_RUN).single()
        val figures = json(core.trackFigures(line.encodeToByteArray())).jsonObject
        replay(listOf(line))
        val run = json(core.view()).jsonObject["runs"]!!.jsonArray.single().jsonObject
        assertTrue(run["has_gpx"]!!.jsonPrimitive.boolean, "the recorded run has a route")
        assertEquals(run["distance_km"]!!.jsonPrimitive.double, figures["distance_km"]!!.jsonPrimitive.double)
        assertEquals(run["duration_min"]!!.jsonPrimitive.double, figures["duration_min"]!!.jsonPrimitive.double)
        val gpx = core.exportGpx(runRef(run["entry_id"]!!.jsonPrimitive.long, run["observed_at"]!!.jsonPrimitive.long))!!.decodeToString()
        assertTrue(gpx.startsWith("<?xml") && "<trkpt" in gpx, "GPX document: ${gpx.take(80)}")
        assertEquals(0, core.exportGpx(runRef(1, 0))!!.size, "an unknown run exports nothing")
        assertEquals("bad_input", json(core.exportGpx(runRef(-1, 0))).errorKind(), "a negative id names no run")
        assertEquals("track_figures", json(core.trackFigures(PROFILE.encodeToByteArray())).jsonObject["error"]!!
            .jsonObject["context"]!!.jsonPrimitive.content)
    }

    @Test
    fun parseRunFileReportsTheCoresImportVerdicts() {
        fun refusal(bytes: ByteArray) = json(core.parseRunFile(bytes)).jsonObject["refusal"]?.jsonPrimitive?.content
        assertEquals("wrong_type", refusal("not a run file".encodeToByteArray()))
        assertEquals("wrong_type", refusal(ByteArray(0)))
        val ride = json(core.parseRunFile(readRepoFile("shared/tests/fixtures/garmin-fenix-5-bike.fit"))).jsonObject
        assertEquals("not_a_run", ride["refusal"]?.jsonPrimitive?.content, "a bike ride: $ride")
        assertEquals("cycling", ride["sport"]?.jsonPrimitive?.content, "a bike ride: $ride")
        assertEquals("too_short", refusal(readRepoFile("shared/tests/fixtures/Activity.fit")), "a 13 s fragment")
    }

    @Test
    fun theRecordersThinningIsTheCores() {
        val thin = json(core.thinTrack("""{"fixes":10,"max_points":4,"segment_starts":[5]}""".encodeToByteArray())).jsonObject
        assertEquals(listOf(0, 3, 5, 6, 9), thin["kept"]!!.jsonArray.map { it.jsonPrimitive.int })
        assertEquals(listOf(2), thin["segment_starts"]!!.jsonArray.map { it.jsonPrimitive.int })
        assertEquals("bad_input", json(core.thinTrack("{".encodeToByteArray())).errorKind())
    }

    @Test
    fun updatesFromFourThreadsAllLand() {
        replay(listOf(PROFILE))
        val workers = List(4) { Worker.start() }
        val accepted = workers.mapIndexed { w, worker ->
            worker.execute(TransferMode.SAFE, { w }) { w ->
                (1..25).count { i -> accepted(NativeCore.update(setEvent(w * 25 + i).encodeToByteArray())) }
            }
        }.sumOf { it.result }
        workers.forEach { it.requestTermination().result }
        assertEquals(100, accepted, "every update answered with an effect-request array")
        val lifts = json(core.view()).jsonObject["lifts"]!!.jsonArray
        assertEquals(100, lifts.size, "every set from every thread is in the view")
        assertEquals((1..100).toSet(), lifts.map { it.jsonObject["entry_id"]!!.jsonPrimitive.int }.toSet())
    }

    @Test
    fun anEmptyLogCompactsToAnEmptyVerdict() {
        val verdict = json(core.compactLog(emptyArray())).jsonObject
        assertEquals(0, verdict["dropped"]!!.jsonArray.size)
        assertNull(verdict["error"])
    }
}
