package app.milestone

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit tests for [EventLog.compact], the Kotlin half of the compaction lockstep
 * with Rust `compact_event_log` (shared/src/log.rs). Lines are produced by the
 * real [Event.toJson] so a wire-shape drift would surface here too.
 */
class EventLogTest {

    private fun line(e: Event) = e.toJson().toString()

    // Pin observedAt + entryId so a helper called twice (input AND expected)
    // produces byte-identical lines: the LogSet/LogRun defaults are the wall
    // clock (seconds / millis), which would otherwise differ between calls.
    private fun set(exercise: String) = line(Event.LogSet(exercise, 100.0, 5, 8.0, observedAt = 0, entryId = 0))
    private fun run(km: Double) = line(Event.LogRun(km, km * 5.0, 75.0, 0.0, observedAt = 0, entryId = 0))
    private fun profile(weeklySets: Int) = line(
        Event.SetProfile(
            ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
            ConcurrentGoal.Strength, weeklySets, 4, 45.0, false, 75.0,
        )
    )
    private fun protein(bw: Double) = line(Event.ComputeProtein(bw, masters = false, deficit = true))

    // The Kotlin Event type has no variants for the dormant calculators yet, so
    // these lines are raw serde wire shapes (struct variants = single-key object,
    // unit variants = bare string) exactly as app.rs serializes them.
    private fun cooper(distanceM: Double) = """{"ComputeCooper":{"distance_m_12min":$distanceM}}"""
    private fun criticalSpeed(distanceM: Double) =
        """{"ComputeCriticalSpeed":{"efforts":[{"distance_m":$distanceM,"time_sec":720.0}]}}"""
    private fun apre(reps: Int) =
        """{"ComputeApre":{"scheme":"Apre6","reps":$reps,"current_load_lb":225.0}}"""
    private val clearCooper = "\"ClearCooper\""
    private val clearCriticalSpeed = "\"ClearCriticalSpeed\""
    private val clearApre = "\"ClearApre\""

    private fun readiness(signal: ReadinessSignal, value: Double = 1.0) =
        line(Event.SubmitReadiness(signal, value, observedAt = 0))

    private fun removeReadiness(signal: ReadinessSignal) =
        line(Event.RemoveReadiness(signal))

    @Test
    fun removeReadinessCancelsItsSubmitPairwise() {
        // Mirrors log.rs Rule 0: pain + its undo are inert; the wellness input
        // logged in between survives untouched.
        val out = EventLog.compact(
            listOf(
                readiness(ReadinessSignal.Pain),
                readiness(ReadinessSignal.WellnessZ, -1.5),
                removeReadiness(ReadinessSignal.Pain),
            ),
        )
        assertEquals(listOf(readiness(ReadinessSignal.WellnessZ, -1.5)), out)
    }

    @Test
    fun removeReadinessCancelsOnlyTheLatestMatchingSubmit() {
        val out = EventLog.compact(
            listOf(
                readiness(ReadinessSignal.Pain),
                readiness(ReadinessSignal.Pain),
                removeReadiness(ReadinessSignal.Pain),
            ),
        )
        assertEquals(listOf(readiness(ReadinessSignal.Pain)), out)
    }

    @Test
    fun unmatchedRemoveReadinessIsDroppedAlone() {
        // Across a clear the remove has nothing left to cancel; before a
        // submit it replays as a no-op, both drop just the remove itself.
        val acrossClear = EventLog.compact(
            listOf(
                readiness(ReadinessSignal.Pain),
                line(Event.ClearReadiness),
                removeReadiness(ReadinessSignal.Pain),
            ),
        )
        assertTrue(acrossClear.isEmpty())

        val beforeSubmit = EventLog.compact(
            listOf(removeReadiness(ReadinessSignal.Pain), readiness(ReadinessSignal.Pain)),
        )
        assertEquals(listOf(readiness(ReadinessSignal.Pain)), beforeSubmit)
    }

    @Test
    fun clearDropsFamilyPrefixButKeepsLaterEntries() {
        val out = EventLog.compact(listOf(set("a"), set("b"), line(Event.ClearSets), set("c")))
        assertEquals(listOf(set("c")), out)
    }

