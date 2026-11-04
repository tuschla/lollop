import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget
import org.jetbrains.kotlin.gradle.plugin.mpp.NativeBinary

// The iOS app's Kotlin: the `Lollop` framework the Xcode app links. It joins
// :lollop (the app, whose iOS code compiles without the core) and :core-bridge
// (the Rust core over its C ABI), and its entry point installs the core engine
// (`LollopApp.start()`). Dynamic, so the framework carries the Rust core and
// the app only embeds it. Links on macOS only; see ios/README.md.
plugins {
    id("org.jetbrains.kotlin.multiplatform")
}

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
        }
        iosTest.dependencies {
            implementation(kotlin("test"))
            // Event.toJson's JsonElement, :lollop's implementation dependency.
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
        }
    }
}
