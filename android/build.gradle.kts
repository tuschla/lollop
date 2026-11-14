import org.jetbrains.kotlin.gradle.targets.native.tasks.KotlinNativeSimulatorTest

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.kotlin.multiplatform.library) apply false
    alias(libs.plugins.android.lint) apply false
    alias(libs.plugins.kotlin.multiplatform) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.compose.multiplatform) apply false
    alias(libs.plugins.roborazzi) apply false
    id("lollo.rust-core")
}

// `-Plollo.iosSimulator=<udid>` picks the simulator every module's iOS tests
// boot (CI passes an iPhone the runner image has); unset keeps the plugin's default.
subprojects {
    tasks.withType<KotlinNativeSimulatorTest>().configureEach {
        providers.gradleProperty("lollo.iosSimulator").orNull?.let { device.set(it) }
    }
}