    @Test
    fun trailingClearCollapsesToEmpty() {
        val out = EventLog.compact(listOf(run(5.0), run(6.0), line(Event.ClearRuns)))
        assertTrue(out.isEmpty())
    }

    @Test
    fun clearOnlyTouchesItsOwnFamily() {
        val out = EventLog.compact(listOf(set("a"), run(5.0), line(Event.ClearSets)))
        assertEquals(listOf(run(5.0)), out)
    }

    @Test
    fun clearRunsWipesBothLogRunAndLogRunTrack() {
        val track = line(Event.LogRunTrack(listOf(GpsPoint(0.0, 0.0, 0, 5.0)), 0.0, 0.0))
        val out = EventLog.compact(listOf(run(5.0), track, line(Event.ClearRuns), run(8.0)))
        assertEquals(listOf(run(8.0)), out)
    }

    @Test
    fun onlyLastProfileSurvives() {
        val out = EventLog.compact(listOf(profile(10), set("a"), profile(20)))
        assertEquals(listOf(set("a"), profile(20)), out)
    }

    @Test
    fun onlyLastProteinSurvives() {
        val out = EventLog.compact(listOf(protein(70.0), set("a"), protein(90.0)))
        assertEquals(listOf(set("a"), protein(90.0)), out)
    }

    // ── families 9–11: Cooper / CriticalSpeed / Apre (lockstep with log.rs) ──

    @Test
    fun onlyLastCooperSurvives() {
        val out = EventLog.compact(listOf(cooper(2400.0), set("a"), cooper(2600.0)))
        assertEquals(listOf(set("a"), cooper(2600.0)), out)
    }

    @Test
    fun clearWipesTheCooperFamily() {
        val out = EventLog.compact(listOf(cooper(2400.0), cooper(2600.0), clearCooper))
        assertTrue(out.isEmpty())
    }

    @Test
    fun onlyLastCriticalSpeedSurvives() {
        val out = EventLog.compact(listOf(criticalSpeed(3000.0), set("a"), criticalSpeed(3200.0)))
        assertEquals(listOf(set("a"), criticalSpeed(3200.0)), out)
    }

    @Test
    fun clearWipesTheCriticalSpeedFamilyButKeepsLaterFit() {
        val out = EventLog.compact(
            listOf(criticalSpeed(3000.0), clearCriticalSpeed, criticalSpeed(3200.0))
        )
        assertEquals(listOf(criticalSpeed(3200.0)), out)
    }

    @Test
    fun onlyLastApreSurvives() {
        val out = EventLog.compact(listOf(apre(4), set("a"), apre(8)))
        assertEquals(listOf(set("a"), apre(8)), out)
    }

    @Test
    fun clearWipesTheApreFamily() {
        val out = EventLog.compact(listOf(apre(4), apre(8), clearApre))
        assertTrue(out.isEmpty())
    }

    // ── families 13–14: plan request + SetToday (Phase 6, lockstep with log.rs) ──

    private fun generatePlan(start: Long) = line(Event.GeneratePlan(start))
    private fun setToday(day: Long) = line(Event.SetToday(day))
    private val clearPlan = line(Event.ClearPlan)

    @Test
    fun onlyLastPlanRequestSurvives() {
        val out = EventLog.compact(listOf(generatePlan(1), set("a"), generatePlan(8)))
        assertEquals(listOf(set("a"), generatePlan(8)), out)
    }

    @Test
    fun clearWipesThePlanFamily() {
        val out = EventLog.compact(listOf(generatePlan(1), generatePlan(8), clearPlan))
        assertTrue(out.isEmpty())
    }

    @Test
    fun onlyLastSetTodaySurvives() {
        // SetToday is sent every foreground; compaction keeps exactly one line.
        val out = EventLog.compact(listOf(setToday(1), setToday(2), setToday(3)))
        assertEquals(listOf(setToday(3)), out)
    }

