package app.milestone

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DatePickerState
import androidx.compose.material3.DisplayMode
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.time.LocalDate

// The guided setup's widgets and copy, shared with the Profile editor (health
// rows, answer chips, date and race-time answers): one chip set for both screens.

// ── Health screen ───────────────────────────────────────────────────────────

/** One PAR-Q+ question in plain language (onboard-051). */
internal val ParqItem.questionRes: Int
    get() = when (this) {
        ParqItem.HeartOrBloodPressure -> R.string.setup_parq_q1
        ParqItem.ChestPain -> R.string.setup_parq_q2
        ParqItem.DizzyOrFainting -> R.string.setup_parq_q3
        ParqItem.OtherCondition -> R.string.setup_parq_q4
        ParqItem.Medication -> R.string.setup_parq_q5
        ParqItem.BoneOrJoint -> R.string.setup_parq_q6
        ParqItem.SupervisedOnly -> R.string.setup_parq_q7
    }

/**
 * The seven PAR-Q+ questions + the "Cleared by a doctor" follow-up they unlock.
 * Every change goes through [onChange] with the whole new [HealthScreen], the gate
 * flags re-derived from the answers ([HealthScreen.withParqAnswer]) so a hidden
 * stale flag never defeats or pins a gate (HARD RULE 3). [revealItem] /
 * [revealClearance] scroll that row into view, then [onRevealed].
 */
@Composable
internal fun ParqQuestions(
    health: HealthScreen,
    divider: @Composable () -> Unit,
    showIntro: Boolean = true,
    leadingDivider: Boolean = true,
    revealItem: ParqItem? = null,
    revealClearance: Boolean = false,
    onRevealed: () -> Unit = {},
    onChange: (HealthScreen) -> Unit,
) {
    if (showIntro) Text(stringResource(R.string.setup_parq_intro), color = OnBgMuted, style = Type.Caption)
    if (health.legacyParq) {
        Text(stringResource(R.string.setup_parq_legacy), color = OnBgMuted, style = Type.Caption)
    }
    // A raised answer says what it means under its row: a chest-pain or fainting Yes
    // (once, on the first symptom answered Yes) and the bone/joint Yes (the
    // injury/rehab hold); any other Yes unlocks the clearance row.
    val firstSymptom = ParqItem.entries.firstOrNull { it.symptom && health.parqYes(it) }
    ParqItem.entries.forEachIndexed { i, item ->
        if (i > 0 || leadingDivider) divider()
        RevealTarget(revealItem == item, onRevealed) {
            LabeledSwitchRow(
                label = stringResource(item.questionRes),
                subtitle = when {
                    item.injury && health.parqYes(item) -> stringResource(R.string.setup_health_injury_body)
                    item == firstSymptom -> stringResource(R.string.setup_parq_symptom_body)
                    else -> null
                },
                checked = health.parqYes(item),
                onCheckedChange = { onChange(health.withParqAnswer(item, it)) },
            )
        }
    }
    if (health.parq_positive) {
        divider()
        RevealTarget(revealClearance, onRevealed) {
            LabeledSwitchRow(
                label = stringResource(R.string.setup_health_cleared_label),
                subtitle = stringResource(R.string.setup_health_cleared_body),
                checked = health.medically_cleared,
                onCheckedChange = { onChange(health.withClearance(it)) },
            )
        }
    }
}

/** The current-state health rows (illness, pregnancy, RED-S) below the PAR-Q+.
 *  A consequence line shows under a raised answer only; a line that defines the
 *  question (the warning signs, what under-eating looks like) always shows.
 *  Dependent children are cleared with their parent. [reveal] names the row a
 *  hold banner's deep link scrolls into view, then [onRevealed]. */
