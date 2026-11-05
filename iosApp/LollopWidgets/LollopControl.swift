import AppIntents
import SwiftUI
import WidgetKit

/// The Control Center control (iOS 18): Android's Quick Settings tile
/// (QuickTileService.kt, visual-design-system §10 "Quick Settings tile"). It shows
/// the tile's face for today from the widget payload, never the core: Start run,
/// Log set, Run · recording, else Log, with the widget's title under the label,
/// and a tap opens that screen. The app reloads it whenever it writes the payload.
@available(iOS 18.0, *)
struct LollopControl: ControlWidget {
    static let kind = "app.milestone.lollop.control"

    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: Self.kind, provider: ControlFaceProvider()) { face in
            ControlWidgetButton(action: OpenLollopIntent(target: face.screen)) {
                Label(face.label, systemImage: face.symbol)
                if let subtitle = face.subtitle {
                    Text(subtitle)
                }
            }
        }
        // Android's tile picker name (tile_log).
        .displayName("Log")
        .description("Opens today's next step: Start run, Log set or Log.")
    }
}

/// Today's tile face from the payload; Log before the app has written one.
@available(iOS 18.0, *)
struct ControlFaceProvider: ControlValueProvider {
    var previewValue: ControlFace { .fallback }

    func currentValue() async throws -> ControlFace {
        WidgetPayload.load()?.controlFace(at: Date()) ?? .fallback
    }
}