    @Test
    fun planAndTodayClearsTouchOnlyTheirOwnFamilies() {
        // A ClearPlan must not disturb SetToday, sets, or the profile.
        val out = EventLog.compact(
            listOf(profile(10), setToday(2), generatePlan(1), set("a"), clearPlan)
        )
        assertEquals(listOf(profile(10), setToday(2), set("a")), out)
    }

    @Test
    fun calculatorClearsOnlyTouchTheirOwnFamily() {
        // A ClearCooper must not disturb the CS fit or APRE adjustment (nor any
        // other family): each calculator clears independently.
        val out = EventLog.compact(
            listOf(cooper(2400.0), criticalSpeed(3000.0), apre(6), clearCooper)
        )
        assertEquals(listOf(criticalSpeed(3000.0), apre(6)), out)
    }

    // ── family 12: check-ins (Phase 2 / B1, lockstep with log.rs) ──

    private fun checkin(observedAt: Long) =
        line(Event.SubmitCheckin(observedAt, sleepQuality = 3, soreness = 3, mood = 3))

    @Test
    fun checkinsAreRetainedHistoryOnlyAClearWipes() {
        // Family 12 is NOT day-scoped: multiple check-ins across days all survive
        // (they ARE the rolling baseline). Only a ClearCheckins supersedes them.
        val kept = EventLog.compact(listOf(checkin(0), checkin(86_400), checkin(172_800)))
        assertEquals(3, kept.size)

        val cleared = EventLog.compact(listOf(checkin(0), checkin(86_400), line(Event.ClearCheckins)))
        assertTrue(cleared.isEmpty())

        val after = EventLog.compact(listOf(checkin(0), line(Event.ClearCheckins), checkin(86_400)))
        assertEquals(listOf(checkin(86_400)), after)
    }

    @Test
    fun clearCheckinsOnlyTouchesItsOwnFamily() {
        val out = EventLog.compact(
            listOf(readiness(ReadinessSignal.WellnessZ), set("a"), checkin(0), line(Event.ClearCheckins))
        )
        assertEquals(listOf(readiness(ReadinessSignal.WellnessZ), set("a")), out)
    }

    // ── Rule 4: check-in retention window (45 days, lockstep with log.rs) ──────

    private val DAY = 86_400L

    @Test
    fun rule4DropsCheckinsOlderThanTheRetentionWindow() {
        // Newest check-in is day 60; the 45-day window's cutoff is day 15. Day 0
        // (60 days old) is dropped; day 20 (inside 45) and day 60 survive.
        val out = EventLog.compact(listOf(checkin(0), checkin(20 * DAY), checkin(60 * DAY)))
        assertEquals(listOf(checkin(20 * DAY), checkin(60 * DAY)), out)
    }

    @Test
    fun rule4KeepsTheCheckinExactlyAtTheWindowEdge() {
        // observed_at == cutoff (newest − 45 days) is NOT strictly older → kept.
        val out = EventLog.compact(listOf(checkin(0), checkin(45 * DAY)))
        assertEquals(listOf(checkin(0), checkin(45 * DAY)), out)
    }

    @Test
    fun rule4IsANoopWithoutCheckins() {
        val out = EventLog.compact(listOf(run(5.0), readiness(ReadinessSignal.WellnessZ)))
        assertEquals(listOf(run(5.0), readiness(ReadinessSignal.WellnessZ)), out)
    }

    @Test
    fun rule4AnchorsOnTheNewestSurvivingCheckinNotAClearedOne() {
        // A ClearCheckins wipes the pre-clear history (Rule 1); Rule 4 then
        // anchors on the post-clear check-in, so a lone recent check-in after a
        // clear is never treated as stale against a wiped older reference.
        val out = EventLog.compact(
            listOf(checkin(0), checkin(DAY), line(Event.ClearCheckins), checkin(100 * DAY))
        )
        assertEquals(listOf(checkin(100 * DAY)), out)
    }

