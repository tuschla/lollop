package app.milestone

import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long

/**
 * Result of [EventLog.load] / [EventLog.read]: the surviving replayable lines plus
 * whether this is a genuinely fresh install. [freshInstall] is true ONLY when the
 * log file never existed, a log that exists but compacted to zero surviving lines
 * (the user cleared all their data) is a RETURNING user, and the caller must not
 * re-seed the onboarding profile over their deliberate empty state. [maxEntryId]
 * is the largest `entry_id` any intact line carries (0 when none).
 */
data class RestoredLog(val lines: List<String>, val freshInstall: Boolean, val maxEntryId: Long = 0L)

/**
 * Event-log compaction over raw serde wire lines. Lives outside [Core] (which
 * loads `libshared.so` in its initializer) so it stays pure Kotlin and is unit
 * testable on the JVM without a device or the native library.
 *
 * Mirrors the authoritative Rust implementation `compact_event_log`
 * (shared/src/log.rs), whose
 * replay-equivalence tests pin the rules. That function isn't exposed over the
 * JSON FFI, so this reimplements it over raw wire lines; the two must stay in
 * lockstep: a new `Event` family added there must be added here.
 */
object EventLog {
    private val json = Json { ignoreUnknownKeys = true }

    /** Clear-variant → the member variants its clear supersedes. `DeleteEntry`
     *  belongs to ClearSets, ClearRuns or ClearActivities depending on its `kind`, so it is NOT
     *  listed statically here: [memberOfClear] resolves it per-line. The review
     *  family is NOT here: a ClearReview / ClearWeeklyCheckin leaves the core's
     *  weekly history intact, so it never sweeps a submit (see the review pass in [compact]). */
    private val families = mapOf(
        "ClearReadiness" to setOf("SubmitReadiness", "RemoveReadiness", "ResolveHold"),
        "ClearSets" to setOf("LogSet", "AmendSet", "SetExerciseEnded", "SetCalisthenicsRung"),
        "ClearRuns" to setOf("LogRun", "LogRunTrack", "AmendRun", "TrimRunTrack", "AmendRunMeta"),
        "ClearProfile" to setOf("SetProfile"),
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
        "ClearCheckins" to setOf("SubmitCheckin", "RestartHrvBaseline"),
        // Family 13: the accepted plan request, with a ClearPlan reset. It is
        // keep-FIRST, NOT last-write-wins (see the singletons list and the
        // keep-first pass in compact, which only touches GeneratePlan). A
        // MoveSession joins the family: every move survives, ClearPlan sweeps
        // them. Lockstep with log.rs classify.
        "ClearPlan" to setOf("GeneratePlan", "MoveSession"),
        // Family 15: other activity minutes (youth weekly activity card). A
        // DeleteEntry with kind Activity joins it via memberOfClear. Lockstep
        // with log.rs classify.
        "ClearActivities" to setOf("LogActivity"),
    )

    /** Last-write-wins singleton variants (assign a scalar model field outright).
     *  SetProfile is NOT here: it is history-keyed (a dedicated eff-death pass in
     *  compact mirrors update()'s profile_history reducer). SubmitReview is NOT
     *  here either: every review is a review-history entry. Lockstep with log.rs. */
    private val singletons = listOf(
        "PredictRace", "PlanHypertrophyMeso", "ComputeProtein",
        "ComputeHrZones", "ComputeCooper", "ComputeCriticalSpeed", "ComputeApre",
        // SetToday (family 14, the shell's clock sent every foreground; keep
        // exactly one line). Family 13 (GeneratePlan) is NOT here: it is keep-
        // FIRST, handled by the dedicated pass in compact.
        "SetToday",
    )

    /** Event variants that serialize as a BARE JSON string (Rust unit variants: no
     *  payload). A named object such as {"ClearSets":{}} is NOT one of these — the
     *  core rejects a unit variant carrying a payload at serde — so it must classify
     *  as no variant here, or it could act as a family clear the core would reject. */
    private val unitVariants = setOf(
        "ClearReadiness", "ClearSets", "ClearRuns", "ClearCheckins", "ClearProfile",
        "ClearReview", "ClearWeeklyCheckin", "ClearRacePrediction", "ClearHypertrophyPlan", "ClearProtein",
        "ClearHrZones", "ClearCooper", "ClearCriticalSpeed", "ClearApre", "ClearPlan", "ClearActivities",
    )

