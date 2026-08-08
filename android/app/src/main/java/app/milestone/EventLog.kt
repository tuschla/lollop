package app.milestone

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long

/**
 * Result of [EventLog.load]: the surviving replayable lines plus whether this is
 * a genuinely fresh install. [freshInstall] is true ONLY when the log file never
 * existed, a log that exists but compacted to zero surviving lines (the user
 * cleared all their data) is a RETURNING user, and the caller must not re-seed
 * the onboarding profile over their deliberate empty state.
 */
data class RestoredLog(val lines: List<String>, val freshInstall: Boolean)

/**
 * Event-log compaction over raw serde wire lines. Lives outside [Core] (which
 * loads `libshared.so` in its initializer) so it stays pure Kotlin and is unit
 * testable on the JVM without a device or the native library.
 *
 * Mirrors the authoritative Rust implementation `compact_event_log`
 * (shared/src/log.rs), which the web shell uses directly and whose
 * replay-equivalence tests pin the rules. That function isn't exposed over the
 * JSON FFI, so this reimplements it over raw wire lines; the two must stay in
 * lockstep: a new `Event` family added there must be added here.
 */
object EventLog {
    private val json = Json { ignoreUnknownKeys = true }

    /** Clear-variant → the member variants its clear supersedes. `DeleteEntry`
     *  belongs to ClearSets OR ClearRuns depending on its `kind`, so it is NOT
     *  listed statically here: [memberOfClear] resolves it per-line. */
    private val families = mapOf(
        "ClearReadiness" to setOf("SubmitReadiness", "RemoveReadiness"),
        "ClearSets" to setOf("LogSet", "AmendSet"),
        "ClearRuns" to setOf("LogRun", "LogRunTrack", "AmendRun"),
        "ClearProfile" to setOf("SetProfile"),
        "ClearReview" to setOf("SubmitReview"),
        "ClearRacePrediction" to setOf("PredictRace"),
        "ClearHypertrophyPlan" to setOf("PlanHypertrophyMeso"),
        "ClearProtein" to setOf("ComputeProtein"),
        "ClearHrZones" to setOf("ComputeHrZones"),
        "ClearCooper" to setOf("ComputeCooper"),
        "ClearCriticalSpeed" to setOf("ComputeCriticalSpeed"),
        "ClearApre" to setOf("ComputeApre"),
        // Family 12: retained morning check-ins. NOT day-scoped;
        // multiple across days all survive (they ARE the rolling baseline); only
        // a ClearCheckins supersedes them. Lockstep with log.rs classify.
        "ClearCheckins" to setOf("SubmitCheckin"),
        // Family 13: the accepted plan request, a last-write-wins
        // singleton with a ClearPlan reset. Lockstep with log.rs classify.
        "ClearPlan" to setOf("GeneratePlan"),
    )

    /** Last-write-wins singleton variants (assign a scalar model field outright). */
    private val singletons = listOf(
        "SetProfile", "SubmitReview", "PredictRace", "PlanHypertrophyMeso", "ComputeProtein",
        "ComputeHrZones", "ComputeCooper", "ComputeCriticalSpeed", "ComputeApre",
        // Plan request (family 13) + SetToday (family 14, the
        // shell's clock sent every foreground; keep exactly one line).
        "GeneratePlan", "SetToday",
    )

    /** Rule 4 retention window (lockstep with log.rs RETAIN_CHECKIN_DAYS): a
     *  SubmitCheckin more than this many days before the PER-CHANNEL anchor (the
     *  min over channels present of that channel's own newest reading) is dropped.
     *  Safely larger than autoreg's 30-day BASELINE_WINDOW_DAYS, so every dropped
     *  line is already outside the window derive_readiness reads → the drop is
     *  replay-equivalent. */
    private const val RETAIN_CHECKIN_DAYS = 45L
    /** Seconds per day, for the Rule 4 window (matches the core's day size). */
    private const val CHECKIN_DAY_SEC = 86_400L

