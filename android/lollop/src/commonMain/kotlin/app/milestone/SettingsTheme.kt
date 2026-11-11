package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

/** [LollopTheme] as the user's settings ([AppSettings]) choose it: the root of
 *  every host (Android's `MainActivity`, iOS's root view, the UI tests). */
@Composable
fun MilestoneTheme(content: @Composable () -> Unit) {
    val dynamicColor by AppSettings.dynamicAccent.collectAsState()
    val appTheme by AppSettings.theme.collectAsState()
    val darkMode by AppSettings.darkMode.collectAsState()
    LollopTheme(appTheme, darkMode, dynamicColor, content)
}
