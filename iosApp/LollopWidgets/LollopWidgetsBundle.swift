import SwiftUI
import WidgetKit

/// The app's one extension (one app ID on a free Apple Account): the home and lock
/// screen widget, the run's and the rest's Live Activity, and the Control Center
/// control. It runs no Kotlin: everything it shows comes from the app, as JSON
/// (the widget payload in the App Group container, the activities' content).
@main
struct LollopWidgetsBundle: WidgetBundle {
    var body: some Widget {
        TodayWidget()
        if #available(iOS 16.2, *) {
            LollopActivityWidget()
        }
        if #available(iOS 18.0, *) {
            LollopControl()
        }
    }
}
