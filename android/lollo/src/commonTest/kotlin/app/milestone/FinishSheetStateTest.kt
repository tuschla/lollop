package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

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
        val ask = finishTransition(FinishStep.Review, FinishAction.Discard)
        assertEquals(FinishTransition(FinishStep.ConfirmDiscard, FinishEffect.None), ask)
        assertEquals(
            FinishTransition(FinishStep.Review, FinishEffect.None),
            finishTransition(FinishStep.ConfirmDiscard, FinishAction.CancelDiscard),
        )
        assertEquals(
            FinishTransition(FinishStep.Closed, FinishEffect.DiscardRun),
            finishTransition(FinishStep.ConfirmDiscard, FinishAction.ConfirmDiscard),
        )
        // A confirm that arrives without the confirm step (skipped dialog, stale tap)
        // never discards.
        for (step in listOf(FinishStep.Closed, FinishStep.Review)) {
            assertNotEquals(
                FinishEffect.DiscardRun,
                finishTransition(step, FinishAction.ConfirmDiscard).effect,
                "ConfirmDiscard from $step must not discard",
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
        assertFalse(isShortRun(km = CoreConstants.MIN_RUN_KM, movingSec = CoreConstants.MIN_RUN_SEC.toLong()))
        assertFalse(isShortRun(km = 5.0, movingSec = 1_800))
    }
}
