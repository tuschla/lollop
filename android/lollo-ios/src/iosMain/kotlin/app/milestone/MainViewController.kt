package app.milestone

import androidx.compose.ui.window.ComposeUIViewController
import platform.UIKit.UIViewController

/**
 * The iOS app's whole UI, for the Swift host (`iosApp/iosApp/LolloIosApp.swift`,
 * which shows it full screen after [LolloApp.start]).
 */
fun MainViewController(): UIViewController = ComposeUIViewController { LolloIosRoot() }

/** A URL the system opened the app with: a GPX, TCX or FIT file shared or opened
 *  from Files (the document types in Info.plist). Safe before the UI composes. */
fun openUrl(url: String) = LolloIosOpen(url)
