package app.milestone

import androidx.compose.runtime.Composable
import org.jetbrains.compose.resources.PluralStringResource
import org.jetbrains.compose.resources.StringArrayResource
import org.jetbrains.compose.resources.StringResource

// Text outside composition: the builders shared by screens and platform code
// (spoken summaries, notification lines, the coach export), which can't call the
// @Composable `stringResource`. Resources are named by the CMP handles
// (`Res.string.x`); composables use CMP's `stringResource` / `pluralStringResource`.
//
// Android: `PlatformResources` is android.content.res.Resources and each handle
// reads through the `R` id of the same name (AndroidResourceIds.kt, generated),
// so the text is exactly what getString / getQuantityString return for the
// Resources passed in (the Latin-digit copy a notification builds, AppLocale.kt).
// iOS: CMP's resources, read synchronously.

/** A source of text resources: Android's `Resources`; on iOS the app's CMP resources. */
expect open class PlatformResources

/** [resource]'s text; with [formatArgs], formatted (`%1$s`, `%1$d`). */
internal expect fun PlatformResources.getString(resource: StringResource, vararg formatArgs: Any): String

/** [resource]'s text for [quantity]; with [formatArgs], formatted. */
internal expect fun PlatformResources.getQuantityString(
    resource: PluralStringResource,
    quantity: Int,
    vararg formatArgs: Any,
): String

/** [resource]'s items. */
internal expect fun PlatformResources.getStringArray(resource: StringArrayResource): List<String>

/** The resources the current composition reads (Android: `LocalResources`). */
@Composable
internal expect fun currentResources(): PlatformResources

/** The platform's own label for a dialog's OK button (Android: `android.R.string.ok`,
 *  in the device's language). */
@Composable
internal expect fun platformOkLabel(): String