    /** Event variants that serialize as a SINGLE-KEY OBJECT ({"LogSet":{…}}): every
     *  struct/newtype variant carries an object payload. A known name with a
     *  non-object payload ({"LogSet":5}) is a shape the core rejects at serde, so it
     *  names no event here. SetExerciseEnded is a known variant for import validation.
     *  Lockstep with the Rust Event enum. */
    private val structVariants = setOf(
        "SubmitReadiness", "RemoveReadiness", "ResolveHold", "SubmitCheckin", "RestartHrvBaseline", "LogSet", "AmendSet",
        "LogRun", "LogRunTrack", "AmendRun", "TrimRunTrack", "AmendRunMeta", "DeleteEntry",
        "SetProfile", "SubmitReview", "SubmitWeeklyCheckin", "PredictRace", "PlanHypertrophyMeso",
        "ComputeProtein", "ComputeHrZones", "ComputeCooper", "ComputeCriticalSpeed",
        "ComputeApre", "GeneratePlan", "SetToday", "SetExerciseEnded", "SetCalisthenicsRung",
        "MoveSession", "LogActivity",
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
     * The compaction rules (0, 1, 2, 2b, 3, 4) are all grounded in the core's `update` (app/mod.rs): the model stores
     * only raw inputs and derives everything in `view`, and no event's `update`
     * reads another family's state.
     *  0. **An undated RemoveReadiness cancels its submit; a dated one keeps it.**
     *     A dated remove (observed_at > 0) retracts the input, which stays in the
     *     core for the past days it shaped, so both lines survive.
     *  1. **Clear supersedes its family.** A `Clear<F>` empties family F's vec, so
     *     every F event (and the clear) at or before the *last* `Clear<F>` leaves
     *     no residue, F events after it replay against an already-empty vec. The
     *     review family is the exception (rule 2).
     *  2. **Last write wins for singletons; reviews are a history.** The
     *     calculators assign a scalar model field outright, and nothing else reads
     *     it at update time, so only the last surviving one matters. Every
     *     `SubmitReview` / `SubmitWeeklyCheckin` survives (the core keeps weekly
     *     check-ins as a history that dates past days' deload/downgrade windows);
     *     a `ClearReview` / `ClearWeeklyCheckin` is dropped only when a later line
     *     replaces the same store anyway.
     *  4. **Check-ins age out of a trailing window, PER CHANNEL.** The core windows
     *     EACH channel (wellness / HRV / resting-HR) on that channel's OWN newest
     *     reading, so the cutoff anchors on the MIN over channels present of that
     *     channel's newest reading. A `SubmitCheckin` more than [RETAIN_CHECKIN_DAYS]
     *     before that anchor is out of window for every channel it carries, so it is
     *     dropped, deterministic, no clock. Family 0 readiness is never windowed: a
     *     safety hold lives until explicitly cleared.
     */
    fun compact(lines: List<String>): List<String> =
        compact(lines, lines.map { runCatching { json.parseToJsonElement(it) }.getOrNull() })

    /** Core compaction over lines already parsed once into [elems] (null = a torn
     *  line load() already dropped). Parsing once and threading the elements keeps
     *  a large LogRunTrack line from being re-parsed by the variant + field reads. */
    private fun compact(lines: List<String>, elems: List<JsonElement?>): List<String> {
        val variant = elems.map(::variantOfElement)
        val remove = BooleanArray(lines.size)

        // Per-line identity cache. [entryIdentityOf] re-parses a whole wire line to
        // read its (family, entry_id, observed_at); a LogRunTrack line carries the
        // entire GPS track, so that parse is O(track size). Rule 3's delete/amend
        // chain-walk below is O(n²) in line count and, uncached, re-parsed each visited
        // line on every pass — quadratic in the largest track on a cold-start
        // compaction. Memoize: parse each line's identity at most once, lazily (an
        // append-only log with no deletes/amends/trims never triggers a parse). Output
        // stays unchanged: an identity is a pure function of its line.
        val identityCached = BooleanArray(lines.size)
        val identityCache = arrayOfNulls<Triple<Int, Long, Long>>(lines.size)
        fun identityAt(k: Int): Triple<Int, Long, Long>? {
            if (!identityCached[k]) {
                identityCache[k] = entryIdentityOf(elems[k], variant[k])
                identityCached[k] = true
            }
            return identityCache[k]
        }

        // Rule 0 (mirrors log.rs): an UNDATED RemoveReadiness cancels the latest
        // active prior SubmitReadiness with the same signal (the core deletes
        // it). A DATED remove (observed_at > 0) retracts that input instead, so
        // both lines survive and the submit is consumed: a later remove pairs
        // with the next older one, as the core does. An unmatched remove of
        // either kind replays as a no-op and is dropped alone.
        val consumed = BooleanArray(lines.size)
        for (j in lines.indices) {
            // ResolveHold (mirrors log.rs): a dated one retracts EVERY active prior
            // submit of its signal (all lines survive, all consumed); an undated or
            // unmatched one replays as a no-op and is dropped alone.
            if (variant[j] == "ResolveHold") {
                val rs = readinessSignalOf(elems[j], "ResolveHold") ?: continue
                val dated = longField(elems[j], "ResolveHold", "observed_at") > 0L
                var any = false
                for (k in 0 until j) {
                    if (!remove[k] && !consumed[k] && variant[k] == "SubmitReadiness" &&
                        readinessSignalOf(elems[k], "SubmitReadiness") == rs
                    ) {
                        any = true
                        if (dated) consumed[k] = true
                    }
                }
                if (!any || !dated) remove[j] = true
                continue
            }
            if (variant[j] != "RemoveReadiness") continue
            // Unparsable line → never removable, same contract as variantOf.
            val signal = readinessSignalOf(elems[j], "RemoveReadiness") ?: continue
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && !consumed[it] && variant[it] == "SubmitReadiness" &&
                    readinessSignalOf(elems[it], "SubmitReadiness") == signal
            }
            when {
                i == null -> remove[j] = true
                longField(elems[j], "RemoveReadiness", "observed_at") > 0L -> consumed[i] = true
                else -> {
                    remove[i] = true
                    remove[j] = true
                }
            }
        }

        // Rule 3 (mirrors log.rs): a DeleteEntry cancels its entry; an AmendSet/
        // AmendRun supersedes a prior edit but KEEPS the entry's base log. Newest
        // match, id-first (observed_at fallback for a legacy row), the same
        // predicate the core's find_set/find_run use.
        //
        // The core's amend is a STRICT update (replace-on-
        // match, no-op on miss), so a lone amend with no base row replays to
        // nothing. Two consequences, both replay-equivalent: a matched amend
        // telescopes away a superseded prior amend but keeps the base log (so the
        // strict amend has a row to replace); a delete removes the WHOLE entry (its
        // newest match and, when that is an amend, the base chain down to the log)
        // plus itself. An unmatched delete OR amend is a no-op → dropped alone.
        // A RUN removal is skipped when ANOTHER run line (one NOT in the delete's
        // own removal chain) sits between the earliest removed line and this
        // delete/amend (a run bakes its spike baseline from the runs present when
        // logged, so removing it could change a later run's baseline). The delete's
        // own chain (its base LogRunTrack and its trims) is removed together, so
        // those lines cannot bake any survivor's baseline and are skipped by
        // runLineBetween; a set carries no such baked state.
        //
        // INVARIANT (lockstep with log.rs): Rule 3's chain-walk correctness
        // depends on legacy (entry_id 0) rows NEVER being re-dated: the walk keys a
        // whole set/run chain by ONE (fam, id, observed_at) match key, valid only
        // while a legacy row's observed_at is immutable. THIS SHELL enforces it:
        // the log editors (setEditEvent / runEditEvent) pin observed_at == observed_at_fallback for id-0 rows,
        // blocking re-dating. If that pin is lifted, a re-dated legacy
        // Log→Amend→Amend reverts to the original value and a re-dated
        // Log→Amend→Delete resurrects the pre-edit row on the next launch.
        for (j in lines.indices) {
            val v = variant[j] ?: continue
            // An invalid AmendSet (fails update's set gate) replays as a no-op, like
            // an unmatched amend: drop it alone before the telescope search so it
            // cannot collapse a valid prior amend (mirrors log.rs).
            if (v == "AmendSet" && !setLineValid(elems[j], v)) {
                remove[j] = true
                continue
            }
            val isDelete = v == "DeleteEntry"
            val fam: Int
            val id: Long
            val time: Long
            when (v) {
                "DeleteEntry" -> {
                    fam = when (entryKindOf(elems[j])) { "Set" -> 1; "Run" -> 2; "Activity" -> 15; else -> continue }
                    id = entryIdField(elems[j], v)
                    time = longField(elems[j], v, "observed_at_fallback")
                }
                // Match key must mirror log.rs exactly: an amend targets the OLD
                // identity, so use observed_at_fallback when nonzero (a re-dated
                // legacy row), else observed_at.
                "AmendSet" -> { fam = 1; id = entryIdField(elems[j], v); time = amendMatchTime(elems[j], v) }
                "AmendRun" -> { fam = 2; id = entryIdField(elems[j], v); time = amendMatchTime(elems[j], v) }
                else -> continue
            }
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && entryIdMatches(identityAt(it), fam, id, time)
            }
            if (i == null) {
                // Unmatched delete OR amend: a no-op on replay (strict amend) → drop alone.
                remove[j] = true
                continue
            }
            if (isDelete) {
                // Collect the entry: the newest match, and, when that is an amend
                // OR a trim, its base chain back through and INCLUDING the first log
                // line (so a run's base LogRunTrack and all its trims go together).
                // A bare (shared-id) log stops at itself, mirroring update's
                // newest-only row removal. removal is built descending, so its last
                // element is the earliest index.
                val removal = mutableListOf(i)
                while (isAmendLine(variant[removal.last()]) || isTrimLine(variant[removal.last()])) {
                    val last = removal.last()
                    val p = (last - 1 downTo 0).firstOrNull {
                        !remove[it] && entryIdMatches(identityAt(it), fam, id, time)
                    } ?: break
                    removal.add(p)
                }
                val earliest = removal.last()
                if (fam != 2 || !runLineBetween(earliest, j, skip = removal) { identityAt(it) }) {
                    removal.forEach { remove[it] = true }
                    remove[j] = true
                }
                // else: baseline-unsafe → keep the entry and the delete.
            } else {
                // Amend: telescope away a superseded prior amend, but keep the base
                // log line (the strict amend needs a row to replace on replay).
                if (isAmendLine(variant[i]) && (fam == 1 || !runLineBetween(i, j) { identityAt(it) })) {
                    remove[i] = true
                }
            }
        }

        for ((clear, members) in families) {
            val lastClear = variant.indexOfLast { it == clear }
            if (lastClear < 0) continue
            for (i in 0..lastClear) {
                if (variant[i] == clear || variant[i] in members ||
                    memberOfClear(clear, variant[i], elems[i])
                ) {
                    remove[i] = true
                }
            }
        }
        for (singleton in singletons) {
            val survivors = lines.indices.filter { !remove[it] && variant[it] == singleton }
            survivors.dropLast(1).forEach { remove[it] = true }
        }

        // Rule 2 (reviews, mirrors log.rs): every SubmitReview / SubmitWeeklyCheckin
        // survives (weekly check-ins are a history). A ClearReview only empties the
        // session review, so it is inert when a later ClearReview or a later
        // SubmitReview that sets the session review follows; a ClearWeeklyCheckin
        // only empties the weekly check-in, so it is inert when a later
        // ClearWeeklyCheckin or SubmitWeeklyCheckin follows. (log.rs also treats a
        // later weekly-carrying SubmitReview as replacing it; keeping the clear
        // here is the conservative, still replay-equivalent choice.)
        run {
            for (j in lines.indices) {
                val inert = when (variant[j]) {
                    "ClearReview" -> (j + 1 until lines.size).any {
                        variant[it] == "ClearReview" ||
                            (variant[it] == "SubmitReview" && reviewSetsSession(elems[it]))
                    }
                    "ClearWeeklyCheckin" -> (j + 1 until lines.size).any {
                        variant[it] == "ClearWeeklyCheckin" || variant[it] == "SubmitWeeklyCheckin"
                    }
                    else -> false
                }
                if (inert) remove[j] = true
            }
        }

        // Rule 2 (profile history, mirrors log.rs): a SetProfile line is dead iff a
        // LATER surviving SetProfile has eff <= its eff (None = -inf). Mirrors
        // update()'s profile_history reducer, so survivors are exactly the entries
        // that remain after replay — NOT a plain keep-last singleton; a run of
        // ascending-dated profiles all survive, a None-dated edit replaces the whole
        // history. The last line is never dead, so model.profile is always kept.
        run {
            val profiles = lines.indices.filter { !remove[it] && variant[it] == "SetProfile" }
            fun effAt(j: Int): Long? =
                longFieldOrNull(elems[j], "SetProfile", "effective_from_epoch_day")
            for ((a, j) in profiles.withIndex()) {
                val effJ = effAt(j)
                if (profiles.subList(a + 1, profiles.size).any { k -> leEff(effAt(k), effJ) }) {
                    remove[j] = true
                }
            }
        }

        // Mirrors log.rs: SetExerciseEnded last-write-wins per EXACT key
        // (exercise.trim().lowercase(), epoch_day). Family 1, so the clear pass above
        // already dropped markers at/before the last ClearSets; only survivors count.
        run {
            val lastEnded = HashMap<Pair<String, Long>, Int>()
            for (j in lines.indices) {
                if (remove[j] || variant[j] != "SetExerciseEnded") continue
                val ex = (stringField(elems[j], "SetExerciseEnded", "exercise") ?: "")
                    .trim().lowercase()
                val day = longField(elems[j], "SetExerciseEnded", "epoch_day")
                lastEnded.put(ex to day, j)?.let { remove[it] = true }
            }
        }

        // Rule 2b (mirrors log.rs): family 13 (GeneratePlan) is keep-FIRST, not
        // last-write-wins. update()'s GeneratePlan arm is first-write-wins - it
        // preserves the ORIGINAL plan anchor and treats every later request as a
        // no-op once plan_request is set. The shell auto-fires GeneratePlan every
        // launch, so a last-write-wins compaction would re-date the plan and
        // irreversibly lose the anchor. Rule 1 (the clear pass above) already
        // dropped everything at/before the last ClearPlan, so the first surviving
        // request is the anchor-defining line; keep it and drop the rest. See
        // app/mod.rs Event::GeneratePlan.
        run {
            val survivors = lines.indices.filter { !remove[it] && variant[it] == "GeneratePlan" }
            survivors.drop(1).forEach { remove[it] = true }
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
            val at = longField(elems[j], "SubmitCheckin", "observed_at")
            // per_day_series skips observed_at <= 0: such readings anchor nothing.
            if (at <= 0L) continue
            val hasWellness =
                doubleFieldOrNull(elems[j], "SubmitCheckin", "sleep_quality") != null ||
                    doubleFieldOrNull(elems[j], "SubmitCheckin", "soreness") != null ||
                    doubleFieldOrNull(elems[j], "SubmitCheckin", "fatigue") != null ||
                    doubleFieldOrNull(elems[j], "SubmitCheckin", "stress") != null ||
                    doubleFieldOrNull(elems[j], "SubmitCheckin", "mood") != null
            if (hasWellness) wellnessNewest = maxOf(wellnessNewest ?: at, at)
            val rhr = doubleFieldOrNull(elems[j], "SubmitCheckin", "resting_hr_bpm")
            if (rhr != null && rhr > 0.0) rhrNewest = maxOf(rhrNewest ?: at, at)
            val hrv = doubleFieldOrNull(elems[j], "SubmitCheckin", "hrv_rmssd_ms")
            if (hrv != null && hrv > 0.0) hrvNewest = maxOf(hrvNewest ?: at, at)
        }
        val anchor = listOfNotNull(wellnessNewest, hrvNewest, rhrNewest).minOrNull()
        if (anchor != null) {
            val cutoff = anchor - RETAIN_CHECKIN_DAYS * CHECKIN_DAY_SEC
            for (j in lines.indices) {
                if (variant[j] == "SubmitCheckin" &&
                    longField(elems[j], "SubmitCheckin", "observed_at") < cutoff
                ) {
                    remove[j] = true
                }
            }
        }
        return lines.filterIndexed { i, _ -> !remove[i] }
    }

