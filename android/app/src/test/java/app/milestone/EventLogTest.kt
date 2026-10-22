package app.milestone

import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit tests for [EventLog.compact], the Kotlin half of the compaction lockstep
 * with Rust `compact_event_log` (shared/src/log.rs). Lines are produced by the
 * real [Event.toJson] so a wire-shape drift would surface here too.
 *
 * Also covers [readBounded], the guard on the other end of the same durable log:
 * a restore reads a user-chosen document before it may replace the log, and that
 * read must stay bounded.
 */
class EventLogTest {

    private fun line(e: Event) = e.toJson().toString()

    // Pin observedAt + entryId so a helper called twice (input AND expected)
    // produces identical lines: the LogSet/LogRun defaults are the wall
    // clock (seconds / millis), which would otherwise differ between calls.
    private fun set(exercise: String) = line(Event.LogSet(exercise, 100.0, 5, 8.0, observedAt = 0, entryId = 0))
    private fun run(km: Double) = line(Event.LogRun(km, km * 5.0, 75.0, 0.0, observedAt = 0, entryId = 0))
    private fun profile(weeklySets: Int) = line(
        Event.SetProfile(
            ProgressionCadence.WeekToWeek, LiftGoal.MaxStrength, GoalDistance.TenK,
            ConcurrentGoal.Strength, weeklySets, 4, 45.0, false,
        )
    )

    // The Kotlin Event type has no variants for the dormant calculators yet, so
    // these lines are raw serde wire shapes (struct variants = single-key object,
    // unit variants = bare string) exactly as app/mod.rs serializes them.
    private fun cooper(distanceM: Double) = """{"ComputeCooper":{"distance_m_12min":$distanceM}}"""
    private fun criticalSpeed(distanceM: Double) =
        """{"ComputeCriticalSpeed":{"efforts":[{"distance_m":$distanceM,"time_sec":720.0}]}}"""
    private fun apre(reps: Int) =
        """{"ComputeApre":{"scheme":"Apre6","reps":$reps,"current_load_kg":100.0}}"""
    private val clearCooper = "\"ClearCooper\""
    private val clearCriticalSpeed = "\"ClearCriticalSpeed\""
    private val clearApre = "\"ClearApre\""

    private fun readiness(signal: ReadinessSignal, value: Double = 1.0) =
        line(Event.SubmitReadiness(signal, value, observedAt = 0))

    /** An undated (legacy) remove: the core deletes its input outright. */
    private fun removeReadiness(signal: ReadinessSignal) =
        line(Event.RemoveReadiness(signal, observedAt = 0))

    /** A dated remove: the core retracts its input from [observedAt] on. */
    private fun removeReadinessAt(signal: ReadinessSignal, observedAt: Long) =
        line(Event.RemoveReadiness(signal, observedAt = observedAt))

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
    fun aDatedRemoveKeepsItsSubmitAndConsumesIt() {
        // The dated remove retracts the newer Soreness (both lines stay); the
        // undated remove then pairs with the older one, as the core does.
        val older = line(Event.SubmitReadiness(ReadinessSignal.Soreness, 6.0, observedAt = 100))
        val newer = line(Event.SubmitReadiness(ReadinessSignal.Soreness, 5.0, observedAt = 200))
        val dated = removeReadinessAt(ReadinessSignal.Soreness, 300)
        val out = EventLog.compact(listOf(older, newer, dated, removeReadiness(ReadinessSignal.Soreness)))
        assertEquals(listOf(newer, dated), out)
    }

    // ── review family: every review survives (lockstep with log.rs) ──

    private fun review(count: Int, observedAt: Long) =
        line(Event.SubmitReview(overtrainingSignalCount = count, observedAt = observedAt))

    /** A review that sets the per-session review (an off day). */
    private fun sessionReview(observedAt: Long) =
        line(Event.SubmitReview(badDay = true, observedAt = observedAt))

