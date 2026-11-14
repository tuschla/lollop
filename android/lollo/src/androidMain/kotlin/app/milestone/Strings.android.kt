package app.milestone

import android.content.res.Resources
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import org.jetbrains.compose.resources.PluralStringResource
import org.jetbrains.compose.resources.StringArrayResource
import org.jetbrains.compose.resources.StringResource

actual typealias PlatformResources = Resources

// Without arguments Android returns the text unformatted (no `%%` → `%`), so
// the calls keep the two forms apart as the callers did.
internal actual fun PlatformResources.getString(resource: StringResource, vararg formatArgs: Any): String {
    val id = androidStringId(resource.key)
    return if (formatArgs.isEmpty()) getString(id) else getString(id, *formatArgs)
}

internal actual fun PlatformResources.getQuantityString(
    resource: PluralStringResource,
    quantity: Int,
    vararg formatArgs: Any,
): String {
    val id = androidPluralsId(resource.key)
    return if (formatArgs.isEmpty()) getQuantityString(id, quantity) else getQuantityString(id, quantity, *formatArgs)
}

internal actual fun PlatformResources.getStringArray(resource: StringArrayResource): List<String> =
    getStringArray(androidArrayId(resource.key)).asList()

@Composable
internal actual fun currentResources(): PlatformResources = LocalResources.current

@Composable
internal actual fun platformOkLabel(): String = stringResource(android.R.string.ok)
