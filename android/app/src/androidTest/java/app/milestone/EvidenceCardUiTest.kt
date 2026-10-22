package app.milestone

import androidx.annotation.StringRes
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import java.util.Locale
import org.junit.Rule
import org.junit.Test

/**
 * Instrumented Compose UI tests for [EvidenceCard], the honesty-invariant card
 * that every recommendation renders through. The card contract: the collapsed
 * face shows ONLY the summary, the SAFETY marker (never hidden) and a single "?"
 * disclosure; the grade badge, CONTESTED marker, confidence figure and citation
 * ALL live behind the "?".
 *
 * Runs on device/emulator (needs the real theme + composition locals), so it
 * complements the JVM-only wire-shape tests rather than duplicating them.
 */
class EvidenceCardUiTest {

    @get:Rule
    val rule = createComposeRule()

    private val ctx = InstrumentationRegistry.getInstrumentation().targetContext

    private fun str(@StringRes id: Int, vararg args: Any): String = ctx.getString(id, *args)

    /** The grade badge text: the human label, uppercased like every chip. */
    private fun badge(@StringRes id: Int): String = str(id).uppercase(Locale.US)

    private fun setCard(
        summary: String = DEFAULT_SUMMARY,
        grade: String = "Moderate",
        citation: String = "STR-PROG-001",
        confidence: Float = 0.72f,
        safetyCritical: Boolean = false,
        contested: Boolean = false,
        why: WhyView? = null,
    ) = rule.setContent {
        MilestoneTheme {
            EvidenceCard(
                summary = summary,
                grade = grade,
                citation = citation,
                confidence = confidence,
                safetyCritical = safetyCritical,
                contested = contested,
                why = why,
            )
        }
    }

    /** Tap the card's "?" disclosure, found by its spoken label (the card names
     *  its subject after the summary's first clause). */
    private fun openDetails(summary: String = DEFAULT_SUMMARY) {
        rule.onNodeWithContentDescription(str(R.string.main_details_subject, summary)).performClick()
    }

    @Test
    fun summaryOnFaceGradeBehindDisclosure() {
        val summary = "Deload week: cut volume 40%"
        setCard(summary = summary, grade = "Strong")
        rule.onNodeWithText(summary).assertIsDisplayed()
        rule.onNodeWithText(badge(R.string.main_grade_strong)).assertDoesNotExist()
        openDetails(summary)
        rule.onNodeWithText(badge(R.string.main_grade_strong)).assertIsDisplayed()
        // The open disclosure announces how to close it.
        rule.onNodeWithContentDescription(str(R.string.main_hide_details_subject, summary)).assertIsDisplayed()
    }

    @Test
    fun safetyStaysOnFaceContestedMovesBehindDisclosure() {
        setCard(safetyCritical = true, contested = true)
        // SAFETY is a HARD-RULE signal: never hidden.
        rule.onNodeWithText(str(R.string.main_chip_safety)).assertIsDisplayed()
        // CONTESTED is an evidence-quality tag: it lives behind the "?".
        rule.onNodeWithText(str(R.string.main_chip_contested)).assertDoesNotExist()
        openDetails()
        rule.onNodeWithText(str(R.string.main_chip_contested)).assertIsDisplayed()
    }

    @Test
    fun gradeAndCitationHiddenUntilDisclosureTap() {
        setCard(citation = "STR-PROG-001", confidence = 0.72f, grade = "Moderate")
        rule.onNodeWithText(badge(R.string.main_grade_moderate)).assertDoesNotExist()
        rule.onNodeWithText("STR-PROG-001").assertDoesNotExist()

        openDetails()

        rule.onNodeWithText(badge(R.string.main_grade_moderate)).assertIsDisplayed()
        rule.onNodeWithText("STR-PROG-001").assertIsDisplayed()
    }

    @Test
    fun whyBlockRendersBasisGradeNoteWithConfidenceAndImprovement() {
        val why = WhyView(
            basis = "Two randomized trials in trained lifters",
            grade_note = "Consistent effect, small samples",
            improves = "Four more logged sessions",
        )
        setCard(confidence = 0.72f, why = why)
        rule.onNodeWithText(str(R.string.main_why_based_on)).assertDoesNotExist()

        openDetails()

        rule.onNodeWithText(str(R.string.main_why_based_on)).assertIsDisplayed()
        rule.onNodeWithText(why.basis).assertIsDisplayed()
        rule.onNodeWithText(str(R.string.main_why_this_grade)).assertIsDisplayed()
        rule.onNodeWithText(str(R.string.main_grade_line_confidence, why.grade_note, 72)).assertIsDisplayed()
        rule.onNodeWithText(str(R.string.main_why_to_improve)).assertIsDisplayed()
        rule.onNodeWithText(why.improves).assertIsDisplayed()
    }

    private companion object {
        const val DEFAULT_SUMMARY = "Add 2.5 kg to next Back Squat top set"
    }
}