    @Test
    fun everyReviewSurvivesAndOnlyATrailingClearReviewIsKept() {
        val r1 = review(1, 100)
        val r2 = sessionReview(200)
        val clear = line(Event.ClearReview)
        assertEquals(listOf(r1, r2), EventLog.compact(listOf(r1, r2)))
        assertEquals(listOf(r1, r2, clear), EventLog.compact(listOf(r1, clear, r2, clear)))
        // A later session review replaces the store the clear emptied: inert.
        assertEquals(listOf(r1, r2), EventLog.compact(listOf(r1, clear, r2)))
        // A weekly-only review never sets the session review, so the clear stays.
        val weeklyOnly = review(2, 300)
        assertEquals(listOf(r1, clear, weeklyOnly), EventLog.compact(listOf(r1, clear, weeklyOnly)))
    }

    @Test
    fun aClearWeeklyCheckinIsInertOnlyBeforeALaterWeeklyCheckin() {
        val w1 = line(Event.SubmitWeeklyCheckin(Event.SubmitReview(overtrainingSignalCount = 2, observedAt = 100)))
        val w2 = line(
            Event.SubmitWeeklyCheckin(
                Event.SubmitReview(
                    overtrainingSigns = OvertrainingSigns(worseSleep = true),
                    overtrainingSignalWeeks = 1.5,
                    observedAt = 200,
                ),
            ),
        )
        val clear = line(Event.ClearWeeklyCheckin)
        assertEquals(listOf(w1, w2), EventLog.compact(listOf(w1, clear, w2)))
        // A session review never replaces the weekly check-in: the clear stays.
        val s = sessionReview(300)
        assertEquals(listOf(w1, clear, s), EventLog.compact(listOf(w1, clear, s)))
        // A trailing clear is kept.
        assertEquals(listOf(w1, w2, clear), EventLog.compact(listOf(w1, w2, clear)))
    }

