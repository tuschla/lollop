package app.milestone

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.setup_age_not_given
import app.milestone.resources.setup_age_value
import app.milestone.resources.setup_altitude_value
import app.milestone.resources.setup_birthday_add
import app.milestone.resources.setup_birthday_add_prompt
import app.milestone.resources.setup_birthday_change
import app.milestone.resources.setup_birthday_skipped_note
import app.milestone.resources.setup_bodyweight_value
import app.milestone.resources.setup_bpm_value
import app.milestone.resources.setup_break_none
import app.milestone.resources.setup_break_restart_label
import app.milestone.resources.setup_break_value
import app.milestone.resources.setup_break_weeks_label
import app.milestone.resources.setup_date_none
import app.milestone.resources.setup_days_count
import app.milestone.resources.setup_days_scale_label
import app.milestone.resources.setup_days_split_sub
import app.milestone.resources.setup_days_weekdays_hint
import app.milestone.resources.setup_distance_value
import app.milestone.resources.setup_focus_add_lift_body
import app.milestone.resources.setup_focus_add_lift_title
import app.milestone.resources.setup_focus_add_run_body
import app.milestone.resources.setup_focus_add_run_title
import app.milestone.resources.setup_focus_answer_lift
import app.milestone.resources.setup_focus_answer_run
import app.milestone.resources.setup_focus_resume_lift_body
import app.milestone.resources.setup_focus_resume_lift_title
import app.milestone.resources.setup_focus_resume_run_body
import app.milestone.resources.setup_focus_resume_run_title
import app.milestone.resources.setup_focus_switch_confirm
import app.milestone.resources.setup_focus_to_lift_body
import app.milestone.resources.setup_focus_to_lift_only_body
import app.milestone.resources.setup_focus_to_lift_only_title
import app.milestone.resources.setup_focus_to_lift_title
import app.milestone.resources.setup_focus_to_run_body
import app.milestone.resources.setup_focus_to_run_only_body
import app.milestone.resources.setup_focus_to_run_only_title
import app.milestone.resources.setup_focus_to_run_title
import app.milestone.resources.setup_health_defer_youth
import app.milestone.resources.setup_max_hr_hint
import app.milestone.resources.setup_modality_calis_short
import app.milestone.resources.setup_modality_strength_q
import app.milestone.resources.setup_modality_weights_short
import app.milestone.resources.setup_not_set
import app.milestone.resources.setup_not_sure
import app.milestone.resources.setup_priority_none
import app.milestone.resources.setup_rerun_setup
import app.milestone.resources.setup_row_advanced
import app.milestone.resources.setup_row_advanced_sub
import app.milestone.resources.setup_row_age
import app.milestone.resources.setup_row_bodyweight
import app.milestone.resources.setup_row_break
import app.milestone.resources.setup_row_env_altitude
import app.milestone.resources.setup_row_env_temp
import app.milestone.resources.setup_row_environment
import app.milestone.resources.setup_row_equipment
import app.milestone.resources.setup_row_experience
import app.milestone.resources.setup_row_focus
import app.milestone.resources.setup_row_goal_distance
import app.milestone.resources.setup_row_health_questions
import app.milestone.resources.setup_row_high_load
import app.milestone.resources.setup_row_high_load_sub
import app.milestone.resources.setup_row_lift_days
import app.milestone.resources.setup_row_lift_experience
import app.milestone.resources.setup_row_longest_run
import app.milestone.resources.setup_row_max_hr
import app.milestone.resources.setup_row_meet
import app.milestone.resources.setup_row_priority_muscle
import app.milestone.resources.setup_row_race_date
import app.milestone.resources.setup_row_recent_race
import app.milestone.resources.setup_row_resting_hr
import app.milestone.resources.setup_row_run_experience
import app.milestone.resources.setup_row_runner_strength
import app.milestone.resources.setup_row_running_days
import app.milestone.resources.setup_row_running_volume
import app.milestone.resources.setup_row_training_goal
import app.milestone.resources.setup_row_training_goal_sub
import app.milestone.resources.setup_row_weekdays
import app.milestone.resources.setup_row_weekly_sets
import app.milestone.resources.setup_row_which_leads
import app.milestone.resources.setup_section_context
import app.milestone.resources.setup_section_health
import app.milestone.resources.setup_section_lifting
import app.milestone.resources.setup_section_plan
import app.milestone.resources.setup_section_running
import app.milestone.resources.setup_section_you
import app.milestone.resources.setup_sex_caption
import app.milestone.resources.setup_sex_label
import app.milestone.resources.setup_temp_value
import app.milestone.resources.setup_unit_bpm
import app.milestone.resources.setup_weekdays_q
import app.milestone.resources.setup_weeks_value
import app.milestone.resources.ui_cd_collapsed
import app.milestone.resources.ui_cd_expanded
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/**
 * User-editable training profile. Every committed edit calls [onApply] with the
 * new draft at once (no separate Apply step).
 *
 * [distanceUnit] is the km|mi the running rows show; the wire stays km. [today]
 * (the core's local epoch day) bounds the date rows: race/meet dates are
 * future-only, the restart date today or earlier. [onRerunSetup] opens the
 * guided setup: null = a plain re-run; a [Modality] = a Focus switch that needs
 * the new side's answers. [focusRow] expands that row and brings it into view
 * once; [onFocusHandled] tells the caller the request was taken.
 */
