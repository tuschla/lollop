package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import app.milestone.resources.Res
import app.milestone.resources.coach_export_action
import app.milestone.resources.coach_export_action_sub
import app.milestone.resources.live_updates_off_caption
import app.milestone.resources.live_updates_open_settings
import app.milestone.resources.log_unit_min
import app.milestone.resources.main_appearance
import app.milestone.resources.main_cancel
import app.milestone.resources.main_caption_keep_screen_on
import app.milestone.resources.main_caption_rest_alert
import app.milestone.resources.main_caption_route_maps
import app.milestone.resources.main_clear_all_confirm
import app.milestone.resources.main_clear_all_data
import app.milestone.resources.main_clear_all_message
import app.milestone.resources.main_clear_all_title
import app.milestone.resources.main_coach_reads_sub
import app.milestone.resources.main_danger_zone_sub
import app.milestone.resources.main_dark_mode_dark
import app.milestone.resources.main_dark_mode_light
import app.milestone.resources.main_dark_mode_pinned
import app.milestone.resources.main_dark_mode_system
import app.milestone.resources.main_evidence_references_title
import app.milestone.resources.main_evidence_row_sub
import app.milestone.resources.main_export_first
import app.milestone.resources.main_import_date_range
import app.milestone.resources.main_import_failed_generic
import app.milestone.resources.main_import_file_empty
import app.milestone.resources.main_import_file_entries
import app.milestone.resources.main_import_file_entries_undated
import app.milestone.resources.main_import_invalid
import app.milestone.resources.main_import_refused_run
import app.milestone.resources.main_import_replace_body
import app.milestone.resources.main_import_replace_confirm
import app.milestone.resources.main_import_replace_title
import app.milestone.resources.main_label_distance_units
import app.milestone.resources.main_label_keep_screen_on
import app.milestone.resources.main_label_rest_alert
import app.milestone.resources.main_label_show_route_maps
import app.milestone.resources.main_label_weight
import app.milestone.resources.main_live_pace_split
import app.milestone.resources.main_live_pace_split_sub
import app.milestone.resources.main_overline_calculators
import app.milestone.resources.main_overline_danger_zone
import app.milestone.resources.main_overline_during_training
import app.milestone.resources.main_overline_evidence
import app.milestone.resources.main_overline_plan_answers
import app.milestone.resources.main_overline_units
import app.milestone.resources.main_overline_your_data
import app.milestone.resources.main_plan_answers_sub
import app.milestone.resources.main_plan_answers_title
import app.milestone.resources.main_restoring_data
import app.milestone.resources.main_section_how_coach_reads
import app.milestone.resources.main_theme_iris
import app.milestone.resources.main_theme_meadow
import app.milestone.resources.main_theme_paper
import app.milestone.resources.main_theme_rose
import app.milestone.resources.main_unit_km
import app.milestone.resources.main_unit_mi
import app.milestone.resources.main_use_system_accent
import app.milestone.resources.main_use_system_accent_sub
import app.milestone.resources.profile_export_data
import app.milestone.resources.profile_export_data_sub
import app.milestone.resources.profile_import_data
import app.milestone.resources.profile_import_data_sub
import app.milestone.resources.setup_export_first_done
import app.milestone.resources.setup_export_first_done_cd
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/** Profile (04-profile), everyday things first; then the guided-setup answers
 *  folded under "Your plan answers"; Clear all data last. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ProfileDestination(
    ctx: PlatformContext,
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onClearAll: () -> Unit,
    // null = a plain re-run; a Modality = a Focus switch that needs that side's answers.
    onRerunSetup: (Modality?) -> Unit,
    // A row to expand and scroll to (a hold banner's "Check age" / "Review health
    // answers"); [onFocusHandled] fires once the request is taken.
    focusRow: ProfileRowId?,
    onFocusHandled: () -> Unit,
    // Whether "Your plan answers" is unfolded, hoisted above the tabs so it stays
    // as the user left it when they come back to Profile.
    planAnswersOpen: Boolean,
    onPlanAnswersOpenChange: (Boolean) -> Unit,
) {
    var showReferences by rememberSaveable { mutableStateOf(false) }
    if (showReferences) {
        AppSheet(onDismiss = { showReferences = false }) {
            ReferenceLibrarySheet(model)
        }
    }

    // Calculator forms and their result cards live in a sheet opened from the
    // row, never inline in the Profile scroll, otherwise computing e.g. HR zones
    // dumps ~7 EvidenceCards into the list.
    var activeTool by rememberSaveable { mutableStateOf<CoachTool?>(null) }
    val tool = activeTool
    if (tool != null) {
        AppSheet(onDismiss = { activeTool = null }) {
            CoachToolSheet(tool, model, onEvent)
        }
    }
    val listState = rememberLazyListState()
    // The setup answers fold away below the everyday settings. A banner's deep link
    // ("Check age", "Review health answers") opens the group and scrolls it into
    // composition; the editor then expands and reveals its row high in this list.
    LaunchedEffect(focusRow) {
        if (focusRow == null) return@LaunchedEffect
        onPlanAnswersOpenChange(true)
        val items = snapshotFlow { listState.layoutInfo.totalItemsCount }.first { it > 0 }
        listState.scrollToItem(items - 1 - ITEMS_AFTER_PLAN_ANSWERS)
    }
    val revealViewport = remember { RevealViewport() }
    // The pinned hold banner above comes and goes with a health or age answer,
    // moving this list's top edge by its height. Scroll by the same amount so the
    // row just toggled stays under the finger.
    val scope = rememberCoroutineScope()
    // Last laid-out top edge (NaN before the first layout); read only in the layout
    // callback, so a plain holder, not snapshot state.
    val viewportTop = remember { floatArrayOf(Float.NaN) }
    CompositionLocalProvider(LocalRevealViewport provides revealViewport) {
        ReadableWidth { gutter ->
            LazyColumn(
                state = listState,
                modifier = Modifier
                    .fillMaxSize()
                    .onGloballyPositioned { coords ->
                        revealViewport.height = coords.size.height
                        val top = coords.positionInWindow().y
                        val shift = if (viewportTop[0].isNaN()) 0f else top - viewportTop[0]
                        viewportTop[0] = top
                        if (shift != 0f) scope.launch { listState.scrollBy(shift) }
                    },
                contentPadding = PaddingValues(horizontal = gutter, vertical = Space.Screen.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Screen.dp),
            ) {
                CalculatorsSection(model, onOpen = { activeTool = it })
                UnitsSection(ctx, model, onEvent)
                AppearanceSection(ctx)
                TrainingAlertsSection(ctx, model)
                DataSection(ctx, model)
                EvidenceReferencesSection(onShowReferences = { showReferences = true })
                PlanAnswersSection(model, onEvent, onRerunSetup, planAnswersOpen, onPlanAnswersOpenChange, focusRow, onFocusHandled)
                DangerZoneSection(onClearAll)
            }
        }
    }
}

/** LazyColumn items below [PlanAnswersSection] (the Danger zone), so a deep link
 *  can find the group's index from the end of the list. */