    /**
     * Read + compact the persisted event log, rewriting [file] in place when
     * either compaction dropped enough lines to be worth the fsync (the ordinary
     * threshold case) OR the durable tail is torn — an unparseable line or a
     * newline-less last line left by a process kill mid-append. The
     * rewrite is atomic (tmp-file + rename) and always terminates the file with a
     * newline, so a torn tail self-heals instead of the next append gluing its
     * JSON onto the partial bytes (which would make that event unparseable, and
     * lost, on the next boot). Pure file/JSON logic, no native library, so the
     * fresh-install-vs-compacted-empty distinction is unit testable on the JVM
     * (see EventLogTest); [Core.restore] replays the returned survivors regardless
     * of whether the file was rewritten.
     *
     * Mutates the log directory, so it runs only at boot, before anything appends.
     * Anything reading the log while the core may be appending uses [read].
     */
    fun load(file: File): RestoredLog {
        // A crash between the two renames of a non-atomic import swap can leave the live
        // log moved aside to .bak with the destination missing; restore it before treating
        // the install as fresh. Any leftover swap/import temporaries (.tmp/.import/.bak) are
        // stale the moment no operation is in flight, so drop them.
        val bak = File(file.parentFile, file.name + ".bak")
        if (!file.exists() && bak.exists()) bak.renameTo(file)
        File(file.parentFile, file.name + ".tmp").delete()
        File(file.parentFile, file.name + ".import").delete()
        bak.delete()
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        val parsed = parse(file)
        val kept = parsed.restored.lines
        val dropped = parsed.total - kept.size
        // Rewrite the durable log when compaction bought enough to justify the
        // fsync ([compactionWorthRewriting]) OR the tail is torn: an unparseable
        // line, or a newline-less last line from a kill mid-append. A launch that
        // shaves a line or two (one superseded SetToday, one aged-out check-in)
        // replays the result in memory and leaves disk untouched, so a steady-state
        // log isn't rewritten on every boot. A torn tail always forces the rewrite
        // so the atomic write's trailing newline heals it, or the next append glues
        // its JSON onto the partial bytes and loses that event on the next boot.
        val torn = parsed.unparseable || !endsWithNewline(file)
        if (torn || compactionWorthRewriting(dropped, parsed.total)) {
            val tmp = File(file.parentFile, file.name + ".tmp")
            runCatching {
                // fsync the bytes to disk BEFORE the rename: a rename-replace
                // without fsync can commit the directory metadata (the new name)
                // before the file's data reaches disk, so a power loss right after
                // the rename can truncate the WHOLE log to zero/garbage.
                FileOutputStream(tmp).use {
                    it.write(kept.joinToString("\n", postfix = "\n").toByteArray())
                    it.fd.sync()
                }
                if (!tmp.renameTo(file)) tmp.delete()
            }
        }
        return parsed.restored
    }