@Composable
internal fun ProfileEditor(
    initial: Event.SetProfile,
    distanceUnit: DistanceUnit,
    today: Long,
    onRerunSetup: (Modality?) -> Unit,
    focusRow: ProfileRowId?,
    onFocusHandled: () -> Unit,
    onApply: (Event.SetProfile) -> Unit,
) {
    val ctx = currentPlatformContext()
    val res = currentResources()
    // Saveable: this item leaves composition when scrolled away. Keyed on
    // `initial` so a new core profile (e.g. after a re-run setup) replaces a stale
    // draft that the next edit would otherwise re-send over it (HARD RULE 3).
    var draft by rememberSaveable(initial, stateSaver = jsonSaver(Event.SetProfile.serializer())) { mutableStateOf(initial) }
    val commit: (Event.SetProfile) -> Unit = {
        draft = it
        onApply(it)
    }

    // Which row's inline picker is open. One at a time; null = all collapsed.
    // Saveable so an open picker survives scroll-away / rotation like the draft.
    var open by rememberSaveable { mutableStateOf<ProfileRowId?>(null) }
    val toggle: (ProfileRowId) -> Unit = { id -> open = if (open == id) null else id }
    // A row asked for from outside (a hold banner): its picker (if any) expanded
    // now, the row itself scrolled to once it has laid out.
    var reveal by remember { mutableStateOf<ProfileRowId?>(null) }
    LaunchedEffect(focusRow) {
        if (focusRow != null) {
            open = focusRow.pickerRow
            reveal = focusRow
            onFocusHandled()
        }
    }

    @Composable
    fun FutureDateRow(id: ProfileRowId, label: String, stored: Long?, onPick: (Long?) -> Unit) {
        val day = stored?.takeIf { it > today }
        ProfileRow(open == id, { toggle(id) }, label, day?.let { epochDayLabel(it) }, placeholder = stringResource(Res.string.setup_date_none)) {
            DateAnswerRow(day, today, future = true, onChange = onPick)
        }
    }

    // An environment follow-up scale (temperature, altitude): shown for [env], or
    // while it holds a value so a set value can always be seen and cleared.
    @Composable
    fun EnvScaleRow(env: Environment, id: ProfileRowId, label: String, valueRes: StringResource, value: Double?, options: IntProgression, set: (Double) -> Event.SetProfile) {
        if (draft.environment != env && value == null) return
        RowHairline()
        ProfileRow(open == id, { toggle(id) }, label, value?.let { stringResource(valueRes, it.toInt()) }) {
            ScrollableScaleRow(options.toList(), value?.toInt(), { "$it" }, label = label) { commit(set(it.toDouble())) }
        }
    }

    val lift = draft.weeklySets > 0
    val run = draft.runningDaysPerWeek > 0
    val both = lift && run
    val focus = modalityOf(draft) ?: Modality.Lift

    // A Focus change reprograms the week, so it is confirmed first. Turning a side
    // ON needs that side's answers: a side set up before resumes from its stash
    // when every answer is there, else the guided setup asks them, seeded. Only
    // dropping a side applies here otherwise. Nothing invented is ever written.
    var pendingFocus by rememberSaveable { mutableStateOf<Modality?>(null) }
    pendingFocus?.let { target ->
        val addsSide = (target != Modality.Run && !lift) || (target != Modality.Lift && !run)
        val resumed = remember(target, draft) {
            if (addsSide) draft.resumedSide(target, ModalityStash.lift(ctx), ModalityStash.run(ctx), today) else null
        }
        val copy = focusSwitchCopy(focus, target)
        val title = if (resumed != null) {
            stringResource(if (target == Modality.Lift) Res.string.setup_focus_resume_lift_title else Res.string.setup_focus_resume_run_title)
        } else {
            stringResource(copy.titleRes)
        }
        val body = if (resumed != null) {
            val days = pluralStringResource(Res.plurals.setup_days_count, resumed.trainingDayTotal(), resumed.trainingDayTotal())
            if (target == Modality.Lift) {
                stringResource(Res.string.setup_focus_resume_lift_body, stringResource(resumed.liftGoal.labelRes), days)
            } else {
                stringResource(Res.string.setup_focus_resume_run_body, stringResource(resumed.goalDistance.labelRes), days)
            }
        } else {
            stringResource(copy.bodyRes)
        }
        ConfirmDialog(
            title = title,
            body = body,
            confirmLabel = stringResource(if (resumed != null) Res.string.setup_focus_switch_confirm else copy.confirmRes),
            onDismiss = { pendingFocus = null },
            onConfirm = {
                pendingFocus = null
                open = null
                if (target == Modality.Lift && run) ModalityStash.stashRun(ctx, draft)
                if (target == Modality.Run && lift) ModalityStash.stashLift(ctx, draft)
                when {
                    resumed != null -> commit(resumed)
                    addsSide -> onRerunSetup(target)
                    else -> commit(draft.withoutSide(target))
                }
            },
        )
    }

    // The Plan rows come first for every Focus, so a row never moves when the
    // modality changes.
    Column(verticalArrangement = Arrangement.spacedBy(Space.Screen.dp)) {
        ProfileGroup(stringResource(Res.string.setup_section_plan)) {
            SegmentedEnumRow(
                label = stringResource(Res.string.setup_row_focus),
                values = Modality.entries.toList(),
                current = focus,
                display = { res.getString(it.focusLabelRes) },
                onSelect = { if (it != focus) pendingFocus = it },
            )
            RowHairline()
            // A two-way choice (the core only reads lift-first vs run-first);
            // lifting leading means the lift goal itself leads, so it can never
            // contradict Training goal.
            if (both) {
                SegmentedEnumRow(
                    label = stringResource(Res.string.setup_row_which_leads),
                    values = BothPriority.entries.toList(),
                    current = draft.concurrentGoal.lead(),
                    display = { res.getString(it.labelRes) },
                ) { commit(draft.withLead(it)) }
                RowHairline()
            }
            // The ONE days control. The caption carries the Both split and the
            // weekdays, so the bold value stays one line.
            val dayTotal = draft.trainingDayTotal()
            val daysCaption = listOfNotNull(
                if (both) stringResource(Res.string.setup_days_split_sub, draft.shownLiftDays(), draft.runningDaysPerWeek) else null,
                weekdaysLine(draft.trainingWeekdays, dayTotal),
            ).joinToString(" · ")
            ProfileRow(
                open == ProfileRowId.TrainingDays,
                { toggle(ProfileRowId.TrainingDays) },
                stringResource(Res.string.setup_row_weekdays),
                pluralStringResource(Res.plurals.setup_days_count, dayTotal, dayTotal),
                subtitle = daysCaption,
            ) {
                // The setup's rule: the ticked weekdays must cover every training
                // day (or be "Any days"). Picks stage here and apply only once
                // they agree; collapsing the row drops a mismatched pick.
                var work by rememberSaveable(draft, stateSaver = jsonSaver(Event.SetProfile.serializer())) { mutableStateOf(draft) }
                val stage: (Event.SetProfile) -> Unit = { next ->
                    work = next
                    if (weekdaysCover(next.trainingWeekdays, next.trainingDayTotal())) commit(next)
                }
                val workTotal = work.trainingDayTotal()
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    val lead = work.concurrentGoal.lead()
                    if (lift) {
                        val liftLabel = stringResource(if (both) Res.string.setup_row_lift_days else Res.string.setup_days_scale_label)
                        val liftOptions = if (both) hybridLiftDayRange(lead) else dayOptions(Modality.Lift)
                        FieldLabel(liftLabel)
                        ChoiceScaleRow(liftOptions.toList(), work.shownLiftDays(), { "$it" }, label = liftLabel) { days ->
                            val runMax = hybridRunDayOptions(days, lead).last
                            stage(work.copy(liftDaysPerWeek = days, runningDaysPerWeek = work.runningDaysPerWeek.coerceAtMost(runMax)))
                        }
                    }
                    if (run) {
                        // Dropping running is the Focus row's job.
                        val runLabel = stringResource(if (both) Res.string.setup_row_running_days else Res.string.setup_days_scale_label)
                        val runOptions = if (both) hybridRunDayOptions(work.shownLiftDays(), lead) else dayOptions(Modality.Run)
                        FieldLabel(runLabel)
                        ChoiceScaleRow(runOptions.toList(), work.runningDaysPerWeek, { "$it" }, label = runLabel) {
                            stage(work.copy(runningDaysPerWeek = it))
                        }
                    }
                    FieldLabel(stringResource(Res.string.setup_weekdays_q))
                    WeekdayChips(work.trainingWeekdays) { stage(work.copy(trainingWeekdays = it)) }
                    if (!weekdaysCover(work.trainingWeekdays, workTotal)) {
                        Text(
                            pluralStringResource(Res.plurals.setup_days_weekdays_hint, workTotal, workTotal),
                            color = DangerText,
                            style = Type.Caption,
                        )
                    }
                }
            }
            RowHairline()
            // Experience: the same three answers and wording as the guided setup's
            // level step (progression cadence, training age, starting volumes). A
            // Both profile answers it per side, so it shows (and edits) both.
            val level = SetupLevel.fromCadence(draft.progressionCadence)
            ProfileRow(
                open == ProfileRowId.Experience,
                { toggle(ProfileRowId.Experience) },
                stringResource(if (both) Res.string.setup_row_lift_experience else Res.string.setup_row_experience),
                stringResource(level.titleRes),
            ) {
                OptionList(SetupLevel.entries.toList(), level, { res.getString(it.titleRes) }) {
                    commit(draft.copy(progressionCadence = it.cadence))
                    open = null
                }
            }
            if (both) {
                RowHairline()
                // The running level drives only `advanced` in the core; the answer
                // itself is the shell's record, shown while it agrees with that flag.
                var runLevel by remember(draft.advanced) { mutableStateOf(RunLevelAnswer.read(ctx, draft.advanced)) }
                val shownRunLevel = runLevel ?: SetupLevel.Advanced.takeIf { draft.advanced }
                ProfileRow(
                    open == ProfileRowId.RunExperience,
                    { toggle(ProfileRowId.RunExperience) },
                    stringResource(Res.string.setup_row_run_experience),
                    shownRunLevel?.let { stringResource(it.titleRes) } ?: stringResource(Res.string.setup_not_set),
                ) {
                    OptionList<SetupLevel?>(SetupLevel.entries.toList(), shownRunLevel, { l -> l?.let { res.getString(it.titleRes) }.orEmpty() }) { l ->
                        if (l != null) {
                            RunLevelAnswer.save(ctx, l)
                            runLevel = l
                            commit(draft.copy(advanced = l == SetupLevel.Advanced))
                            open = null
                        }
                    }
                }
            }
            RowHairline()
            ChevronRow(
                { onRerunSetup(null) },
                Modifier.clip(RoundedCornerShape(Space.Md.dp)),
                contentPadding = PaddingValues(vertical = Space.Sm.dp),
                spacing = Space.Md.dp,
            ) {
                Text(stringResource(Res.string.setup_rerun_setup), color = OnBgBody, style = Type.Body)
            }
        }

        if (lift) {
            ProfileGroup(stringResource(Res.string.setup_section_lifting)) {
                // In the wizard's order and with its labels.
                SegmentedEnumRow(
                    label = stringResource(Res.string.setup_row_training_goal),
                    values = LiftFocus.entries.map { it.liftGoal },
                    current = draft.liftGoal,
                    display = { res.getString(it.labelRes) },
                ) { commit(draft.withLiftGoal(it)) }
                Text(stringResource(Res.string.setup_row_training_goal_sub), color = OnBgFaint, style = Type.Caption)
                RowHairline()
                SegmentedEnumRow(
                    label = stringResource(Res.string.setup_modality_strength_q),
                    values = ResistanceModality.entries.toList(),
                    current = draft.resistanceModality,
                    display = {
                        res.getString(
                            when (it) {
                                ResistanceModality.Weights -> Res.string.setup_modality_weights_short
                                ResistanceModality.Calisthenics -> Res.string.setup_modality_calis_short
                            },
                        )
                    },
                ) { commit(draft.copy(resistanceModality = it)) }
                RowHairline()
                if (draft.liftGoal == LiftGoal.Hypertrophy) {
                    // Optional specialization; the core applies it only for
                    // experienced lifters (HYP-SPEC-BLOCK-001).
                    val muscle = HypertrophyMuscle.of(draft.specializationMuscle)
                    ProfileRow(
                        open == ProfileRowId.PriorityMuscle,
                        { toggle(ProfileRowId.PriorityMuscle) },
                        stringResource(Res.string.setup_row_priority_muscle),
                        muscle?.let { stringResource(it.labelRes) } ?: draft.specializationMuscle,
                        placeholder = stringResource(Res.string.setup_priority_none),
                    ) {
                        OptionList(
                            listOf<HypertrophyMuscle?>(null) + HypertrophyMuscle.entries,
                            muscle,
                            { m -> res.getString(m?.labelRes ?: Res.string.setup_priority_none) },
                        ) {
                            commit(draft.copy(specializationMuscle = it?.wire))
                            open = null
                        }
                    }
                } else {
                    // A test/meet date shapes the taper and peak; a past one no longer does.
                    FutureDateRow(ProfileRowId.Meet, stringResource(Res.string.setup_row_meet), draft.meetEpochDay) {
                        commit(draft.copy(meetEpochDay = it))
                    }
                }
                if (draft.resistanceModality == ResistanceModality.Calisthenics) {
                    RowHairline()
                    ProfileRow(
                        open == ProfileRowId.Equipment,
                        { toggle(ProfileRowId.Equipment) },
                        stringResource(Res.string.setup_row_equipment),
                        draft.calisEquipment?.let { equipmentText(it) },
                    ) {
                        EquipmentChips(draft.calisEquipment) { commit(draft.copy(calisEquipment = it)) }
                    }
                }
                RowHairline()
                val setsLabel = stringResource(Res.string.setup_row_weekly_sets)
                ProfileRow(open == ProfileRowId.WeeklySets, { toggle(ProfileRowId.WeeklySets) }, setsLabel, "${draft.weeklySets}") {
                    ScrollableScaleRow(WEEKLY_SETS_OPTIONS.toList(), draft.weeklySets, { "$it" }, label = setsLabel) {
                        commit(draft.copy(weeklySets = it))
                    }
                }
            }
        }

        if (run) {
            ProfileGroup(stringResource(Res.string.setup_section_running)) {
                ProfileRow(
                    open == ProfileRowId.GoalDistance,
                    { toggle(ProfileRowId.GoalDistance) },
                    stringResource(Res.string.setup_row_goal_distance),
                    stringResource(draft.goalDistance.labelRes),
                ) {
                    OptionList(GoalDistance.entries.toList(), draft.goalDistance, { res.getString(it.labelRes) }) {
                        commit(draft.copy(goalDistance = it))
                        open = null
                    }
                }
                if (draft.goalDistance.isRace) {
                    RowHairline()
                    FutureDateRow(ProfileRowId.RaceDate, stringResource(Res.string.setup_row_race_date), draft.goalRaceEpochDay) {
                        commit(draft.copy(goalRaceEpochDay = it))
                    }
                    RowHairline()
                    ProfileRow(
                        open == ProfileRowId.RecentRace,
                        { toggle(ProfileRowId.RecentRace) },
                        stringResource(Res.string.setup_row_recent_race),
                        draft.recentRace?.let { recentRaceText(it, distanceUnit) },
                    ) {
                        RecentRaceEditor(draft.recentRace) { commit(draft.copy(recentRace = it)) }
                    }
                }
                RowHairline()
                // Weekly volume in the user's distance unit (km or mi), the unit in
                // the value: options, highlight and value are whole display units; a
                // pick converts back to canonical km for the wire (mi via
                // miToKmWire), so what is shown is what is stored. Starts at 1, not
                // 0: "I don't run yet" is a setup answer and dropping running is the
                // Focus row's job. Never given → "Not set"; the engine then picks its
                // own conservative start, and "Not sure" returns to that.
                val volumeLabel = stringResource(Res.string.setup_row_running_volume)
                val volume = weeklyVolumeScaleValue(draft.runningKmPerWeek, distanceUnit).takeIf { draft.runningKmPerWeekSet }
                ProfileRow(
                    open == ProfileRowId.WeeklyDistance,
                    { toggle(ProfileRowId.WeeklyDistance) },
                    volumeLabel,
                    volume?.let { stringResource(Res.string.setup_distance_value, "$it", distanceUnit.distanceLabel) },
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        ScrollableScaleRow(
                            weeklyVolumeScaleOptions(distanceUnit),
                            volume,
                            { "$it" },
                            label = volumeLabel,
                        ) {
                            commit(draft.copy(runningKmPerWeek = weeklyVolumeScaleToKm(it, distanceUnit), runningKmPerWeekSet = true))
                        }
                        AnswerChip(stringResource(Res.string.setup_not_sure), selected = !draft.runningKmPerWeekSet) {
                            commit(draft.copy(runningKmPerWeek = 0.0, runningKmPerWeekSet = false))
                        }
                    }
                }
                RowHairline()
                // Longest run in the last month (RUN-SPIKE-001 day-one anchor; logged
                // runs supersede it), in the user's distance unit.
                val longestLabel = stringResource(Res.string.setup_row_longest_run)
                ProfileRow(
                    open == ProfileRowId.LongestRun,
                    { toggle(ProfileRowId.LongestRun) },
                    longestLabel,
                    draft.statedLongestRecentKm?.let { stringResource(Res.string.setup_distance_value, distanceBufferText(it, distanceUnit), distanceUnit.distanceLabel) },
                    revealContent = true,
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                        key(distanceUnit) {
                            ProfileKeypadEditor(
                                longestLabel,
                                distanceUnit.distanceLabel,
                                draft.statedLongestRecentKm?.let { distanceBufferText(it, distanceUnit) },
                                distanceBounds(LONGEST_RUN_KM, distanceUnit),
                                allowDecimal = true,
                            ) { commit(draft.copy(statedLongestRecentKm = displayToKm(it, distanceUnit))) }
                        }
                        // Clears the stated value (the plan then anchors on logged runs).
                        AnswerChip(stringResource(Res.string.setup_not_sure), selected = draft.statedLongestRecentKm == null) {
                            commit(draft.copy(statedLongestRecentKm = null))
                        }
                    }
                }
                if (!lift) {
                    // A pure runner can opt into two short strength sessions.
                    RowHairline()
                    LabeledSwitchRow(
                        label = stringResource(Res.string.setup_row_runner_strength),
                        checked = draft.runnerStrength,
                        onCheckedChange = { commit(draft.copy(runnerStrength = it)) },
                    )
                }
                // Advanced: its only consumer in the core is the marathon goal-week
                // plan (coaching depth follows Experience, INDIV-TRAGE-001), so the
                // row shows for a marathon goal, or while it is on so it can be
                // turned off.
                if (draft.goalDistance == GoalDistance.Marathon || draft.advanced) {
                    RowHairline()
                    LabeledSwitchRow(
                        label = stringResource(Res.string.setup_row_advanced),
                        subtitle = stringResource(Res.string.setup_row_advanced_sub),
                        subtitleColor = OnBgFaint,
                        checked = draft.advanced,
                        onCheckedChange = { commit(draft.copy(advanced = it)) },
                    )
                }
            }
        }

        // You: consolidated person data, entered once here; the protein /
        // HR-zone calculators (Profile → Calculators) prefill from it. All optional: "Not set" until given.
        ProfileGroup(stringResource(Res.string.setup_section_you)) {
            ProfileRow(open == ProfileRowId.Sex, { toggle(ProfileRowId.Sex) }, stringResource(Res.string.setup_sex_label), stringResource(sexLabelRes(draft.female))) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    // Full-width rows (not a segmented cell row) so "Prefer not to
                    // say" never clips at large font sizes; it stores no answer (null).
                    OptionList(
                        listOf<Boolean?>(false, true, null),
                        draft.female,
                        { res.getString(sexLabelRes(it)) },
                    ) {
                        commit(draft.copy(female = it))
                        open = null
                    }
                    Text(stringResource(Res.string.setup_sex_caption), color = OnBgMuted, style = Type.Caption)
                }
            }
            // Age comes from the birthday (a date picker); the core derives it each
            // day. A legacy stated age shows with a prompt to add the birthday;
            // "Prefer not to say" clears both (re-picking a birthday undoes it).
            // Bodyweight and heart rates are typed on the in-form keypad: each
            // valid entry applies at once; an out-of-range buffer is flagged and
            // never sent.
            RowHairline()
            val birthDay = draft.birthEpochDay
            val ageGiven = birthDay != null || draft.ageYears != null
            ProfileRow(
                open == ProfileRowId.Age,
                { toggle(ProfileRowId.Age) },
                stringResource(Res.string.setup_row_age),
                when {
                    birthDay != null -> birthdayLabel(birthDay, today)
                    else -> draft.ageYears?.let { stringResource(Res.string.setup_age_value, trimDecimal(it)) }
                },
                subtitle = if (birthDay == null && draft.ageYears != null) stringResource(Res.string.setup_birthday_add_prompt) else null,
                placeholder = stringResource(Res.string.setup_age_not_given),
                revealRequest = reveal == ProfileRowId.Age,
                onRevealed = { reveal = null },
                revealContent = true,
            ) {
                // The row above already shows the birthday, so the picker button
                // offers the change instead of repeating it. Without an age the HR
                // targets fall back to effort unless a measured max HR anchors them.
                BirthdayAnswer(
                    birthDay,
                    today,
                    hint = when {
                        draftFacts(ProfileDraft(ageYears = draft.ageYears)).youth -> stringResource(Res.string.setup_health_defer_youth)
                        !ageGiven && draft.measuredHrMax == null -> stringResource(Res.string.setup_birthday_skipped_note)
                        else -> null
                    },
                    skipped = !ageGiven,
                    pickLabel = when {
                        birthDay != null -> stringResource(Res.string.setup_birthday_change)
                        !ageGiven -> stringResource(Res.string.setup_birthday_add)
                        else -> null
                    },
                    onSkip = { if (ageGiven) commit(draft.copy(birthEpochDay = null, ageYears = null)) },
                ) { day -> commit(draft.copy(birthEpochDay = day, ageYears = birthdayAge(day, today))) }
            }
            RowHairline()
            val bwUnit = draft.weightUnit
            val bwLabel = stringResource(Res.string.setup_row_bodyweight)
            ProfileRow(
                open == ProfileRowId.Bodyweight,
                { toggle(ProfileRowId.Bodyweight) },
                bwLabel,
                draft.bodyweightKg?.let { stringResource(Res.string.setup_bodyweight_value, bodyweightBufferText(it, bwUnit), weightSuffix(bwUnit)) },
                revealContent = true,
            ) {
                // Honors the kg|lb preference like every other weight surface: the
                // buffer is in the display unit and a valid entry converts back to
                // the canonical kg wire value. Keyed on the unit so a unit switch
                // re-seeds the buffer instead of reinterpreting the typed number.
                key(bwUnit) {
                    ProfileKeypadEditor(bwLabel, weightSuffix(bwUnit), draft.bodyweightKg?.let { bodyweightBufferText(it, bwUnit) }, bodyweightBounds(bwUnit), allowDecimal = true) {
                        commit(draft.copy(bodyweightKg = bodyweightDisplayToKg(it, bwUnit)))
                    }
                }
            }
            RowHairline()
            val rhrLabel = stringResource(Res.string.setup_row_resting_hr)
            ProfileRow(
                open == ProfileRowId.RestingHr,
                { toggle(ProfileRowId.RestingHr) },
                rhrLabel,
                draft.restingHrBpm?.let { stringResource(Res.string.setup_bpm_value, trimDecimal(it)) },
                revealContent = true,
            ) {
                ProfileKeypadEditor(rhrLabel, stringResource(Res.string.setup_unit_bpm), draft.restingHrBpm?.let(::trimDecimal), RESTING_HR_RANGE) {
                    commit(draft.copy(restingHrBpm = it))
                }
            }
            RowHairline()
            val maxHrLabel = stringResource(Res.string.setup_row_max_hr)
            ProfileRow(
                open == ProfileRowId.MaxHr,
                { toggle(ProfileRowId.MaxHr) },
                maxHrLabel,
                draft.measuredHrMax?.let { stringResource(Res.string.setup_bpm_value, trimDecimal(it)) },
                revealContent = true,
            ) {
                ProfileKeypadEditor(
                    maxHrLabel,
                    stringResource(Res.string.setup_unit_bpm),
                    draft.measuredHrMax?.let(::trimDecimal),
                    MAX_HR_RANGE,
                    hint = stringResource(Res.string.setup_max_hr_hint),
                ) {
                    commit(draft.copy(measuredHrMax = it))
                }
            }
        }

        // Health & safety, directly under the person data so the safety gates never
        // sink below the optional context fields. These arm the core's
        // medical-deferral gates (PAR-Q+, pregnancy, injury/rehab, RED-S): a raised
        // gate makes the engine defer to a professional rather than program a plan;
        // SAFETY overrides goals (HARD RULE 3). Age drives the youth gate core-side.
        ProfileGroup(stringResource(Res.string.setup_section_health)) {
            // The seven PAR-Q+ questions fold into one summary row ("None apply" /
            // "2 apply") that expands to the questions and the clearance follow-up.
            // A banner's deep link lands on the answer that holds training: a
            // question or the clearance inside the open list, else a switch row.
            val parqFacts = draftFacts(ProfileDraft(health = draft.health))
            val parqItem = parqRevealItem(reveal, draft.health)
            val clearanceReveal = reveal == ProfileRowId.ParqClearance && draft.health.parq_positive
            val parqInner = parqItem != null || clearanceReveal
            ProfileRow(
                open == ProfileRowId.HealthQuestions,
                { toggle(ProfileRowId.HealthQuestions) },
                stringResource(Res.string.setup_row_health_questions),
                healthSummary(parqFacts, count = parqFacts.parq_yes.size),
                revealRequest = reveal?.pickerRow == ProfileRowId.HealthQuestions && !parqInner,
                onRevealed = { reveal = null },
                revealOnExpand = !parqInner,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                    ParqQuestions(
                        draft.health,
                        divider = { RowHairline() },
                        revealItem = parqItem,
                        revealClearance = clearanceReveal,
                        onRevealed = { reveal = null },
                    ) { commit(draft.copy(health = it)) }
                }
            }
            RowHairline()
            CurrentHealthRows(
                draft.health,
                divider = { RowHairline() },
                reveal = reveal,
                onRevealed = { reveal = null },
            ) { commit(draft.copy(health = it)) }
        }

        // Training context: environment / re-entry / overload (indiv-025 &
        // safety-024 environment modifiers, running-041 pace correction,
        // REENTRY-001 re-entry ramp, autoreg-029 overload guard). All optional:
        // "Not set" until chosen, so a plain profile arms none of them.
        ProfileGroup(stringResource(Res.string.setup_section_context)) {
            if (run) {
                // Environment only changes running guidance.
                ProfileRow(
                    open == ProfileRowId.Environment,
                    { toggle(ProfileRowId.Environment) },
                    stringResource(Res.string.setup_row_environment),
                    draft.environment?.let { stringResource(it.labelRes) },
                ) {
                    OptionList(
                        listOf<Environment?>(null) + Environment.entries,
                        draft.environment,
                        { it?.let { e -> res.getString(e.labelRes) } ?: res.getString(Res.string.setup_not_set) },
                    ) {
                        commit(draft.copy(environment = it))
                        open = null
                    }
                }
                EnvScaleRow(Environment.Heat, ProfileRowId.EnvTemp, stringResource(Res.string.setup_row_env_temp), Res.string.setup_temp_value, draft.envTempC, ENV_TEMP_OPTIONS_C) {
                    draft.copy(envTempC = it)
                }
                EnvScaleRow(Environment.Altitude, ProfileRowId.EnvAltitude, stringResource(Res.string.setup_row_env_altitude), Res.string.setup_altitude_value, draft.envAltitudeM, ENV_ALTITUDE_OPTIONS_M) {
                    draft.copy(envAltitudeM = it)
                }
                RowHairline()
            }
            // Back from a break (REENTRY-001 / reentry-002): weeks fully off plus the
            // day training restarted, so the re-entry ramp runs from that day.
            val breakValue = draft.weeksOff?.let {
                stringResource(Res.string.setup_break_value, epochDayShortLabel(draft.layoffReturnedEpochDay ?: today, today))
            }
            ProfileRow(
                open == ProfileRowId.Break,
                { toggle(ProfileRowId.Break) },
                stringResource(Res.string.setup_row_break),
                breakValue,
                placeholder = stringResource(Res.string.setup_break_none),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                    val weeksLabel = stringResource(Res.string.setup_break_weeks_label)
                    FieldLabel(weeksLabel)
                    ScrollableScaleRow(WEEKS_OFF_OPTIONS.toList(), draft.weeksOff?.toInt(), { res.getString(Res.string.setup_weeks_value, it) }, label = weeksLabel) {
                        commit(draft.copy(weeksOff = it.toDouble(), layoffReturnedEpochDay = draft.layoffReturnedEpochDay ?: today))
                    }
                    AnswerChip(stringResource(Res.string.setup_break_none), selected = draft.weeksOff == null) {
                        commit(draft.copy(weeksOff = null, layoffReturnedEpochDay = null))
                    }
                    if (draft.weeksOff != null) {
                        FieldLabel(stringResource(Res.string.setup_break_restart_label))
                        DateAnswerRow(draft.layoffReturnedEpochDay ?: today, today, future = false) {
                            commit(draft.copy(layoffReturnedEpochDay = it))
                        }
                    }
                }
            }
            RowHairline()
            LabeledSwitchRow(
                label = stringResource(Res.string.setup_row_high_load),
                subtitle = stringResource(Res.string.setup_row_high_load_sub),
                checked = draft.highLoadBlock,
                onCheckedChange = { commit(draft.copy(highLoadBlock = it)) },
            )
        }
    }
}