    /**
     * Drop log lines whose effect on the replayed model is provably nil, returning
     * a shorter but replay-equivalent line list (same relative order of survivors).
     *
     * Two rules, both grounded in the core's `update` (app.rs): the model stores
     * only raw inputs and derives everything in `view`, and no event's `update`
     * reads another family's state.
     *  1. **Clear supersedes its family.** A `Clear<F>` empties family F's vec, so
     *     every F event (and the clear) at or before the *last* `Clear<F>` leaves
     *     no residue, F events after it replay against an already-empty vec.
     *  2. **Last write wins for singletons.** `SetProfile`/`SubmitReview` assign a
     *     scalar model field outright, and nothing else reads it at update time, so
     *     only the last surviving one matters.
     *  4. **Check-ins age out of a trailing window, PER CHANNEL.** The core windows
     *     EACH channel (wellness / HRV / resting-HR) on that channel's OWN newest
     *     reading, so the cutoff anchors on the MIN over channels present of that
     *     channel's newest reading. A `SubmitCheckin` more than [RETAIN_CHECKIN_DAYS]
     *     before that anchor is out of window for every channel it carries, so it is
     *     dropped, deterministic, no clock. Family 0 readiness is never windowed: a
     *     safety hold lives until explicitly cleared.
     */
    fun compact(lines: List<String>): List<String> {
        val variant = lines.map(::variantOf)
        val remove = BooleanArray(lines.size)

        // Rule 0 (mirrors log.rs): a RemoveReadiness cancels the latest
        // not-yet-cancelled prior SubmitReadiness with the same signal; an
        // unmatched remove replays as a no-op and is dropped alone.
        for (j in lines.indices) {
            if (variant[j] != "RemoveReadiness") continue
            // Unparsable line → never removable, same contract as variantOf.
            val signal = readinessSignalOf(lines[j], "RemoveReadiness") ?: continue
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && variant[it] == "SubmitReadiness" &&
                    readinessSignalOf(lines[it], "SubmitReadiness") == signal
            }
            if (i != null) remove[i] = true
            remove[j] = true
        }

