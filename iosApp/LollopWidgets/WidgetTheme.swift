import SwiftUI

// The app's type and spacing tokens (android/lollop's Theme.kt `Type`, `Space`)
// in the system font. Text styles rather than fixed sizes, so the widgets follow
// the reader's text size as Android's follow its font scale.

enum WidgetType {
    /// `Type.Display` (32 sp bold, tabular figures).
    static let display = Font.system(.largeTitle).weight(.bold).monospacedDigit()
    /// A small card's hero figure: one size down from `display`, so the title, the
    /// figure, its pace and the button fit a small widget's height.
    static let displaySmall = Font.system(.title).weight(.bold).monospacedDigit()
    /// `Type.Title` (20 sp bold).
    static let title = Font.system(.title3).weight(.bold)
    /// `Type.Body` (16 sp).
    static let body = Font.system(.callout)
    /// `Type.BodyBold`.
    static let bodyBold = Font.system(.callout).weight(.bold)
    /// `Type.Caption` (14 sp).
    static let caption = Font.system(.subheadline)
    /// `Type.Overline` (12 sp bold, tracked): set uppercase.
    static let overline = Font.system(.caption).weight(.bold)
    static let overlineRegular = Font.system(.caption)
    /// `Type.Chip` (13 sp bold).
    static let chip = Font.system(.footnote).weight(.bold)
    /// A button label (Glance's FilledButton, 14 sp medium).
    static let button = Font.system(.subheadline).weight(.semibold)
}

enum WidgetSpace {
    static let sm: CGFloat = 4
    static let md: CGFloat = 8
    static let screen: CGFloat = 16

    /// A card's padding: `Space.Screen`, a little less on a small widget, which is
    /// shorter than Android's two rows.
    static func edge(wide: Bool) -> CGFloat { wide ? screen : 14 }

    /// A round button's and a labelled button's height (Android: 48 dp
    /// `ActionMinHeight`): the 44 pt touch target, 36 pt on a small widget, which is
    /// one tap target (iOS 16) or one button.
    static func action(wide: Bool) -> CGFloat { wide ? 44 : 36 }
}

/// The extension's own bundle: its asset catalog (also compiled into the tests).
private final class WidgetBundleToken {}

extension Bundle {
    static let lollopWidgets = Bundle(for: WidgetBundleToken.self)
}

/// The Android drawables (rendered by iosApp/make-assets.py into the extension's
/// asset catalog as template images): the tile faces' icons, the route mark of
/// every notification, the tracker's pause, play and stop.
enum WidgetIcon: String {
    case trackRun = "IconTrackRun"
    case logSet = "IconLogSet"
    case log = "IconLog"
    case mark = "IconMark"
    case pause = "IconPause"
    case play = "IconPlay"
    case stop = "IconStop"

    /// The icon of a tile face (recording | start_run | log_set | log), as Android's `TileFace.icon`.
    init(face: String) {
        switch face {
        case "recording", "start_run": self = .trackRun
        case "log_set": self = .logSet
        default: self = .log
        }
    }

    var image: Image {
        Image(rawValue, bundle: .lollopWidgets).renderingMode(.template)
    }
}