    @Test
    fun rule4KeepsAFullBaselineWindowAndDropsOnlyTheStaleTail() {
        // A stale week (days 0..6) then a fresh burst (days 60..66): only the old
        // week ages out; every check-in inside the 45-day window is retained.
        val stale = (0L..6L).map { checkin(it * DAY) }
        val fresh = (60L..66L).map { checkin(it * DAY) }
        val out = EventLog.compact(stale + fresh)
        assertEquals(fresh, out)
    }

    @Test
    fun rule4PerChannelWindowKeepsASparseChannelAlive() {
        // F1 (lockstep with log.rs): derive_readiness windows EACH channel on ITS
        // OWN newest reading, not the log's global newest. Eight resting-HR-ONLY
        // check-ins on days 0..7 build a full RHR baseline; sleep-ONLY check-ins on
        // days 60..63 carry NO resting-HR. A global-newest anchor (day 63) would
        // drop the whole RHR week (>45 days back) and silently vanish the
        // resting-HR readiness row. The per-channel anchor (min channel newest =
        // day 7 for RHR) keeps every check-in, so replay is unchanged.
        val rhrWeek = (0L..7L).map {
            line(Event.SubmitCheckin(it * DAY, restingHrBpm = 50.0 + it))
        }
        val sleepBurst = (60L..63L).map {
            line(Event.SubmitCheckin(it * DAY, sleepQuality = 3))
        }
        val out = EventLog.compact(rhrWeek + sleepBurst)
        assertEquals(rhrWeek + sleepBurst, out)
    }

    // ── Rule 3: DeleteEntry / AmendSet / AmendRun (Phase 4 / M4, lockstep) ──

    private fun setId(exercise: String, id: Long, observedAt: Long) =
        line(Event.LogSet(exercise, 100.0, 5, 8.0, observedAt = observedAt, entryId = id))
    private fun runId(km: Double, id: Long, observedAt: Long) =
        line(Event.LogRun(km, km * 5.0, 75.0, 0.0, observedAt = observedAt, entryId = id))
    private fun delSet(id: Long) = line(Event.DeleteEntry(Event.EntryKind.Set, id))
    private fun delRun(id: Long) = line(Event.DeleteEntry(Event.EntryKind.Run, id))
    private fun amendSet(id: Long, weightKg: Double) =
        line(Event.AmendSet(id, "Bench", weightKg, 5, 8.0, observedAt = 0))
    private fun amendRun(id: Long, km: Double) =
        line(Event.AmendRun(id, km, km * 5.0, 0.0, 0.0, observedAt = 0))

    @Test
    fun deleteCancelsItsLoggedSetPairwise() {
        val out = EventLog.compact(listOf(setId("Bench", 7, 0), run(5.0), delSet(7)))
        assertEquals(listOf(run(5.0)), out)
    }

    @Test
    fun deleteTargetsTheNewestMatchingRow() {
        val out = EventLog.compact(listOf(setId("A", 3, 0), setId("B", 3, 86_400), delSet(3)))
        assertEquals(listOf(setId("A", 3, 0)), out)
    }

    @Test
    fun amendSupersedesThePriorSetButSurvives() {
        // B8: a strict amend needs its base row on replay, so Log→Amend keeps BOTH
        // the base log AND the amend (lockstep with log.rs).
        val out = EventLog.compact(listOf(setId("Bench", 5, 0), amendSet(5, 120.0)))
        assertEquals(listOf(setId("Bench", 5, 0), amendSet(5, 120.0)), out)
    }

    @Test
    fun amendChainCollapsesToLogPlusLastAmend() {
        // B8: Log→Amend→Amend (no delete) collapses to [log, last amend], the
        // superseded middle amend is dropped, the base log is retained.
        val out = EventLog.compact(
            listOf(setId("Bench", 9, 0), amendSet(9, 110.0), amendSet(9, 130.0))
        )
        assertEquals(listOf(setId("Bench", 9, 0), amendSet(9, 130.0)), out)
    }

    @Test
    fun amendChainEndingInDeleteCancelsAway() {
        // The whole edit chain plus its delete is inert.
        val out = EventLog.compact(
            listOf(setId("Bench", 9, 0), amendSet(9, 110.0), amendSet(9, 130.0), delSet(9))
        )
        assertTrue(out.isEmpty())
    }