private const val ITEMS_AFTER_PLAN_ANSWERS = 1

/**
 * "Your plan answers": one item holding a whole-row tile (like the other Profile
 * action tiles, its chevron turning as it opens) that unfolds the training-
 * profile groups ([ProfileEditor], each group its own labelled card, with the
 * quiet "Re-run guided setup" row), then the collapsed "How the coach reads you"
 * evidence-cited guidance. Guided setup asks all of it at first start, so it
 * sits below the everyday settings; [open] is hoisted so a deep link can unfold it.
 */
@OptIn(ExperimentalMaterial3Api::class)
private fun LazyListScope.PlanAnswersSection(
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onRerunSetup: (Modality?) -> Unit,
    open: Boolean,
    onOpenChange: (Boolean) -> Unit,
    focusRow: ProfileRowId?,
    onFocusHandled: () -> Unit,
) {
    // The core's evidence-cited profile-context rows (the "Profile" guidance
    // section, training age from cadence etc.), folded into ONE collapsed
    // section so they don't wall the Profile tab; expanding reveals the same
    // full-chrome EvidenceCards. No training-age row until there's logged
    // history to base it on: on a zero-data day-1 user it derives from a
    // profile default, not from evidence about them.
    val hasHistory = model.lifts.isNotEmpty() || model.runs.isNotEmpty()
    val profileGuidance = if (model.profile == null) {
        emptyList()
    } else {
        model.profile_guidance
            .filterNot { !hasHistory && it.training_age.isNotEmpty() }
            .byGuidancePriority()
    }
    item(key = "plan_answers") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_plan_answers))
            ProfileActionTile(
                title = stringResource(Res.string.main_plan_answers_title),
                subtitle = stringResource(Res.string.main_plan_answers_sub),
                expanded = open,
            ) { onOpenChange(!open) }
            if (open) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Screen.dp)) {
                    val initialProfile = model.profile?.let { Event.SetProfile.from(it) } ?: Event.SetProfile.SEED
                    ProfileEditor(
                        initial = initialProfile,
                        distanceUnit = rememberDistanceUnit(),
                        today = model.today(),
                        onRerunSetup = onRerunSetup,
                        focusRow = focusRow,
                        onFocusHandled = onFocusHandled,
                    ) { draft ->
                        onEvent(draft.effectiveFrom(model.today()))
                    }
                    if (profileGuidance.isNotEmpty()) {
                        // A standard action tile, no count pill on an overline: the
                        // rules open in a sheet of their full EvidenceCards.
                        var coachReadsOpen by rememberSaveable { mutableStateOf(false) }
                        ProfileActionTile(
                            title = stringResource(Res.string.main_section_how_coach_reads),
                            subtitle = pluralStringResource(Res.plurals.main_coach_reads_sub, profileGuidance.size, profileGuidance.size),
                        ) { coachReadsOpen = true }
                        if (coachReadsOpen) {
                            AppSheet(onDismiss = { coachReadsOpen = false }) {
                                Column(
                                    modifier = Modifier.sheetBody(rememberScrollState()),
                                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                                ) {
                                    SectionOverline(stringResource(Res.string.main_section_how_coach_reads))
                                    profileGuidance.forEach {
                                        EvidenceCard(
                                            guidanceTitle(it), it.evidence, why = it.why, glossary = it.glossary, spokenSubject = it.subject,
                                            // A training-age row's face is its short title, not the summary.
                                            proseSummary = it.prose && it.training_age.isEmpty(),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}


private fun LazyListScope.AppearanceSection(ctx: PlatformContext) {
    item(key = "appearance") {
        val res = currentResources()
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_appearance))
            val currentTheme by AppSettings.theme.collectAsState()
            ThemeSwatchPicker(currentTheme) { AppSettings.setTheme(ctx, it) }

            PlainCard {
                val darkMode by AppSettings.darkMode.collectAsState()
                SegmentedEnumRow(
                    label = stringResource(Res.string.main_dark_mode_pinned),
                    values = listOf(DarkMode.System, DarkMode.Light, DarkMode.Dark),
                    current = darkMode,
                    display = {
                        res.getString(
                            when (it) {
                                DarkMode.System -> Res.string.main_dark_mode_system
                                DarkMode.Light -> Res.string.main_dark_mode_light
                                DarkMode.Dark -> Res.string.main_dark_mode_dark
                            },
                        )
                    },
                    onSelect = { AppSettings.setDarkMode(ctx, it) },
                )
                // System accent: only where dynamic (wallpaper) colour exists
                // (Android 12+); safety colours never follow it.
                if (platformHasDynamicAccent) {
                    val dynamicAccent by AppSettings.dynamicAccent.collectAsState()
                    LabeledSwitchRow(
                        label = stringResource(Res.string.main_use_system_accent),
                        subtitle = stringResource(Res.string.main_use_system_accent_sub),
                        subtitleColor = OnBgFaint,
                        checked = dynamicAccent,
                        onCheckedChange = { AppSettings.setDynamicAccent(ctx, it) },
                    )
                }
                val showRouteMaps by AppSettings.showRouteMaps.collectAsState()
                LabeledSwitchRow(
                    label = stringResource(Res.string.main_label_show_route_maps),
                    subtitle = stringResource(Res.string.main_caption_route_maps),
                    subtitleColor = OnBgFaint,
                    checked = showRouteMaps,
                    onCheckedChange = { AppSettings.setShowRouteMaps(ctx, it) },
                )
            }
        }
    }
}

/** Units; all apply immediately, like every Profile change. */
private fun LazyListScope.UnitsSection(ctx: PlatformContext, model: ViewModel, onEvent: (Event) -> Unit) {
    item(key = "units") {
        val res = currentResources()
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_units))
            PlainCard {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    // Weight and distance units live on the PROFILE: each toggle
                    // reads the profile and commits the full profile with the unit
                    // changed (ProfileEditor's SetProfile funnel), so a
                    // training-field edit never last-write-wins the choice away.
                    // Hidden until a profile exists (nothing to write to yet).
                    val profile = model.profile
                    if (profile != null) {
                        val draft = Event.SetProfile.from(profile)
                        SegmentedEnumRow(
                            label = stringResource(Res.string.main_label_weight),
                            values = listOf(WeightUnit.Kg, WeightUnit.Lb),
                            current = draft.weightUnit,
                            display = { weightSuffix(it) },
                            onSelect = { sel ->
                                val lb = sel == WeightUnit.Lb
                                if (lb != draft.weightLb) {
                                    onEvent(draft.effectiveFrom(model.today()).copy(weightLb = lb))
                                }
                            },
                        )
                        SegmentedEnumRow(
                            label = stringResource(Res.string.main_label_distance_units),
                            values = listOf(DistanceUnit.Km, DistanceUnit.Mi),
                            current = draft.distanceUnit,
                            display = {
                                when (it) {
                                    DistanceUnit.Km -> res.getString(Res.string.main_unit_km)
                                    DistanceUnit.Mi -> res.getString(Res.string.main_unit_mi)
                                }
                            },
                            onSelect = { sel ->
                                if (sel != draft.distanceUnit) {
                                    onEvent(draft.effectiveFrom(model.today()).copy(distanceUnit = sel))
                                }
                            },
                        )
                    }
                    // Live pace-bucket size: the run screen groups pace into buckets
                    // of this many minutes of moving time.
                    val bucketMin by AppSettings.paceBucketMinutes.collectAsState()
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                        FieldLabel(stringResource(Res.string.main_live_pace_split), trailing = stringResource(Res.string.log_unit_min))
                        Text(
                            stringResource(Res.string.main_live_pace_split_sub),
                            color = OnBgFaint,
                            style = Type.Caption,
                        )
                        SegmentedEnumRow(
                            label = null,
                            values = PACE_BUCKET_MINUTE_OPTIONS,
                            current = bucketMin,
                            display = { "$it" },
                            onSelect = { AppSettings.setPaceBucketMinutes(ctx, it) },
                        )
                    }
                }
            }
        }
    }
}

