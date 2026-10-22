package app.milestone

import android.content.Context
import android.net.Uri
import android.os.Build
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Profile (04-profile), everyday things first: Calculators, Units, Appearance ·
 *  Theme, During training, Your data, Evidence & references; then the guided-setup
 *  answers folded under "Your plan answers"; Clear all data last. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ProfileDestination(
    ctx: Context,
    model: ViewModel,
    onEvent: (Event) -> Unit,
    onClearAll: () -> Unit,
    // null = a plain re-run; a Modality = a Focus switch that needs that side's answers.
    onRerunSetup: (Modality?) -> Unit,
    // A row to expand and scroll to (a hold banner's "Check age" / "Review health
    // answers"); [onFocusHandled] fires once the request is taken.
    focusRow: ProfileRowId? = null,
    onFocusHandled: () -> Unit = {},
    // Whether "Your plan answers" is unfolded, hoisted above the tabs so it stays
    // as the user left it when they come back to Profile.
    planAnswersOpen: Boolean,
    onPlanAnswersOpenChange: (Boolean) -> Unit,
) {
    // Reference library, opened from the Evidence row below. A ModalBottomSheet
    // keeps it a read-only overlay. `showReferences` is shared between the row
    // that opens it and this pre-LazyColumn sheet, so it stays in the parent; the
    // section gets a callback.
    var showReferences by remember { mutableStateOf(false) }
    if (showReferences) {
        ModalBottomSheet(
            onDismissRequest = { showReferences = false },
            // Fully expanded only: a half-height detent made Back collapse the
            // sheet to it first, so closing took two presses.
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = BgSunken,
        ) {
            SheetNavBarScrim()
            ReferenceLibrarySheet(model)
        }
    }

    // Which calculator's form + results are shown in the bottom sheet. null =
    // sheet closed. Calculator forms and their result cards live in a
    // ModalBottomSheet opened from the row, never inline in the Profile scroll,
    // otherwise computing e.g. HR zones dumps ~7 EvidenceCards into the list.
    var activeTool by rememberSaveable { mutableStateOf<CoachTool?>(null) }
    val tool = activeTool
    if (tool != null) {
        ModalBottomSheet(
            onDismissRequest = { activeTool = null },
            // Open fully expanded: every calculator form (keypad + fields +
            // result cards) exceeds the partial detent, which left the keypad
            // and results below the fold and forced a second drag.
            sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = BgSunken,
        ) {
            SheetNavBarScrim()
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
    // row just toggled stays under the finger instead of jumping ~450 px (and a
    // follow-up tap landing on a different toggle).
    val scope = rememberCoroutineScope()
    // Last laid-out top edge (NaN before the first layout); read only in the layout
    // callback, so a plain holder, not snapshot state.
    val viewportTop = remember { floatArrayOf(Float.NaN) }
    CompositionLocalProvider(LocalRevealViewport provides revealViewport) {
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
            contentPadding = PaddingValues(Space.Screen.dp),
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
        model.guidance
            .filter { it.section == PROFILE_GUIDANCE_SECTION }
            .filterNot { !hasHistory && it.training_age.isNotEmpty() }
            .byGuidancePriority()
    }
    item(key = "plan_answers") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_overline_plan_answers))
            ProfileActionTile(
                title = stringResource(R.string.main_plan_answers_title),
                subtitle = stringResource(R.string.main_plan_answers_sub),
                expanded = open,
            ) { onOpenChange(!open) }
            if (open) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Screen.dp)) {
                    // Training profile, grouped rows; tapping a row opens its inline
                    // editor (OptionList / scale rows, no dropdowns, no steppers as
                    // primary). Profile owns bodyweight (the "You" group carries the
                    // Bodyweight row); the protein calculator prefills from it and
                    // keeps any per-calculation override local to the calculator,
                    // never written back here. Re-run guided setup reopens the wizard
                    // PRE-FILLED from the current profile; the seed + onboarding
                    // semantics live at the call site.
                    val initialProfile = model.profile?.let { ProfileDraft.from(it) } ?: ProfileDraft.SEED
                    ProfileEditor(
                        initial = initialProfile,
                        distanceUnit = rememberDistanceUnit(),
                        today = model.today(),
                        onRerunSetup = onRerunSetup,
                        focusRow = focusRow,
                        onFocusHandled = onFocusHandled,
                    ) { draft ->
                        onEvent(draft.toEvent(model.today()))
                    }
                    if (profileGuidance.isNotEmpty()) {
                        // A standard action tile, no count pill on an overline: the
                        // rules open in a sheet of their full EvidenceCards.
                        var coachReadsOpen by rememberSaveable { mutableStateOf(false) }
                        ProfileActionTile(
                            title = stringResource(R.string.main_section_how_coach_reads),
                            subtitle = pluralStringResource(R.plurals.main_coach_reads_sub, profileGuidance.size, profileGuidance.size),
                        ) { coachReadsOpen = true }
                        if (coachReadsOpen) {
                            ModalBottomSheet(
                                onDismissRequest = { coachReadsOpen = false },
                                sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
                                containerColor = BgSunken,
                            ) {
                                SheetNavBarScrim()
                                Column(
                                    modifier = Modifier
                                        .verticalScroll(rememberScrollState())
                                        .padding(horizontal = Space.Sheet.dp)
                                        .padding(bottom = Space.Lg.dp),
                                    verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
                                ) {
                                    SectionOverline(stringResource(R.string.main_section_how_coach_reads))
                                    profileGuidance.forEach {
                                        EvidenceCard(guidanceTitle(it), it.grade, it.citation, it.confidence, it.safety_critical, it.contested, why = it.why)
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

/** The core's guidance section of profile-context rows (shared/src/app/guidance.rs). */
private const val PROFILE_GUIDANCE_SECTION = "Profile"

/** Appearance: theme swatches, then one card with light/dark, the system accent
 *  (API 31+) and route maps. By default light/dark follows the OS. */
private fun LazyListScope.AppearanceSection(ctx: Context) {
    item(key = "appearance") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_appearance))
            val currentTheme by AppSettings.theme.collectAsState()
            ThemeSwatchPicker(currentTheme) { AppSettings.setTheme(ctx, it) }

            PlainCard {
                // Light / dark: one three-way choice, System follows the phone.
                // Applies immediately, no Apply button (Profile changes apply at once).
                val darkMode by AppSettings.darkMode.collectAsState()
                SegmentedEnumRow(
                    label = stringResource(R.string.main_dark_mode_pinned),
                    values = listOf(DarkMode.System, DarkMode.Light, DarkMode.Dark),
                    current = darkMode,
                    display = {
                        ctx.getString(
                            when (it) {
                                DarkMode.System -> R.string.main_dark_mode_system
                                DarkMode.Light -> R.string.main_dark_mode_light
                                DarkMode.Dark -> R.string.main_dark_mode_dark
                            },
                        )
                    },
                    onSelect = { AppSettings.setDarkMode(ctx, it) },
                )
                // System accent: gated to API 31+ where dynamic (wallpaper) colour
                // exists; safety colours never follow it.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    val dynamicAccent by AppSettings.dynamicAccent.collectAsState()
                    LabeledSwitchRow(
                        label = stringResource(R.string.main_use_system_accent),
                        subtitle = stringResource(R.string.main_use_system_accent_sub),
                        subtitleColor = OnBgFaint,
                        checked = dynamicAccent,
                        onCheckedChange = { AppSettings.setDynamicAccent(ctx, it) },
                    )
                }
                val showRouteMaps by AppSettings.showRouteMaps.collectAsState()
                LabeledSwitchRow(
                    label = stringResource(R.string.main_label_show_route_maps),
                    subtitle = stringResource(R.string.main_caption_route_maps),
                    subtitleColor = OnBgFaint,
                    checked = showRouteMaps,
                    onCheckedChange = { AppSettings.setShowRouteMaps(ctx, it) },
                )
            }
        }
    }
}

/** Units: weight (kg | lb, profile-backed), the distance unit and the live
 *  pace-bucket size. All apply immediately, like every Profile change. */
private fun LazyListScope.UnitsSection(ctx: Context, model: ViewModel, onEvent: (Event) -> Unit) {
    item(key = "units") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_overline_units))
            PlainCard {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    // Weight units live on the PROFILE (kg | lb): the toggle reads
                    // the profile flag and commits the full profile with the flag
                    // flipped (mirrors ProfileEditor's SetProfile funnel), so a
                    // training-field edit never last-write-wins the choice away.
                    // Hidden until a profile exists (nothing to write to yet).
                    val profile = model.profile
                    if (profile != null) {
                        SegmentedEnumRow(
                            label = stringResource(R.string.main_label_weight),
                            values = listOf(WeightUnit.Kg, WeightUnit.Lb),
                            current = if (profile.weight_lb) WeightUnit.Lb else WeightUnit.Kg,
                            display = { weightSuffix(it) },
                            onSelect = { sel ->
                                val lb = sel == WeightUnit.Lb
                                if (lb != profile.weight_lb) {
                                    onEvent(ProfileDraft.from(profile).toEvent(model.today()).copy(weightLb = lb))
                                }
                            },
                        )
                    }
                    // Distance units: Auto follows the phone locale (mi for
                    // US/UK/Liberia/Myanmar, else km); Km/Mi pin it.
                    val unitOverride by AppSettings.distanceUnitOverride.collectAsState()
                    SegmentedEnumRow(
                        label = stringResource(R.string.main_label_distance_units),
                        values = listOf(
                            DistanceUnitOverride.System,
                            DistanceUnitOverride.Km,
                            DistanceUnitOverride.Mi,
                        ),
                        current = unitOverride,
                        display = {
                            when (it) {
                                DistanceUnitOverride.System -> ctx.getString(R.string.main_distance_auto)
                                DistanceUnitOverride.Km -> ctx.getString(R.string.main_unit_km)
                                DistanceUnitOverride.Mi -> ctx.getString(R.string.main_unit_mi)
                            }
                        },
                        onSelect = { AppSettings.setDistanceUnitOverride(ctx, it) },
                    )
                    // Live pace-bucket size (minutes): the run screen groups
                    // pace into buckets of this many minutes of moving time. Digit
                    // segments with the unit on the label; a list when they don't fit.
                    val bucketMin by AppSettings.paceBucketMinutes.collectAsState()
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                        FieldLabel(stringResource(R.string.main_live_pace_split), trailing = stringResource(R.string.log_unit_min))
                        Text(
                            stringResource(R.string.main_live_pace_split_sub),
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

/** During training: the rest-timer alert (lifting) and keep-screen-on while a
 *  run records (running). Each row hides with its modality. Device prefs, not
 *  coaching state, so they live in [AppSettings], never in the event log. */
private fun LazyListScope.TrainingAlertsSection(ctx: Context, model: ViewModel) {
    val showLifting = model.profile == null || model.showLifting()
    val showRunning = model.profile == null || model.showRunning()
    if (!showLifting && !showRunning) return
    item(key = "training_alerts") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_overline_during_training))
            PlainCard {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    if (showLifting) {
                        val restAlert by AppSettings.restAlert.collectAsState()
                        LabeledSwitchRow(
                            label = stringResource(R.string.main_label_rest_alert),
                            subtitle = stringResource(R.string.main_caption_rest_alert),
                            subtitleColor = OnBgFaint,
                            checked = restAlert,
                            onCheckedChange = { AppSettings.setRestAlert(ctx, it) },
                        )
                    }
                    if (showRunning) {
                        val keepOn by AppSettings.keepScreenOn.collectAsState()
                        LabeledSwitchRow(
                            label = stringResource(R.string.main_label_keep_screen_on),
                            subtitle = stringResource(R.string.main_caption_keep_screen_on),
                            subtitleColor = OnBgFaint,
                            checked = keepOn,
                            onCheckedChange = { AppSettings.setKeepScreenOn(ctx, it) },
                        )
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
            SectionOverline(stringResource(R.string.main_overline_calculators))
            CalculatorsCard(model, onOpen)
        }
    }
}

