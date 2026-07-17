package de.tuschla.fitnessanlage

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test

/**
 * Instrumented Compose UI tests for [EvidenceCard], the honesty-invariant card
 * that every recommendation renders through. These assert the always-visible
 * signals (grade chip, SAFETY/CONTESTED markers) stay on screen and that the
 * citation/confidence only appear behind the disclosure tap, the visual
 * contract that keeps a weak or contested claim from masquerading as strong.
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
        FitnessAnlageTheme {
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
    fun summaryAndGradeChipAlwaysVisible() {
        setCard(summary = "Deload week: cut volume 40%", grade = "Strong")
        rule.onNodeWithText("Deload week: cut volume 40%").assertIsDisplayed()
        rule.onNodeWithText("Strong").assertIsDisplayed()
    }

    @Test
    fun safetyAndContestedChipsShowWithoutTapping() {
        setCard(safetyCritical = true, contested = true)
        // Honesty invariant: these are never hidden behind the disclosure.
        rule.onNodeWithText("SAFETY").assertIsDisplayed()
        rule.onNodeWithText("CONTESTED").assertIsDisplayed()
    }

    @Test
    fun citationHiddenUntilDisclosureTap() {
        setCard(citation = "STR-PROG-001", confidence = 0.72f)
        // Collapsed: reference detail absent, affordance invites the tap.
        rule.onNodeWithText("STR-PROG-001").assertDoesNotExist()
        rule.onNodeWithText("conf 0.72").assertDoesNotExist()
        rule.onNodeWithText("why?").assertIsDisplayed()

        rule.onNodeWithText("why?").performClick()

        // Expanded: citation + confidence revealed, label flips to "less".
        rule.onNodeWithText("STR-PROG-001").assertIsDisplayed()
        rule.onNodeWithText("conf 0.72").assertIsDisplayed()
        rule.onNodeWithText("less").assertIsDisplayed()
    }
}
