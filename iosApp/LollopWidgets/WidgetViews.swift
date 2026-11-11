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

extension WidgetType {
    /// A small card's hero figure: one size down from `display`, so the title, the
    /// figure, its pace and the button fit a small widget's height.
    static let displaySmall = Font.system(.title).weight(.bold).monospacedDigit()
    /// A button label (Glance's FilledButton, 14 sp medium).
    static let button = Font.system(.subheadline).weight(.semibold)
}

extension WidgetSpace {
    /// A card's padding: `Space.Screen`, a little less on a small widget, which is
    /// shorter than Android's two rows.
    static func edge(wide: Bool) -> CGFloat { wide ? screen : 14 }

    /// A round button's and a labelled button's height (Android: 48 dp
    /// `ActionMinHeight`): the 44 pt touch target, 36 pt on a small widget, which is
    /// one tap target (iOS 16) or one button.
    static func action(wide: Bool) -> CGFloat { wide ? 44 : 36 }
}

/// The extension's own bundle: its asset catalog and fallback.json (also compiled
/// into the tests).
private final class WidgetBundleToken {}

extension Bundle {
    static let lollopWidgets = Bundle(for: WidgetBundleToken.self)
}

extension SurfaceIcon {
    /// The image set iosApp/make-assets.py renders from the icon's Android drawable,
    /// as a template image.
    var image: Image {
        Image("Icon" + rawValue, bundle: .lollopWidgets).renderingMode(.template)
    }
}

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

    /// How the hero shows: not at all, beside the buttons (a wide card's last row:
    /// the figure and its pace to the left of Log and the primary), or stacked
    /// between the title and the week block.
    enum Hero: Int, Hashable { case hidden = 0, beside = 1, stacked = 2 }

    var overline: Bool
    var hero: Hero
    var week: Week
    /// Body line slots ([WidgetLines.lines]).
    var lines: Int
    /// Body lines in `Type.Body` rather than `Type.Caption`.
    var large: Bool
    /// A statement's title in the display type, where it fits on one line.
    var bigTitle: Bool

    /// The order a card spends its height in, after Android's (TodayWidget.kt
    /// `widgetLayout`): a line under the title (or the hero), the hero (a run's
    /// distance, the next session), the overline, the week block, then more lines,
    /// a statement's large title last. The hero comes before the overline: an iOS
    /// card is shorter than Android's two rows, and the hero is the day's key fact.
    /// Ranking the candidates in this order and taking the first that fits gives
    /// what Android's greedy pass gives.
    var rank: Int {
        var r = 0
        if lines >= 1 || hero != .hidden { r |= 1 << 20 }
        r |= hero.rawValue << 18
        if overline { r |= 1 << 17 }
        r |= week.rawValue << 15
        r |= (lines * 2 + (large ? 1 : 0)) << 1
        if bigTitle { r |= 1 }
        return r
    }

    /// The candidates for [face], richest first. [wide]: a medium or large widget,
    /// where the week strip fits (never on a small one, like Android's two cells)
    /// and the hero can stand beside the buttons.
    static func candidates(for face: WidgetFace, wide: Bool) -> [CardLayout] {
        let weeks: [Week] = face.week.map { wide && !$0.days.isEmpty ? [.strip, .count, .hidden] : [.count, .hidden] } ?? [.hidden]
        let heroes: [Hero] = face.hero == nil ? [.hidden] : (wide ? [.stacked, .beside, .hidden] : [.stacked, .hidden])
        let plain = face.body.total
        var out: [CardLayout] = []
        for overline in [true, false] {
            for hero in heroes {
                for week in weeks {
                    // A hero keeps its lead lines ("Session done"); the week block
                    // keeps the hero or two lines.
                    let heroBody = hero != .hidden ? face.hero?.body : nil
                    let total = heroBody?.total ?? plain
                    let least = heroBody?.lead ?? (week == .hidden ? 0 : min(2, plain))
                    let statement = hero == .hidden && week == .hidden && face.body.leadOnly
                    for (lines, large) in lineOptions(total: total, least: least) {
                        for bigTitle in statement ? [true, false] : [false] {
                            out.append(CardLayout(overline: overline, hero: hero, week: week, lines: lines, large: large, bigTitle: bigTitle))
                        }
                    }
                }
            }
        }
        return out.sorted { $0.rank > $1.rank }
    }

    /// Every line in the larger type where at most six are shown, then fewer and
    /// fewer caption lines down to [least], none where none are needed. A line
    /// shows whole or not at all.
    private static func lineOptions(total: Int, least: Int) -> [(Int, Bool)] {
        var out: [(Int, Bool)] = []
        if total >= 1 && total <= 6 { out.append((total, true)) }
        var n = min(total, 6)
        while n >= max(least, 1) {
            out.append((n, false))
            n -= 1
        }
        if least == 0 { out.append((0, false)) }
        return out
    }
}

