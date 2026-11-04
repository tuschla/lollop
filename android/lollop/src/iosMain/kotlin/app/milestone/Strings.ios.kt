package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import app.milestone.resources.Res
import app.milestone.resources.log_ok
import kotlinx.coroutines.runBlocking
import org.jetbrains.compose.resources.PluralStringResource
import org.jetbrains.compose.resources.ResourceEnvironment
import org.jetbrains.compose.resources.StringArrayResource
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.getPluralString as cmpPluralString
import org.jetbrains.compose.resources.getString as cmpString
import org.jetbrains.compose.resources.getStringArray as cmpStringArray
import org.jetbrains.compose.resources.getSystemResourceEnvironment
import org.jetbrains.compose.resources.rememberResourceEnvironment
import org.jetbrains.compose.resources.stringResource

/** CMP's resources in [environment] (language, region, theme, density). The
 *  reads block, as CMP's own `stringResource` does outside the web targets. */
actual open class PlatformResources internal constructor(internal val environment: ResourceEnvironment)

/** The resources for the system's current locale, for text built outside composition. */
internal fun systemResources(): PlatformResources = PlatformResources(getSystemResourceEnvironment())

internal actual fun PlatformResources.getString(resource: StringResource, vararg formatArgs: Any): String =
    runBlocking { cmpString(environment, resource, *formatArgs) }

internal actual fun PlatformResources.getQuantityString(
    resource: PluralStringResource,
    quantity: Int,
    vararg formatArgs: Any,
): String = runBlocking { cmpPluralString(environment, resource, quantity, *formatArgs) }

internal actual fun PlatformResources.getStringArray(resource: StringArrayResource): List<String> =
    runBlocking { cmpStringArray(environment, resource) }

@Composable
internal actual fun currentResources(): PlatformResources {
    val environment = rememberResourceEnvironment()
    return remember(environment) { PlatformResources(environment) }
}

/** UIKit has no stock "OK" string outside its own alerts; the app's (English) one. */
@Composable
internal actual fun platformOkLabel(): String = stringResource(Res.string.log_ok)
