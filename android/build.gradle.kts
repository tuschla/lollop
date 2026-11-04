import org.jetbrains.kotlin.gradle.targets.native.tasks.KotlinNativeSimulatorTest

plugins {
    id("com.android.application") version "9.3.3" apply false
    id("com.android.kotlin.multiplatform.library") version "9.3.3" apply false
    id("com.android.lint") version "9.3.3" apply false
    id("org.jetbrains.kotlin.multiplatform") version "2.4.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "2.4.21" apply false
    id("org.jetbrains.compose") version "1.12.2" apply false
    id("io.github.takahirom.roborazzi") version "1.76.0" apply false
}

// `-Plollop.iosSimulator=<udid>` picks the simulator every module's iOS tests
// boot (CI passes an iPhone the runner image has); unset keeps the plugin's default.
subprojects {
    tasks.withType<KotlinNativeSimulatorTest>().configureEach {
        providers.gradleProperty("lollop.iosSimulator").orNull?.let { device.set(it) }
    }
}