    /** A compaction drop at least this many lines long is always worth a rewrite. */
    internal const val REWRITE_MIN_DROPPED_LINES = 32

    /** …as is one of at least 1/[REWRITE_MIN_DROPPED_SHARE_INVERSE] (5%) of the log. */
    internal const val REWRITE_MIN_DROPPED_SHARE_INVERSE = 20

    /** Whether dropping [dropped] of [total] lines justifies rewriting (and fsyncing)
     *  the durable log: a large absolute drop, or a large share of it. */
    internal fun compactionWorthRewriting(dropped: Int, total: Int): Boolean =
        dropped > 0 &&
            (dropped >= REWRITE_MIN_DROPPED_LINES || dropped * REWRITE_MIN_DROPPED_SHARE_INVERSE >= total)

    /**
     * Read + compact the event log WITHOUT touching the disk: no temporaries are
     * removed and nothing is rewritten, so it is safe while the core may append (a
     * half-written tail line just fails to parse and is left out of the result).
     * The data export and the mid-session panic-reset replay read through this.
     */
    fun read(file: File): RestoredLog {
        if (!file.exists()) return RestoredLog(emptyList(), freshInstall = true)
        return parse(file).restored
    }

    /** [read]'s result plus what [load] needs to decide on a rewrite: the count of
     *  non-blank lines on disk and whether any of them failed to parse. */
    private class Parsed(val restored: RestoredLog, val total: Int, val unparseable: Boolean)