@Composable
internal fun CurrentHealthRows(
    health: HealthScreen,
    divider: @Composable () -> Unit,
    reveal: ProfileRowId? = null,
    onRevealed: () -> Unit = {},
    onChange: (HealthScreen) -> Unit,
) {
    RevealTarget(reveal == ProfileRowId.Ill, onRevealed) {
        LabeledSwitchRow(
            label = stringResource(R.string.setup_health_ill_label),
            subtitle = stringResource(R.string.setup_health_ill_body).takeIf { health.acute_illness },
            checked = health.acute_illness,
            onCheckedChange = { onChange(health.copy(acute_illness = it)) },
        )
    }
    divider()
    RevealTarget(reveal == ProfileRowId.Pregnant, onRevealed) {
        LabeledSwitchRow(
            label = stringResource(R.string.setup_health_pregnant_label),
            subtitle = stringResource(R.string.setup_health_pregnant_body).takeIf { health.pregnant },
            checked = health.pregnant,
            // Turning pregnant OFF clears the warning sign, whose row would otherwise
            // vanish while the flag stays true (a hidden, un-liftable hold).
            onCheckedChange = { onChange(health.copy(pregnant = it, pregnancy_warning_sign = if (it) health.pregnancy_warning_sign else false)) },
        )
    }
    if (health.pregnant) {
        divider()
        RevealTarget(reveal == ProfileRowId.PregnancyWarning, onRevealed) {
            LabeledSwitchRow(
                label = stringResource(R.string.setup_health_pregnancy_warning_label),
                subtitle = stringResource(R.string.setup_health_pregnancy_warning_body),
                checked = health.pregnancy_warning_sign,
                onCheckedChange = { onChange(health.copy(pregnancy_warning_sign = it)) },
            )
        }
    }
    divider()
    RevealTarget(reveal == ProfileRowId.Reds, onRevealed) {
        LabeledSwitchRow(
            label = stringResource(R.string.setup_health_reds_label),
            subtitle = stringResource(R.string.setup_health_reds_body),
            checked = health.reds_signal,
            onCheckedChange = { onChange(health.copy(reds_signal = it)) },
        )
    }
}

/** How many health answers apply ("None apply", "2 apply"), or the re-answer
 *  prompt for a pre-questions flag. [count] defaults to every answer (the review);
 *  the Profile PAR-Q+ row passes the PAR-Q+ answers alone. */
@Composable
internal fun healthSummary(
    health: HealthScreen,
    count: Int = health.parqYesCount +
        listOf(health.acute_illness, health.pregnant, health.pregnancy_warning_sign, health.reds_signal).count { it },
): String = when {
    count > 0 -> pluralStringResource(R.plurals.setup_health_flag_count, count, count)
    health.legacyParq -> stringResource(R.string.setup_parq_value_legacy)
    else -> stringResource(R.string.setup_parq_value_clear)
}

// ── Answer copy ─────────────────────────────────────────────────────────────

/** A one-line, plain-language blurb for each race-distance goal (display only); a
 *  distance it names is shown in the user's [unit]. */
@Composable
internal fun goalBlurb(goal: GoalDistance, unit: DistanceUnit): String = when (goal) {
    GoalDistance.General -> stringResource(R.string.setup_goaldist_general_blurb)
    GoalDistance.C25k -> stringResource(R.string.setup_goaldist_c25k_blurb)
    GoalDistance.FiveK -> stringResource(R.string.setup_goaldist_5k_blurb)
    GoalDistance.TenK -> stringResource(R.string.setup_goaldist_10k_blurb)
    GoalDistance.HalfMarathon -> stringResource(R.string.setup_goaldist_half_blurb, distanceText(RaceDistance.Half.km, unit))
    GoalDistance.Marathon -> stringResource(R.string.setup_goaldist_marathon_blurb)
}

/** A canonical km distance with its unit, in the user's [unit] ("21.1 km" / "13.1 mi"). */
@Composable
private fun distanceText(km: Double, unit: DistanceUnit): String =
    stringResource(R.string.setup_distance_value, distanceBufferText(km, unit), unit.distanceLabel)

/** The weekdays line under a day count: "Mon, Wed, Fri"; "Any 3 of Mon, Tue,
 *  Wed, Thu" when more days are ticked than trained (the plan picks among them);
 *  "Any days" when none are. */
@Composable
internal fun weekdaysLine(mask: Int?, days: Int): String = when {
    mask == null -> stringResource(R.string.setup_weekdays_any)
    weekdayCount(mask) > days -> stringResource(R.string.setup_weekdays_any_of, days, weekdaysLabel(mask))
    else -> weekdaysLabel(mask)
}