/** Live pace-bucket sizes offered, in minutes of moving time. */
private val PACE_BUCKET_MINUTE_OPTIONS = listOf(1, 2, 3, 4, 5, 6, 8, 10)

/** During training: the rest-timer alert (lifting), keep-screen-on while a run
 *  records (running) and the session reminder ([SessionReminderRows]). The first
 *  two hide with their modality. Device prefs, not coaching state, so they live
 *  in [AppSettings] and [SessionReminder], never in the event log. On
 *  Android 16+ with Live Updates turned off for the app, a notice says why the run
 *  and rest timer have no status bar chip and links to the system switch. */
private fun LazyListScope.TrainingAlertsSection(ctx: PlatformContext, model: ViewModel) {
    val showLifting = model.profile == null || model.showLifting()
    val showRunning = model.profile == null || model.showRunning()
    if (!showLifting && !showRunning) return
    item(key = "training_alerts") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_during_training))
            PlainCard {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    if (showLifting) {
                        val restAlert by AppSettings.restAlert.collectAsState()
                        LabeledSwitchRow(
                            label = stringResource(Res.string.main_label_rest_alert),
                            subtitle = stringResource(Res.string.main_caption_rest_alert),
                            subtitleColor = OnBgFaint,
                            checked = restAlert,
                            onCheckedChange = { AppSettings.setRestAlert(ctx, it) },
                        )
                    }
                    if (showRunning) {
                        val keepOn by AppSettings.keepScreenOn.collectAsState()
                        LabeledSwitchRow(
                            label = stringResource(Res.string.main_label_keep_screen_on),
                            subtitle = stringResource(Res.string.main_caption_keep_screen_on),
                            subtitleColor = OnBgFaint,
                            checked = keepOn,
                            onCheckedChange = { AppSettings.setKeepScreenOn(ctx, it) },
                        )
                    }
                    SessionReminderRows(ctx)
                    val liveUpdates = liveUpdatesSwitch
                    if (liveUpdates != null) {
                        // Re-checked on every resume, so it clears once the user turns Live
                        // Updates back on in the system screen it opens.
                        var liveUpdatesOff by remember { mutableStateOf(liveUpdates.isOff(ctx)) }
                        LifecycleResumeEffect(Unit) {
                            liveUpdatesOff = liveUpdates.isOff(ctx)
                            onPauseOrDispose {}
                        }
                        if (liveUpdatesOff) {
                            InlineNotice(
                                stringResource(Res.string.live_updates_off_caption),
                                actionLabel = stringResource(Res.string.live_updates_open_settings),
                                onAction = { liveUpdates.open(ctx) },
                            )
                        }
                    }
                }
            }
        }
    }
}