    private fun parse(file: File): Parsed {
        val lines = file.readLines().filter { it.isNotBlank() }
        // Parse each surviving line exactly ONCE and thread the elements through
        // compaction, so a large LogRunTrack line isn't re-parsed by the variant +
        // field reads. A torn mid-append line fails to parse (element null) and is
        // dropped; a parseable line whose variant this build doesn't recognize DOES
        // parse and is kept — a future event a newer build wrote, which compaction's
        // variant reads leave be (forward-compat).
        val parsed = lines.map { runCatching { json.parseToJsonElement(it) }.getOrNull() }
        val intact = ArrayList<String>(lines.size)
        val intactEls = ArrayList<JsonElement?>(lines.size)
        var maxEntryId = 0L
        for (i in lines.indices) {
            val el = parsed[i] ?: continue
            intact.add(lines[i])
            intactEls.add(el)
            val v = variantOfElement(el)
            if (v != null) maxEntryId = maxOf(maxEntryId, entryIdField(el, v))
        }
        val kept = compact(intact, intactEls)
        return Parsed(
            RestoredLog(kept, freshInstall = false, maxEntryId = maxEntryId),
            total = lines.size,
            unparseable = intact.size < lines.size,
        )
    }

    /** Whether [file]'s durable bytes end in '\n'. An empty file counts as
     *  terminated (nothing to glue onto); otherwise the last byte is read
     *  directly so a newline-less tail from a kill mid-append forces the rewrite. */
    internal fun endsWithNewline(file: File): Boolean =
        RandomAccessFile(file, "r").use {
            if (it.length() == 0L) true else { it.seek(it.length() - 1); it.read() == '\n'.code }
        }