/** The honest Both-week split for the athlete's available days: the lift days
 *  they chose and the run days the engine will actually schedule, saying so when
 *  the lifting-leads run cap leaves some days as rest (HYB-RUNCAP-001). */
@Composable
internal fun hybridSplitText(totalDays: Int, liftDays: Int, lead: BothPriority?): String {
    val run = hybridRunDays(totalDays, liftDays, lead)
    val split = stringResource(
        R.string.setup_days_split,
        pluralStringResource(R.plurals.setup_days_lift_count, liftDays, liftDays),
        pluralStringResource(R.plurals.setup_days_run_count, run, run),
    )
    val rest = totalDays - liftDays - run
    if (rest <= 0) return split
    val restDays = pluralStringResource(R.plurals.setup_days_rest_count, rest, rest)
    return split + " " + stringResource(R.string.setup_days_split_capped, HYBRID_CAPPED_DAYS, restDays)
}

/** A recent race as a short line: "5K in 26:15"; a distance no chip matches is
 *  shown in the user's [unit]. */
@Composable
internal fun recentRaceText(race: RecentRace, unit: DistanceUnit): String {
    val dist = RaceDistance.of(race.distance_km)?.let { stringResource(it.labelRes) } ?: distanceText(race.distance_km, unit)
    return stringResource(R.string.setup_recent_race_value, dist, raceTimeDisplay(raceTimeDigits(race.time_sec)))
}

/** "Pull-up bar, rings" / "None" for an answered equipment set. */
@Composable
internal fun equipmentText(e: CalisEquipment): String =
    CalisPiece.entries.filter { it.has(e) }.map { stringResource(it.labelRes) }
        .ifEmpty { listOf(stringResource(R.string.setup_equip_none)) }
        .joinToString(", ")

// ── Headings and choice cards ───────────────────────────────────────────────

/** The step's question (bold, a TalkBack heading, the screen's one title) and,
 *  where it helps answer, one short line under it. */
@Composable
internal fun StepHeader(question: String, subtitle: String? = null) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        Text(question, color = OnBgBody, style = Type.Title, modifier = Modifier.semantics { heading() })
        if (subtitle != null) Text(subtitle, color = OnBgMuted, style = Type.Body)
    }
}

/** A secondary question inside a step (e.g. "How do you train strength?"): the
 *  app's one field-label style, announced as a heading. */
@Composable
internal fun SubHeading(text: String) {
    FieldLabel(text, heading = true)
}

/** One exclusive set of [SetupChoiceCard]s, one per option, announced as a radio group. */
@Composable
internal fun <T> ChoiceCards(
    options: List<T>,
    selected: T?,
    title: @Composable (T) -> String,
    subtitle: @Composable (T) -> String,
    onPick: (T) -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth().selectableGroup(),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        options.forEach { option ->
            SetupChoiceCard(title(option), subtitle(option), selected = option == selected) { onPick(option) }
        }
    }
}

/** A selectable option: title + one-line consequence on the shared choice-card
 *  surface (selected = solid accent, like every other selection). A radio button
 *  to TalkBack, so the chosen card is announced as selected. */
@Composable
private fun SetupChoiceCard(title: String, subtitle: String, selected: Boolean, onClick: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .choiceCard(selected)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .padding(Space.Card.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Text(title, color = choiceCellContent(selected), style = Type.BodyBold)
        Text(subtitle, color = choiceCellSecondaryContent(selected), style = Type.Caption)
    }
}

/** One whole-row option of a pick-one list inside a `selectableGroup`, on the
 *  scale cells' surface: [title] (bold while selected) over an optional
 *  [subtitle]. A radio button to TalkBack. */
@Composable
internal fun ChoiceRow(title: String, subtitle: String?, selected: Boolean, onClick: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .choiceCell(selected)
            .selectable(selected = selected, role = Role.RadioButton, onClick = onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp, Alignment.CenterVertically),
    ) {
        Text(
            title,
            color = choiceCellContent(selected),
            style = if (selected) Type.BodyBold else Type.Body,
        )
        if (subtitle != null) Text(subtitle, color = choiceCellContent(selected), style = Type.Caption)
    }
}

// ── Answer chips ────────────────────────────────────────────────────────────

/** Opacity of a disabled answer chip (Material's disabled-content alpha). */
private const val DisabledChipAlpha = 0.38f

