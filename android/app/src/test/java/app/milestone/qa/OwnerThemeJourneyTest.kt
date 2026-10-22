package app.milestone.qa

import android.content.Context
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isSelectable
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.AppTheme
import app.milestone.EventDispatcher
import app.milestone.R
import java.io.File
import java.util.Locale
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The appearance defaults and the theme ↔ system-accent coupling, driven through
 * Profile › Appearance on the real app: a true fresh install (no event log, guided
 * setup walked to Finish) starts on Paper with the system accent ON; picking Rose,
 * Iris or Meadow turns the accent OFF and Paper turns it back ON; re-picking the
 * current theme and leaving/re-entering Profile keep the user's own flip; the
 * choice survives a relaunch. An install already in use that never saved a theme
 * keeps Rose with the accent OFF.
 */
@RunWith(RobolectricTestRunner::class)
class OwnerThemeJourneyTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    /** A theme swatch card: a radio-button card named by the theme's label. */
    private fun swatch(t: AppTheme) = hasText(t.label) and isSelectable()

    /** The "Use system accent color" row (the whole row is the switch). */
    private val accent: SemanticsMatcher get() = hasText(str(R.string.main_use_system_accent)) and isToggleable()

    /** A bottom-nav destination (selectable, so "Today" never matches body copy). */
    private fun tab(@androidx.annotation.StringRes label: Int) = hasText(str(label)) and isSelectable()

    private fun scrollTo(m: SemanticsMatcher) {
        rule.onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(m)
        rule.waitForIdle()
    }

    private fun openProfile() {
        rule.onNode(tab(R.string.main_dest_profile)).performClick()
        rule.waitForIdle()
    }

    private fun pick(t: AppTheme) {
        scrollTo(swatch(t))
        rule.onNode(swatch(t)).performClick()
        rule.waitForIdle()
    }

    private fun flipAccent() {
        scrollTo(accent)
        rule.onNode(accent).performClick()
        rule.waitForIdle()
    }

    /** Exactly [selected] is the picked swatch, and the accent switch reads [accentOn]. */
    private fun assertAppearance(selected: AppTheme, accentOn: Boolean) {
        scrollTo(swatch(selected))
        AppTheme.entries.forEach { t ->
            if (t == selected) rule.onNode(swatch(t)).assertIsSelected() else rule.onNode(swatch(t)).assertIsNotSelected()
        }
        scrollTo(accent)
        if (accentOn) rule.onNode(accent).assertIsOn() else rule.onNode(accent).assertIsOff()
    }

    @Test
    fun freshInstall_startsOnPaperWithSystemAccent_andThemeChoiceCouplesTheAccent() {
        // A true first run: no event log, no saved theme, guided setup on screen.
        assertFalse(File(ctx.filesDir, "event-log.ndjson").exists())
        rule.launchAppExpectingSetup()
        GuidedSetupWalk.walk(rule)
        rule.onNode(hasText(str(R.string.setup_btn_finish)) and hasClickAction()).performClick()
        rule.waitUntil(30_000) {
            rule.onAllNodesWithText(str(R.string.main_dest_profile)).fetchSemanticsNodes().isNotEmpty() &&
                EventDispatcher.model.value?.profile != null
        }
        rule.waitForIdle()
        openProfile()

        // The fresh-install default.
        assertAppearance(AppTheme.Paper, accentOn = true)

        // Rose / Iris / Meadow each turn the accent OFF, even after the user turned it
        // back on; Paper turns it ON again.
        pick(AppTheme.Rose)
        assertAppearance(AppTheme.Rose, accentOn = false)
        flipAccent()
        assertAppearance(AppTheme.Rose, accentOn = true)
        pick(AppTheme.Iris)
        assertAppearance(AppTheme.Iris, accentOn = false)
        flipAccent()
        pick(AppTheme.Meadow)
        assertAppearance(AppTheme.Meadow, accentOn = false)
        pick(AppTheme.Paper)
        assertAppearance(AppTheme.Paper, accentOn = true)

        // Re-picking the current theme leaves the user's toggle alone.
        flipAccent()
        pick(AppTheme.Paper)
        assertAppearance(AppTheme.Paper, accentOn = false)

        // Rose with the accent the user turned ON stays so across leaving Profile and
        // coming back.
        pick(AppTheme.Rose)
        assertAppearance(AppTheme.Rose, accentOn = false)
        flipAccent()
        assertAppearance(AppTheme.Rose, accentOn = true)
        rule.onNode(tab(R.string.main_dest_today)).performClick()
        rule.waitForIdle()
        openProfile()
        assertAppearance(AppTheme.Rose, accentOn = true)

        // And across a relaunch: the in-memory flows are knocked back to the
        // fresh-install values, so only the saved preferences can restore the choice.
        QaHarness.teardown()
        AppSettings.theme.value = AppTheme.Paper
        AppSettings.dynamicAccent.value = false
        rule.launchApp()
        openProfile()
        assertAppearance(AppTheme.Rose, accentOn = true)
    }

    @Test
    fun existingInstallWithoutASavedTheme_keepsRoseWithTheAccentOff() {
        // In use before Paper became the default: an event log, no theme preferences.
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        openProfile()
        assertAppearance(AppTheme.Rose, accentOn = false)
    }
}