    @Test
    fun amendAfterDeleteNeverResurrectsTheEntry() {
        // B8 core prevention (lockstep with log.rs): Log→Delete→Amend targeting the
        // already-deleted row. The delete cancels the entry; the orphan amend
        // matches nothing and is dropped → the entry STAYS deleted (before B8 the
        // amend survived as a push and resurrected the row on the next launch).
        val setOut = EventLog.compact(listOf(setId("Bench", 5, 0), delSet(5), amendSet(5, 120.0)))
        assertTrue(setOut.isEmpty())

        // Same for a run, with an unrelated survivor.
        val runOut = EventLog.compact(
            listOf(runId(5.0, 2, 0), runId(8.0, 3, 86_400), delRun(3), amendRun(3, 9.0))
        )
        assertEquals(listOf(runId(5.0, 2, 0)), runOut)
    }

    @Test
    fun unmatchedDeleteIsDroppedAlone() {
        val before = EventLog.compact(listOf(delSet(42), setId("Bench", 42, 0)))
        assertEquals(listOf(setId("Bench", 42, 0)), before)

        val orphan = EventLog.compact(listOf(run(5.0), delSet(1)))
        assertEquals(listOf(run(5.0)), orphan)
    }

    @Test
    fun deleteAcrossAClearCollapsesAway() {
        val out = EventLog.compact(listOf(setId("Bench", 4, 0), line(Event.ClearSets), delSet(4)))
        assertTrue(out.isEmpty())
    }

    @Test
    fun legacyRowWithoutIdIsMatchedOnObservedAt() {
        val out = EventLog.compact(
            listOf(
                setId("Old", 0, 5000),
                line(Event.DeleteEntry(Event.EntryKind.Set, entryId = 0, observedAtFallback = 5000)),
            )
        )
        assertTrue(out.isEmpty())
    }

    @Test
    fun rule3LegacyChainCollapsesOnlyWhilePinnedToOneDate() {
        // F2 guard (lockstep with log.rs). Rule 3's chain-walk keys a legacy
        // (entry_id 0) chain by ONE (fam, id, observed_at) match key: correct only
        // while the row's date is immutable. THIS SHELL enforces that: LogEntry.kt
        // pins observed_at == observed_at_fallback for id-0 rows, blocking
        // re-dating. SUPPORTED (pinned) path: a one-date legacy Log→Amend→Amend
        // collapses to [log, last amend], exactly like an id-bearing chain. (The
        // replay-equivalence of the UNSUPPORTED re-dated case, which reverts /
        // resurrects rows, is pinned Rust-side in
        // rule3_walk_assumes_legacy_rows_are_never_redated, where a core replay
        // is available; the JVM shell has no engine.)
        val log = line(Event.LogSet("Bench", 100.0, 5, 8.0, observedAt = DAY, entryId = 0))
        val amend1 = line(Event.AmendSet(0, "Bench", 110.0, 5, 8.0, observedAt = DAY, observedAtFallback = DAY))
        val amend2 = line(Event.AmendSet(0, "Bench", 130.0, 5, 8.0, observedAt = DAY, observedAtFallback = DAY))
        val out = EventLog.compact(listOf(log, amend1, amend2))
        assertEquals(listOf(log, amend2), out)
    }

    @Test
    fun runDeleteKeepsBothWhenALaterRunBakedItsBaseline() {
        // A run logged between the target run and its delete could have baked the
        // target's distance into its baseline, so the pair is NOT dropped (both
        // survive), matching log.rs run_line_between. Deleting the LAST run does
        // compact.
        val kept = EventLog.compact(listOf(runId(5.0, 2, 0), runId(8.0, 4, 100), delRun(2)))
        assertEquals(listOf(runId(5.0, 2, 0), runId(8.0, 4, 100), delRun(2)), kept)

        val dropped = EventLog.compact(listOf(runId(5.0, 2, 0), runId(8.0, 4, 100), delRun(4)))
        assertEquals(listOf(runId(5.0, 2, 0)), dropped)
    }