        // Rule 3 (mirrors log.rs): a DeleteEntry cancels its entry; an AmendSet/
        // AmendRun supersedes a prior edit but KEEPS the entry's base log. Newest
        // match, id-first (observed_at fallback for a legacy row), the same
        // predicate the core's find_set/find_run use.
        //
        // B8 full prevention: the core's amend is a STRICT update (replace-on-
        // match, no-op on miss), so a lone amend with no base row replays to
        // nothing. Two consequences, both replay-equivalent: a matched amend
        // telescopes away a superseded prior amend but keeps the base log (so the
        // strict amend has a row to replace); a delete removes the WHOLE entry (its
        // newest match and, when that is an amend, the base chain down to the log)
        // plus itself. An unmatched delete OR amend is a no-op → dropped alone.
        // A RUN removal is skipped when a run line sits between the earliest removed
        // line and this delete/amend (a run bakes its spike baseline from the runs
        // present when logged, so removing it could change a later run's baseline);
        // a set carries no such baked state.
        //
        // F2 INVARIANT (lockstep with log.rs): Rule 3's chain-walk correctness
        // depends on legacy (entry_id 0) rows NEVER being re-dated: the walk keys a
        // whole set/run chain by ONE (fam, id, observed_at) match key, valid only
        // while a legacy row's observed_at is immutable. THIS SHELL enforces it:
        // LogEntry.kt pins observed_at == observed_at_fallback for id-0 rows,
        // blocking re-dating. If that pin is lifted, a re-dated legacy
        // Log→Amend→Amend reverts to the original value and a re-dated
        // Log→Amend→Delete resurrects the pre-edit row on the next launch.
        for (j in lines.indices) {
            val v = variant[j] ?: continue
            val isDelete = v == "DeleteEntry"
            val fam: Int
            val id: Long
            val time: Long
            when (v) {
                "DeleteEntry" -> {
                    fam = when (entryKindOf(lines[j])) { "Set" -> 1; "Run" -> 2; else -> continue }
                    id = longField(lines[j], v, "entry_id")
                    time = longField(lines[j], v, "observed_at_fallback")
                }
                // Match key must mirror log.rs exactly: an amend targets the OLD
                // identity, so use observed_at_fallback when nonzero (a re-dated
                // legacy row), else observed_at.
                "AmendSet" -> { fam = 1; id = longField(lines[j], v, "entry_id"); time = amendMatchTime(lines[j], v) }
                "AmendRun" -> { fam = 2; id = longField(lines[j], v, "entry_id"); time = amendMatchTime(lines[j], v) }
                else -> continue
            }
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && entryLineMatches(lines[it], variant[it], fam, id, time)
            }
            if (i == null) {
                // Unmatched delete OR amend: a no-op on replay (strict amend) → drop alone.
                remove[j] = true
                continue
            }
            if (isDelete) {
                // Collect the entry: the newest match, and, when that is an amend,
                // its base chain back through and INCLUDING the first log line. A
                // bare (shared-id) log stops at itself, mirroring update's
                // newest-only row removal. removal is built descending, so its last
                // element is the earliest index.
                val removal = mutableListOf(i)
                while (isAmendLine(variant[removal.last()])) {
                    val last = removal.last()
                    val p = (last - 1 downTo 0).firstOrNull {
                        !remove[it] && entryLineMatches(lines[it], variant[it], fam, id, time)
                    } ?: break
                    removal.add(p)
                }
                val earliest = removal.last()
                if (fam == 1 || !runLineBetween(lines, variant, earliest, j)) {
                    removal.forEach { remove[it] = true }
                    remove[j] = true
                }
                // else: baseline-unsafe → keep the entry and the delete.
            } else {
                // Amend: telescope away a superseded prior amend, but keep the base
                // log line (the strict amend needs a row to replace on replay).
                if (isAmendLine(variant[i]) && (fam == 1 || !runLineBetween(lines, variant, i, j))) {
                    remove[i] = true
                }
            }
        }

        for ((clear, members) in families) {
            val lastClear = variant.indexOfLast { it == clear }
            if (lastClear < 0) continue
            for (i in 0..lastClear) {
                if (variant[i] == clear || variant[i] in members ||
                    memberOfClear(clear, variant[i], lines[i])
                ) {
                    remove[i] = true
                }
            }
        }
        for (singleton in singletons) {
            val survivors = lines.indices.filter { !remove[it] && variant[it] == singleton }
            survivors.dropLast(1).forEach { remove[it] = true }
        }

        // Rule 4 (mirrors log.rs): age check-ins (family 12) out of a trailing
        // window, PER CHANNEL. derive_readiness windows EACH channel (wellness /
        // HRV / resting-HR) on THAT channel's OWN newest reading (per_day_series),
        // NOT the log's global newest: a sparsely-logged channel is still read on
        // replay. So the cutoff anchors on the MIN over channels PRESENT of that
        // channel's own newest reading; any check-in older than that is out of
        // window for every channel it carries, so dropping it is replay-equivalent
        // (a global-newest anchor could silently vanish a sparse resting-HR/HRV
        // readiness row, safety-adjacent). Reference is surviving log data, never
        // a clock. Family 0 readiness is NEVER windowed: a safety hold lives until
        // cleared.
        var wellnessNewest: Long? = null
        var hrvNewest: Long? = null
        var rhrNewest: Long? = null
        for (j in lines.indices) {
            if (remove[j] || variant[j] != "SubmitCheckin") continue
            val at = longField(lines[j], "SubmitCheckin", "observed_at")
            // per_day_series skips observed_at <= 0: such readings anchor nothing.
            if (at <= 0L) continue
            val hasWellness =
                doubleFieldOrNull(lines[j], "SubmitCheckin", "sleep_quality") != null ||
                    doubleFieldOrNull(lines[j], "SubmitCheckin", "soreness") != null ||
                    doubleFieldOrNull(lines[j], "SubmitCheckin", "mood") != null
            if (hasWellness) wellnessNewest = maxOf(wellnessNewest ?: at, at)
            val rhr = doubleFieldOrNull(lines[j], "SubmitCheckin", "resting_hr_bpm")
            if (rhr != null && rhr > 0.0) rhrNewest = maxOf(rhrNewest ?: at, at)
            val hrv = doubleFieldOrNull(lines[j], "SubmitCheckin", "hrv_rmssd_ms")
            if (hrv != null && hrv > 0.0) hrvNewest = maxOf(hrvNewest ?: at, at)
        }
        val anchor = listOfNotNull(wellnessNewest, hrvNewest, rhrNewest).minOrNull()
        if (anchor != null) {
            val cutoff = anchor - RETAIN_CHECKIN_DAYS * CHECKIN_DAY_SEC
            for (j in lines.indices) {
                if (variant[j] == "SubmitCheckin" &&
                    longField(lines[j], "SubmitCheckin", "observed_at") < cutoff
                ) {
                    remove[j] = true
                }
            }
        }
        return lines.filterIndexed { i, _ -> !remove[i] }
    }

    /**
     * Read + compact the persisted event log, rewriting [file] in place when
     * compaction dropped lines (atomic tmp-file + rename, so a crash mid-write
     * can never truncate the durable log). Pure file/JSON logic, no native
     * library, so the fresh-install-vs-compacted-empty distinction is unit
     * testable on the JVM (see EventLogTest); [Core.restore] replays the result.
     */
    fun load(file: File): RestoredLog {
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        val lines = file.readLines().filter { it.isNotBlank() }
        // Garbage-collect lines a crash mid-[Core.appendText] tore off before the
        // record was complete: those are not valid JSON at all, so replay would
        // reject them on every launch forever. FORWARD-COMPAT: a line that DOES
        // parse as JSON but whose variant this (old) build doesn't recognize -
        // an unknown single-key object OR an unknown bare-string unit variant is
        // a future event a newer build wrote; it must be preserved, not dropped.
        // So the filter is strictly JSON-parseability, never variant recognition
        // (compaction's [variantOf] already leaves unknown-but-parseable lines be).
        val intact = lines.filter(::parsesAsJson)
        val kept = compact(intact)
        if (kept.size < lines.size) {
            val tmp = File(file.parentFile, file.name + ".tmp")
            runCatching {
                tmp.writeText(kept.joinToString("\n", postfix = "\n"))
                if (!tmp.renameTo(file)) tmp.delete()
            }
        }
        return RestoredLog(kept, freshInstall = false)
    }

    /**
     * The serde variant tag of one wire line, or null if it doesn't parse (a
     * hand-edited or forward-incompatible line is then never removable). Unit
     * variants serialize as a bare JSON string (`"ClearRuns"`); struct/newtype
     * variants as a single-key object (`{"LogRun": …}`).
     */
    /** Whether `line` is well-formed JSON at all (any shape). A torn mid-append
     *  line fails this; a parseable line with an unknown variant passes it, the
     *  forward-compat contract [load] relies on to keep future events. */
    private fun parsesAsJson(line: String): Boolean =
        runCatching { json.parseToJsonElement(line) }.isSuccess

    fun variantOf(line: String): String? = runCatching {
        when (val el = json.parseToJsonElement(line)) {
            is JsonPrimitive -> if (el.isString) el.content else null
            is JsonObject -> el.keys.singleOrNull()
            else -> null
        }
    }.getOrNull()

    /** The `signal` field of a readiness wire line, or null if it doesn't parse
     *  (an unparsable pair then never cancels, conservative, like [variantOf]). */
    private fun readinessSignalOf(line: String, variant: String): String? = runCatching {
        json.parseToJsonElement(line)
            .jsonObject[variant]!!
            .jsonObject["signal"]!!
            .jsonPrimitive.content
    }.getOrNull()

    // ── Rule 3 helpers (lockstep with log.rs) ──────────────────

    /** Whether `clear` supersedes a [DeleteEntry] line by its `kind` (the one
     *  member whose family is line-dependent, so not in the static [families]). */
    private fun memberOfClear(clear: String, variant: String?, line: String): Boolean =
        variant == "DeleteEntry" && when (entryKindOf(line)) {
            "Set" -> clear == "ClearSets"
            "Run" -> clear == "ClearRuns"
            else -> false
        }

    /** A single-key struct variant's payload object, or null if it doesn't parse. */
    private fun payloadOf(line: String, variant: String): JsonObject? = runCatching {
        json.parseToJsonElement(line).jsonObject[variant]!!.jsonObject
    }.getOrNull()

    /** A Long field of a struct-variant payload, or 0 when absent/unparsable
     *  (matches serde `#[serde(default)]` on the Rust side). */
    private fun longField(line: String, variant: String, name: String): Long =
        payloadOf(line, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.long }.getOrNull()
        } ?: 0L

    /** A numeric field's value, or null when the key is absent / JSON null /
     *  non-numeric. Distinguishes "channel present" from "absent" for the Rule 4
     *  per-channel window, mirrors the Rust `Option<..>` extracts (a None field is
     *  omitted from the wire by the shell, so an absent key means the channel is
     *  not carried). */
    private fun doubleFieldOrNull(line: String, variant: String, name: String): Double? =
        payloadOf(line, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.doubleOrNull }.getOrNull()
        }

    /** The OLD-identity timestamp an AmendSet/AmendRun targets: observed_at_fallback
     *  when nonzero (a re-dated legacy row), else observed_at. Mirrors log.rs. */
    private fun amendMatchTime(line: String, variant: String): Long {
        val fb = longField(line, variant, "observed_at_fallback")
        return if (fb != 0L) fb else longField(line, variant, "observed_at")
    }

    /** The `kind` of a DeleteEntry line ("Set"/"Run"), or null if unparsable. */
    private fun entryKindOf(line: String): String? = runCatching {
        payloadOf(line, "DeleteEntry")?.get("kind")?.jsonPrimitive?.content
    }.getOrNull()

    /** Whether a variant is an AmendSet/AmendRun (an edit of an existing entry), as
     *  opposed to an original log line. Mirrors log.rs is_amend_line: Rule 3 keeps a
     *  base log alive for a surviving amend but telescopes away a superseded prior
     *  amend, and a delete walks the amend chain back to the base log. */
    private fun isAmendLine(variant: String?): Boolean =
        variant == "AmendSet" || variant == "AmendRun"

    /** A set/run log-or-amend line's `(family, entryId, observedAt)` identity, or
     *  null if it is not one (then it never matches, conservative). */
    private fun entryIdentityOf(line: String, variant: String?): Triple<Int, Long, Long>? = when (variant) {
        "LogSet", "AmendSet" -> Triple(1, longField(line, variant, "entry_id"), longField(line, variant, "observed_at"))
        "LogRun", "LogRunTrack", "AmendRun" ->
            Triple(2, longField(line, variant, "entry_id"), longField(line, variant, "observed_at"))
        else -> null
    }

    /** Whether `line` is the log/amend line a delete/amend targeting
     *  `(fam, id, time)` matches, same family, same id (nonzero) or same
     *  observed_at for a legacy (id 0) row. Mirrors log.rs entry_line_matches. */
    private fun entryLineMatches(line: String, variant: String?, fam: Int, id: Long, time: Long): Boolean {
        val (f, cid, ctime) = entryIdentityOf(line, variant) ?: return false
        if (f != fam) return false
        return if (id != 0L) cid == id else cid == 0L && ctime == time
    }

    /** Whether any run-family log/amend line sits strictly between `i` and `j`
     *  (the run spike-baseline safety guard). Mirrors log.rs run_line_between. */
    private fun runLineBetween(lines: List<String>, variant: List<String?>, i: Int, j: Int): Boolean {
        for (k in i + 1 until j) {
            if (entryIdentityOf(lines[k], variant[k])?.first == 2) return true
        }
        return false
    }
}