internal class FocusSwitchCopy(val titleRes: StringResource, val bodyRes: StringResource, val confirmRes: StringResource)

internal fun focusSwitchCopy(from: Modality, to: Modality): FocusSwitchCopy = when (to) {
    Modality.Lift -> if (from == Modality.Both) {
        FocusSwitchCopy(Res.string.setup_focus_to_lift_only_title, Res.string.setup_focus_to_lift_only_body, Res.string.setup_focus_switch_confirm)
    } else {
        FocusSwitchCopy(Res.string.setup_focus_to_lift_title, Res.string.setup_focus_to_lift_body, Res.string.setup_focus_answer_lift)
    }
    Modality.Run -> if (from == Modality.Both) {
        FocusSwitchCopy(Res.string.setup_focus_to_run_only_title, Res.string.setup_focus_to_run_only_body, Res.string.setup_focus_switch_confirm)
    } else {
        FocusSwitchCopy(Res.string.setup_focus_to_run_title, Res.string.setup_focus_to_run_body, Res.string.setup_focus_answer_run)
    }
    Modality.Both -> if (from == Modality.Run) {
        FocusSwitchCopy(Res.string.setup_focus_add_lift_title, Res.string.setup_focus_add_lift_body, Res.string.setup_focus_answer_lift)
    } else {
        FocusSwitchCopy(Res.string.setup_focus_add_run_title, Res.string.setup_focus_add_run_body, Res.string.setup_focus_answer_run)
    }
}