/** Size of the glyph leading a date chip. */
private val ChipIconSize = 18.dp

/** Size of the corner check on a selected multi-select chip. */
private val ChipCheckSize = 12.dp

/** The tap semantics of one answer chip, by [role]: RadioButton = one option of a
 *  pick-one group (`selectable`), Checkbox = an independent toggle (`toggleable`),
 *  anything else a plain action (`clickable`). The same roles as [ChoiceChip]. */
private fun Modifier.chipAction(selected: Boolean, role: Role, enabled: Boolean, onClick: () -> Unit): Modifier = when (role) {
    Role.RadioButton -> selectable(selected = selected, enabled = enabled, role = role, onClick = onClick)
    Role.Checkbox -> toggleable(value = selected, enabled = enabled, role = role, onValueChange = { onClick() })
    else -> clickable(enabled = enabled, role = role, onClick = onClick)
}

/** One content-sized answer chip (≥48dp), drawn as the shared [choiceCell] so it
 *  reads as a tap target on any ground. [role] as in [ChoiceChip]: RadioButton
 *  (default) for an option of a pick-one group; Checkbox for an independent toggle,
 *  with a corner check while on. A disabled chip is faded and inert. */
@Composable
internal fun AnswerChip(
    label: String,
    selected: Boolean,
    modifier: Modifier = Modifier,
    role: Role = Role.RadioButton,
    contentAlignment: Alignment = Alignment.Center,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Box(
        modifier = modifier
            .heightIn(min = ActionMinHeight)
            .widthIn(min = ActionMinHeight)
            .then(if (enabled) Modifier else Modifier.alpha(DisabledChipAlpha))
            .choiceCell(selected)
            .chipAction(selected, role, enabled, onClick),
        contentAlignment = contentAlignment,
    ) {
        Text(
            label,
            color = choiceCellContent(selected),
            style = Type.Body,
            modifier = Modifier.padding(horizontal = answerChipPadding(label), vertical = Space.Md.dp),
        )
        if (role == Role.Checkbox && selected) {
            // In the corner padding, so it never shifts or squeezes the label.
            Icon(
                painterResource(R.drawable.ic_ui_check),
                contentDescription = null,
                tint = choiceCellContent(true),
                modifier = Modifier.align(Alignment.TopEnd).padding(Space.Xs.dp).size(ChipCheckSize),
            )
        }
    }
}

/** An [AnswerChip]'s side padding: short labels (weekday names) get the tighter
 *  one so a week fits one row. */
private fun answerChipPadding(label: String): Dp = if (label.length <= 3) Space.Md.dp else Space.Card.dp

/** Wrapping pill choices (content-sized, so long labels like "Prefer not to say"
 *  wrap to a new line instead of clipping at large font sizes). Nothing is
 *  highlighted while [current] matches no option; a `null` option (e.g. "Not
 *  sure") is highlighted while [current] is null. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun <T> ChipChoiceRow(options: List<T>, current: T?, label: @Composable (T) -> String, onSelect: (T) -> Unit) {
    FlowRow(
        modifier = Modifier.fillMaxWidth().selectableGroup(),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        options.forEach { opt -> AnswerChip(label(opt), selected = opt == current) { onSelect(opt) } }
    }
}

/** The recent-race distance chips; null is "I don't know". */
@Composable
internal fun RaceDistanceChips(current: RaceDistance?, onPick: (RaceDistance?) -> Unit) {
    ChipChoiceRow(listOf<RaceDistance?>(null) + RaceDistance.entries, current, { stringResource(it?.labelRes ?: R.string.setup_dont_know) }, onPick)
}

