// The iOS app's Kotlin: the `Lollo` framework the Xcode app (iosApp/) links. It
// packages :lollo (the app, with :core-bridge, the Rust core over its C ABI)
// and the core itself. Its entry points install the core engine
// (`LolloApp.start()`) and host the app's Compose UI (`MainViewController()`).
// Dynamic, so the framework carries the Rust core and the app only embeds it;
// Xcode's build phase runs `:lollo-ios:embedAndSignAppleFrameworkForXcode`.
// Links on macOS only; see ios/README.md.
plugins {
    id("org.jetbrains.kotlin.multiplatform")
    // MainViewController's content lambda is @Composable.
    id("org.jetbrains.kotlin.plugin.compose")
    // Packs :lollo's Compose resources (strings, drawables) into the app:
    // embedAndSignAppleFrameworkForXcode then also copies `compose-resources`
    // into the .app, where the resource reader looks for them.
    id("org.jetbrains.compose")
}

kotlin {
    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.framework { baseName = "Lollo" }
        target.binaries.all { rustCore.link(target, this) }
    }

    sourceSets {
        iosMain.dependencies {
            implementation(project(":lollo"))
            implementation(libs.compose.runtime)
            implementation(libs.compose.ui)
        }
    }
}
