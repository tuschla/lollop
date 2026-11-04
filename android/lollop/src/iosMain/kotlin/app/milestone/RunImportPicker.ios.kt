package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState

@Composable
internal actual fun rememberRunImport(onPicked: () -> Unit): () -> Unit {
    val currentOnPicked by rememberUpdatedState(onPicked)
    return remember {
        {
            presentDocumentPicker(multiple = true) { urls ->
                if (urls.isNotEmpty()) {
                    currentOnPicked()
                    importRunFiles(urls)
                }
            }
        }
    }
}