/** "Any days" (null mask, the default) on its own line, then Mon–Sun as equal
 *  multi-select cells: one row of seven when the widest day name fits, else a
 *  balanced 4 + 3, so no weekday is ever orphaned onto a line of its own. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun WeekdayChips(mask: Int?, onChange: (Int?) -> Unit) {
    val names = (0 until WEEK_DAYS).map { weekdayShortName(it) }
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val gap = Space.Sm.dp
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        AnswerChip(stringResource(R.string.setup_weekdays_any), selected = mask == null) { onChange(null) }
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val widest = names.maxOf { name ->
                with(density) { measurer.measure(name, style = Type.Body, maxLines = 1, softWrap = false).size.width.toDp() } + answerChipPadding(name) * 2
            }
            val cell = maxOf(ActionMinHeight, widest)
            val perRow = if (cell * WEEK_DAYS + gap * (WEEK_DAYS - 1) <= maxWidth) WEEK_DAYS else 4
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(gap),
                verticalArrangement = Arrangement.spacedBy(gap),
                maxItemsInEachRow = perRow,
            ) {
                names.forEachIndexed { day, name ->
                    AnswerChip(
                        name,
                        selected = mask != null && mask and (1 shl day) != 0,
                        modifier = Modifier.weight(1f),
                        role = Role.Checkbox,
                    ) {
                        onChange(toggleWeekday(mask, day))
                    }
                }
            }
        }
    }
}

/** Calisthenics equipment chips (several can be on) plus "None of these". */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun EquipmentChips(equipment: CalisEquipment?, onChange: (CalisEquipment) -> Unit) {
    FlowRow(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        CalisPiece.entries.forEach { piece ->
            AnswerChip(stringResource(piece.labelRes), selected = piece.has(equipment), role = Role.Checkbox) { onChange(piece.toggle(equipment)) }
        }
        AnswerChip(stringResource(R.string.setup_equip_none), selected = equipment == CalisEquipment()) { onChange(CalisEquipment()) }
    }
}

// ── Date and race-time answers ──────────────────────────────────────────────

/** DatePicker days are UTC-midnight stamps whose Y/M/D is the picked date, so a
 *  local epoch day maps to and from them exactly. */
private const val DAY_MS = 86_400_000L

/** The picked day as a local epoch day. */
@OptIn(ExperimentalMaterial3Api::class)
private fun DatePickerState.pickedDay(): Long? = selectedDateMillis?.let { Math.floorDiv(it, DAY_MS) }

/** The date picker dialog of a date answer: OK (enabled once [pick] is set) sends
 *  it to [onPick]; either button closes through [onDismiss]. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun EpochDayPickerDialog(state: DatePickerState, pick: Long?, onPick: (Long) -> Unit, onDismiss: () -> Unit) {
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(
                onClick = {
                    pick?.let(onPick)
                    onDismiss()
                },
                enabled = pick != null,
            ) { Text(stringResource(R.string.log_ok)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.main_cancel)) }
        },
    ) { DatePicker(state) }
}

/**
 * A date answer: a calendar pill showing the picked date (or "Pick a date") that
 * opens the date picker. A [future] date (a race or meet) is optional: a "No date
 * yet" pill makes the two a radio pair and the picker offers days after [today].
 * Otherwise (a restart date) the pill is a plain picker button, never a selection,
 * and the picker offers today or earlier. Values are local epoch days.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun DateAnswerRow(value: Long?, today: Long, future: Boolean, onChange: (Long?) -> Unit) {
    var picking by rememberSaveable { mutableStateOf(false) }
    FlowRow(
        modifier = Modifier.fillMaxWidth().then(if (future) Modifier.selectableGroup() else Modifier),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        if (future) AnswerChip(stringResource(R.string.setup_date_none), selected = value == null) { onChange(null) }
        DateChip(
            value?.let { epochDayLabel(it) } ?: stringResource(R.string.setup_date_pick),
            selected = future && value != null,
            role = if (future) Role.RadioButton else Role.Button,
        ) { picking = true }
    }
    if (picking) {
        val todayYear = LocalDate.ofEpochDay(today).year
        val state = rememberDatePickerState(
            initialSelectedDateMillis = (value ?: today.takeIf { !future })?.let { it * DAY_MS },
            selectableDates = object : SelectableDates {
                override fun isSelectableDate(utcTimeMillis: Long): Boolean {
                    val day = Math.floorDiv(utcTimeMillis, DAY_MS)
                    return if (future) day > today else day <= today
                }

                override fun isSelectableYear(year: Int): Boolean = if (future) year >= todayYear else year <= todayYear
            },
        )
        EpochDayPickerDialog(state, state.pickedDay(), onPick = onChange, onDismiss = { picking = false })
    }
}

/** "Born 12 Mar 1991 · 35": a birthday with the age it gives on [today]. */
@Composable
internal fun birthdayLabel(birthDay: Long, today: Long): String =
    stringResource(R.string.setup_birthday_value, epochDayLabel(birthDay), ageOnDay(birthDay, today))

