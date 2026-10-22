package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import app.milestone.EventDispatcher
import app.milestone.R
import java.io.File
import java.util.Locale
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

/**
 * Drivers for the owner journeys ([OwnerJourneysTest], [OwnerProfileJourneysTest]):
 * every step is a real tap or typed key on the real screens, and the durable
 * event log is read back the way the core will replay it.
 */

/** Wait for app state the screen doesn't show (the model, the event log). The
 *  semantics fetch idles the main looper under Robolectric, so the core's reply
 *  can land while this polls. */
internal fun ComposeTestRule.waitForState(timeoutMs: Long = 10_000, cond: () -> Boolean) =
    waitUntil(timeoutMs) {
        onAllNodes(isRoot()).fetchSemanticsNodes()
        cond()
    }

/** Wait until [matcher] is on screen (within the visible window), then assert it. */
internal fun ComposeTestRule.waitUntilDisplayed(matcher: SemanticsMatcher, timeoutMs: Long = 10_000) {
    try {
        waitUntil(timeoutMs) { runCatching { onNode(matcher).assertIsDisplayed() }.isSuccess }
    } catch (_: ComposeTimeoutException) {
        // Say where it is instead (off screen, under the banner, not composed).
        val at = onAllNodes(matcher, useUnmergedTree = false).fetchSemanticsNodes().map { it.boundsInRoot }
        val root = onAllNodes(isRoot()).fetchSemanticsNodes().map { it.boundsInRoot }
        throw AssertionError("not displayed within ${timeoutMs}ms: $matcher; matching nodes at $at; roots $root")
    }
}

/** The durable event log, one wire event per line, as [app.milestone.Core] wrote it. */
internal fun journeyLog(): List<JsonElement> =
    File(ApplicationProvider.getApplicationContext<Context>().filesDir, "event-log.ndjson")
        .takeIf { it.exists() }?.readLines().orEmpty()
        .filter { it.isNotBlank() }
        .map { Json.parseToJsonElement(it) }

/** A wire event's variant name ("SetProfile", "ClearPlan"). */
internal val JsonElement.variant: String
    get() = when (this) {
        is JsonObject -> keys.first()
        is JsonPrimitive -> content
        else -> ""
    }

/** A single-key wire event's payload. */
internal val JsonElement.payload: JsonObject get() = jsonObject[variant]!!.jsonObject

/** The health toggle row for [label] (the whole row is the switch). */
internal fun toggleRow(label: String): SemanticsMatcher = hasText(label) and isToggleable()

/** The profile's "I'm ill right now" toggle row. */
internal fun illToggle(): SemanticsMatcher = toggleRow(str(R.string.setup_health_ill_label))

/** A Profile answer row (label + value in one tappable row) by its label. */
internal fun profileRow(label: String): SemanticsMatcher =
    hasText(label) and hasClickAction() and
        SemanticsMatcher("has a state description") { it.config.getOrNull(SemanticsProperties.StateDescription) != null }

/** [profileRow] whose inline picker is open. */
internal fun expandedRow(label: String): SemanticsMatcher =
    profileRow(label) and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, str(R.string.ui_cd_expanded))

/** Today's plan-state card shows a planned session (today's, the next one, or a
 *  rest day's preview of it). */
internal fun sessionShown(): SemanticsMatcher =
    hasText(str(R.string.main_todays_session).uppercase(Locale.US)) or
        hasText(str(R.string.main_next_session_overline).uppercase(Locale.US)) or
        hasText(str(R.string.main_rest_day_next_overline).uppercase(Locale.US))

/** A tappable node showing [text] (scrolled into view first). */
internal fun ComposeTestRule.tapText(text: String) {
    onNode(hasText(text) and hasClickAction()).performScrollTo().performClick()
    waitForIdle()
}

/** The wizard's Next (or, on the review, Finish) button. */
internal fun ComposeTestRule.setupNext() {
    onNode(hasText(str(R.string.setup_btn_next)) and hasClickAction()).performClick()
    waitForIdle()
}

/**
 * Walk the guided setup's shortest (Lifting) path from the first screen: Lifting ·
 * Strength · 3 days · a year or two in, then the About screen, where the walk
 * stops ([birthdayDigits] null and not [skipBirthday]), or picks that birthday
 * (or, with [skipBirthday], "Prefer not to say") and goes on to the health
 * screen (where [ill] turns "I'm ill right now" on) and the review.
 */
internal fun ComposeTestRule.walkLiftSetup(birthdayDigits: String?, ill: Boolean = false, skipBirthday: Boolean = false) {
    waitForIdle()
    tapText(str(R.string.setup_modality_lift_title))
    setupNext()
    tapText(str(R.string.setup_liftgoal_strength))
    setupNext()
    tapText("3")
    setupNext()
    tapText(str(R.string.setup_level_intermediate_title))
    setupNext()
    when {
        skipBirthday -> skipBirthday()
        birthdayDigits != null -> pickBirthday(str(R.string.setup_birthday_pick), birthdayDigits)
        else -> return
    }
    setupNext()
    if (ill) {
        onNode(illToggle()).performScrollTo().performClick()
        waitForIdle()
    }
    setupNext()
}

