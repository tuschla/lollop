package app.milestone

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.AccessibilityManager
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch

/**
 * One transient confirmation ("Run deleted", "Set updated") with an optional action
 * ("Undo", "View"). [onAction] runs on the main thread when the action is tapped and
 * must only do process-safe work (send an event through [EventDispatcher], flip
 * process-scoped state): the composition that posted it may be gone by then. When
 * [details] is non-empty, the action (labelled e.g. "Details") opens a dialog listing
 * them, one per line, under [detailsTitle]. [long] keeps a message that takes reading
 * (one that names files) up longer. Prefer named arguments.
 */
internal data class UiMessage(
    val text: String,
    val actionLabel: String? = null,
    val details: List<String> = emptyList(),
    val detailsTitle: String? = null,
    val long: Boolean = false,
    val onAction: (() -> Unit)? = null,
)

/**
 * The shell's one confirmation channel. Process-scoped and buffered, so a message
 * posted just before a rotation or a sheet closing is still shown by the next
 * [UiMessageHost] that collects. Messages queue and show one at a time, always in
 * the most recently composed host: a modal sheet is its own window, so a host
 * inside it is the only one the user can see while it is open.
 */
internal object UiMessages {
    private val channel = Channel<UiMessage>(capacity = Channel.BUFFERED)

    fun post(message: UiMessage) {
        channel.trySend(message)
    }

    internal val messages = channel.receiveAsFlow()

    internal val hosts = mutableStateListOf<Any>()

    /** Drops queued messages; for a fresh process-scoped state between QA scenarios. */
    fun clear() {
        while (channel.tryReceive().isSuccess) Unit
    }
}

/**
 * A run History should open (entry_id, observed_at), e.g. from an import's "View"
 * action. The app scaffold switches to the History tab while one is pending;
 * History opens the run's sheet and [consume]s it.
 */
internal object HistoryFocus {
    private val state = MutableStateFlow<Pair<Long, Long>?>(null)
    val pending: StateFlow<Pair<Long, Long>?> = state.asStateFlow()

    fun request(entryId: Long, observedAt: Long) {
        state.value = entryId to observedAt
    }

    fun consume() {
        state.value = null
    }
}

/** How long a message with an action ("Undo", "View") stays up: Material's long
 *  snackbar duration, so the action is still there after a sheet's close animation
 *  and a glance at the result; one that takes reading stays longer. The user's
 *  accessibility timeout preference stretches both. */
private const val ACTION_MESSAGE_MILLIS = 10_000L
private const val LONG_ACTION_MESSAGE_MILLIS = 15_000L

/** How long a message with an action stays up under [a11y]'s recommended timeout. */
internal fun actionMessageMillis(long: Boolean, a11y: AccessibilityManager?): Long {
    val base = if (long) LONG_ACTION_MESSAGE_MILLIS else ACTION_MESSAGE_MILLIS
    return a11y?.calculateRecommendedTimeoutMillis(
        base, containsIcons = false, containsText = true, containsControls = true,
    ) ?: base
}

/**
 * Renders [UiMessages] as snackbars. One per screen tree, plus one inside any modal
 * sheet that posts messages while it stays open; only the most recently composed host
 * collects. A host that leaves mid-message hands the message back to the queue, so the
 * next host shows it in full.
 */
@Composable
internal fun UiMessageHost(modifier: Modifier = Modifier) {
    val host = remember { SnackbarHostState() }
    var shownDetails by rememberSaveable { mutableStateOf<List<String>?>(null) }
    var shownDetailsTitle by rememberSaveable { mutableStateOf<String?>(null) }
    val token = remember { Any() }
    DisposableEffect(token) {
        UiMessages.hosts += token
        onDispose { UiMessages.hosts -= token }
    }
    val isTop by remember { derivedStateOf { UiMessages.hosts.lastOrNull() === token } }
    val a11y = LocalAccessibilityManager.current
    LaunchedEffect(host, isTop) {
        if (!isTop) return@LaunchedEffect
        UiMessages.messages.collect { m ->
            var handled = false
            try {
                val result = if (m.actionLabel != null) {
                    val millis = actionMessageMillis(m.long, a11y)
                    coroutineScope {
                        val timeout = launch {
                            delay(millis)
                            host.currentSnackbarData?.dismiss()
                        }
                        host.showSnackbar(m.text, m.actionLabel, duration = SnackbarDuration.Indefinite)
                            .also { timeout.cancel() }
                    }
                } else {
                    host.showSnackbar(m.text, duration = if (m.long) SnackbarDuration.Long else SnackbarDuration.Short)
                }
                handled = true
                if (result == SnackbarResult.ActionPerformed) {
                    if (m.details.isNotEmpty()) {
                        shownDetailsTitle = m.detailsTitle
                        shownDetails = ArrayList(m.details)
                    }
                    m.onAction?.invoke()
                }
            } finally {
                if (!handled) UiMessages.post(m)
            }
        }
    }
    SnackbarHost(host, modifier)
    shownDetails?.let { lines ->
        AlertDialog(
            onDismissRequest = { shownDetails = null },
            shape = RoundedCornerShape(Space.Card.dp),
            title = shownDetailsTitle?.let { t -> @Composable { Text(t) } },
            text = {
                Column(
                    modifier = Modifier.verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    lines.forEach { Text(it, color = OnBgBody, style = Type.Body) }
                }
            },
            confirmButton = {
                TextButton(onClick = { shownDetails = null }) { Text(stringResource(android.R.string.ok)) }
            },
        )
    }
}
