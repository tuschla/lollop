import AppIntents
import Foundation
#if !LOLLOP_WIDGET_EXTENSION
import Lollop
#endif

// The App Intents the widgets, the control and the Live Activities run. Compiled
// into the app and the widget extension: the extension needs the types for its
// buttons, and the system runs both intents in the app's process (an OpenIntent
// brings the app to the front; a LiveActivityIntent runs in the app). The calls
// into the Kotlin app are compiled into the app only.

/// A screen a system surface opens: Android's `OpenDest` (`lollop://open/<dest>`);
/// android/lollop's AppIdentifiersTest checks the cases and the URL against it.
enum LollopScreen: String, AppEnum {
    case today
    case log
    case track
    case logSet = "log_set"

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "lollop screen"
    static let caseDisplayRepresentations: [LollopScreen: DisplayRepresentation] = [
        .today: "Today",
        .log: "Log",
        .track: "Track run",
        .logSet: "Log set",
    ]

    /// The destination's wire name ([OpenDest.wire]); an unknown one opens Today.
    init(wire: String) {
        self = LollopScreen(rawValue: wire) ?? .today
    }

    var url: URL { URL(string: "lollop://open/\(rawValue)")! }
}

/// Open the app at a screen: the control's tap and the small widget's button,
/// as Android's tile and widget buttons open their destination.
struct OpenLollopIntent: OpenIntent {
    static let title: LocalizedStringResource = "Open lollop"

    @Parameter(title: "Screen")
    var target: LollopScreen

    init() {
        target = .today
    }

    init(target: LollopScreen) {
        self.target = target
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        #if !LOLLOP_WIDGET_EXTENSION
        MainViewControllerKt.openUrl(url: target.url.absoluteString)
        #endif
        return .result()
    }
}

/// The run activity's Pause and Resume: Android's notification actions.
@available(iOS 17.0, *)
struct SetRunPausedIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Pause or resume the run"

    @Parameter(title: "Paused")
    var paused: Bool

    init() {
        paused = true
    }

    init(paused: Bool) {
        self.paused = paused
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        #if !LOLLOP_WIDGET_EXTENSION
        LollopSurfaces.shared.setRunPaused(paused: paused)
        #endif
        return .result()
    }
}