/** Evidence & references: a whole-row tap target opening the read-only Reference
 *  library (grade chips + why? intact) as a sheet. */
private fun LazyListScope.EvidenceReferencesSection(onShowReferences: () -> Unit) {
    item(key = "evidence") {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_overline_evidence))
            // No count badge: the number moves with the profile and nothing on the
            // row could say why.
            ProfileActionTile(
                title = stringResource(R.string.main_evidence_references_title),
                subtitle = stringResource(R.string.main_evidence_row_sub),
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
            SectionOverline(stringResource(R.string.main_overline_danger_zone))
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .clip(RoundedCornerShape(Space.Card.dp))
                    .border(1.dp, status.danger.copy(alpha = 0.4f), RoundedCornerShape(Space.Card.dp))
                    .clickable(role = Role.Button) { onClearAll() }
                    .padding(Space.Card.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                    Text(
                        stringResource(R.string.main_clear_all_data),
                        color = DangerText,
                        style = Type.Body.copy(fontWeight = FontWeight.SemiBold),
                    )
                    Text(
                        stringResource(R.string.main_danger_zone_sub),
                        color = OnBgFaint,
                        style = Type.Caption,
                    )
                }
            }
        }
    }
}

/** Your data: export the full event log to a file, or import one, and share a
 *  plain-text summary for a coach. Import replaces everything logged now, behind
 *  a confirm. */
