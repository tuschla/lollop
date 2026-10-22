package app.milestone

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The persisted theme name (`AppSettings`, key `app_theme`) resolves through
 * [AppTheme.from]: current names round-trip, the names retired by the 2026-10-10
 * redesign land on the theme that replaced each, anything else on the default.
 */
class AppThemeTest {

    @Test
    fun currentNames_roundTrip() {
        for (t in AppTheme.entries) assertEquals(t, AppTheme.from(t.name))
    }

    @Test
    fun retiredNames_migrateToTheirSuccessor() {
        assertEquals(AppTheme.Rose, AppTheme.from("Beton"))
        assertEquals(AppTheme.Meadow, AppTheme.from("Werkstatt"))
        assertEquals(AppTheme.Iris, AppTheme.from("Signal"))
    }

    @Test
    fun missingOrUnknownName_fallsBackToPaper() {
        assertEquals(AppTheme.Paper, AppTheme.from(null))
        assertEquals(AppTheme.Paper, AppTheme.from("Neon"))
    }

    @Test
    fun enumOrder_isRoseMeadowIrisPaper() {
        assertEquals(
            listOf(AppTheme.Rose, AppTheme.Meadow, AppTheme.Iris, AppTheme.Paper),
            AppTheme.entries.toList(),
        )
    }
}

/**
 * Owner ruling 2026-10-17: a fresh install starts on Paper with the system accent
 * ON; switching to Rose/Meadow/Iris turns it OFF, back to Paper turns it ON, and
 * an install that predates the ruling keeps what it rendered.
 */
@RunWith(RobolectricTestRunner::class)
class AppSettingsThemeTest {
    private val ctx: Context = ApplicationProvider.getApplicationContext()
    private val prefs get() = ctx.getSharedPreferences("theme", Context.MODE_PRIVATE)
    private val log get() = File(ctx.filesDir, "event-log.ndjson")

    @Before
    fun reset() {
        prefs.edit().clear().commit()
        log.delete()
    }

    @Test
    fun freshInstall_bootsIntoPaperWithSystemAccent_andKeepsItOnReload() {
        AppSettings.load(ctx)
        assertEquals(AppTheme.Paper, AppSettings.theme.value)
        assertTrue(AppSettings.dynamicAccent.value)
        log.writeText("")
        AppSettings.load(ctx)
        assertEquals(AppTheme.Paper, AppSettings.theme.value)
        assertTrue(AppSettings.dynamicAccent.value)
    }

    @Test
    fun existingInstallWithoutSavedTheme_keepsRoseAndItsAccent() {
        log.writeText("")
        AppSettings.load(ctx)
        assertEquals(AppTheme.Rose, AppSettings.theme.value)
        assertFalse(AppSettings.dynamicAccent.value)

        reset()
        log.writeText("")
        prefs.edit().putBoolean("dynamic_accent", true).commit()
        AppSettings.load(ctx)
        assertEquals(AppTheme.Rose, AppSettings.theme.value)
        assertTrue(AppSettings.dynamicAccent.value)
    }

    @Test
    fun savedChoice_survivesTheNewDefaults() {
        prefs.edit().putString("app_theme", "Paper").putBoolean("dynamic_accent", false).commit()
        AppSettings.load(ctx)
        assertEquals(AppTheme.Paper, AppSettings.theme.value)
        assertFalse(AppSettings.dynamicAccent.value)
    }

    @Test
    fun switchingTheme_setsTheSystemAccent_butTheToggleStaysTheUsers() {
        AppSettings.load(ctx)
        for (tinted in listOf(AppTheme.Rose, AppTheme.Meadow, AppTheme.Iris)) {
            AppSettings.setTheme(ctx, AppTheme.Paper)
            AppSettings.setTheme(ctx, tinted)
            assertFalse("$tinted turns the system accent off", AppSettings.dynamicAccent.value)
        }
        AppSettings.setTheme(ctx, AppTheme.Paper)
        assertTrue(AppSettings.dynamicAccent.value)

        AppSettings.setDynamicAccent(ctx, false)
        AppSettings.setTheme(ctx, AppTheme.Paper)
        assertFalse("re-picking the current theme keeps the user's toggle", AppSettings.dynamicAccent.value)
        AppSettings.setTheme(ctx, AppTheme.Iris)
        AppSettings.setDynamicAccent(ctx, true)
        AppSettings.load(ctx)
        assertEquals(AppTheme.Iris, AppSettings.theme.value)
        assertTrue("a flip after switching persists", AppSettings.dynamicAccent.value)
    }
}
