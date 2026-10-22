package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The guided-run step machine (URH-1): pure, so every cue boundary is pinned here. */
class RunStepMachineTest {

    private fun machine(s: RunStructureView) = RunStepMachine(buildRunSteps(s)).also { it.start(0L, 0.0) }

    @Test
    fun sixBy800WithJogRecovery_advancesOnDistance() {
        val m = machine(RunStructureView(reps = 6, rep_m = 800, recovery_m = 400))
        assertEquals("6 reps + 5 recoveries", 11, m.steps.size)
        assertEquals(RunStep(RunStepKind.Rep, 1, 6, distanceM = 800), m.progress().step)

        // Time alone never ends a distance rep.
        assertFalse(m.advance(600_000L, 799.0, paused = false))
        assertEquals(1, m.progress().remainingM)
        assertTrue(m.advance(601_000L, 800.0, paused = false))
        val rec = m.progress()
        assertEquals(RunStepKind.Recovery, rec.step?.kind)
        assertEquals(400, rec.remainingM)
        assertEquals(RunStepKind.Rep, rec.next?.kind)

        // Walk the rest of the session on distance: rep k starts at (k-1)·1200 m.
        var d = 800.0
        for (k in 2..6) {
            d += 400.0
            assertTrue(m.advance(602_000L + k, d, paused = false))
            assertEquals(RunStep(RunStepKind.Rep, k, 6, distanceM = 800), m.progress().step)
            d += 800.0
            assertTrue(m.advance(603_000L + k, d, paused = false))
        }
        assertEquals(6.0 * 800 + 5 * 400, d, 0.0)
        assertTrue(m.finished)
        assertNull(m.progress().step)
    }

    @Test
    fun distanceOvershootCarriesIntoTheNextStep() {
        val m = machine(RunStructureView(reps = 2, rep_m = 400, recovery_m = 200))
        // One late fix lands 30 m past the rep's end: the recovery is already 30 m in.
        assertTrue(m.advance(90_000L, 430.0, paused = false))
        assertEquals(170, m.progress().remainingM)
    }

    @Test
    fun timeBasedReps_advanceOnActiveTime() {
        val m = machine(RunStructureView(reps = 3, rep_sec = 60, recovery_sec = 30))
        assertEquals(60, m.progress().remainingSec)
        // Distance alone never ends a timed rep.
        assertFalse(m.advance(59_000L, 5_000.0, paused = false))
        assertEquals(1, m.progress().remainingSec)
        assertTrue(m.advance(60_000L, 5_000.0, paused = false))
        assertEquals(RunStepKind.Recovery, m.progress().step?.kind)
        assertEquals(30, m.progress().remainingSec)
        assertTrue(m.advance(90_000L, 5_000.0, paused = false))
        assertEquals(RunStep(RunStepKind.Rep, 2, 3, durationSec = 60), m.progress().step)
        // A long gap between ticks crosses several steps and keeps the overshoot.
        assertTrue(m.advance(250_000L, 5_000.0, paused = false))
        assertTrue(m.finished)
    }

    @Test
    fun recoveryEqualToRep_lastsAsLongAsTheRepTook() {
        val m = machine(RunStructureView(reps = 2, rep_m = 400, recovery_equal_to_rep = true))
        assertTrue(m.advance(100_000L, 400.0, paused = false))
        val rec = m.progress()
        assertEquals(RunStepKind.Recovery, rec.step?.kind)
        assertEquals(100, rec.remainingSec)
        assertFalse(m.advance(199_000L, 500.0, paused = false))
        assertTrue(m.advance(200_000L, 520.0, paused = false))
        assertEquals(RunStep(RunStepKind.Rep, 2, 2, distanceM = 400), m.progress().step)
        assertEquals("the second rep starts where the recovery ended", 400, m.progress().remainingM)
    }

    @Test
    fun couchTo5kRunWalk_alternatesRunAndWalk() {
        val s = RunStructureView(
            warmup_min = 5,
            run_walk = List(3) { RunWalkStepView(run_sec = 60, walk_sec = 90) },
            // The bouts win over the mirrored rep count.
            reps = 3, rep_sec = 60,
        )
        val steps = buildRunSteps(s)
        assertEquals(
            listOf(
                RunStepKind.WarmUp,
                RunStepKind.Run, RunStepKind.Walk,
                RunStepKind.Run, RunStepKind.Walk,
                RunStepKind.Run, RunStepKind.Walk,
            ),
            steps.map { it.kind },
        )
        val m = RunStepMachine(steps).also { it.start(0L, 0.0) }
        var t = 300_000L
        assertTrue(m.advance(t, 0.0, paused = false))
        for (k in 1..3) {
            assertEquals(RunStep(RunStepKind.Run, k, 3, durationSec = 60), m.progress().step)
            t += 60_000L
            assertTrue(m.advance(t, 0.0, paused = false))
            assertEquals(RunStep(RunStepKind.Walk, k, 3, durationSec = 90), m.progress().step)
            t += 90_000L
            assertTrue(m.advance(t, 0.0, paused = false))
        }
        assertTrue(m.finished)
    }