private fun LazyListScope.DataSection(ctx: Context, model: ViewModel) {
    item(key = "data") {
        // The picked file awaiting the replace confirmation. The Uri survives
        // rotation; its validated lines + summary are re-read from it when the
        // in-memory copy is gone.
        var pendingUri by rememberSaveable { mutableStateOf<Uri?>(null) }
        var pending by remember { mutableStateOf<PendingImport?>(null) }
        val restoring by dataRestoreRunning.collectAsState()
        // "Export first" from inside the replace dialog: acknowledged in the dialog
        // itself (the toast lands behind it), per picked file.
        var exportedBeforeImport by remember(pendingUri) { mutableStateOf(false) }
        val exportData = rememberDataExport(onExported = { exportedBeforeImport = true })
        val importLauncher = rememberLauncherForActivityResult(
            ActivityResultContracts.GetContent(),
        ) { uri ->
            if (uri != null) {
                pending = null
                pendingUri = uri
            }
        }
        LaunchedEffect(pendingUri) {
            val uri = pendingUri ?: return@LaunchedEffect
            if (pending?.uri == uri) return@LaunchedEffect
            when (val check = readImportCandidate(ctx, uri)) {
                is ImportCheck.Valid -> {
                    val summary = withContext(Dispatchers.Default) {
                        summarizeImport(check.lines, ZoneId.systemDefault())
                    }
                    pending = PendingImport(uri, check.lines, summary)
                }
                ImportCheck.Invalid -> {
                    pendingUri = null
                    Toast.makeText(ctx, R.string.main_import_invalid, Toast.LENGTH_LONG).show()
                }
            }
        }

        pending?.takeIf { it.uri == pendingUri }?.let { p ->
            val dismiss = {
                pendingUri = null
                pending = null
            }
            AlertDialog(
                onDismissRequest = dismiss,
                shape = RoundedCornerShape(Space.Card.dp),
                title = { Text(stringResource(R.string.main_import_replace_title)) },
                text = {
                    ExportFirstBody(
                        importSummaryText(p.summary) + " " + stringResource(R.string.main_import_replace_body),
                        onExport = exportData,
                        exported = exportedBeforeImport,
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        dismiss()
                        restoreData(ctx.applicationContext, p.lines, p.summary.entries)
                    }) {
                        Text(stringResource(R.string.main_import_replace_confirm), color = DangerText)
                    }
                },
                dismissButton = {
                    TextButton(onClick = dismiss) { Text(stringResource(R.string.main_cancel)) }
                },
            )
        }

        if (restoring) {
            AlertDialog(
                onDismissRequest = { /* the restart follows; nothing to go back to */ },
                confirmButton = {},
                text = {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
                    ) {
                        CircularProgressIndicator(Modifier.size(24.dp), color = Accent)
                        Text(stringResource(R.string.main_restoring_data))
                    }
                },
            )
        }

        Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            SectionOverline(stringResource(R.string.main_overline_your_data))
            ProfileActionTile(
                title = stringResource(R.string.profile_export_data),
                subtitle = stringResource(R.string.profile_export_data_sub),
            ) { exportData() }
            ProfileActionTile(
                title = stringResource(R.string.profile_import_data),
                subtitle = stringResource(R.string.profile_import_data_sub),
            ) { importLauncher.launch("*/*") }
            ProfileActionTile(
                title = stringResource(R.string.coach_export_action),
                subtitle = pluralStringResource(R.plurals.coach_export_action_sub, CoachSummaryWeeks, CoachSummaryWeeks),
            ) { shareCoachSummary(ctx, model) }
        }
    }
}