    /**
     * The serde variant tag of one wire line, or null if it doesn't parse (a
     * hand-edited or forward-incompatible line is then never removable). Unit
     * variants serialize as a bare JSON string (`"ClearRuns"`); struct/newtype
     * variants as a single-key object (`{"LogRun": …}`).
     */
    fun variantOf(line: String): String? =
        variantOfElement(runCatching { json.parseToJsonElement(line) }.getOrNull())

    /** The variant tag of an already-parsed element (null when it isn't a wire
     *  event). Compaction reads this off the once-parsed element so a large
     *  LogRunTrack line is never re-parsed just to read its tag. */
    private fun variantOfElement(el: JsonElement?): String? = when (el) {
        // A unit variant is a bare JSON string; a string that is not a known unit
        // variant is a shape the core rejects, so it names no event.
        is JsonPrimitive -> if (el.isString && el.content in unitVariants) el.content else null
        // A struct variant is a single-key object whose VALUE is the payload object.
        // A known name with a non-object payload — {"ClearSets":{}} (a unit variant
        // given a body) or {"LogSet":5} — is a serde-reject shape, so it names no
        // event and can never act as a family clear/member in compaction: Kotlin
        // must never interpret a line the core would reject.
        is JsonObject -> el.keys.singleOrNull()?.takeIf { it in structVariants && el[it] is JsonObject }
        else -> null
    }

    /** The `signal` field of a readiness wire line, or null if it doesn't parse
     *  (an unparsable pair then never cancels, conservative, like [variantOf]). */
    private fun readinessSignalOf(el: JsonElement?, variant: String): String? = runCatching {
        (el as JsonObject)[variant]!!.jsonObject["signal"]!!.jsonPrimitive.content
    }.getOrNull()

    // ── Rule 3 helpers (lockstep with log.rs) ──────────────────

    /** Whether `clear` supersedes a [DeleteEntry] line by its `kind` (the one
     *  member whose family is line-dependent, so not in the static [families]). */
    private fun memberOfClear(clear: String, variant: String?, el: JsonElement?): Boolean =
        variant == "DeleteEntry" && when (entryKindOf(el)) {
            "Set" -> clear == "ClearSets"
            "Run" -> clear == "ClearRuns"
            "Activity" -> clear == "ClearActivities"
            else -> false
        }

