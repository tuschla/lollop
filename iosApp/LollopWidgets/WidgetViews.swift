import AppIntents
import SwiftUI
import UIKit
import WidgetKit

// The home screen card and the lock screen accessories, after Android's widget
// (android/lollop's TodayWidget.kt, visual-design-system §10 "Home screen widget").
// The card shows the richest layout whose parts all fit at the reader's text size,
// spending height in Android's order: the actions, the title, a line, a wrapped
// title's second line, the overline, the hero, the week block while the hero or
// two lines still fit, then the lines. SwiftUI measures the candidates
// (`ViewThatFits`), so a larger text size drops parts instead of clipping them.

/// How a card's buttons respond to a tap.
enum CardButtons {
    /// A link each (medium and large widgets: each part of the widget is its own target).
    case links
    /// An App Intent that opens the app (a small widget, iOS 17+: the widget opens Today, the button its screen).
    case intents
    /// Drawn only (a small widget on iOS 16 opens the primary's screen as a whole; snapshots).
    case drawn
}

/// What one card candidate shows.
struct CardLayout: Hashable {
    enum Week: Int, Hashable { case hidden = 0, count = 1, strip = 2 }

    var overline: Bool
    var hero: Bool
    var week: Week
    /// Body line slots ([visibleLines]).
    var lines: Int
    /// Body lines in `Type.Body` rather than `Type.Caption`.
    var large: Bool
    /// The one body line cut to a single line (Android's last resort for a list).
    var cut: Bool
    var titleLines: Int
    /// A statement's title in the display type, where it fits on one line.
    var bigTitle: Bool

    /// Android spends a card's height in this order (TodayWidget.kt `widgetLayout`):
    /// a line under the title (or the hero), a wrapped title's second line, the
    /// overline, the hero, the week block, then more lines, a statement's large
    /// title last. Ranking the candidates by the same order and taking the first
    /// that fits gives the layout Android's greedy pass gives.
    var rank: Int {
        var r = 0
        if lines >= 1 || hero { r |= 1 << 20 }
        if titleLines == 2 { r |= 1 << 19 }
        if overline { r |= 1 << 18 }
        if hero { r |= 1 << 17 }
        r |= week.rawValue << 15
        r |= (lines * 4 + (large ? 2 : 0) + (cut ? 0 : 1)) << 1
        if bigTitle { r |= 1 }
        return r
    }

    /// The candidates for [face], richest first. [wide]: a medium or large widget,
    /// where the week strip fits (never on a small one, like Android's two cells).
    static func candidates(for face: Face, wide: Bool) -> [CardLayout] {
        let weeks: [Week] = face.week == nil ? [.hidden] : (wide && face.days.count == 7 ? [.strip, .count, .hidden] : [.count, .hidden])
        let plain = face.lead.count + face.items.count
        let heroLines = face.heroLead.count + face.heroItems.count
        var out: [CardLayout] = []
        for titleLines in [2, 1] {
            for overline in [true, false] {
                for hero in face.hero == nil ? [false] : [true, false] {
                    for week in weeks {
                        // A hero keeps its lead lines ("Session done"); the week
                        // block keeps the hero or two lines.
                        let total = hero ? heroLines : plain
                        let least = hero ? face.heroLead.count : (week == .hidden ? 0 : min(2, plain))
                        let statement = !hero && week == .hidden && face.items.isEmpty
                        for (lines, large, cut) in lineOptions(total: total, least: least, list: !hero) {
                            for bigTitle in statement ? [true, false] : [false] {
                                out.append(CardLayout(
                                    overline: overline, hero: hero, week: week, lines: lines, large: large,
                                    cut: cut, titleLines: titleLines, bigTitle: bigTitle
                                ))
                            }
                        }
                    }
                }
            }
        }
        return out.sorted { $0.rank > $1.rank }
    }

    /// Every line in the larger type where at most six are shown, then fewer and
    /// fewer caption lines down to [least]; a list's first line cut to one line;
    /// no lines where none are needed.
    private static func lineOptions(total: Int, least: Int, list: Bool) -> [(Int, Bool, Bool)] {
        var out: [(Int, Bool, Bool)] = []
        if total >= 1 && total <= 6 { out.append((total, true, false)) }
        var n = min(total, 6)
        while n >= max(least, 1) {
            out.append((n, false, false))
            n -= 1
        }
        if list && total >= 1 && least <= 1 { out.append((1, false, true)) }
        if least == 0 { out.append((0, false, false)) }
        return out
    }
}