/** A validated import file: its lines and what they hold. */
private class PendingImport(val uri: Uri, val lines: List<String>, val summary: ImportSummary)

/** True while a confirmed import swaps the log; the process restarts when it is
 *  done. Process-scoped like the swap itself, so a rotation can't strand it. */
private val dataRestoreRunning = MutableStateFlow(false)

/** Replace the log with [lines] and restart into it, or say why not. [entries] is
 *  what the next boot confirms ("Restored N entries"). */
private fun restoreData(app: Context, lines: List<String>, entries: Int) {
    if (RunSession.tracking.value || RunSession.armed.value) {
        Toast.makeText(app, R.string.main_import_refused_run, Toast.LENGTH_LONG).show()
        return
    }
    dataRestoreRunning.value = true
    EventDispatcher.scope.launch {
        when (replaceEventLog(app, lines)) {
            DataImportResult.Replaced -> {
                // commit, not apply: the process ends right after.
                app.setupPrefs().edit().putInt(KEY_RESTORED_ENTRIES, entries).commit()
                restartForImport(app)
            }
            DataImportResult.RunInProgress -> {
                dataRestoreRunning.value = false
                Toast.makeText(app, R.string.main_import_refused_run, Toast.LENGTH_LONG).show()
            }
            DataImportResult.Failed -> {
                dataRestoreRunning.value = false
                Toast.makeText(app, R.string.main_import_failed_generic, Toast.LENGTH_LONG).show()
            }
        }
    }
}