/**
 * Edit a recent race: distance chips ("I don't know" clears it) and a finish
 * time typed on the keypad. Only a complete, plausible time applies, so the
 * stored race is always one the field displayed.
 */
@Composable
private fun RecentRaceEditor(race: RecentRace?, onChange: (RecentRace?) -> Unit) {
    var distance by rememberSaveable { mutableStateOf(race?.let { RaceDistance.of(it.distance_km) }) }
    var digits by rememberSaveable { mutableStateOf(race?.let { secondsToHmsDigits(it.time_sec) } ?: "") }
    var fresh by rememberSaveable { mutableStateOf(true) }
    val invalid = distance != null && digits.isNotEmpty() && recentRaceOf(distance, digits) == null
    fun edit(next: String) {
        digits = next
        fresh = false
        recentRaceOf(distance, next)?.let(onChange)
    }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        RaceDistanceChips(distance) { pick ->
            distance = pick
            fresh = true
            if (pick == null) {
                digits = ""
                onChange(null)
            } else {
                recentRaceOf(pick, digits)?.let(onChange)
            }
        }
        if (distance != null) {
            RaceTimeField(digits, active = true, invalid = invalid) { fresh = true }
            NumericKeypad(
                onKey = { key -> appendRaceDigit(digits, key, fresh)?.let(::edit) },
                onBackspace = { edit(digits.dropLast(1)) },
                allowDecimal = false,
            )
        }
    }
}

