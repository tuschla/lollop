package app.milestone

import app.milestone.core.JniCore
import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The generated Kotlin types (WireTypes.kt) against the core's own fixtures
 * (`shared/tests/fixtures`):
 *  - every view snapshot decodes with no key left unread and re-encodes to the
 *    same JSON;
 *  - every line the old hand-written encoder persisted
 *    (`events/shell_encoder.ndjson`) decodes and re-encodes to the same bytes, so
 *    a log written before and after the switch reads the same;
 *  - every line of the core's event logs (view snapshots, compaction goldens,
 *    upgrade logs) the shell can decode and that carries its own identity
 *    replays, re-encoded, into the same view as the line itself (on the real
 *    core, in the one Robolectric sandbox the host suite loads `libshared.so` into).
 */
@RunWith(RobolectricTestRunner::class)
class WireConformanceTest {

    private val strict = Json(coreJson) { ignoreUnknownKeys = false }

    /** The keys the shell fills with a fresh value when it creates an event. */
    private val shellFilled = setOf("entry_id", "observed_at")

    private fun fixtures(dir: String, suffix: String): List<File> {
        val files = File(REPO_ROOT, "shared/tests/fixtures/$dir").listFiles { f -> f.name.endsWith(suffix) }.orEmpty().sortedBy { it.name }
        assertTrue("no $dir/*$suffix fixtures", files.isNotEmpty())
        return files
    }

    @Test
    fun everyViewSnapshotDecodesStrictlyAndReencodesTheSame() {
        for (file in fixtures("view_snapshot", ".view.json")) {
            val text = file.readText()
            val view = strict.decodeFromString(ViewModel.serializer(), text)
            val again = coreJson.encodeToJsonElement(ViewModel.serializer(), view)
            assertSameJson(file.name, Json.parseToJsonElement(text), again)
        }
    }

    @Test
    fun everyShellLineReencodesByteForByte() {
        val lines = fixtures("events", "shell_encoder.ndjson").single().readLines().filter { it.isNotBlank() }
        for (line in lines) {
            val event = strict.decodeFromString(Event.serializer(), line)
            assertEquals(line, event.toJson().toString())
            assertTrue(line, Event.tagOf(Json.parseToJsonElement(line)) != null)
        }
    }

    @Test
    fun everyCoreLogLineReplaysTheSameReencoded() {
        val logs = fixtures("view_snapshot", ".events.ndjson") +
            fixtures("compaction", ".ndjson") +
            fixtures("upgrade", ".log.ndjson") +
            fixtures("upgrade", ".backup.ndjson")
        var reencoded = 0
        for (log in logs) {
            val lines = log.readLines().filter { it.isNotBlank() }
            val shell = lines.map { line -> reencoded(line)?.also { reencoded++ } ?: line }
            assertEquals(log.name, replayedView(lines), replayedView(shell))
        }
        assertTrue("re-encoded $reencoded lines", reencoded > 0)
    }

    /** [line] decoded and re-encoded by the shell, or null when the shell can't decode
     *  it or would fill in its identity: a legacy line without `entry_id` or
     *  `observed_at` decodes with a fresh id / the current time (the shell's defaults
     *  for an event it creates), so it is no longer the line's event. The shell never
     *  re-encodes a stored line; this is only the conformance check's own round trip. */
    private fun reencoded(line: String): String? {
        val event = runCatching { coreJson.decodeFromString(Event.serializer(), line) }.getOrNull() ?: return null
        val again = event.toJson()
        val had = payloadKeys(Json.parseToJsonElement(line))
        if (shellFilled.any { it in payloadKeys(again) && it !in had }) return null
        return again.toString()
    }

    private fun payloadKeys(event: JsonElement): Set<String> =
        ((event as? JsonObject)?.values?.singleOrNull() as? JsonObject)?.keys.orEmpty()

    private fun replayedView(lines: List<String>): String {
        JniCore.reset()
        try {
            lines.forEach { JniCore.update(it.encodeToByteArray()) }
            return checkNotNull(JniCore.view()).decodeToString()
        } finally {
            JniCore.reset()
        }
    }

    /** [expected] and [actual] equal key for key, numbers by value (Rust and
     *  Kotlin spell some floats differently: `1e-7` / `1.0E-7`). */
    private fun assertSameJson(at: String, expected: JsonElement, actual: JsonElement) {
        when (expected) {
            is JsonObject -> {
                actual as JsonObject
                assertEquals("$at: keys", expected.keys, actual.keys)
                expected.forEach { (k, v) -> assertSameJson("$at.$k", v, actual.getValue(k)) }
            }
            is JsonArray -> {
                actual as JsonArray
                assertEquals("$at: length", expected.size, actual.size)
                expected.forEachIndexed { i, v -> assertSameJson("$at[$i]", v, actual[i]) }
            }
            JsonNull -> assertEquals(at, JsonNull, actual)
            is JsonPrimitive -> {
                actual as JsonPrimitive
                if (expected.isString || actual.isString || expected.content in setOf("true", "false")) {
                    assertEquals(at, expected, actual)
                } else {
                    assertEquals(at, expected.content.toDouble(), actual.content.toDouble(), 0.0)
                }
            }
        }
    }
}
