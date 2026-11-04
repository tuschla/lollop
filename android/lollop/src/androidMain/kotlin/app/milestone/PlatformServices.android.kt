package app.milestone

import android.widget.Toast
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView

internal actual fun showNotice(ctx: PlatformContext, text: String, long: Boolean) {
    Toast.makeText(ctx, text, if (long) Toast.LENGTH_LONG else Toast.LENGTH_SHORT).show()
}

@Composable
internal actual fun KeepScreenOn() {
    val view = LocalView.current
    DisposableEffect(view) {
        val prev = view.keepScreenOn
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = prev }
    }
}

internal actual fun openAppSettings(ctx: PlatformContext) = ctx.openFirst(appDetailsIntent(ctx))