/// How many lines a text [em] wide (in em of its font size, Kotlin's estimate)
/// wraps to in [fontSize] at [width].
func estimatedLines(em: Double, fontSize: CGFloat, width: CGFloat) -> Int {
    max(1, Int((CGFloat(em) * fontSize / max(width, 1)).rounded(.up)))
}

/// The home screen card: the fitting candidate of [CardLayout.candidates].
struct TodayCard: View {
    let face: WidgetFace
    let colors: WidgetColors
    /// A medium or large widget: the overline with "Today ·", the week strip, Log beside the primary.
    let wide: Bool
    let buttons: CardButtons

    var body: some View {
        GeometryReader { geo in
            let shown = Self.shortLead(face, textWidth: geo.size.width - 2 * WidgetSpace.edge(wide: wide))
            ViewThatFits(in: .vertical) {
                ForEach(CardLayout.candidates(for: shown, wide: wide), id: \.self) { layout in
                    CardContent(face: shown, layout: layout, colors: colors, wide: wide, buttons: buttons)
                }
            }
            .padding(WidgetSpace.edge(wide: wide))
            .frame(width: geo.size.width, height: geo.size.height, alignment: .topLeading)
        }
    }

    /// A lone lead line too long for two lines ("Next: Heavy volume day, Thu, Oct 30"
    /// on a small card) gives way to the short one ("Next: Thu"), as on Android.
    static func shortLead(_ face: WidgetFace, textWidth: CGFloat) -> WidgetFace {
        guard let short = face.shortBody,
              estimatedLines(em: short.leadEm, fontSize: UIFont.preferredFont(forTextStyle: .subheadline).pointSize, width: textWidth) > 2
        else { return face }
        var shown = face
        shown.body = short.body
        return shown
    }
}

/// One card candidate, composed for what it holds (TodayWidget.kt `StackLayout`):
/// a list from the title down with the week block under it, a hero centred
/// between the title and the week block (or beside the buttons), or a statement
/// (a title and its line) set on the actions. Every text shows whole: a part that
/// would be cut makes the candidate too tall, and a sparser one is drawn.
struct CardContent: View {
    let face: WidgetFace
    let layout: CardLayout
    let colors: WidgetColors
    let wide: Bool
    let buttons: CardButtons

    private var hero: WidgetHero? { layout.hero != .hidden ? face.hero : nil }

    private var statement: Bool { hero == nil && layout.week == .hidden && face.body.leadOnly }

    private var source: WidgetLines { hero?.body ?? face.body }

    private var lines: [String] { source.lines(layout.lines) }

