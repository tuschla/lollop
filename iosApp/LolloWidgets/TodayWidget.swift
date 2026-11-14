import SwiftUI
import WidgetKit

struct TodayEntry: TimelineEntry {
    let date: Date
    let payload: WidgetPayload?

    /// The face for this entry's moment: the snapshot's while it is current, else
    /// "Open lollo to update" (Android's day rule, WidgetSnapshot.currentOn).
    var face: WidgetFace { payload?.shownFace(at: date) ?? WidgetFallback.bundled.face }
}

/// Reads the payload the app writes (never the core) and rolls the widget over to
/// "Open lollo to update" when the snapshot's day ends, as Android's midnight tick
/// does. The app reloads the timelines whenever it writes a new payload.
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

    /// Now, then an entry wherever the shown face changes (the start and the end of
    /// the snapshot's day); nothing to ask for after the last one until the app
    /// writes again.
    func timeline(at now: Date) -> Timeline<TodayEntry> {
        let payload = load()
        let changes = payload?.changes(after: now) ?? []
        let entries = ([now] + changes).map { TodayEntry(date: $0, payload: payload) }
        return Timeline(entries: entries, policy: changes.isEmpty ? .never : .atEnd)
    }
}

/// The home and lock screen widget: today's session and its items, the rest day
/// and the next session, or the pause, with the control's action (and Log) as its
/// buttons.
struct TodayWidget: Widget {
    static let kind = "app.milestone.lollo.today"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: TodayProvider()) { entry in
            TodayEntryView(entry: entry)
        }
        // Android's widget picker name and description.
        .configurationDisplayName("widget_overline_today")
        .description("widget_description")
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
        let colors = entry.payload?.colors(for: scheme) ?? .system
        switch family {
        case .accessoryRectangular:
            RectangularView(face: face)
                .widgetURL(LolloScreen.today.url)
                .widgetBackground(Color.clear)
        case .accessoryCircular:
            CircularView(face: face)
                .widgetURL(LolloScreen.today.url)
                .widgetBackground(Color.clear)
        case .accessoryInline:
            InlineView(face: face)
                .widgetURL(LolloScreen.today.url)
                .widgetBackground(Color.clear)
        default:
            TodayCard(
                face: face,
                colors: colors,
                wide: family != .systemSmall,
                buttons: family == .systemSmall ? Self.smallButtons : .links
            )
            .widgetURL(family == .systemSmall && Self.smallButtons == .drawn ? (face.actions.first?.screen ?? .today).url : LolloScreen.today.url)
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