/** On the review: Finish, then wait for Today's scaffold with the profile applied. */
internal fun ComposeTestRule.finishSetup() {
    onNode(hasText(str(R.string.setup_btn_finish)) and hasClickAction()).performClick()
    waitForState(30_000) {
        onAllNodes(hasText(str(R.string.main_dest_history))).fetchSemanticsNodes().isNotEmpty() &&
            EventDispatcher.model.value?.profile != null
    }
}

/** The birthday's "Prefer not to say" (the first of the About step's two: the
 *  birthday sits above the sex question). */
internal fun ComposeTestRule.skipBirthday() {
    onAllNodes(hasText(str(R.string.setup_birthday_skip)) and hasClickAction())[0].performScrollTo().performClick()
    waitForIdle()
}

/**
 * Open the birthday picker from its pill (showing [pillText]: "Pick your
 * birthday" or the current "Born … · N"), type [digits] into its date field
 * (typed-date mode, US order MMDDYYYY) and confirm with OK. Returns whether the
 * dialog closed: a date out of range keeps it open (with its error) on the
 * current tree, or closes without a change on an older one; either way nothing
 * is picked, and an open dialog is cancelled here.
 */
internal fun ComposeTestRule.pickBirthday(pillText: String, digits: String): Boolean {
    // The pill is the radio beside "Prefer not to say"; the Profile row stating
    // the same birthday is not.
    val pill = hasText(pillText) and hasClickAction() and !hasAnyAncestor(isDialog()) and
        SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton)
    onNode(pill).performScrollTo().performClick()
    waitForIdle()
    val field = onAllNodes(hasSetTextAction() and hasAnyAncestor(isDialog()))[0]
    field.performTextClearance()
    field.performTextInput(digits)
    waitForIdle()
    onNode(hasText(str(R.string.log_ok)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
    waitForIdle()
    val open = onAllNodes(isDialog()).fetchSemanticsNodes().isNotEmpty()
    if (open) {
        onNode(hasText(str(R.string.main_cancel)) and hasClickAction() and hasAnyAncestor(isDialog())).performClick()
        waitForIdle()
    }
    return !open
}

/** Log › Morning check-in, answer sleep, soreness and mood with 3 and "Feeling
 *  ill?" with [illnessAnswer], then Save. */
internal fun ComposeTestRule.checkInIll(illnessAnswer: Int) {
    onNodeWithContentDescription(str(R.string.main_log)).performClick()
    onNodeWithText(str(R.string.main_morning_checkin_title)).performScrollTo().performClick()
    waitUntil(5_000) { onAllNodes(hasTestTag("checkin-sleep")).fetchSemanticsNodes().isNotEmpty() }
    for (key in listOf("sleep", "soreness", "mood")) {
        pickRating("checkin-$key", 3)
    }
    onNodeWithText(str(illnessAnswer), substring = true).performScrollTo().performClick()
    onNodeWithTag("editor-primary").performClick()
    waitForState { journeyLog().any { it.variant == "SubmitReadiness" && it.payload["signal"].toString().contains("Illness") } }
}

/** The core's resolve action labels on offer right now ("I'm better", "I've recovered"). */
internal fun resolveLabels(): List<String> =
    EventDispatcher.model.value?.hold_resolutions.orEmpty().map { it.action_label }.filter { it.isNotBlank() }

/** Tap the core's one illness action, then confirm it. */
internal fun ComposeTestRule.tapResolveAndConfirm(label: String) {
    // The banner is pinned (not in a scroll), and a Today card's action is in view.
    onNode(hasText(label) and hasClickAction()).performClick()
    waitForIdle()
    onNode(hasText(str(R.string.main_resolve_hold_confirm)) and hasClickAction()).performClick()
    waitForIdle()
}

/** Profile tab › unfold "Your plan answers" › scroll [target] into view. */
internal fun ComposeTestRule.openProfileAnswersAt(target: SemanticsMatcher) {
    selectTab(str(R.string.main_dest_profile))
    openPlanAnswers()
    try {
        onAllNodes(hasScrollToNodeAction())[0].performScrollToNode(target)
    } catch (e: AssertionError) {
        // Name what the screen does show, so a missing row is obvious.
        val texts = onAllNodes(SemanticsMatcher("has text") { SemanticsProperties.Text in it.config })
            .fetchSemanticsNodes().flatMap { n -> n.config[SemanticsProperties.Text].map { it.text } }
        throw AssertionError("${e.message}\nOn screen: $texts", e)
    }
    waitForIdle()
}
