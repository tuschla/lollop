package app.milestone

import androidx.compose.ui.window.ComposeUIViewController
import platform.UIKit.UIViewController

/**
 * The iOS app's whole UI, for the Swift host (`iosApp/iosApp/LollopIosApp.swift`,
 * which shows it full screen after [LollopApp.start]).
 */
fun MainViewController(): UIViewController = ComposeUIViewController { LollopIosRoot() }

/** A URL the system opened the app with: a GPX, TCX or FIT file shared or opened
 *  from Files (the document types in Info.plist). Safe before the UI composes. */
fun openUrl(url: String) = LollopIosOpen(url)
