package app.milestone

import androidx.compose.runtime.Composable

/**
 * The run import's file picker: the returned launcher opens the platform's picker for
 * one or many files of any type. When at least one file was picked it calls
 * [onPicked] (a sheet closes itself there) and then imports them as one batch
 * ([launchRunImport], process-scoped). Android: the system content picker
 * (`GetMultipleContents` for any MIME type); iOS: the Files document picker.
 */
@Composable
internal expect fun rememberRunImport(onPicked: () -> Unit = {}): () -> Unit