/// Android's estimate of where a line wraps (TodayWidget.kt `charEm`): a
/// character's advance in em by class, a little wide.
func estimatedLines(_ text: String, fontSize: CGFloat, width: CGFloat) -> Int {
    let em = text.reduce(CGFloat(0)) { sum, c in
        let w: CGFloat
        if c == " " || c == "\u{00A0}" {
            w = 0.27
        } else if "iljtfrI.,:;·'!|()".contains(c) {
            w = 0.32
        } else if "mwMW@".contains(c) {
            w = 0.82
        } else if c.isUppercase {
            w = 0.64
        } else if c.isNumber {
            w = 0.57
        } else {
            w = 0.54
        }
        return sum + w
    }
    return max(1, Int((em * fontSize / max(width, 1)).rounded(.up)))
}

/// The home screen card: the fitting candidate of [CardLayout.candidates].
struct TodayCard: View {
    let face: Face
    let colors: WidgetColors
    let moreItems: [String]
    /// A medium or large widget: the overline with "Today ·", the week strip, Log beside the primary.
    let wide: Bool
    let buttons: CardButtons

    var body: some View {
        GeometryReader { geo in
            let shown = Self.shortLead(face, textWidth: geo.size.width - 2 * WidgetSpace.screen)
            ViewThatFits(in: .vertical) {
                ForEach(CardLayout.candidates(for: shown, wide: wide), id: \.self) { layout in
                    CardContent(face: shown, layout: layout, colors: colors, moreItems: moreItems, wide: wide, buttons: buttons)
                }
            }
            .padding(WidgetSpace.screen)
            .frame(width: geo.size.width, height: geo.size.height, alignment: .topLeading)
        }
    }

    /// A lone lead line too long for two lines ("Next: Heavy volume day, Thu, Oct 30"
    /// on a small card) gives way to the short one ("Next: Thu"), as on Android.
    static func shortLead(_ face: Face, textWidth: CGFloat) -> Face {
        guard face.lead.count == 1, let lead = face.lead.first, let brief = face.brief,
              estimatedLines(lead, fontSize: UIFont.preferredFont(forTextStyle: .subheadline).pointSize, width: textWidth) > 2
        else { return face }
        var shown = face
        shown.lead = [brief]
        return shown
    }
}

/// One card candidate, composed for what it holds (TodayWidget.kt `StackLayout`):
/// a list from the title down, a hero centred between the title and the week
/// block, or a statement (a title and its line) set on the actions.
struct CardContent: View {
    let face: Face
    let layout: CardLayout
    let colors: WidgetColors
    let moreItems: [String]
    let wide: Bool
    let buttons: CardButtons

    private var statement: Bool { !layout.hero && layout.week == .hidden && face.items.isEmpty }

    private var lines: [String] {
        layout.hero
            ? visibleLines(lead: face.heroLead, items: face.heroItems, maxLines: layout.lines, moreItems: moreItems)
            : visibleLines(lead: face.lead, items: face.items, maxLines: layout.lines, moreItems: moreItems)
    }