    /** A single-key struct variant's payload object, or null if it doesn't parse. */
    private fun payloadOf(el: JsonElement?, variant: String): JsonObject? = runCatching {
        (el as JsonObject)[variant]!!.jsonObject
    }.getOrNull()

    /** A Long field of a struct-variant payload, or 0 when absent/unparsable
     *  (matches serde `#[serde(default)]` on the Rust side). */
    private fun longField(el: JsonElement?, variant: String, name: String): Long =
        payloadOf(el, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.long }.getOrNull()
        } ?: 0L

    /** A Long field of a struct-variant payload, or null when absent / JSON null /
     *  non-numeric — distinguishes a real 0 from an omitted field (an absent
     *  effective_from_epoch_day means None = -inf, NOT epoch day 0). */
    private fun longFieldOrNull(el: JsonElement?, variant: String, name: String): Long? =
        payloadOf(el, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.long }.getOrNull()
        }

    /** `eff(a) <= eff(b)` with null = -inf (undated profile), for the profile-history
     *  compaction pass. Mirrors log.rs le_eff. */
    private fun leEff(a: Long?, b: Long?): Boolean = when {
        a == null -> true
        b == null -> false
        else -> a <= b
    }

    /** Per-session review keys (lockstep with reviews.rs `take_session_fields`);
     *  every other key is a weekly/clinical field. */
    private val sessionReviewKeys = setOf(
        "single_session_spike_frac", "lift", "interval", "decoupling",
        "easy_frac_time_above_vt1", "positive_split_pct", "mcv_delta_m_s",
        "first_set_reps_met", "first_set_rpe_delta", "rep_drop_frac",
        "interval_reps_over_target", "can_hold_easy_pace_under_hr_cap",
        "hrv_unreliable_last_three", "planned_hard", "bad_day",
    )

    /** Whether a SubmitReview line replaces the core's session review (mirrors
     *  reviews.rs `submit_review_sets_session`): it carries a session field, or no
     *  weekly field. Errs to `false` (keep the earlier ClearReview) on anything it
     *  cannot read: a false `true` would drop a load-bearing clear. A weekly field
     *  counts as unset only when null, `false`, or a 0 `overtraining_signal_count`
     *  (the core's non-optional count); any other value marks the line weekly. */
    private fun reviewSetsSession(el: JsonElement?): Boolean {
        val p = payloadOf(el, "SubmitReview") ?: return false
        var session = false
        var weekly = false
        for ((k, v) in p) {
            if (k == "observed_at" || v is JsonNull) continue
            val prim = v as? JsonPrimitive
            if (k in sessionReviewKeys) {
                if (!(k == "bad_day" && prim?.booleanOrNull == false)) session = true
            } else {
                val unset = prim?.booleanOrNull == false ||
                    (k == "overtraining_signal_count" && prim?.doubleOrNull == 0.0)
                if (!unset) weekly = true
            }
        }
        return session || !weekly
    }

    /** A String field of a struct-variant payload, or null when absent / non-string. */
    private fun stringField(el: JsonElement?, variant: String, name: String): String? =
        payloadOf(el, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.content }.getOrNull()
        }

    /** An `entry_id` field under the same clamp the core applies on store and
     *  match (`sanitize_entry_id`): a u64 token above Long.MAX_VALUE names the row
     *  stored as Long.MAX_VALUE, so it must not read as 0 (the legacy-row id). */
    private fun entryIdField(el: JsonElement?, variant: String): Long =
        payloadOf(el, variant)?.get("entry_id")?.let {
            runCatching { it.jsonPrimitive.content }.getOrNull()?.let { s ->
                s.toLongOrNull() ?: if (s.isNotEmpty() && s.all(Char::isDigit)) Long.MAX_VALUE else 0L
            }
        } ?: 0L

    /** A numeric field's value, or null when the key is absent / JSON null /
     *  non-numeric. Distinguishes "channel present" from "absent" for the Rule 4
     *  per-channel window, mirrors the Rust `Option<..>` extracts (a None field is
     *  omitted from the wire by the shell, so an absent key means the channel is
     *  not carried). */
    private fun doubleFieldOrNull(el: JsonElement?, variant: String, name: String): Double? =
        payloadOf(el, variant)?.get(name)?.let {
            runCatching { it.jsonPrimitive.doubleOrNull }.getOrNull()
        }

    /** The OLD-identity timestamp an AmendSet/AmendRun targets: observed_at_fallback
     *  when nonzero (a re-dated legacy row), else observed_at. Mirrors log.rs. */
    private fun amendMatchTime(el: JsonElement?, variant: String): Long {
        val fb = longField(el, variant, "observed_at_fallback")
        return if (fb != 0L) fb else longField(el, variant, "observed_at")
    }

    /** The `kind` of a DeleteEntry line ("Set"/"Run"), or null if unparsable. */
    private fun entryKindOf(el: JsonElement?): String? = runCatching {
        payloadOf(el, "DeleteEntry")?.get("kind")?.jsonPrimitive?.content
    }.getOrNull()

    /** Whether a variant is an AmendSet/AmendRun (an edit of an existing entry), as
     *  opposed to an original log line. Mirrors log.rs is_amend_line: Rule 3 keeps a
     *  base log alive for a surviving amend but telescopes away a superseded prior
     *  amend, and a delete walks the amend chain back to the base log. */
    private fun isAmendLine(variant: String?): Boolean =
        variant == "AmendSet" || variant == "AmendRun"

    /** Whether a variant is a TrimRunTrack (a head/tail slice of a GPS run's
     *  track) or an AmendRunMeta (in-place rating/tag edit). Mirrors log.rs
     *  is_trim_line: such a line NEVER telescopes (every one survives
     *  compaction), but a delete's chain-walk treats it as a non-base line so the
     *  walk reaches and cancels the base log line. */
    private fun isTrimLine(variant: String?): Boolean =
        variant == "TrimRunTrack" || variant == "AmendRunMeta"

    /** Whether a LogSet/AmendSet passes update()'s set-validity gate
     *  ((weight>=0 || NaN) && (reps>0 || hold present)). update() drops an invalid set
     *  as a NO-OP, so compaction treats it the same: an invalid LogSet has no identity
     *  (never a delete/amend target) and an invalid AmendSet supersedes nothing.
     *  Non-set variants have no such gate and are always valid. Mirrors log.rs
     *  set_line_valid. */
    private fun setLineValid(el: JsonElement?, variant: String?): Boolean {
        if (variant != "LogSet" && variant != "AmendSet") return true
        val weight = doubleFieldOrNull(el, variant, "weight_kg") ?: 0.0
        val reps = longField(el, variant, "reps")
        val holdPresent = doubleFieldOrNull(el, variant, "hold_sec") != null
        return (weight >= 0.0 || weight.isNaN()) && (reps > 0L || holdPresent)
    }

    /** Whether a LogActivity passes update()'s minutes gate (finite, 0 < minutes
     *  ≤ 600). An invalid one replays as a no-op, so it has no identity. Mirrors
     *  log.rs. */
    private fun activityLineValid(el: JsonElement?): Boolean {
        val minutes = doubleFieldOrNull(el, "LogActivity", "minutes") ?: return false
        return minutes.isFinite() && minutes > 0.0 && minutes <= 600.0
    }

    /** A set/run log-or-amend line's `(family, entryId, observedAt)` identity, or
     *  null if it is not one (then it never matches, conservative). */
    private fun entryIdentityOf(el: JsonElement?, variant: String?): Triple<Int, Long, Long>? = when (variant) {
        "LogSet", "AmendSet" ->
            if (!setLineValid(el, variant)) null
            else Triple(1, entryIdField(el, variant), longField(el, variant, "observed_at"))
        "LogRun", "LogRunTrack", "AmendRun" ->
            Triple(2, entryIdField(el, variant), longField(el, variant, "observed_at"))
        "LogActivity" ->
            if (!activityLineValid(el)) null
            else Triple(15, entryIdField(el, variant), longField(el, variant, "observed_at"))
        // A trim / meta edit has no observed_at of its own; identity keys on the
        // run's observed_at carried as observed_at_fallback. Mirrors log.rs entry_identity.
        "TrimRunTrack", "AmendRunMeta" ->
            Triple(2, entryIdField(el, variant), longField(el, variant, "observed_at_fallback"))
        else -> null
    }

    /** Whether a precomputed set/run identity matches a delete/amend targeting
     *  `(fam, id, time)`: same family, same id (nonzero) or same observed_at for a
     *  legacy (id 0) row. Mirrors log.rs entry_line_matches. The identity comes from
     *  [compact]'s per-line cache, so a large LogRunTrack line is parsed once, not on
     *  every chain-walk visit. */
    private fun entryIdMatches(identity: Triple<Int, Long, Long>?, fam: Int, id: Long, time: Long): Boolean {
        val (f, cid, ctime) = identity ?: return false
        if (f != fam) return false
        return if (id != 0L) cid == id else cid == 0L && ctime == time
    }

    /** Whether any run-family log/amend line sits strictly between `i` and `j`,
     *  ignoring indices in `skip` (a delete's own removal chain: those lines are
     *  removed together with the base, so they cannot bake a survivor's
     *  baseline). A run bakes its spike baseline from the runs present when
     *  logged, so any OTHER run line in the window blocks the pair-drop (Rule 3).
     *  Mirrors log.rs run_line_between. */
    private fun runLineBetween(i: Int, j: Int, skip: List<Int> = emptyList(), identityOf: (Int) -> Triple<Int, Long, Long>?): Boolean {
        for (k in i + 1 until j) {
            if (k in skip) continue
            if (identityOf(k)?.first == 2) return true
        }
        return false
    }
}