/**
 * The birthday answer (setup About step, Profile › Age): a calendar pill showing
 * [birthdayLabel] (or [pickLabel] where the birthday already shows above it)
 * beside a "Prefer not to say" chip ([skipped] while chosen; [onSkip] answers
 * with no birthday and no age). The picker opens in typed-date mode and only
 * accepts birthdays giving an age inside [AGE_RANGE] on [today]. [hint] is a
 * caption under the pair.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun BirthdayAnswer(
    birthDay: Long?,
    today: Long,
    hint: String?,
    skipped: Boolean,
    pickLabel: String? = null,
    onSkip: () -> Unit,
    onChange: (Long) -> Unit,
) {
    var picking by rememberSaveable { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(R.string.setup_about_birthday_label))
        FlowRow(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            DateChip(
                pickLabel ?: birthDay?.let { birthdayLabel(it, today) } ?: stringResource(R.string.setup_birthday_pick),
                selected = birthDay != null,
                role = Role.RadioButton,
            ) { picking = true }
            AnswerChip(stringResource(R.string.setup_birthday_skip), selected = skipped, onClick = onSkip)
        }
        if (hint != null) Text(hint, color = OnBgMuted, style = Type.Caption)
    }
    if (picking) {
        val range = birthdayRange(today)
        val years = LocalDate.ofEpochDay(range.first).year..LocalDate.ofEpochDay(range.last).year
        val state = rememberDatePickerState(
            initialSelectedDateMillis = birthDay?.let { it * DAY_MS },
            initialDisplayedMonthMillis = birthdayPickerStart(birthDay, today) * DAY_MS,
            yearRange = years,
            initialDisplayMode = DisplayMode.Input,
            selectableDates = object : SelectableDates {
                override fun isSelectableDate(utcTimeMillis: Long): Boolean = Math.floorDiv(utcTimeMillis, DAY_MS) in range

                override fun isSelectableYear(year: Int): Boolean = year in years
            },
        )
        EpochDayPickerDialog(state, state.pickedDay()?.takeIf { it in range }, onPick = onChange, onDismiss = { picking = false })
    }
}

/** The answer-chip pill that opens the date picker: a calendar glyph + the date.
 *  [role] RadioButton = half of a "No date yet | date" pair; Button = a plain
 *  picker button. */
@Composable
private fun DateChip(label: String, selected: Boolean, role: Role, onClick: () -> Unit) {
    val tint = choiceCellContent(selected)
    Row(
        modifier = Modifier
            .heightIn(min = ActionMinHeight)
            .choiceCell(selected)
            .chipAction(selected, role, enabled = true, onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(painterResource(R.drawable.ic_ui_calendar), contentDescription = null, tint = tint, modifier = Modifier.size(ChipIconSize))
        Text(label, color = tint, style = Type.Body)
    }
}

/** The finish-time display for a recent race: digits typed on the shared keypad
 *  fill from the right ("2615" → 26:15, "15200" → 1:52:00), so the faint "0:00"
 *  of an empty field already shows the shape. Drawn like [KeypadValueField]. */
@Composable
internal fun RaceTimeField(digits: String, active: Boolean, invalid: Boolean, onActivate: () -> Unit) {
    val label = stringResource(R.string.setup_recent_race_time_label)
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(label)
        val shown = raceTimeDisplay(digits)
        Text(
            shown,
            color = if (digits.isEmpty()) OnBgFaint else OnBgBody,
            style = Type.Display.merge(TabularFigures),
            maxLines = 1,
            modifier = Modifier
                .fillMaxWidth()
                .keypadWell(invalid, active, if (digits.isEmpty()) label else "$label, $shown", onActivate),
        )
        if (invalid) {
            Text(stringResource(R.string.setup_recent_race_time_invalid), color = DangerText, style = Type.Caption)
        }
    }
}

// ── Review and frame ────────────────────────────────────────────────────────

/** One review row: its label, what was answered, and the step it edits. */
internal class ReviewEntry(val label: String, val value: String, val step: SetupStep)

