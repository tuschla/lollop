package app.milestone.qa

import android.content.Context
import androidx.test.core.app.ApplicationProvider

/** A string a library the UI is built from (Compose UI, Material 3) defines,
 *  by resource [name], in the current locale; null when none defines it. */
fun libraryString(name: String): String? {
    val ctx = ApplicationProvider.getApplicationContext<Context>()
    return listOf(ctx.packageName, "app.milestone", "androidx.compose.material3", "androidx.compose.ui").firstNotNullOfOrNull { pkg ->
        ctx.resources.getIdentifier(name, "string", pkg).takeIf { it != 0 }
    }?.let { ctx.getString(it) }
}

/** The spoken name of a modal bottom sheet's scrim, what a tap outside the sheet
 *  hits: Compose UI's `close_sheet`, which Material 3's sheet speaks. */
val sheetScrimLabel: String
    get() = checkNotNull(libraryString("close_sheet")) { "no library defines close_sheet: the sheet scrim's name moved" }
