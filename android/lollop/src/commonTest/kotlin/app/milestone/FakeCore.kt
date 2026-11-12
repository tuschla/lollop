package app.milestone

import app.milestone.core.CoreBridge
import kotlin.concurrent.Volatile
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/** The [ErrorEnvelope] the core's FFI firewall answers a failed op with. */
internal fun coreErrorJson(kind: ErrorKind, context: String, message: String = ""): String =
    coreJson.encodeToString(ErrorEnvelope.serializer(), ErrorEnvelope(CoreError(kind, context, message)))

/**
 * The tests' stand-in for the Rust core, on the wire types. Like the core: an
 * event naming a known variant ([EventLog.variantOf]) is applied and answers an
 * effect array, any other line is dropped (no bytes); a [poison] line answers
 * [ErrorKind.PanicReset] and empties the core, as the FFI firewall does. The view
 * counts the applied events (`input_count`), echoes the [profile] (the last
 * `SetProfile`'s), holds a program from a `GeneratePlan` until a `ClearPlan`,
 * which sets `plan_removed` until the next `GeneratePlan`. Compaction drops every
 * `LogSet` a later `ClearSets` supersedes, reports the lines that are not JSON as
 * torn and the largest `entry_id`. It answers no other op.
 *
 * Faults: the variants [refuses] names are dropped, as a core rejects an event;
 * [viewFails] answers every view with an [ErrorKind.Panic] envelope (the core stays
 * intact); [dead] answers every op with null (nothing could be allocated);
 * [linkError] is thrown by every op (a native library that never loaded). [reset]
 * always empties the core, so a process reset never inherits a fault.
 */
internal class FakeCore(
    @Volatile var profile: Profile? = null,
    programAtStart: Boolean = false,
) : CoreBridge {
    /** The event lines the core holds, in the order applied. */
    val applied = mutableListOf<String>()

    @Volatile var poison: Set<String> = emptySet()

    @Volatile var refuses: (variant: String) -> Boolean = { false }

    @Volatile var viewFails = false

    @Volatile var dead = false

    @Volatile var linkError: Throwable? = null

    private var program = programAtStart
    private var planRemoved = false

    fun variants(): List<String?> = applied.map(EventLog::variantOf)

    private inline fun answer(op: () -> ByteArray): ByteArray? {
        linkError?.let { throw it }
        return if (dead) null else op()
    }

    private fun empty() {
        applied.clear()
        profile = null
        program = false
        planRemoved = false
    }

    override fun update(event: ByteArray): ByteArray? = answer {
        val line = event.decodeToString()
        val variant = EventLog.variantOf(line)
        when {
            line in poison -> {
                empty()
                coreErrorJson(ErrorKind.PanicReset, "update").encodeToByteArray()
            }
            variant == null || refuses(variant) -> ByteArray(0)
            else -> {
                applied += line
                when (variant) {
                    "SetProfile" -> profile = coreJson.decodeFromJsonElement(Profile.serializer(), payload(line))
                    "GeneratePlan" -> {
                        program = true
                        planRemoved = false
                    }
                    "ClearPlan" -> {
                        program = false
                        planRemoved = true
                    }
                }
                "[]".encodeToByteArray()
            }
        }
    }

    override fun view(): ByteArray? = answer {
        if (viewFails) {
            coreErrorJson(ErrorKind.Panic, "view").encodeToByteArray()
        } else {
            val view = ViewModel(
                input_count = applied.size,
                profile = profile,
                program = if (program) ProgramSummaryView() else null,
                plan_removed = planRemoved,
            )
            coreJson.encodeToString(ViewModel.serializer(), view).encodeToByteArray()
        }
    }

    override fun compactLog(lines: Array<String>): ByteArray? = answer {
        val parsed: List<JsonElement?> = lines.map { runCatching { coreJson.parseToJsonElement(it) }.getOrNull() }
        val lastClear = lines.indexOfLast { EventLog.variantOf(it) == "ClearSets" }
        val verdict = Compaction(
            dropped = lines.indices.filter { it < lastClear && EventLog.variantOf(lines[it]) == "LogSet" },
            torn = parsed.indices.filter { parsed[it] == null },
            max_entry_id = parsed.maxOfOrNull { entryId(it) } ?: 0L,
        )
        coreJson.encodeToString(Compaction.serializer(), verdict).encodeToByteArray()
    }

    override fun reset(): ByteArray {
        empty()
        return ByteArray(0)
    }

    override fun parseRunFile(file: ByteArray): ByteArray? = null

    override fun exportGpx(run: ByteArray): ByteArray? = null

    override fun runTrack(run: ByteArray): ByteArray? = null

    override fun trackFigures(event: ByteArray): ByteArray? = null

    override fun exerciseKeys(names: Array<String>): ByteArray? = null

    override fun warmUpRamp(query: ByteArray): ByteArray? = null

    override fun draftFacts(draft: ByteArray): ByteArray? = null

    override fun thinTrack(track: ByteArray): ByteArray? = null

    override fun importedHrPct(hr: ByteArray): ByteArray? = null

    override fun relogEvent(row: ByteArray): ByteArray? = null

    private fun payload(line: String): JsonObject = coreJson.parseToJsonElement(line).jsonObject.values.single().jsonObject

    private fun entryId(line: JsonElement?): Long =
        ((line as? JsonObject)?.values?.singleOrNull() as? JsonObject)?.get("entry_id")?.jsonPrimitive?.longOrNull ?: 0L
}