/** The review as ONE card of hairline-divided rows. Each row is a whole-row tap
 *  that jumps back to its step: the label is the faint tile overline, only the
 *  headline answer (first line) is bold and detail lines stay regular, so the
 *  card scans; a chevron marks the row as tappable and TalkBack hears "Edit".
 *  [reveal] (the step an edit came back from) scrolls its first row into view
 *  once laid out, then [onRevealed]. */
@Composable
internal fun ReviewCard(rows: List<ReviewEntry>, reveal: SetupStep?, onRevealed: () -> Unit, onEdit: (SetupStep) -> Unit) {
    val edit = stringResource(R.string.setup_review_edit)
    val revealAt = rows.indexOfFirst { it.step == reveal }
    val revealRequester = remember { BringIntoViewRequester() }
    LaunchedEffect(reveal) {
        if (reveal == null) return@LaunchedEffect
        if (revealAt >= 0) {
            withFrameNanos { }
            revealRequester.bringIntoView()
        }
        onRevealed()
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            // Clip before the rows' clickables so every ripple stays in the card.
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated),
    ) {
        rows.forEachIndexed { i, row ->
            if (i > 0) {
                RowHairline(Modifier.padding(horizontal = Space.Card.dp))
            }
            val lines = row.value.split('\n')
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(if (i == revealAt) Modifier.bringIntoViewRequester(revealRequester) else Modifier)
                    .heightIn(min = ActionMinHeight)
                    .clickable(role = Role.Button, onClickLabel = edit) { onEdit(row.step) }
                    .padding(Space.Card.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(
                    modifier = Modifier.weight(1f).padding(end = Space.Md.dp),
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                ) {
                    TileOverline(row.label)
                    Text(lines.first(), color = OnBgBody, style = Type.BodyBold)
                    if (lines.size > 1) {
                        Text(lines.drop(1).joinToString("\n"), color = OnBgMuted, style = Type.Body)
                    }
                }
                RowChevron()
            }
        }
    }
}

/** Widen a child of a horizontally padded column by [inset] on each side, so a
 *  band (the pinned keypad) runs edge to edge while its siblings keep the margin. */
internal fun Modifier.fullBleed(inset: Dp): Modifier = layout { measurable, constraints ->
    val extra = inset.roundToPx() * 2
    val width = constraints.maxWidth + extra
    val placeable = measurable.measure(constraints.copy(minWidth = width, maxWidth = width))
    layout(constraints.maxWidth, placeable.height) { placeable.place(-extra / 2, 0) }
}

/** Height of the [StepProgress] bar. */
private val StepProgressHeight = 4.dp

/** Opacity of the [StepProgress] track behind the accent fill (OnBgBody). */
private const val StepProgressTrackAlpha = 0.08f

/** A thin full-width, fully rounded progress bar: the accent fill eases along the
 *  modality's longest path ([fraction], from [setupProgress]), so an answer that
 *  adds a screen never moves it backwards, and step 1 already shows its share.
 *  TalkBack reads "Step N of [stepCount]" and re-announces it when the step changes. */
@Composable
internal fun StepProgress(stepNumber: Int, stepCount: Int, fraction: Float, modifier: Modifier = Modifier) {
    val progress = stringResource(R.string.setup_step_progress, stepNumber, stepCount)
    val shown by animateFloatAsState(fraction.coerceIn(0f, 1f), label = "setup-progress")
    ProgressPill(
        shown,
        StepProgressHeight,
        OnBgBody.copy(alpha = StepProgressTrackAlpha),
        modifier.clearAndSetSemantics {
            contentDescription = progress
            liveRegion = LiveRegionMode.Polite
        },
    )
}

/** A fully rounded bar: an accent fill over [fraction] (capped full) of a [track]
 *  of [height]; [modifier] sets the width and the semantics. */
@Composable
internal fun ProgressPill(fraction: Float, height: Dp, track: Color, modifier: Modifier) {
    val shape = RoundedCornerShape(50)
    Box(modifier.height(height).clip(shape).background(track)) {
        Box(
            Modifier
                .fillMaxWidth(fraction.coerceIn(0f, 1f))
                .fillMaxHeight()
                .clip(shape)
                .background(Accent),
        )
    }
}