    @Test
    fun emptyLogStaysEmpty() {
        assertTrue(EventLog.compact(emptyList()).isEmpty())
    }

    @Test
    fun unparseableLineIsNeverRemoved() {
        val out = EventLog.compact(listOf("{ hand edited garbage", set("a"), line(Event.ClearSets)))
        assertEquals(listOf("{ hand edited garbage"), out)
    }

    @Test
    fun variantOfReadsUnitAndStructTags() {
        assertEquals("ClearSets", EventLog.variantOf(line(Event.ClearSets)))
        assertEquals("LogSet", EventLog.variantOf(set("a")))
        assertEquals(null, EventLog.variantOf("not json"))
    }

    // ── load(): fresh-install vs compacted-empty (Core.restore's seed decision) ──

    private fun tempLog(): File =
        File.createTempFile("event-log", ".ndjson").apply { deleteOnExit() }

    @Test
    fun loadMissingFileIsFreshInstall() {
        val file = tempLog().also { it.delete() }
        val restored = EventLog.load(file)
        assertTrue(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadCompactedEmptyLogIsReturningUserNotFreshInstall() {
        // A user who logged sets then cleared them: every line compacts away, but
        // the log file exists: this must NOT read as a fresh install, or the
        // caller would re-seed the SEED profile + onboarding over a deliberate
        // empty state.
        val file = tempLog()
        file.writeText(
            listOf(set("a"), set("b"), line(Event.ClearSets))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadBlankButExistingLogIsNotFreshInstall() {
        // A previous compaction that emptied the log leaves "\n" behind; the file
        // still exists, so this too is a returning user.
        val file = tempLog()
        file.writeText("\n")
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertTrue(restored.lines.isEmpty())
    }

    @Test
    fun loadKeepsSurvivorsAndRewritesFileCompacted() {
        val file = tempLog()
        file.writeText(
            listOf(set("a"), line(Event.ClearSets), run(5.0))
                .joinToString("\n", postfix = "\n")
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(run(5.0)), restored.lines)
        // The durable log was compacted in place (atomic rewrite).
        assertEquals(listOf(run(5.0)), file.readLines().filter { it.isNotBlank() })
    }

    // ── load(): torn-line GC (item I4), drop crash-truncated lines, but never
    //    a valid-JSON future event we simply don't recognize (forward-compat) ──

    @Test
    fun loadGarbageCollectsATornLineAndRewritesTheFile() {
        // A crash mid-appendText left a truncated, non-JSON tail on the last
        // record. It parses as JSON never, so replay would reject it on every
        // launch. load() must drop it and rewrite the durable log without it.
        val file = tempLog()
        val torn = """{"LogSet":{"exercise":"Bench","weigh"""
        file.writeText(
            (listOf(set("a"), torn, run(5.0)).joinToString("\n")) + "\n"
        )
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(set("a"), run(5.0)), restored.lines)
        // The torn line is gone from the durable log too.
        assertEquals(listOf(set("a"), run(5.0)), file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadPreservesUnknownButValidJsonVariants() {
        // FORWARD-COMPAT: a newer build wrote events this build doesn't know -
        // an unknown single-key struct variant AND an unknown bare-string unit
        // variant. Both are valid JSON, so both MUST survive load() untouched
        // (an old build has to hand them back when the user upgrades again).
        val file = tempLog()
        val unknownObject = """{"FutureStructEvent":{"weeks":6,"note":"hi"}}"""
        val unknownString = "\"FutureUnitEvent\""
        val original = listOf(set("a"), unknownObject, unknownString, run(5.0))
        file.writeText(original.joinToString("\n") + "\n")
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        // Nothing dropped → the file is left exactly as-is (no rewrite needed).
        assertEquals(original, file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadLeavesANormalMixedLogUnchanged() {
        // No torn lines and nothing compaction can drop: load() returns the lines
        // verbatim and does not rewrite the file.
        val file = tempLog()
        val original = listOf(set("a"), run(5.0), checkin(0))
        val bytes = original.joinToString("\n") + "\n"
        file.writeText(bytes)
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        assertEquals(bytes, file.readText())
    }
}