    /// The first lines are lead lines (muted), the rest items.
    private var leadCount: Int {
        min(layout.hero ? face.heroLead.count : face.lead.count, lines.count)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if layout.overline {
                OverlineText(face: face, wide: wide, color: colors.accent)
                Spacer().frame(height: WidgetSpace.sm)
            }
            if statement {
                Spacer(minLength: 0)
                if layout.bigTitle {
                    StatementTitle(text: face.title, color: colors.title(tone: face.tone), lines: layout.titleLines)
                } else {
                    Text(face.title)
                        .font(WidgetType.title)
                        .foregroundColor(colors.title(tone: face.tone))
                        .lineLimit(layout.titleLines)
                }
                Spacer().frame(height: WidgetSpace.sm)
                bodyLines(lines, leadCount: lines.count)
            } else {
                Text(face.title)
                    .font(WidgetType.title)
                    .foregroundColor(colors.title(tone: face.tone))
                    .lineLimit(layout.titleLines)
                Spacer().frame(height: WidgetSpace.sm)
                if layout.hero, let hero = face.hero {
                    Spacer(minLength: 0)
                    bodyLines(Array(lines.prefix(leadCount)), leadCount: leadCount)
                    HeroBlock(hero: hero, colors: colors)
                    bodyLines(Array(lines.dropFirst(leadCount)), leadCount: 0)
                    Spacer(minLength: 0)
                } else {
                    bodyLines(lines, leadCount: leadCount)
                    Spacer(minLength: 0)
                }
            }
            if layout.week != .hidden, let week = face.week {
                Spacer().frame(height: WidgetSpace.md)
                WeekBlock(week: week, days: layout.week == .strip ? face.days : [], colors: colors)
            }
            if !face.actions.isEmpty {
                Spacer().frame(height: WidgetSpace.md)
                ActionRow(actions: face.actions, colors: colors, wide: wide, buttons: buttons)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func bodyLines(_ lines: [String], leadCount: Int) -> some View {
        ForEach(Array(lines.enumerated()), id: \.offset) { i, line in
            Text(line)
                .font(layout.large ? WidgetType.body : WidgetType.caption)
                .foregroundColor(i < leadCount ? colors.muted : colors.body)
                .lineLimit(layout.cut ? 1 : 2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// "TODAY · WED, OCT 28" where it fits on its line (wide), else the date alone.
struct OverlineText: View {
    let face: Face
    let wide: Bool
    let color: Color

    var body: some View {
        let date = (face.date ?? face.overline).uppercased()
        if wide {
            ViewThatFits(in: .horizontal) {
                label(face.overline.uppercased()).fixedSize()
                label(date)
            }
        } else {
            label(date)
        }
    }

    private func label(_ text: String) -> some View {
        Text(text).font(WidgetType.overline).tracking(1.2).foregroundColor(color).lineLimit(1)
    }
}

/// A statement's title: large where it fits on one line, else the title type.
struct StatementTitle: View {
    let text: String
    let color: Color
    let lines: Int

    var body: some View {
        ViewThatFits(in: .horizontal) {
            Text(text).font(WidgetType.display).foregroundColor(color).lineLimit(1).fixedSize()
            Text(text).font(WidgetType.title).foregroundColor(color).lineLimit(lines)
        }
    }
}

/// A run's figure over its pace, or the next session's name over its date.
struct HeroBlock: View {
    let hero: Face.Hero
    let colors: WidgetColors

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(hero.text)
                .font(hero.figure ? WidgetType.display : WidgetType.bodyBold)
                .foregroundColor(colors.body)
                .lineLimit(hero.figure ? 1 : 2)
                .minimumScaleFactor(hero.figure ? 0.7 : 1)
            if let note = hero.note {
                Text(note).font(WidgetType.caption).foregroundColor(colors.muted).lineLimit(2)
            }
        }
    }
}

/// The week count, then Mo–Su with each day's mark (TodayWidget.kt `WeekStrip`):
/// ✓ trained, ● the next session, ○ planned, a muted ○ missed, nothing on a rest
/// day, – before the plan; today's label in the accent on an accent tint.
struct WeekBlock: View {
    let week: String
    let days: [Face.Day]
    let colors: WidgetColors

    var body: some View {
        VStack(alignment: .leading, spacing: WidgetSpace.sm) {
            Text(week).font(WidgetType.caption).foregroundColor(colors.muted).lineLimit(2)
            if days.count == 7 {
                HStack(spacing: 0) {
                    ForEach(Array(days.enumerated()), id: \.offset) { _, day in
                        DayMark(day: day, colors: colors)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(week)
            }
        }
    }
}

struct DayMark: View {
    let day: Face.Day
    let colors: WidgetColors

    private var glyph: (String, Color) {
        switch day.mark {
        case "done": return ("\u{2713}", colors.done)
        case "next": return ("\u{25CF}", colors.accent)
        case "missed": return ("\u{25CB}", colors.muted)
        case "rest": return ("\u{00A0}", colors.faint)
        case "none": return ("\u{2013}", colors.faint)
        default: return ("\u{25CB}", colors.faint)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            Text(day.label)
                .font(day.today ? WidgetType.overline : WidgetType.overlineRegular)
                .foregroundColor(day.today ? colors.accent : colors.muted)
                .lineLimit(1)
            Text(glyph.0).font(WidgetType.chip).foregroundColor(glyph.1).lineLimit(1)
        }
        .padding(.vertical, WidgetSpace.sm)
        .frame(maxWidth: .infinity)
        .background(
            RoundedRectangle(cornerRadius: WidgetSpace.md)
                .fill(day.today ? colors.accentTint : Color.clear)
        )
    }
}

/// The buttons: the primary (the tile's face) labelled in the accent, Log beside
/// it as a round tinted button where both fit (wide), else the primary alone,
/// full width on a small card. A label never gives way: the icon does.
struct ActionRow: View {
    let actions: [Face.Action]
    let colors: WidgetColors
    let wide: Bool
    let buttons: CardButtons

    var body: some View {
        if let primary = actions.first {
            if wide {
                HStack(spacing: WidgetSpace.md) {
                    Spacer(minLength: 0)
                    if actions.count > 1 {
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: WidgetSpace.md) {
                                round(actions[1])
                                labelled(primary, icon: true, fixed: true)
                            }
                            labelled(primary, icon: true, fixed: true)
                            labelled(primary, icon: false, fixed: false)
                        }
                    } else {
                        ViewThatFits(in: .horizontal) {
                            labelled(primary, icon: true, fixed: true)
                            labelled(primary, icon: false, fixed: false)
                        }
                    }
                }
            } else {
                ViewThatFits(in: .horizontal) {
                    labelled(primary, icon: true, fixed: true)
                    labelled(primary, icon: false, fixed: false)
                }
            }
        }
    }

    /// The primary: its natural width on a wide card, the card's width on a small
    /// one. [fixed]: measured whole, so a label that doesn't fit beside its icon
    /// makes this candidate too wide.
    private func labelled(_ action: Face.Action, icon: Bool, fixed: Bool) -> some View {
        tappable(action) {
            HStack(spacing: WidgetSpace.md) {
                if icon {
                    WidgetIcon(face: action.face).image
                        .resizable()
                        .scaledToFit()
                        .frame(width: 18, height: 18)
                }
                Text(action.label).font(WidgetType.button).lineLimit(1)
            }
            .fixedSize(horizontal: fixed, vertical: false)
            .foregroundColor(colors.onAccent)
            .padding(.horizontal, WidgetSpace.screen)
            .frame(minHeight: WidgetSpace.action)
            .frame(maxWidth: wide ? nil : .infinity)
            .background(Capsule().fill(colors.accent))
        }
    }

    private func round(_ action: Face.Action) -> some View {
        tappable(action) {
            WidgetIcon(face: action.face).image
                .resizable()
                .scaledToFit()
                .frame(width: 20, height: 20)
                .foregroundColor(colors.accent)
                .frame(width: WidgetSpace.action, height: WidgetSpace.action)
                .background(Circle().fill(colors.accentTint))
                .accessibilityLabel(action.label)
        }
    }

    @ViewBuilder
    private func tappable<Content: View>(_ action: Face.Action, @ViewBuilder _ content: () -> Content) -> some View {
        switch buttons {
        case .links:
            Link(destination: action.screen.url) { content() }
        case .intents:
            if #available(iOS 17.0, *) {
                Button(intent: OpenLollopIntent(target: action.screen)) { content() }
                    .buttonStyle(.plain)
            } else {
                content()
            }
        case .drawn:
            content()
        }
    }
}

// MARK: Lock screen

/// The rectangular lock screen widget: Android's one-row widget without its
/// buttons: the title over the status line where it fits, else the short one.
struct RectangularView: View {
    let face: Face

    var body: some View {
        ViewThatFits(in: .vertical) {
            if let status = face.status {
                block(status, lines: 2)
            }
            if let brief = face.brief {
                block(brief, lines: 1)
            }
            Text(face.title).font(.headline).lineLimit(2).widgetAccentable()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func block(_ line: String, lines: Int) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(face.title).font(.headline).lineLimit(1).widgetAccentable()
            Text(line).font(.subheadline).lineLimit(lines)
        }
    }
}

/// The circular lock screen widget: the week's sessions done of planned, else
/// the primary action's icon.
struct CircularView: View {
    let face: Face
    /// This week's done and planned sessions, for today's snapshot.
    let week: (done: Int, planned: Int)?

    var body: some View {
        if let week, week.planned > 0 {
            Gauge(value: Double(min(week.done, week.planned)), in: 0...Double(week.planned)) {
                Text(face.week ?? "")
            } currentValueLabel: {
                Text(verbatim: "\(week.done)/\(week.planned)").monospacedDigit()
            }
            .gaugeStyle(.accessoryCircularCapacity)
            .accessibilityLabel(face.week ?? "")
        } else {
            ZStack {
                AccessoryWidgetBackground()
                WidgetIcon(face: face.actions.first?.face ?? "log").image
                    .resizable()
                    .scaledToFit()
                    .padding(16)
                    .widgetAccentable()
            }
            .accessibilityLabel(face.title)
        }
    }
}

/// The inline lock screen widget: the title and the short line where both fit.
struct InlineView: View {
    let face: Face

    var body: some View {
        ViewThatFits {
            if let brief = face.brief {
                Text(verbatim: "\(face.title) · \(brief)")
            }
            Text(face.title)
        }
    }
}
