import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget
import org.jetbrains.kotlin.gradle.plugin.mpp.NativeBinary

// The iOS app's Kotlin: the `Lollop` framework the Xcode app (iosApp/) links. It
// joins :lollop (the app, whose iOS code compiles without the core) and
// :core-bridge (the Rust core over its C ABI). Its entry points install the core
// engine (`LollopApp.start()`) and host the app's Compose UI
// (`MainViewController()`). Dynamic, so the framework carries the Rust core and
// the app only embeds it; Xcode's build phase runs
// `:lollop-ios:embedAndSignAppleFrameworkForXcode`. Links on macOS only; see
// ios/README.md.
plugins {
    id("org.jetbrains.kotlin.multiplatform")
    // MainViewController's content lambda is @Composable.
    id("org.jetbrains.kotlin.plugin.compose")
    // Packs :lollop's Compose resources (strings, drawables) into the app:
    // embedAndSignAppleFrameworkForXcode then also copies `compose-resources`
    // into the .app, where the resource reader looks for them.
    id("org.jetbrains.compose")
}

// The Compose Multiplatform version :lollop builds with.
val cmpVersion = "1.12.2"

evaluationDependsOn(":core-bridge")

@Suppress("UNCHECKED_CAST")
val linkRustCore = project(":core-bridge").extra["linkRustCore"] as (KotlinNativeTarget, NativeBinary) -> Unit

kotlin {
    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.framework { baseName = "Lollop" }
        target.binaries.all { linkRustCore(target, this) }
    }

    sourceSets {
        iosMain.dependencies {
            implementation(project(":lollop"))
            implementation(project(":core-bridge"))
            implementation("org.jetbrains.compose.runtime:runtime:$cmpVersion")
            implementation("org.jetbrains.compose.ui:ui:$cmpVersion")
        }
        iosTest.dependencies {
            implementation(kotlin("test"))
            // Event.toJson's JsonElement, :lollop's implementation dependency.
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
        }
    }
}