/** Calculators: the modality-gated calculator rows, each opening its sheet; no
 *  section at all when no calculator applies (an under-18 lifter). Profile's
 *  items are keyed, so a section coming or going (a birthday edit making the
 *  user 15) never shifts the row being edited. */
private fun LazyListScope.CalculatorsSection(model: ViewModel, onOpen: (CoachTool) -> Unit) {
    if (!model.hasCalculators()) return
    item(key = "calculators") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_calculators))
            CalculatorsCard(model, onOpen)
        }
    }
}

private fun LazyListScope.EvidenceReferencesSection(onShowReferences: () -> Unit) {
    item(key = "evidence") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_evidence))
            // No count badge: the number moves with the profile and nothing on the
            // row could say why.
            ProfileActionTile(
                title = stringResource(Res.string.main_evidence_references_title),
                subtitle = stringResource(Res.string.main_evidence_row_sub),
                onClick = onShowReferences,
            )
        }
    }
}

/** Danger zone: the ONE destructive action, at the very bottom of Profile. The
 *  "Danger zone" overline and the confirm dialog keep it from being hit by accident. */
private fun LazyListScope.DangerZoneSection(onClearAll: () -> Unit) {
    item(key = "danger_zone") {
        val status = LocalStatusColors.current
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_danger_zone))
            ChevronRow(
                onClick = onClearAll,
                modifier = Modifier
                    .clip(RoundedCornerShape(Space.Card.dp))
                    .border(1.dp, status.danger.copy(alpha = 0.4f), RoundedCornerShape(Space.Card.dp)),
                chevron = false,
                contentPadding = PaddingValues(Space.Card.dp),
            ) {
                Text(
                    stringResource(Res.string.main_clear_all_data),
                    color = DangerText,
                    style = Type.Body.copy(fontWeight = FontWeight.SemiBold),
                )
                Text(
                    stringResource(Res.string.main_danger_zone_sub),
                    color = OnBgFaint,
                    style = Type.Caption,
                )
            }
        }
    }
}

