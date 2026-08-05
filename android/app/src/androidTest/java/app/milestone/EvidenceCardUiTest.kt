package app.milestone

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test

/**
 * Instrumented Compose UI tests for [EvidenceCard], the honesty-invariant card
 * that every recommendation renders through. Owner ruling 2026-07-31: the
 * collapsed face shows ONLY the summary + the always-visible SAFETY/CONTESTED
 * markers + a single unified "?" disclosure; the grade badge, confidence figure
 * and citation ALL live behind the "?". These tests assert that contract, no
 * grade/confidence/citation leaks onto the face, but the SAFETY/CONTESTED
 * honesty signals never hide.
 *
 * Runs on device/emulator (needs the real theme + composition locals), so it
 * complements the JVM-only wire-shape tests rather than duplicating them.
 */
class EvidenceCardUiTest {

    @get:Rule
    val rule = createComposeRule()

    private fun setCard(
        summary: String = "Add 2.5 kg to next Back Squat top set",
        grade: String = "Moderate",
        citation: String = "STR-PROG-001",
        confidence: Float = 0.72f,
        safetyCritical: Boolean = false,
        contested: Boolean = false,
        section: String? = null,
    ) = rule.setContent {
        MilestoneTheme {
            EvidenceCard(
                summary = summary,
                grade = grade,
                citation = citation,
                confidence = confidence,
                safetyCritical = safetyCritical,
                contested = contested,
                section = section,
            )
        }
    }

    @Test
    fun summaryOnFaceGradeBehindDisclosure() {
        setCard(summary = "Deload week: cut volume 40%", grade = "Strong")
        rule.onNodeWithText("Deload week: cut volume 40%").assertIsDisplayed()
        // Grade badge is NO LONGER on the collapsed face: it lives behind the "?".
        rule.onNodeWithText("STRONG").assertDoesNotExist()
        rule.onNodeWithText("?").performClick()
        rule.onNodeWithText("STRONG").assertIsDisplayed()
    }

    @Test
    fun safetyStaysOnFaceContestedMovesBehindDisclosure() {
        setCard(safetyCritical = true, contested = true)
        // SAFETY is a HARD-RULE signal: never hidden.
        rule.onNodeWithText("SAFETY").assertIsDisplayed()
        // CONTESTED is an evidence-quality tag (declutter, owner 2026-07-31) → it
        // lives behind the "?" now, not on the collapsed face.
        rule.onNodeWithText("CONTESTED").assertDoesNotExist()
        rule.onNodeWithText("?").performClick()
        rule.onNodeWithText("CONTESTED").assertIsDisplayed()
    }

    @Test
    fun gradeAndCitationHiddenUntilDisclosureTap() {
        setCard(citation = "STR-PROG-001", confidence = 0.72f, grade = "Moderate")
        // Collapsed: grade badge + citation are BOTH behind the disclosure now;
        // the unified trigger is a "?" (not the old "why?"/"less" text links).
        rule.onNodeWithText("MODERATE").assertDoesNotExist()
        rule.onNodeWithText("STR-PROG-001").assertDoesNotExist()
        rule.onNodeWithText("why?").assertDoesNotExist()
        rule.onNodeWithText("?").assertIsDisplayed()

        rule.onNodeWithText("?").performClick()

        // Expanded: grade badge + citation revealed.
        rule.onNodeWithText("MODERATE").assertIsDisplayed()
        rule.onNodeWithText("STR-PROG-001").assertIsDisplayed()
    }
}
