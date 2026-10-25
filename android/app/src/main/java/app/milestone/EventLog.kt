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
 * is the largest `entry_id` any intact line carries (0 when none). [planRemoved]
 * is true when the surviving log's last plan line is a `ClearPlan` (the user
 * removed their plan): compaction keeps that line, so boot can honour the
 * removal even when the setup prefs' marker was lost.
 */
data class RestoredLog(
    val lines: List<String>,
    val freshInstall: Boolean,
    val maxEntryId: Long = 0L,
    val planRemoved: Boolean = false,
)

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
     *  listed statically here: its [Facts.deleteClear] resolves it per-line. The review
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
        // DeleteEntry with kind Activity joins it via Facts.deleteClear. Lockstep
        // with log.rs classify.
        "ClearActivities" to setOf("LogActivity"),
    )

    /** Last-write-wins singleton variants (assign a scalar model field outright).
     *  SetProfile is NOT here: it is history-keyed (a dedicated key-death pass in
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

    /**
     * Drop log lines whose effect on the replayed model is provably nil, returning
     * a shorter but replay-equivalent line list (same relative order of survivors).
     *
     * The compaction rules (0, 1, 2, 2b, 3) are all grounded in the core's `update` (app/mod.rs): the model stores
     * only raw inputs and derives everything in `view`, and no event's `update`
     * reads another family's state, except for figures that are a pure function of
     * the surviving model and re-derived whenever their inputs change (a run's
     * typed-bpm %HRmax, its card spike baseline), which a dropped line cannot move.
     *  0. **An undated RemoveReadiness cancels its submit; a dated one keeps it.**
     *     A dated remove (observed_at > 0) retracts the input, which stays in the
     *     core for the past days it shaped, so both lines survive.
     *  1. **Clear supersedes its family.** A `Clear<F>` empties family F's vec, so
     *     every F event (and the clear) at or before the *last* `Clear<F>` leaves
     *     no residue, F events after it replay against an already-empty vec. The
     *     review family is the exception (rule 2); a last `ClearPlan` that no
     *     `GeneratePlan` follows is kept, as the record that the plan was removed.
     *  2. **Last write wins for singletons; reviews are a history.** The
     *     calculators assign a scalar model field outright, and nothing else reads
     *     it at update time, so only the last surviving one matters. Every
     *     `SubmitReview` / `SubmitWeeklyCheckin` survives (the core keeps weekly
     *     check-ins as a history that dates past days' deload/downgrade windows);
     *     a `ClearReview` / `ClearWeeklyCheckin` is dropped only when a later line
     *     replaces the same store anyway.
     *  Check-ins (family 12) are never windowed: the core reads a check-in whenever
     *  it is its channel's newest reading on or before the day judged, and a later
     *  SetToday can move today back to any reading's day, so only a ClearCheckins
     *  (rule 1) drops one. Family 0 readiness is never windowed either: a safety
     *  hold lives until explicitly cleared.
     */
    fun compact(lines: List<String>): List<String> {
        // Each line's JSON tree lives only while its facts are read off it.
        val remove = removals(lines.map { factsOf(runCatching { json.parseToJsonElement(it) }.getOrNull()) })
        return lines.filterIndexed { i, _ -> !remove[i] }
    }

    /**
     * What compaction (and [load]'s entry-id high-water mark) reads off one wire
     * line, taken from its JSON tree once so the tree can be dropped right after.
     * A LogRunTrack's tree is roughly 12–15× its bytes, so holding every line's
     * tree through compaction OOMs a multi-year GPS log in a phone heap
     * (PersistenceTortureTest.aFiveYearLogLoadsInTheMinimumPhoneHeap); these are a
     * few dozen bytes per line whatever the track size.
     */
    private class Facts(
        /** Serde variant tag; null = not a wire event (never removable). */
        val variant: String?,
        /** `entry_id` under the core's clamp, 0 when absent ([entryIdField]). */
        val entryId: Long = 0L,
        /** The `signal` of a SubmitReadiness / RemoveReadiness / ResolveHold. */
        val signal: String? = null,
        /** A RemoveReadiness / ResolveHold with `observed_at > 0`. */
        val dated: Boolean = false,
        /** A log / amend / trim line's `(family, entryId, observedAt)` ([entryIdentityOf]). */
        val identity: Triple<Int, Long, Long>? = null,
        /** The `(family, entryId, time)` a DeleteEntry / AmendSet / AmendRun targets. */
        val target: Triple<Int, Long, Long>? = null,
        /** False for an AmendSet that fails update's set gate ([setLineValid]). */
        val valid: Boolean = true,
        /** The family clear that sweeps a DeleteEntry, by its `kind`. */
        val deleteClear: String? = null,
        /** A SubmitReview that replaces the session review ([reviewSetsSession]). */
        val setsSession: Boolean = false,
        /** A SetProfile's history key ([profileKey]). */
        val profileKey: Pair<Long, Long>? = null,
        /** A SetProfile with no `effective_from_epoch_day` (None-dated). */
        val undatedProfile: Boolean = false,
        /** A SetExerciseEnded's `(exercise.trim().lowercase(), epoch_day)` key. */
        val endedKey: Pair<String, Long>? = null,
    )

    private val noFacts = Facts(variant = null)

    private fun factsOf(el: JsonElement?): Facts {
        val v = variantOfElement(el) ?: return noFacts
        val id = entryIdField(el, v)
        return when (v) {
            "SubmitReadiness" -> Facts(v, id, signal = readinessSignalOf(el, v))
            "RemoveReadiness", "ResolveHold" ->
                Facts(v, id, signal = readinessSignalOf(el, v), dated = longField(el, v, "observed_at") > 0L)
            "DeleteEntry" -> {
                val (fam, clear) = when (entryKindOf(el)) {
                    "Set" -> 1 to "ClearSets"
                    "Run" -> 2 to "ClearRuns"
                    "Activity" -> 15 to "ClearActivities"
                    else -> 0 to null
                }
                Facts(
                    v, id,
                    target = if (clear == null) null else Triple(fam, id, longField(el, v, "observed_at_fallback")),
                    deleteClear = clear,
                )
            }
            // Match key must mirror log.rs exactly: an amend targets the OLD
            // identity, so use observed_at_fallback when nonzero (a re-dated
            // legacy row), else observed_at.
            "AmendSet" -> Facts(
                v, id, identity = entryIdentityOf(el, v), target = Triple(1, id, amendMatchTime(el, v)),
                valid = setLineValid(el, v),
            )
            "AmendRun" -> Facts(v, id, identity = entryIdentityOf(el, v), target = Triple(2, id, amendMatchTime(el, v)))
            "SubmitReview" -> Facts(v, id, setsSession = reviewSetsSession(el))
            "SetProfile" -> Facts(
                v, id, profileKey = profileKey(el),
                undatedProfile = longFieldOrNull(el, v, "effective_from_epoch_day") == null,
            )
            "SetExerciseEnded" -> Facts(
                v, id,
                endedKey = (stringField(el, v, "exercise") ?: "").trim().lowercase() to longField(el, v, "epoch_day"),
            )
            else -> Facts(v, id, identity = entryIdentityOf(el, v))
        }
    }

    /** The compaction rules over per-line [facts]: true = the line is dropped. */
    private fun removals(facts: List<Facts>): BooleanArray {
        val n = facts.size
        val variant = facts.map { it.variant }
        val remove = BooleanArray(n)

        // Rule 0 (mirrors log.rs): an UNDATED RemoveReadiness cancels the latest
        // active prior SubmitReadiness with the same signal (the core deletes
        // it). A DATED remove (observed_at > 0) retracts that input instead, so
        // both lines survive and the submit is consumed: a later remove pairs
        // with the next older one, as the core does. An unmatched remove of
        // either kind replays as a no-op and is dropped alone.
        val consumed = BooleanArray(n)
        for (j in 0 until n) {
            // ResolveHold (mirrors log.rs): a dated one retracts EVERY active prior
            // submit of its signal (all lines survive, all consumed); an undated or
            // unmatched one replays as a no-op and is dropped alone.
            if (variant[j] == "ResolveHold") {
                val rs = facts[j].signal ?: continue
                val dated = facts[j].dated
                var any = false
                for (k in 0 until j) {
                    if (!remove[k] && !consumed[k] && variant[k] == "SubmitReadiness" && facts[k].signal == rs) {
                        any = true
                        if (dated) consumed[k] = true
                    }
                }
                if (!any || !dated) remove[j] = true
                continue
            }
            if (variant[j] != "RemoveReadiness") continue
            // Unparsable signal → never removable, same contract as variantOf.
            val signal = facts[j].signal ?: continue
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && !consumed[it] && variant[it] == "SubmitReadiness" && facts[it].signal == signal
            }
            when {
                i == null -> remove[j] = true
                facts[j].dated -> consumed[i] = true
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
        // No entry carries baked cross-entry state (a set's e1RM chain and a run's
        // card spike baseline are derived by the core from the surviving rows), so
        // removing one is exact.
        //
        // INVARIANT (lockstep with log.rs): Rule 3's chain-walk correctness
        // depends on legacy (entry_id 0) rows NEVER being re-dated: the walk keys a
        // whole set/run chain by ONE (fam, id, observed_at) match key, valid only
        // while a legacy row's observed_at is immutable. THIS SHELL enforces it:
        // the log editors (setEditEvent / runEditEvent) pin observed_at == observed_at_fallback for id-0 rows,
        // blocking re-dating. If that pin is lifted, a re-dated legacy
        // Log→Amend→Amend reverts to the original value and a re-dated
        // Log→Amend→Delete resurrects the pre-edit row on the next launch.
        for (j in 0 until n) {
            val v = variant[j] ?: continue
            // An invalid AmendSet (fails update's set gate) replays as a no-op, like
            // an unmatched amend: drop it alone before the telescope search so it
            // cannot collapse a valid prior amend (mirrors log.rs).
            if (v == "AmendSet" && !facts[j].valid) {
                remove[j] = true
                continue
            }
            if (v != "DeleteEntry" && v != "AmendSet" && v != "AmendRun") continue
            val (fam, id, time) = facts[j].target ?: continue
            val isDelete = v == "DeleteEntry"
            val i = (j - 1 downTo 0).firstOrNull {
                !remove[it] && entryIdMatches(facts[it].identity, fam, id, time)
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
                        !remove[it] && entryIdMatches(facts[it].identity, fam, id, time)
                    } ?: break
                    removal.add(p)
                }
                removal.forEach { remove[it] = true }
                remove[j] = true
            } else {
                // Amend: telescope away a superseded prior amend, but keep the base
                // log line (the strict amend needs a row to replace on replay).
                if (isAmendLine(variant[i])) {
                    remove[i] = true
                }
            }
        }

        for ((clear, members) in families) {
            val lastClear = variant.lastIndexOf(clear)
            if (lastClear < 0) continue
            for (i in 0..lastClear) {
                if (variant[i] == clear || variant[i] in members || facts[i].deleteClear == clear) {
                    remove[i] = true
                }
            }
        }
        // Rule 1's one kept clear (mirrors log.rs): the last ClearPlan survives when
        // no GeneratePlan follows it, because it is then the log's final plan state.
        // Replaying it is a no-op on the already-empty plan family, but the line is
        // the durable record that the user removed their plan: boot reads it
        // ([RestoredLog.planRemoved]) so a plan the user removed is not
        // auto-generated again when the setup prefs were lost.
        val lastClearPlan = variant.lastIndexOf("ClearPlan")
        if (lastClearPlan >= 0 && (lastClearPlan + 1 until n).none { variant[it] == "GeneratePlan" }) {
            remove[lastClearPlan] = false
        }
        for (singleton in singletons) {
            val survivors = (0 until n).filter { !remove[it] && variant[it] == singleton }
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
        for (j in 0 until n) {
            val inert = when (variant[j]) {
                "ClearReview" -> (j + 1 until n).any {
                    variant[it] == "ClearReview" || (variant[it] == "SubmitReview" && facts[it].setsSession)
                }
                "ClearWeeklyCheckin" -> (j + 1 until n).any {
                    variant[it] == "ClearWeeklyCheckin" || variant[it] == "SubmitWeeklyCheckin"
                }
                else -> false
            }
            if (inert) remove[j] = true
        }

        // Rule 2 (profile history, mirrors log.rs): a SetProfile line is dead iff a
        // LATER surviving SetProfile has a history key <= its own (profileKey:
        // effective day, then edited_at), with no pinning edit in between. Mirrors
        // update()'s profile_history reducer, so survivors are exactly the entries
        // that remain after replay — NOT a plain keep-last singleton; ascending
        // profiles (and later same-day edits) all survive, a None-dated edit
        // replaces the whole history. A None-dated edit also pins the %HRmax of
        // every run logged before it (read off the history then): once a run line
        // survives before it, no line at or after it kills an earlier one. The last
        // line is never dead, so model.profile is always kept.
        run {
            val firstRun = (0 until n).firstOrNull {
                !remove[it] && (variant[it] == "LogRun" || variant[it] == "LogRunTrack" || variant[it] == "AmendRun")
            }
            val profiles = (0 until n).filter { !remove[it] && variant[it] == "SetProfile" }
            for ((a, j) in profiles.withIndex()) {
                val keyJ = facts[j].profileKey!!
                for (k in profiles.subList(a + 1, profiles.size)) {
                    val pins = firstRun != null && firstRun < k && facts[k].undatedProfile
                    if (pins) break
                    if (leKey(facts[k].profileKey!!, keyJ)) {
                        remove[j] = true
                        break
                    }
                }
            }
        }

        // Mirrors log.rs: SetExerciseEnded last-write-wins per EXACT key
        // (exercise.trim().lowercase(), epoch_day). Family 1, so the clear pass above
        // already dropped markers at/before the last ClearSets; only survivors count.
        run {
            val lastEnded = HashMap<Pair<String, Long>, Int>()
            for (j in 0 until n) {
                if (remove[j] || variant[j] != "SetExerciseEnded") continue
                lastEnded.put(facts[j].endedKey!!, j)?.let { remove[it] = true }
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
            val survivors = (0 until n).filter { !remove[it] && variant[it] == "GeneratePlan" }
            survivors.drop(1).forEach { remove[it] = true }
        }
        return remove
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
        // shaves a line or two (one superseded SetToday, one superseded edit)
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
                // Streamed line by line: one joined copy of a multi-year log (and its
                // byte array) would double the boot's peak heap.
                FileOutputStream(tmp).use { out ->
                    val w = out.bufferedWriter()
                    if (kept.isEmpty()) w.write("\n")
                    for (line in kept) {
                        w.write(line)
                        w.write("\n")
                    }
                    w.flush()
                    out.fd.sync()
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
        // STREAMED, bounded memory: each line is parsed exactly once, its [Facts]
        // taken and its JSON tree dropped before the next line is read, so peak heap
        // is the line strings plus one line's tree (not every tree at once). A torn
        // mid-append line fails to parse and is dropped; a parseable line whose
        // variant this build doesn't recognize DOES parse and is kept — a future
        // event a newer build wrote, which compaction leaves be (forward-compat).
        val intact = ArrayList<String>()
        val facts = ArrayList<Facts>()
        var total = 0
        var maxEntryId = 0L
        file.bufferedReader().use { r ->
            while (true) {
                val line = r.readLine() ?: break
                if (line.isBlank()) continue
                total++
                val el = runCatching { json.parseToJsonElement(line) }.getOrNull() ?: continue
                val f = factsOf(el)
                intact.add(line)
                facts.add(f)
                maxEntryId = maxOf(maxEntryId, f.entryId)
            }
        }
        val remove = removals(facts)
        val kept = ArrayList<String>(intact.size)
        var lastGenerate = -1
        var lastClearPlan = -1
        for (i in intact.indices) {
            if (remove[i]) continue
            when (facts[i].variant) {
                "GeneratePlan" -> lastGenerate = i
                "ClearPlan" -> lastClearPlan = i
            }
            kept.add(intact[i])
        }
        return Parsed(
            RestoredLog(kept, freshInstall = false, maxEntryId = maxEntryId, planRemoved = lastClearPlan > lastGenerate),
            total = total,
            unparseable = intact.size < total,
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
     *  event). [factsOf] reads this off the line's once-parsed element. */
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

    /** A SetProfile line's profile-history key as the core orders it
     *  (app::history_key, mirrors log.rs profile_key): (effective day, then
     *  edited_at), null = -inf, compared lexicographically; an undated edit is
     *  (-inf, -inf). */
    private fun profileKey(el: JsonElement?): Pair<Long, Long> {
        val eff = longFieldOrNull(el, "SetProfile", "effective_from_epoch_day")
            ?: return Pair(Long.MIN_VALUE, Long.MIN_VALUE)
        return Pair(eff, longFieldOrNull(el, "SetProfile", "edited_at") ?: Long.MIN_VALUE)
    }

    private fun leKey(a: Pair<Long, Long>, b: Pair<Long, Long>): Boolean =
        a.first < b.first || (a.first == b.first && a.second <= b.second)

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
     *  non-numeric, mirroring the Rust `Option<..>` fields (a None field is
     *  omitted from the wire by the shell). */
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
     *  the line's [Facts], read when the line was parsed. */
    private fun entryIdMatches(identity: Triple<Int, Long, Long>?, fam: Int, id: Long, time: Long): Boolean {
        val (f, cid, ctime) = identity ?: return false
        if (f != fam) return false
        return if (id != 0L) cid == id else cid == 0L && ctime == time
    }
}