/** Your data: export the full event log to a file, or import one, and share a
 *  plain-text summary for a coach. Import replaces everything logged now, behind
 *  a confirm. */
private fun LazyListScope.DataSection(ctx: PlatformContext, model: ViewModel) {
    item(key = "data") {
        val res = currentResources()
        // The picked file awaiting the replace confirmation ([rememberBackupPicker]'s
        // reference). It survives rotation; its validated lines + summary are re-read
        // from it when the in-memory copy is gone.
        var pendingUri by rememberSaveable { mutableStateOf<String?>(null) }
        var pending by remember { mutableStateOf<PendingImport?>(null) }
        val restoring by DataRestore.running.collectAsState()
        // "Export first" from inside the replace dialog: acknowledged in the dialog
        // itself (the notice lands behind it), per picked file.
        var exportedBeforeImport by remember(pendingUri) { mutableStateOf(false) }
        val exportData = rememberDataExport(onExported = { exportedBeforeImport = true })
        val pickBackup = rememberBackupPicker { uri ->
            pending = null
            pendingUri = uri
        }
        LaunchedEffect(pendingUri) {
            val uri = pendingUri ?: return@LaunchedEffect
            if (pending?.uri == uri) return@LaunchedEffect
            when (val check = readPickedBackup(ctx, uri)) {
                is ImportCheck.Valid -> {
                    val summary = withContext(Dispatchers.Default) {
                        summarizeImport(check.lines, TimeZone.currentSystemDefault())
                    }
                    pending = PendingImport(uri, check.lines, summary)
                }
                ImportCheck.Invalid -> {
                    pendingUri = null
                    UiMessages.post(UiMessage(res.getString(Res.string.main_import_invalid), long = true))
                }
            }
        }

        pending?.takeIf { it.uri == pendingUri }?.let { p ->
            val dismiss = {
                pendingUri = null
                pending = null
            }
            ExportFirstDialog(
                title = stringResource(Res.string.main_import_replace_title),
                body = importSummaryText(p.summary) + " " + stringResource(Res.string.main_import_replace_body),
                exported = exportedBeforeImport,
                onExport = exportData,
                confirmLabel = stringResource(Res.string.main_import_replace_confirm),
                onDismiss = dismiss,
            ) { restoreData(ctx, res, p.lines, p.summary.entries) }
        }

        if (restoring) {
            AppDialog(
                onDismiss = { /* the restart follows; nothing to go back to */ },
                confirmButton = {},
                text = {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                    ) {
                        CircularProgressIndicator(Modifier.size(24.dp), color = Accent)
                        Text(stringResource(Res.string.main_restoring_data))
                    }
                },
            )
        }

        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(Res.string.main_overline_your_data))
            ProfileActionTile(
                title = stringResource(Res.string.profile_export_data),
                subtitle = stringResource(Res.string.profile_export_data_sub),
            ) { exportData() }
            ProfileActionTile(
                title = stringResource(Res.string.profile_import_data),
                subtitle = stringResource(Res.string.profile_import_data_sub),
            ) { pickBackup() }
            ProfileActionTile(
                title = stringResource(Res.string.coach_export_action),
                subtitle = pluralStringResource(Res.plurals.coach_export_action_sub, CoachSummaryWeeks, CoachSummaryWeeks),
            ) { shareCoachSummary(ctx, model) }
        }
    }
}