/** "This file has 4 entries (Oct 2, 2026 – Oct 14, 2026)." for the replace dialog. */
@Composable
private fun importSummaryText(s: ImportSummary): String {
    if (s.entries == 0) return stringResource(R.string.main_import_file_empty)
    val first = s.firstDay
    val last = s.lastDay
    if (first == null || last == null) {
        return pluralStringResource(R.plurals.main_import_file_entries_undated, s.entries, s.entries)
    }
    val fmt = remember { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM) }
    val from = LocalDate.ofEpochDay(first).format(fmt)
    val range = if (first == last) {
        from
    } else {
        stringResource(R.string.main_import_date_range, from, LocalDate.ofEpochDay(last).format(fmt))
    }
    return pluralStringResource(R.plurals.main_import_file_entries, s.entries, s.entries, range)
}

/** A "save as" picker that writes the full data export to the chosen file and
 *  toasts the outcome; [onExported] follows a successful save. The export runs at
 *  process scope, so closing the screen doesn't cancel it. Invoke the returned
 *  function to open the picker. */
@Composable
internal fun rememberDataExport(onExported: () -> Unit = {}): () -> Unit {
    val app = LocalContext.current.applicationContext
    val exported by rememberUpdatedState(onExported)
    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/x-ndjson"),
    ) { uri ->
        if (uri != null) EventDispatcher.scope.launch {
            val ok = exportDataTo(app, uri)
            if (ok) exported()
            Toast.makeText(
                app,
                if (ok) R.string.main_export_ok else R.string.main_export_failed,
                Toast.LENGTH_LONG,
            ).show()
        }
    }
    return { launcher.launch(dataExportFilename()) }
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
    AlertDialog(
        onDismissRequest = onDismiss,
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(stringResource(R.string.main_clear_all_title)) },
        text = {
            ExportFirstBody(stringResource(R.string.main_clear_all_message), onExport = exportData, exported = exported.value)
        },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onClear()
            }) { Text(stringResource(R.string.main_clear_all_confirm), color = DangerText) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.main_cancel)) }
        },
    )
}

/** The body of a replace/clear confirm: what goes, then "Export first" as a body
 *  action, so both dialogs keep the button row to Cancel · the destructive action.
 *  Once [exported], the action reads "Exported ✓" (tapping it saves another copy). */
@Composable
private fun ExportFirstBody(body: String, onExport: () -> Unit, exported: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        Text(body)
        TextAction(
            stringResource(if (exported) R.string.setup_export_first_done else R.string.main_export_first),
            onClick = onExport,
            modifier = Modifier.offset(x = -TextActionInset),
            description = if (exported) stringResource(R.string.setup_export_first_done_cd) else null,
        )
    }
}

/** A whole-row ≥48dp Profile action tile on its own outlined card: bold title,
 *  faint caption, chevron. With [expanded] it folds a section open below it: the
 *  chevron turns and TalkBack hears the state. */
@Composable
private fun ProfileActionTile(title: String, subtitle: String, expanded: Boolean? = null, onClick: () -> Unit) {
    val state = expanded?.let { stringResource(if (it) R.string.ui_cd_expanded else R.string.ui_cd_collapsed) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), RoundedCornerShape(Space.Card.dp))
            .clickable(role = Role.Button) { onClick() }
            .then(if (state != null) Modifier.semantics { stateDescription = state } else Modifier)
            .padding(Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(title, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
            Text(subtitle, color = OnBgFaint, style = Type.Caption)
        }
        RowChevron(expanded == true)
    }
}