    /// The first lines are lead lines (muted), the rest items.
    private var leadCount: Int { min(source.lead, lines.count) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if layout.overline {
                OverlineText(face: face, wide: wide, color: colors.accent)
                Spacer().frame(height: WidgetSpace.sm)
            }
            if statement {
                Spacer(minLength: 0)
                if layout.bigTitle {
                    StatementTitle(text: face.title, color: colors[face.titleInk])
                } else {
                    title
                }
                Spacer().frame(height: WidgetSpace.sm)
                bodyLines(lines, leadCount: lines.count)
                actions(top: WidgetSpace.md)
            } else if let hero {
                title
                Spacer().frame(height: WidgetSpace.sm)
                if layout.hero == .stacked {
                    Spacer(minLength: 0)
                    heroColumn(hero)
                    Spacer(minLength: 0)
                    week
                    actions(top: WidgetSpace.md)
                } else {
                    Spacer(minLength: 0)
                    week
                    if layout.week != .hidden { Spacer().frame(height: WidgetSpace.md) }
                    HStack(alignment: .bottom, spacing: WidgetSpace.md) {
                        heroColumn(hero).frame(maxWidth: .infinity, alignment: .leading)
                        ActionRow(actions: face.actions, colors: colors, wide: wide, buttons: buttons)
                            .fixedSize()
                    }
                }
            } else {
                title
                Spacer().frame(height: WidgetSpace.sm)
                bodyLines(lines, leadCount: leadCount)
                if layout.week != .hidden { Spacer().frame(height: WidgetSpace.md) }
                week
                Spacer(minLength: 0)
                actions(top: WidgetSpace.md)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var title: some View {
        Text(face.title)
            .font(WidgetType.title)
            .foregroundColor(colors[face.titleInk])
            .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder
    private var week: some View {
        if layout.week != .hidden, let week = face.week {
            WeekBlock(week: week, strip: layout.week == .strip, colors: colors)
        }
    }

    @ViewBuilder
    private func actions(top: CGFloat) -> some View {
        if !face.actions.isEmpty {
            Spacer().frame(height: top)
            ActionRow(actions: face.actions, colors: colors, wide: wide, buttons: buttons)
        }
    }

    private func heroColumn(_ hero: WidgetHero) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            bodyLines(Array(lines.prefix(leadCount)), leadCount: leadCount)
            HeroBlock(hero: hero, colors: colors, compact: !wide)
            bodyLines(Array(lines.dropFirst(leadCount)), leadCount: 0)
        }
    }

    @ViewBuilder
    private func bodyLines(_ lines: [String], leadCount: Int) -> some View {
        ForEach(Array(lines.enumerated()), id: \.offset) { i, line in
            Text(line)
                .font(layout.large ? WidgetType.body : WidgetType.caption)
                .foregroundColor(i < leadCount ? colors.muted : colors.body)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// "TODAY · WED, OCT 28" where it fits on its line (wide), else the date alone.
struct OverlineText: View {
    let face: WidgetFace
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
        Text(text).font(WidgetType.overline).tracking(WidgetType.overlineTracking).foregroundColor(color).lineLimit(1)
    }
}

/// A statement's title: large where it fits on one line, else the title type.
struct StatementTitle: View {
    let text: String
    let color: Color

    var body: some View {
        ViewThatFits(in: .horizontal) {
            Text(text).font(WidgetType.display).foregroundColor(color).lineLimit(1).fixedSize()
            Text(text).font(WidgetType.title).foregroundColor(color).fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// A run's figure over its pace, or the next session's name over its date.
struct HeroBlock: View {
    let hero: WidgetHero
    let colors: WidgetColors
    let compact: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if hero.figure {
                Text(hero.text)
                    .font(compact ? WidgetType.displaySmall : WidgetType.display)
                    .foregroundColor(colors.body)
                    .fixedSize()
            } else {
                Text(hero.text)
                    .font(WidgetType.bodyBold)
                    .foregroundColor(colors.body)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let note = hero.note {
                Text(note).font(WidgetType.caption).foregroundColor(colors.muted).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// The week count, then (with [strip]) Mo–Su with each day's mark (TodayWidget.kt
/// `WeekStrip`); today's label in the accent on an accent tint.
struct WeekBlock: View {
    let week: WidgetWeek
    let strip: Bool
    let colors: WidgetColors

    var body: some View {
        VStack(alignment: .leading, spacing: WidgetSpace.sm) {
            Text(week.text).font(WidgetType.caption).foregroundColor(colors.muted).lineLimit(2)
            if strip && !week.days.isEmpty {
                HStack(spacing: 0) {
                    ForEach(Array(week.days.enumerated()), id: \.offset) { _, day in
                        DayMark(day: day, colors: colors)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(week.text)
            }
        }
    }
}

struct DayMark: View {
    let day: WidgetDay
    let colors: WidgetColors

    var body: some View {
        VStack(spacing: 0) {
            Text(day.label)
                .font(day.today ? WidgetType.overline : WidgetType.overlineRegular)
                .foregroundColor(day.today ? colors.accent : colors.muted)
                .lineLimit(1)
            Text(day.glyph).font(WidgetType.chip).foregroundColor(colors[day.ink]).lineLimit(1)
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
    let actions: [WidgetAction]
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
    private func labelled(_ action: WidgetAction, icon: Bool, fixed: Bool) -> some View {
        tappable(action) {
            HStack(spacing: WidgetSpace.md) {
                if icon {
                    action.icon.image
                        .resizable()
                        .scaledToFit()
                        .frame(width: 18, height: 18)
                }
                Text(action.label).font(WidgetType.button).lineLimit(1)
            }
            .fixedSize(horizontal: fixed, vertical: false)
            .foregroundColor(colors.onAccent)
            .padding(.horizontal, WidgetSpace.screen)
            .frame(minHeight: WidgetSpace.action(wide: wide))
            .frame(maxWidth: wide ? nil : .infinity)
            .background(Capsule().fill(colors.accent))
        }
    }

    private func round(_ action: WidgetAction) -> some View {
        tappable(action) {
            action.icon.image
                .resizable()
                .scaledToFit()
                .frame(width: 20, height: 20)
                .foregroundColor(colors.accent)
                .frame(width: WidgetSpace.action(wide: wide), height: WidgetSpace.action(wide: wide))
                .background(Circle().fill(colors.accentTint))
                .accessibilityLabel(action.label)
        }
    }

    @ViewBuilder
    private func tappable<Content: View>(_ action: WidgetAction, @ViewBuilder _ content: () -> Content) -> some View {
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

/// The rectangular lock screen widget: Android's one-row widget without its
/// buttons: the title over the status line where it fits, else the short one.
struct RectangularView: View {
    let face: WidgetFace

    var body: some View {
        ViewThatFits(in: .vertical) {
            if let status = face.status {
                block(status)
            }
            if let brief = face.brief {
                block(brief)
            }
            Text(face.title).font(.headline).fixedSize(horizontal: false, vertical: true).widgetAccentable()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The title over a line, each whole (a candidate that would cut either is too tall).
    private func block(_ line: String) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(face.title).font(.headline).fixedSize(horizontal: false, vertical: true).widgetAccentable()
            Text(line).font(.subheadline).fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// The circular lock screen widget: the week's sessions done of planned, else
/// the primary action's icon.
struct CircularView: View {
    let face: WidgetFace

    var body: some View {
        if let week = face.week, week.planned > 0 {
            Gauge(value: Double(min(week.done, week.planned)), in: 0...Double(week.planned)) {
                Text(week.text)
            } currentValueLabel: {
                Text(verbatim: "\(week.done)/\(week.planned)").monospacedDigit()
            }
            .gaugeStyle(.accessoryCircularCapacity)
            .accessibilityLabel(week.text)
        } else {
            ZStack {
                AccessoryWidgetBackground()
                (face.actions.first?.icon ?? .log).image
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
    let face: WidgetFace

    var body: some View {
        ViewThatFits {
            if let brief = face.brief {
                Text(verbatim: "\(face.title) · \(brief)")
            }
            Text(face.title)
        }
    }
}