private class PendingImport(val uri: String, val lines: List<String>, val summary: ImportSummary)

/** Restore [lines] ([DataRestore.restore]), or say why not. [entries] is what the
 *  boot into them confirms ("Restored N entries"). */
private fun restoreData(ctx: PlatformContext, res: PlatformResources, lines: List<String>, entries: Int) {
    AppProcess.scope.launch {
        val refusal = when (DataRestore.restore(ctx, lines, entries)) {
            DataImportResult.Replaced -> return@launch
            DataImportResult.RunInProgress -> Res.string.main_import_refused_run
            DataImportResult.Failed -> Res.string.main_import_failed_generic
        }
        UiMessages.post(UiMessage(res.getString(refusal), long = true))
    }
}

/** "This file has 4 entries (Oct 2, 2026 – Oct 14, 2026)." for the replace dialog. */
@Composable
private fun importSummaryText(s: ImportSummary): String {
    if (s.entries == 0) return stringResource(Res.string.main_import_file_empty)
    val first = s.firstDay
    val last = s.lastDay
    if (first == null || last == null) {
        return pluralStringResource(Res.plurals.main_import_file_entries_undated, s.entries, s.entries)
    }
    val from = epochDayLabel(first)
    val range = if (first == last) {
        from
    } else {
        stringResource(Res.string.main_import_date_range, from, epochDayLabel(last))
    }
    return pluralStringResource(Res.plurals.main_import_file_entries, s.entries, s.entries, range)
}

/** "Clear all data?" confirmation. Everything logged goes, the profile stays, and
 *  "Export first" saves a copy before deciding. */
@Composable
internal fun ClearAllDataDialog(visible: Boolean, onDismiss: () -> Unit, onClear: () -> Unit) {
    // Registered unconditionally: an activity-result launcher must outlive the
    // dialog's visibility so the export result still lands. The acknowledgement
    // resets each time the dialog opens.
    val exported = remember(visible) { mutableStateOf(false) }
    val exportData = rememberDataExport(onExported = { exported.value = true })
    if (!visible) return
    ExportFirstDialog(
        title = stringResource(Res.string.main_clear_all_title),
        body = stringResource(Res.string.main_clear_all_message),
        exported = exported.value,
        onExport = exportData,
        confirmLabel = stringResource(Res.string.main_clear_all_confirm),
        onDismiss = onDismiss,
        onConfirm = onClear,
    )
}

/** A replace/clear confirm: what goes ([body]), then "Export first" as a body
 *  action, so the button row stays Cancel · the destructive [confirmLabel], which
 *  closes the dialog, then runs [onConfirm]. Once [exported], the action reads
 *  "Exported ✓" (tapping it saves another copy). */
@Composable
private fun ExportFirstDialog(
    title: String,
    body: String,
    exported: Boolean,
    onExport: () -> Unit,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
) {
    AppDialog(
        onDismiss = onDismiss,
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm()
            }) { Text(confirmLabel, color = DangerText) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(Res.string.main_cancel)) } },
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                Text(body)
                TextAction(
                    stringResource(if (exported) Res.string.setup_export_first_done else Res.string.main_export_first),
                    onClick = onExport,
                    modifier = Modifier.offset(x = -TextActionInset),
                    description = if (exported) stringResource(Res.string.setup_export_first_done_cd) else null,
                )
            }
        },
    )
}