    @Test
    fun pauseFreezesTheStep() {
        val m = machine(RunStructureView(reps = 2, rep_sec = 60, recovery_sec = 30))
        assertFalse(m.advance(30_000L, 100.0, paused = false))
        assertEquals(30, m.progress().remainingSec)
        // Ten minutes paused: nothing moves.
        assertFalse(m.advance(630_000L, 100.0, paused = true))
        assertEquals(30, m.progress().remainingSec)
        assertEquals(RunStep(RunStepKind.Rep, 1, 2, durationSec = 60), m.progress().step)
        // Resumed: the remaining 30 s of active time end the rep.
        assertFalse(m.advance(659_000L, 100.0, paused = false))
        assertTrue(m.advance(660_000L, 100.0, paused = false))
        assertEquals(RunStepKind.Recovery, m.progress().step?.kind)
    }

    @Test
    fun easyRunWithStrides_hasAnOpenMainStepEndedBySkip() {
        val s = RunStructureView(warmup_min = 10, cooldown_min = 5, strides_reps = 3, strides_sec = 20, strides_recovery_sec = 60)
        val steps = buildRunSteps(s)
        assertEquals(
            listOf(
                RunStepKind.WarmUp, RunStepKind.Main, RunStepKind.CoolDown,
                RunStepKind.Stride, RunStepKind.StrideRecovery,
                RunStepKind.Stride, RunStepKind.StrideRecovery,
                RunStepKind.Stride,
            ),
            steps.map { it.kind },
        )
        val m = RunStepMachine(steps).also { it.start(0L, 0.0) }
        assertTrue(m.advance(600_000L, 2_000.0, paused = false))
        assertTrue(m.progress().step!!.open)
        assertNull(m.progress().remainingSec)
        assertNull(m.progress().remainingM)
        // An open step never ends on its own.
        assertFalse(m.advance(6_000_000L, 20_000.0, paused = false))
        assertEquals(RunStepKind.Main, m.progress().step?.kind)
        assertTrue(m.skip(6_000_000L, 20_000.0, paused = false))
        assertEquals(RunStepKind.CoolDown, m.progress().step?.kind)
        assertEquals("the cool-down starts at the skip", 300, m.progress().remainingSec)
    }

    @Test
    fun skipOnTheLastStepFinishes_andAnAllZeroStructureHasNoSteps() {
        val m = machine(RunStructureView(reps = 1, rep_sec = 60))
        assertTrue(m.skip(1_000L, 0.0, paused = false))
        assertTrue(m.finished)
        assertFalse(m.skip(2_000L, 0.0, paused = false))
        assertTrue(buildRunSteps(RunStructureView()).isEmpty())
    }

    @Test
    fun runSessionGuide_followsThePlannedRunAndResetClearsIt() {
        RunSession.reset()
        RunSession.plannedRun.value = PrescriptionView(run_structure = RunStructureView(reps = 2, rep_sec = 60))
        RunSession.startSteps(1_000L)
        assertEquals(RunStepKind.Rep, RunSession.stepProgress.value?.step?.kind)
        assertTrue(RunSession.tickSteps(61_000L))
        assertEquals(RunStepKind.Rep, RunSession.stepProgress.value?.step?.kind)
        assertEquals(2, RunSession.stepProgress.value?.step?.number)
        RunSession.skipStep(62_000L)
        assertTrue(RunSession.stepProgress.value!!.finished)
        RunSession.reset()
        assertNull(RunSession.plannedRun.value)
        assertNull(RunSession.stepProgress.value)
        // A free run (no plan) has no guide at all.
        RunSession.startSteps(0L)
        assertNull(RunSession.stepProgress.value)
    }

    @Test
    fun planPaceIsJudgedOnlyOnTheWork() {
        // A continuous planned run (no guide) is judged throughout.
        assertTrue(paceTargetApplies(null))
        val m = machine(RunStructureView(reps = 2, rep_m = 800, recovery_m = 400, warmup_min = 10))
        // Warm-up: the rep pace doesn't apply.
        assertEquals(RunStepKind.WarmUp, m.progress().step?.kind)
        assertFalse(paceTargetApplies(m.progress()))
        // Rep: it does.
        assertTrue(m.advance(600_000L, 2_000.0, paused = false))
        assertEquals(RunStepKind.Rep, m.progress().step?.kind)
        assertTrue(paceTargetApplies(m.progress()))
        // Recovery jog: not judged against the rep pace.
        assertTrue(m.advance(601_000L, 2_800.0, paused = false))
        assertEquals(RunStepKind.Recovery, m.progress().step?.kind)
        assertFalse(paceTargetApplies(m.progress()))
        // All steps done: nothing left to judge.
        m.skip(602_000L, 2_800.0, paused = false)
        m.skip(603_000L, 2_800.0, paused = false)
        assertNull(m.progress().step)
        assertFalse(paceTargetApplies(m.progress()))
    }
}