/** Weekly sets per muscle the Profile scale offers. Starts at 1, not 0: a
 *  modality's on/off is owned by the Focus row, which confirms the switch first. */
private val WEEKLY_SETS_OPTIONS = 1..30

/** Outdoor temperatures offered, °C: a plausible training band around the ~15 °C
 *  where the heat pace correction arms (running-041). */
private val ENV_TEMP_OPTIONS_C = -10..45

/** Altitudes offered, m: past the ~900 m where the altitude correction arms
 *  (running-041) and the 2,500 m KB environment-modifier mark (indiv-025). */
private val ENV_ALTITUDE_OPTIONS_M = 0..4500 step 100

private val WEEKS_OFF_OPTIONS = 1..52

/** Sex answer → its label; null (not given) → "Prefer not to say". */
internal fun sexLabelRes(female: Boolean?): StringResource = SexAnswer.of(female).labelRes

/**
 * Keypad entry for one Profile person number. The buffer starts from the stored
 * value (or empty) and is exactly what is shown; every keystroke that leaves an
 * in-range number applies it at once (Profile changes apply immediately), while an
 * out-of-range or partial buffer is flagged and never sent, so the stored value is
 * always one the field displayed.
 */
@Composable
private fun ProfileKeypadEditor(
    label: String,
    unit: String,
    seedText: String?,
    range: ClosedFloatingPointRange<Double>,
    // Whole-number fields (heart rates) get no decimal key that would do nothing.
    allowDecimal: Boolean = false,
    hint: String? = null,
    onValid: (Double) -> Unit,
) {
    var text by rememberSaveable { mutableStateOf(seedText ?: "") }
    var fresh by rememberSaveable { mutableStateOf(true) }
    var touched by rememberSaveable { mutableStateOf(false) }
    val valid = text.toDoubleOrNull()?.let { it in range } == true
    val edit: (String) -> Unit = { t ->
        text = t
        fresh = false
        touched = true
        t.toDoubleOrNull()?.takeIf { it in range }?.let(onValid)
    }
    // A keystroke can insert or drop the pinned hold banner above the list (an
    // under-18 age), which slides the field being typed under it: keep the whole
    // editor, and at least the field, on screen after each one.
    val editorRequester = remember { BringIntoViewRequester() }
    val fieldRequester = remember { BringIntoViewRequester() }
    val editorTop = remember { WindowTop() }
    LaunchedEffect(text) {
        if (touched) revealWhenSettled(editorTop, editorRequester, fieldRequester)
    }
    Column(
        modifier = Modifier
            .bringIntoViewRequester(editorRequester)
            .onGloballyPositioned { editorTop.y = it.positionInWindow().y },
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Box(Modifier.bringIntoViewRequester(fieldRequester)) {
            KeypadValueField(
                label, unit, text,
                active = true,
                invalid = touched && text.isNotEmpty() && !valid,
                min = range.start, max = range.endInclusive, decimals = if (allowDecimal) 1 else 0,
                adjustments = emptyList(),
                onActivate = { fresh = true },
                onText = edit,
                hint = hint,
            )
        }
        NumericKeypad(text, fresh, edit, allowDecimal = allowDecimal)
    }
}

