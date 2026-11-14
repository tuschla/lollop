package app.milestone

import kotlin.test.Test
import kotlin.test.assertEquals

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
}
