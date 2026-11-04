import SwiftUI
import Lollop

/// The iOS host: one window showing the app's Compose UI from the Kotlin
/// framework (`MainViewController()` in android/lollop-ios). The UI is drawn
/// edge to edge and keeps clear of the notch, home indicator and keyboard
/// itself (Compose window insets), as on Android.
@main
struct LollopIosApp: App {
    var body: some Scene {
        WindowGroup {
            ComposeView()
                .ignoresSafeArea()
                // A GPX, TCX or FIT file opened from Files or shared with the app
                // (CFBundleDocumentTypes): iOS copies it into Documents/Inbox.
                .onOpenURL { url in MainViewControllerKt.openUrl(url: url.absoluteString) }
        }
    }
}

private struct ComposeView: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        MainViewControllerKt.MainViewController()
    }

    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}