@Composable
private fun ProfileActionTile(title: String, subtitle: String, expanded: Boolean? = null, onClick: () -> Unit) =
    TitleSubtitleRow(
        title, subtitle, onClick,
        modifier = Modifier
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), RoundedCornerShape(Space.Card.dp)),
        expanded = expanded,
        contentPadding = PaddingValues(Space.Card.dp),
    )

private val ThemeSwatchGap = Space.Md.dp

/** A theme's display name; its enum name is the stored key, never shown. */
private val AppTheme.labelRes: StringResource
    get() = when (this) {
        AppTheme.Rose -> Res.string.main_theme_rose
        AppTheme.Meadow -> Res.string.main_theme_meadow
        AppTheme.Iris -> Res.string.main_theme_iris
        AppTheme.Paper -> Res.string.main_theme_paper
    }

private val ThemeSwatchInset = Space.Md.dp

/**
 * The theme picker: one [ThemeSwatchCard] per [AppTheme], in enum order. Four
 * across while every name fits on one line in its card at the current font
 * scale (measured in the exact style the card renders, like [SegmentedEnumRow]);
 * otherwise a 2×2 grid, so a name never breaks mid-word at 130 % / 200 % type.
 * A radio group to TalkBack.
 */
@Composable
fun ThemeSwatchPicker(current: AppTheme, onSelect: (AppTheme) -> Unit) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val measurer = rememberTextMeasurer()
        val density = LocalDensity.current
        val themes = AppTheme.entries
        val names = themes.associateWith { stringResource(it.labelRes) }
        val labelRoomPx = with(density) {
            ((maxWidth - ThemeSwatchGap * (themes.size - 1)) / themes.size - ThemeSwatchInset * 2).toPx()
        }
        val fits = names.values.all {
            measurer.measure(it, style = Type.Chip, maxLines = 1, softWrap = false).size.width <= labelRoomPx
        }
        Column(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(ThemeSwatchGap),
        ) {
            themes.chunked(if (fits) themes.size else 2).forEach { row ->
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(ThemeSwatchGap),
                ) {
                    row.forEach { t -> ThemeSwatchCard(t, names.getValue(t), selected = t == current) { onSelect(t) } }
                }
            }
        }
    }
}

/**
 * A theme choice as a swatch card: a mini preview of the theme's LIGHT face (its
 * ground with an elevated-card bar and its accent dot) over its name, drawn as a
 * [choiceCard] (selected = solid Accent, no outline, like every other selection).
 * The light face is each theme's most recognisable one (rose blush, mint, butter
 * cream, white paper) and it reads the theme's own [AppTheme.light] palette, so
 * every card previews itself regardless of the active theme or dark mode. The
 * preview plate keeps a hairline so a white ground (Paper) never melts into a
 * light card, and an OnAccent hairline on the selected accent fill.
 */
@Composable
private fun RowScope.ThemeSwatchCard(theme: AppTheme, name: String, selected: Boolean, onClick: () -> Unit) {
    val pal = theme.light
    val plateShape = RoundedCornerShape(Space.Md.dp)
    val barShape = RoundedCornerShape(Space.Sm.dp)
    Column(
        modifier = Modifier
            .weight(1f)
            .choiceCard(selected)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .padding(vertical = Space.Card.dp, horizontal = ThemeSwatchInset),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(plateShape)
                .background(pal.bgTop)
                .border(1.dp, if (selected) OnAccent.copy(alpha = 0.4f) else ChoiceCellBorder, plateShape)
                .padding(Space.Sm.dp),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(16.dp).clip(CircleShape).background(pal.accent))
            Box(
                Modifier
                    .weight(1f)
                    .height(16.dp)
                    .clip(barShape)
                    .background(pal.bgElevated)
                    .border(1.dp, pal.onBgFaint.copy(alpha = HairlineAlpha.Strong), barShape),
            )
        }
        // Wraps rather than clipping if a name still outgrows its card.
        Text(
            name,
            color = choiceCellContent(selected),
            style = Type.Chip,
            textAlign = TextAlign.Center,
        )
    }
}