    @Test
    fun clearRunsWipesBothLogRunAndLogRunTrack() {
        val track = line(Event.LogRunTrack(listOf(GpsPoint(0.0, 0.0, 0, 5.0)), 0.0, 0.0))
        val out = EventLog.compact(listOf(run(5.0), track, line(Event.ClearRuns), run(8.0)))
        assertEquals(listOf(run(8.0)), out)
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

    // ── families 13–14: plan request + SetToday (lockstep with log.rs) ──

    private fun generatePlan(start: Long) = line(Event.GeneratePlan(start))
    private fun setToday(day: Long) = line(Event.SetToday(day))
    private val clearPlan = line(Event.ClearPlan)

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

    // ── family 12: check-ins (lockstep with log.rs) ──

    private fun checkin(observedAt: Long) =
        line(Event.SubmitCheckin(observedAt, sleepQuality = 3, soreness = 3, mood = 3))

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
        // Lockstep with log.rs: derive_readiness windows EACH channel on ITS
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

    // ── Rule 3: DeleteEntry / AmendSet / AmendRun (lockstep with log.rs) ──

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

    private fun runTrackId(id: Long, observedAt: Long) = line(
        Event.LogRunTrack(
            listOf(
                GpsPoint(0.0, 0.000, 0L, 5.0),
                GpsPoint(0.0, 0.005, 100L, 5.0),
                GpsPoint(0.0, 0.010, 200L, 5.0),
                GpsPoint(0.0, 0.015, 300L, 5.0),
                GpsPoint(0.0, 0.020, 400L, 5.0),
            ),
            0.0, 0.0, observedAt = observedAt, entryId = id,
        )
    )
    private fun trimRun(id: Long, observedAtFallback: Long, from: Long, until: Long) =
        line(Event.TrimRunTrack(id, observedAtFallback, from, until))

    @Test
    fun anotherRunsTrimBlocksTheDeletePairDrop() {
        // A trim belonging to a DIFFERENT run (7_200_006) than the deleted run
        // (7_200_005) is NOT in the delete's removal chain, so it still counts as a
        // run line in the window and blocks the spike-unsafe pair-drop. The delete
        // and its base survive. Lockstep with log.rs
        // trim_counts_as_a_run_line_for_the_spike_guard.
        val events = listOf(
            runTrackId(7_200_005L, 0),
            runTrackId(7_200_006L, 0),
            trimRun(7_200_006L, 0, 100, 300),
            delRun(7_200_005L),
        )
        assertEquals(events, EventLog.compact(events))
    }

    @Test
    fun clearRunsWipesATrimmedRunBaseAndItsTrims() {
        val events = listOf(
            runTrackId(7_200_003L, 0),
            trimRun(7_200_003L, 0, 100, 300),
            line(Event.ClearRuns),
        )
        assertTrue(EventLog.compact(events).isEmpty())
    }

    @Test
    fun amendAfterDeleteNeverResurrectsTheEntry() {
        // Core prevention (lockstep with log.rs): Log→Delete→Amend targeting the
        // already-deleted row. The delete cancels the entry; the orphan amend
        // matches nothing and is dropped → the entry STAYS deleted (previously the
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
        // Chain-walk guard (lockstep with log.rs). Rule 3's chain-walk keys a legacy
        // (entry_id 0) chain by ONE (fam, id, observed_at) match key: correct only
        // while the row's date is immutable. THIS SHELL enforces that: the log editors
        // pin observed_at == observed_at_fallback for id-0 rows, blocking
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

    // ── Family 15: logged activities (lockstep with log.rs) ──

    private fun activity(id: Long, observedAt: Long, minutes: Double = 45.0) =
        line(Event.LogActivity(ActivityKind.Sport, minutes, observedAt = observedAt, entryId = id))
    private fun delActivity(id: Long, fallback: Long = 0) =
        line(Event.DeleteEntry(Event.EntryKind.Activity, id, observedAtFallback = fallback))

    @Test
    fun activityDeleteCancelsItsEntryEvenWithOtherLinesBetween() {
        // No baked baseline in family 15: a run or another activity in between
        // never blocks the pair-drop.
        val out = EventLog.compact(
            listOf(activity(1, 100), runId(5.0, 2, 150), activity(3, 200), delActivity(1)),
        )
        assertEquals(listOf(runId(5.0, 2, 150), activity(3, 200)), out)
    }

    @Test
    fun activityDeleteMatchesOnlyItsOwnFamily() {
        // An Activity delete sharing a set's id leaves the set alone and, with no
        // activity to match, replays as a no-op: dropped alone.
        val out = EventLog.compact(listOf(setId("Bench", 9, 100), delActivity(9)))
        assertEquals(listOf(setId("Bench", 9, 100)), out)
        // A legacy (id 0) activity is matched on its observed_at.
        assertTrue(EventLog.compact(listOf(activity(0, 5000), delActivity(0, fallback = 5000))).isEmpty())
    }

    @Test
    fun anInvalidActivityHasNoIdentity() {
        // The core ignores minutes outside (0, 600]: such a line is never a delete
        // target, so the delete matches nothing and is dropped alone.
        val tooLong = activity(4, 100, minutes = 601.0)
        assertEquals(listOf(tooLong), EventLog.compact(listOf(tooLong, delActivity(4))))
    }

    @Test
    fun clearActivitiesSweepsOnlyItsFamily() {
        val out = EventLog.compact(
            listOf(
                activity(1, 100), set("a"), run(5.0), activity(2, 200), delActivity(2),
                line(Event.ClearActivities), activity(3, 300),
            ),
        )
        assertEquals(listOf(set("a"), run(5.0), activity(3, 300)), out)
        // A delete kept by an earlier pass is swept with its family too.
        assertTrue(EventLog.compact(listOf(delActivity(7), line(Event.ClearActivities))).isEmpty())
        // ClearSets / ClearRuns leave activities alone.
        assertEquals(
            listOf(activity(1, 100)),
            EventLog.compact(listOf(activity(1, 100), line(Event.ClearSets), line(Event.ClearRuns))),
        )
    }

    @Test
    fun activityVariantsAreRecognized() {
        assertEquals("ClearActivities", EventLog.variantOf(line(Event.ClearActivities)))
        assertEquals("LogActivity", EventLog.variantOf(activity(1, 100)))
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

    // ── shared golden corpus (lockstep with the Rust compaction_golden test) ──

    private fun compactionFixturesDir(): File {
        val relative = File("../../shared/tests/fixtures/compaction")
        if (relative.isDirectory) return relative
        val resource = javaClass.classLoader.getResource("compaction")
            ?: error("compaction fixtures not found (cwd=${System.getProperty("user.dir")})")
        return File(resource.toURI())
    }

    @Test
    fun compactionGoldenFixtures() {
        val dir = compactionFixturesDir()
        val cases = dir.listFiles { f -> f.name.endsWith(".in.ndjson") }
            ?.map { it.name.removeSuffix(".in.ndjson") }
            ?.sorted()
            .orEmpty()
        assertTrue("no compaction fixtures in ${dir.absolutePath}", cases.isNotEmpty())
        for (case in cases) {
            val input = File(dir, "$case.in.ndjson").readLines().filter { it.isNotBlank() }
            val expected = File(dir, "$case.expected.ndjson").readLines().filter { it.isNotBlank() }
            assertEquals("compaction fixture `$case` diverged", expected, EventLog.compact(input))
        }
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

    // ── load(): torn-line GC, drop crash-truncated lines, but never
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

    @Test
    fun loadSkipsTheRewriteForATinyDropButStillReturnsCompactedLines() {
        // A routine launch: one superseded SetToday and one aged-out check-in are
        // the only drops in a log large enough that the bounded rewrite rule calls
        // them not worth an fsync. The file ON DISK stays untouched while replay
        // still gets the compacted survivors. Sized from the rule itself, so a
        // retuned threshold keeps this a below-threshold case.
        val drops = 2
        val filler = (1..drops * EventLog.REWRITE_MIN_DROPPED_SHARE_INVERSE).map { set("f$it") }
        val staleCheckin = checkin(0)
        val freshCheckin = checkin(60 * DAY)
        val original = filler + listOf(setToday(1), setToday(2), staleCheckin, freshCheckin)
        assertFalse(EventLog.compactionWorthRewriting(drops, original.size))
        val file = tempLog()
        file.writeText(original.joinToString("\n") + "\n")

        val restored = EventLog.load(file)
        // Compaction still drops the superseded SetToday and the stale check-in...
        assertFalse(restored.lines.contains(setToday(1)))
        assertTrue(restored.lines.contains(setToday(2)))
        assertFalse(restored.lines.contains(staleCheckin))
        assertTrue(restored.lines.contains(freshCheckin))
        assertEquals(original.size - drops, restored.lines.size)
        // ...but the durable file is left exactly as written.
        assertEquals(original, file.readLines().filter { it.isNotBlank() })
    }

    @Test
    fun loadSelfHealsANewlinelessTornTailThenAcceptsAnAppend() {
        // A process kill mid-appendText left a newline-less partial last line.
        // load() drops it AND rewrites the durable log with a trailing newline, so
        // the next Core.appendText starts a fresh line instead of gluing its JSON
        // onto the torn bytes (which would make that next event unparseable, and
        // lost, on the next boot).
        val file = tempLog()
        val torn = """{"LogSet":{"exercise":"Bench","weigh"""
        file.writeText(set("a") + "\n" + torn) // valid first line, torn tail, NO trailing newline
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(listOf(set("a")), restored.lines)
        // Healed on disk: the intact line joined with a trailing newline.
        assertEquals(listOf(set("a")).joinToString("\n", postfix = "\n"), file.readText())
        // A subsequent append lands on its own line → the whole log reparses.
        file.appendText(run(5.0) + "\n")
        val reloaded = EventLog.load(file)
        assertEquals(listOf(set("a"), run(5.0)), reloaded.lines)
    }

    @Test
    fun loadRewritesAValidButNewlinelessLastLineWithATrailingNewline() {
        // Even when every line parses, a newline-less tail (an append cut off right
        // before its '\n') must be healed: the next append would otherwise glue
        // onto this line and corrupt both events. load() rewrites with the trailing
        // newline even though nothing was dropped.
        val file = tempLog()
        val original = listOf(set("a"), run(5.0))
        file.writeText(original.joinToString("\n")) // valid last line, NO trailing newline
        val restored = EventLog.load(file)
        assertFalse(restored.freshInstall)
        assertEquals(original, restored.lines)
        assertEquals(original.joinToString("\n", postfix = "\n"), file.readText())
    }

    // ── read(): the export's view of the log must never mutate it ──

    @Test
    fun readCompactsWithoutTouchingTheLogOrItsTemporaries() {
        // Everything load() would act on: a compactable prefix (≥5% dropped), a
        // torn newline-less tail, and leftover swap temporaries. read() runs while
        // the core may be appending, so it must change none of it.
        val file = tempLog()
        val torn = """{"LogSet":{"exercise":"Bench","weigh"""
        val bytes = listOf(set("a"), line(Event.ClearSets), run(5.0)).joinToString("\n", postfix = "\n") + torn
        file.writeText(bytes)
        val temps = listOf(".tmp", ".import", ".bak").map { File(file.parentFile, file.name + it) }
        try {
            temps.forEach { it.writeText("x") }

            val read = EventLog.read(file)

            assertFalse(read.freshInstall)
            assertEquals(listOf(run(5.0)), read.lines)
            assertEquals(bytes, file.readText())
            assertTrue(temps.all { it.exists() && it.readText() == "x" })
        } finally {
            temps.forEach { it.delete() }
        }
    }

    @Test
    fun readOfAMissingLogCreatesNothingAndRestoresNothing() {
        // load() would move a crash-left .bak back into place; read() must not.
        val file = tempLog().also { it.delete() }
        val bak = File(file.parentFile, file.name + ".bak").apply { writeText(set("a") + "\n") }
        try {
            val read = EventLog.read(file)
            assertTrue(read.freshInstall)
            assertTrue(read.lines.isEmpty())
            assertFalse(file.exists())
            assertTrue(bak.exists())
        } finally {
            bak.delete()
        }
    }

    @Test
    fun loadReportsTheLargestEntryIdEvenWhenItsLineCompactsAway() {
        // EntryIds must never reuse an id a restored entry carried, including one a
        // later clear removed (a stale amend/delete could still name it).
        val file = tempLog()
        file.writeText(
            listOf(
                line(Event.LogSet("a", 100.0, 5, 8.0, observedAt = 1, entryId = 5)),
                line(Event.LogSet("b", 100.0, 5, 8.0, observedAt = 2, entryId = 900)),
                line(Event.ClearSets),
                line(Event.LogRun(5.0, 25.0, 75.0, 0.0, observedAt = 3, entryId = 12)),
            ).joinToString("\n", postfix = "\n")
        )
        assertEquals(900L, EventLog.load(file).maxEntryId)
    }

    @Test
    fun entryIdsStayAboveARestoredIdAfterTheClockMovedBack() {
        // A restored id from a clock that ran ahead of today's wall clock.
        val restored = System.currentTimeMillis() + 10L * 365 * 86_400_000L
        EntryIds.raiseTo(restored)
        val a = EntryIds.next()
        val b = EntryIds.next()
        assertTrue(a > restored)
        assertTrue(b > a)
    }

    @Test
    fun swapLogFileReplacesTheLogAndLeavesNoTemporary() {
        val dir = java.nio.file.Files.createTempDirectory("swap").toFile()
        try {
            File(dir, "event-log.ndjson").writeText(set("old") + "\n")
            swapLogFile(dir, listOf(set("a"), run(5.0)))
            assertEquals(listOf(set("a"), run(5.0)).joinToString("\n", postfix = "\n"), File(dir, "event-log.ndjson").readText())
            assertFalse(File(dir, "event-log.ndjson.import").exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    /** Emits [n] bytes and records how many the reader actually pulled, so a test
     *  can pin read amplification and not just the returned value. */
    private class CountingSource(private val n: Long) : InputStream() {
        var bytesRead = 0L
            private set
        private var pos = 0L

        override fun read(): Int {
            if (pos >= n) return -1
            pos++
            bytesRead++
            return 0
        }

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (pos >= n) return -1
            val take = minOf(len.toLong(), n - pos).toInt()
            b.fill(0, off, off + take)
            pos += take
            bytesRead += take
            return take
        }
    }

    @Test
    fun streamValidateRejectsAnOversizedStreamWithoutDrainingIt() {
        // A content provider may report an unknown size, so the bound has to hold on
        // the stream itself: a stream past the cap is rejected...
        val src = CountingSource(MAX_IMPORT_BYTES + 1)
        assertEquals(ImportCheck.Invalid, streamValidateImport(src, MAX_IMPORT_BYTES))
        // ...and the reader stops there instead of buffering the whole file: at most
        // one 64 KiB chunk past the cap is ever pulled (bounded memory).
        assertTrue(
            "read ${src.bytesRead} bytes past the cap",
            src.bytesRead <= MAX_IMPORT_BYTES + 64 * 1024,
        )
    }

    @Test
    fun streamValidateAcceptsAWellFormedLogAndRejectsAJunkLine() {
        val log = (set("a") + "\n" + run(5.0) + "\n").toByteArray()
        assertEquals(
            ImportCheck.Valid(listOf(set("a"), run(5.0))),
            streamValidateImport(ByteArrayInputStream(log), MAX_IMPORT_BYTES),
        )
        // A single unknown/garbage line rejects the whole file (streamed, so a large
        // binary file trips this on its first line before it can fill the heap).
        val junk = (set("a") + "\n" + "not a wire line" + "\n").toByteArray()
        assertEquals(
            ImportCheck.Invalid,
            streamValidateImport(ByteArrayInputStream(junk), MAX_IMPORT_BYTES),
        )
    }

    @Test
    fun streamValidateRejectsAMalformedNamedClearTheCoreWouldReject() {
        // {"ClearSets":{}} is a unit variant given a body — a shape the
        // core rejects at serde. Import must NOT accept it, or the next launch's
        // compaction would treat it as a family clear and wipe every logged set.
        val evil = (set("a") + "\n" + """{"ClearSets":{}}""" + "\n").toByteArray()
        assertEquals(
            ImportCheck.Invalid,
            streamValidateImport(ByteArrayInputStream(evil), MAX_IMPORT_BYTES),
        )
    }

    @Test
    fun variantOfRejectsShapeMismatchedNamedLines() {
        // A unit variant with a body, or a struct variant
        // with a non-object payload, or a struct name as a bare string, names NO event
        // — so compaction never treats it as a clear and import rejects it. Valid
        // shapes (unit → bare string, struct → object) still resolve.
        assertEquals(null, EventLog.variantOf("""{"ClearSets":{}}"""))
        assertEquals(null, EventLog.variantOf("""{"LogSet":5}"""))
        assertEquals(null, EventLog.variantOf("\"LogSet\""))
        assertEquals("ClearSets", EventLog.variantOf("\"ClearSets\""))
        assertEquals(
            "SetExerciseEnded",
            EventLog.variantOf(
                """{"SetExerciseEnded":{"exercise":"Bench","epoch_day":1,"ended":true,"observed_at":0}}""",
            ),
        )
    }

    @Test
    fun compactionNeverTreatsAMalformedClearAsAFamilyWipe() {
        // The concrete data-loss vector: a log ending in {"ClearSets":{}} must NOT
        // sweep the logged sets in Kotlin compaction (the core would reject the line).
        val out = EventLog.compact(listOf(set("a"), """{"ClearSets":{}}"""))
        assertEquals(listOf(set("a"), """{"ClearSets":{}}"""), out)
    }
}