/**
 * One Profile group: its accent section overline (a TalkBack heading) over its own
 * card of hairline-divided rows, so the Profile reads as a few short cards rather
 * than one long one.
 */
@Composable
private fun ProfileGroup(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        SectionOverline(title)
        PlainCard {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) { content() }
        }
    }
}

/**
 * A profile field as a tap-row (04-profile §1): label left, value bold right
 * (under the label when it does not fit on one line), chevron; tapping toggles the
 * inline [content] picker below. A null [value] shows [placeholder] (default "Not
 * set") faint. Expanding scrolls the row and its picker into view; [revealRequest]
 * does the same for a row opened from elsewhere, then reports [onRevealed]. With
 * [revealContent] (a keypad picker) the whole picker wins over the row's label.
 * [revealOnExpand] false leaves the scrolling to a [RevealTarget] in the picker.
 */
@Composable
private fun ProfileRow(
    expanded: Boolean,
    onToggle: () -> Unit,
    label: String,
    value: String?,
    // Optional data line under the label (e.g. the ticked weekdays); null keeps
    // the row single-line.
    subtitle: String? = null,
    placeholder: String? = null,
    revealRequest: Boolean = false,
    onRevealed: () -> Unit = {},
    revealContent: Boolean = false,
    revealOnExpand: Boolean = true,
    content: @Composable () -> Unit,
) {
    val state = stringResource(if (expanded) Res.string.ui_cd_expanded else Res.string.ui_cd_collapsed)
    val requester = remember { BringIntoViewRequester() }
    val contentRequester = remember { BringIntoViewRequester() }
    val rowTop = remember { WindowTop() }
    val landIn = LocalRevealViewport.current
    LaunchedEffect(expanded, revealRequest) {
        val onExpand = expanded && revealOnExpand
        if (onExpand && revealContent) {
            revealWhenSettled(rowTop, requester, contentRequester)
        } else if (revealRequest) {
            revealWhenSettled(rowTop, requester, landIn = landIn)
        } else if (onExpand) {
            revealWhenSettled(rowTop, requester)
        }
        if (revealRequest) onRevealed()
    }
    val shown = value ?: placeholder ?: stringResource(Res.string.setup_not_set)
    val valueStyle = if (value == null) Type.Body else Type.BodyBold.merge(TabularFigures)
    val valueColor = if (value == null) OnBgFaint else OnBgBody
    val measurer = rememberTextMeasurer()
    Column(
        modifier = Modifier
            .bringIntoViewRequester(requester)
            .onGloballyPositioned { rowTop.place(it) },
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val valueMaxPx = constraints.maxWidth * 0.4f
            val inline = measurer.measure(shown, style = valueStyle, maxLines = 1, softWrap = false).size.width <= valueMaxPx
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    // Clip before clickable so the ripple stays rounded (no square
                    // flash around the row).
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .clickable(role = Role.Button) { onToggle() }
                    .semantics { stateDescription = state }
                    // Inside the clip so a multi-line caption's last line never
                    // touches the clip edge.
                    .padding(vertical = Space.Sm.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(
                    modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                ) {
                    Text(label, color = OnBgBody, style = Type.Body)
                    if (!inline) {
                        Text(shown, color = valueColor, style = valueStyle)
                    }
                    if (subtitle != null) {
                        Text(subtitle, color = OnBgFaint, style = Type.Caption)
                    }
                }
                if (inline) {
                    Text(shown, color = valueColor, style = valueStyle, textAlign = TextAlign.End, maxLines = 1)
                    Spacer(Modifier.width(Space.Md.dp))
                }
                RowChevron(expanded)
            }
        }
        if (expanded) Box(Modifier.bringIntoViewRequester(contentRequester)) { content() }
    }
}
