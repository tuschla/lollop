package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The tracker's Finish review ([finishTransition]): Stop never saves in one tap,
 * Resume returns to the same live recording, and a discard always needs its confirm.
 */
class FinishSheetStateTest {

    @Test
    fun stopPausesAndOpensTheReviewInsteadOfSaving() {
        assertEquals(
            FinishTransition(FinishStep.Review, FinishEffect.PauseRecording),
            finishTransition(FinishStep.Closed, FinishAction.Stop),
        )
    }

    @Test
    fun resumeClosesTheReviewAndUnpausesTheSameRun() {
        // ResumeRecording only clears the pause on the still-running service: no
        // restart, no new crash-recovery sidecar, no save.
        assertEquals(
            FinishTransition(FinishStep.Closed, FinishEffect.ResumeRecording),
            finishTransition(FinishStep.Review, FinishAction.Resume),
        )
    }

    @Test
    fun saveIsOnlyReachableFromTheReview() {
        assertEquals(
            FinishTransition(FinishStep.Closed, FinishEffect.SaveRun),
            finishTransition(FinishStep.Review, FinishAction.Save),
        )
        // A stale Save tap while the sheet is closed or the discard confirm is up does nothing.
        assertEquals(
            FinishTransition(FinishStep.Closed, FinishEffect.None),
            finishTransition(FinishStep.Closed, FinishAction.Save),
        )
        assertEquals(
            FinishTransition(FinishStep.ConfirmDiscard, FinishEffect.None),
            finishTransition(FinishStep.ConfirmDiscard, FinishAction.Save),
        )
    }

    @Test
    fun discardRequiresTheConfirmStep() {
        // Discard on the review only ASKS: nothing is deleted yet.
        val ask = finishTransition(FinishStep.Review, FinishAction.Discard)
        assertEquals(FinishTransition(FinishStep.ConfirmDiscard, FinishEffect.None), ask)
        // Cancel returns to the review with the run intact.
        assertEquals(
            FinishTransition(FinishStep.Review, FinishEffect.None),
            finishTransition(FinishStep.ConfirmDiscard, FinishAction.CancelDiscard),
        )
        // Only confirming from the confirm step deletes the run.
        assertEquals(
            FinishTransition(FinishStep.Closed, FinishEffect.DiscardRun),
            finishTransition(FinishStep.ConfirmDiscard, FinishAction.ConfirmDiscard),
        )
        // A confirm that arrives without the confirm step (skipped dialog, stale tap)
        // never discards.
        for (step in listOf(FinishStep.Closed, FinishStep.Review)) {
            assertNotEquals(
                "ConfirmDiscard from $step must not discard",
                FinishEffect.DiscardRun,
                finishTransition(step, FinishAction.ConfirmDiscard).effect,
            )
        }
    }

    @Test
    fun aSecondStopWhileReviewingIsANoOp() {
        assertEquals(
            FinishTransition(FinishStep.Review, FinishEffect.None),
            finishTransition(FinishStep.Review, FinishAction.Stop),
        )
    }

    @Test
    fun shortRunIsFlaggedOnDistanceOrMovingTime() {
        // The review notes a short run inline (no dialog that could drop the effort and
        // run-type picks) when EITHER the distance or the moving time is under the bar.
        assertTrue(isShortRun(km = 0.2, movingSec = 600))
        assertTrue(isShortRun(km = 2.0, movingSec = 120))
        assertFalse(isShortRun(km = MIN_RUN_KM, movingSec = MIN_RUN_SEC))
        assertFalse(isShortRun(km = 5.0, movingSec = 1_800))
    }

    @Test
    fun runTypeIsPreselectedFromThePlannedSession() {
        // A long run's band is "easy", so the band alone would read Steady: the
        // planned session type wins when it names a chip.
        assertEquals(WorkoutType.LongRun, workoutTypeForPlan("Run(LongRun)", "easy"))
        assertEquals(WorkoutType.Recovery, workoutTypeForPlan("Run(Recovery)", "recovery"))
        assertEquals(WorkoutType.Tempo, workoutTypeForPlan("Run(Tempo)", "threshold"))
        assertEquals(WorkoutType.Interval, workoutTypeForPlan("Run(Hills)", "hill effort"))
        assertEquals(WorkoutType.Easy, workoutTypeForPlan("Run(Easy)", "easy"))
        assertEquals(WorkoutType.Easy, workoutTypeForPlan(null, "easy"))
        assertEquals(WorkoutType.Steady, workoutTypeForPlan(null, "marathon pace"))
        // Otherwise the band decides; no band = a free run, left untagged.
        assertEquals(WorkoutType.Tempo, workoutTypeForPlan(null, "threshold"))
        assertEquals(null, workoutTypeForPlan(null, null))
    }
}
