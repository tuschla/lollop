package app.milestone

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Card-copy helpers behind the "?" panel: which jargon terms get a glossary
 * link, and that internal KB claim ids never reach the user.
 */
class EvidenceTextTest {

    @Test
    fun prescriptionJargon_getsGlossaryLinks() {
        assertEquals(listOf("rir"), glossaryKeysIn("Romanian deadlift: 3×5 · RIR 3"))
        assertEquals(listOf("e1rm"), glossaryKeysIn("Back Squat: 4×3 @ 117.5 kg · 85% e1RM"))
        assertEquals(listOf("rpe"), glossaryKeysIn("Bench Press: 3×5 @ RPE 8"))
        assertEquals(listOf("hrmax"), glossaryKeysIn("Easy run: 40 min · 70% HRmax"))
    }

    @Test
    fun oneRmTerm_doesNotFireInsideE1rm() {
        assertEquals(listOf("e1rm"), glossaryKeysIn("85% e1RM"))
        assertEquals(listOf("1rm"), glossaryKeysIn("80% 1RM"))
    }

    @Test
    fun plainCopy_getsNoGlossaryLinks() {
        assertEquals(emptyList<String>(), glossaryKeysIn("Rest day. Recovery is part of the plan."))
        // Substrings of ordinary words never match an acronym.
        assertEquals(emptyList<String>(), glossaryKeysIn("Stir the porridge, then ripen"))
    }

    @Test
    fun claimIds_areStrippedFromDetailLines() {
        assertEquals("load −10%: readiness", stripClaimIds("load −10%: readiness (AUTOREG-RIR-001)"))
        assertEquals("modify & monitor: pain", stripClaimIds("modify & monitor: pain (SAFE-PAIN-001)"))
        assertEquals("load −10%: readiness", stripClaimIds("load −10%: readiness (-)"))
    }

    @Test
    fun ordinaryParentheticals_survive() {
        assertEquals("Top set (RPE 8)", stripClaimIds("Top set (RPE 8)"))
        assertEquals("Last: 80 kg × 5 (Tue)", stripClaimIds("Last: 80 kg × 5 (Tue)"))
    }
}
