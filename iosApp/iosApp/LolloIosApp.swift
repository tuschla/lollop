import SwiftUI
import Lollo

/// The iOS host: one window showing the app's Compose UI from the Kotlin
/// framework (`MainViewController()` in android/lollo-ios). The UI is drawn
/// edge to edge and keeps clear of the notch, home indicator and keyboard
/// itself (Compose window insets), as on Android. The widgets, the control and
/// the Live Activities get their content from the Kotlin app through [Surfaces].
@main
struct LolloIosApp: App {
    @Environment(\.scenePhase) private var scenePhase

    init() {
        // The app first, then the surfaces, which follow its published model.
        LolloApp.shared.start()
        Surfaces.shared.install()
    }

    var body: some Scene {
        WindowGroup {
            ComposeView()
                .ignoresSafeArea()
                // A GPX, TCX or FIT file opened from Files or shared with the app
                // (CFBundleDocumentTypes): iOS copies it into Documents/Inbox. A
                // widget's or an activity's lollo://open/<dest> link.
                .onOpenURL { url in MainViewControllerKt.openUrl(url: url.absoluteString) }
        }
        .onChange(of: scenePhase) { phase in
            if phase == .active { Surfaces.shared.appBecameActive() }
        }
    }
}

private struct ComposeView: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        MainViewControllerKt.MainViewController()
    }

    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}
