import SwiftUI
import WidgetKit

/// One timeline entry: the payload the app last wrote, as of [date].
struct TodayEntry: TimelineEntry {
    let date: Date
    let payload: WidgetPayload?

    /// The face for this entry's day: the snapshot's while it is current, else
    /// "Open lollop to update" (Android's day rule, WidgetSnapshotStore.today).
    var face: Face { payload?.shownFace(at: date) ?? Face.fallback }

    var current: Bool { payload?.isCurrent(at: date) ?? false }
}

/// Reads the payload the app writes (never the core) and rolls the widget over to
/// "Open lollop to update" at local midnight, as Android's midnight tick does. The
/// app reloads the timelines whenever it writes a new payload.
struct TodayProvider: TimelineProvider {
    var load: () -> WidgetPayload? = { WidgetPayload.load() }

    func placeholder(in context: Context) -> TodayEntry {
        TodayEntry(date: Date(), payload: nil)
    }

    func getSnapshot(in context: Context, completion: @escaping (TodayEntry) -> Void) {
        completion(TodayEntry(date: Date(), payload: load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<TodayEntry>) -> Void) {
        completion(timeline(at: Date()))
    }

    /// Now, then the stale face from the next local midnight while today's
    /// snapshot is current; a new timeline is asked for after that midnight.
    func timeline(at now: Date) -> Timeline<TodayEntry> {
        let payload = load()
        let midnight = nextMidnight(after: now)
        var entries = [TodayEntry(date: now, payload: payload)]
        if payload?.isCurrent(at: now) == true {
            entries.append(TodayEntry(date: midnight, payload: payload))
        }
        return Timeline(entries: entries, policy: .after(midnight))
    }
}

/// The home and lock screen widget: today's session and its items, the rest day
/// and the next session, or the pause, with the control's action (and Log) as its
/// buttons.
struct TodayWidget: Widget {
    static let kind = "app.milestone.lollop.today"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: TodayProvider()) { entry in
            TodayEntryView(entry: entry)
        }
        // Android's widget picker name and description (widget_overline_today,
        // widget_description in strings_widget.xml).
        .configurationDisplayName("Today")
        .description("Today's session, rest day or pause, with its next step and the plan week.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .accessoryRectangular, .accessoryCircular, .accessoryInline])
        .contentMarginsDisabled()
    }
}

struct TodayEntryView: View {
    let entry: TodayEntry
    @Environment(\.widgetFamily) private var family
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let face = entry.face
        let colors = entry.payload?.palette.colors(for: scheme) ?? .system
        switch family {
        case .accessoryRectangular:
            RectangularView(face: face)
                .widgetURL(LollopScreen.today.url)
                .widgetBackground(Color.clear)
        case .accessoryCircular:
            CircularView(face: face, week: entry.current ? entry.payload.map { (done: $0.snapshot.weekDone, planned: $0.snapshot.weekPlanned) } : nil)
                .widgetURL(LollopScreen.today.url)
                .widgetBackground(Color.clear)
        case .accessoryInline:
            InlineView(face: face)
                .widgetURL(LollopScreen.today.url)
                .widgetBackground(Color.clear)
        default:
            TodayCard(
                face: face,
                colors: colors,
                moreItems: entry.payload?.moreItems ?? [],
                wide: family != .systemSmall,
                buttons: family == .systemSmall ? Self.smallButtons : .links
            )
            .widgetURL(family == .systemSmall && Self.smallButtons == .drawn ? (face.actions.first?.screen ?? .today).url : LollopScreen.today.url)
            .widgetBackground(colors.ground)
        }
    }

    /// A small widget is one tap target before iOS 17; from 17 its button runs an intent.
    private static var smallButtons: CardButtons {
        if #available(iOS 17.0, *) { return .intents }
        return .drawn
    }
}

extension View {
    /// The widget's ground: the container background from iOS 17 (removable by the
    /// system in tinted and StandBy modes), a plain background before.
    @ViewBuilder
    func widgetBackground(_ color: Color) -> some View {
        if #available(iOS 17.0, *) {
            containerBackground(for: .widget) { color }
        } else {
            background(color)
        }
    }
}
