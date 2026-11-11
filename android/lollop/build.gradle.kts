import java.time.Duration
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// The whole app (UI, platform code, manifest components, resources) as a Kotlin
// Multiplatform library; :app only packages it. Namespace app.milestone keeps
// `app.milestone.R` and every FQCN the manifest, JNI and tests name.
//   commonMain   platform-free code (the Compose UI included), for Android and iOS
//   androidMain  Android code (activity, services, widget, tile, the JNI core)
//   androidHostTest  the JVM + Robolectric suite (`:lollop:testAndroidHostTest`)
plugins {
    id("org.jetbrains.kotlin.multiplatform")
    id("com.android.kotlin.multiplatform.library")
    // Lint report tasks for the KMP Android library (`:lollop:lint`).
    id("com.android.lint")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("io.github.takahirom.roborazzi")
}

kotlin {
    android {
        namespace = "app.milestone"
        compileSdk = libs.versions.android.compileSdk.get().toInt()
        buildToolsVersion = libs.versions.android.buildTools.get()
        minSdk = libs.versions.android.minSdk.get().toInt()
        // java.time on API 24/25: :app desugars; this tells lint and the library's
        // D8 that the APIs are backported.
        enableCoreLibraryDesugaring = true

        androidResources { enable = true }

        compilerOptions { jvmTarget.set(JvmTarget.fromTarget(libs.versions.jvm.target.get())) }

        optimization {
            consumerKeepRules.publish = true
            consumerKeepRules.file("consumer-rules.pro")
        }

        withHostTest {
            // Plain-JVM tests (no Robolectric) reach android.util.Log through
            // PlatformLog when the code under test logs a failure; the framework
            // stubs throw unless unmocked calls return defaults.
            isReturnDefaultValues = true
            // Robolectric (the offline UI tests) needs the merged Android
            // resources + manifest to inflate the real theme, strings, and the
            // MainActivity Compose tree on the JVM.
            isIncludeAndroidResources = true
        }
    }

    // iOS klibs: Kotlin compiles them on any host (`compileKotlinIosArm64` runs on
    // Linux CI). Their binaries (the tests; the app's framework is :lollop-ios's)
    // link the Rust core's C ABI, which :core-bridge binds.
    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.all { rustCore.link(target, this) }
    }

    compilerOptions {
        // expect/actual classes (JvmSerializable) are Beta; this acknowledges it.
        freeCompilerArgs.add("-Xexpect-actual-classes")
    }

    sourceSets {
        commonMain.dependencies {
            api(project(":core-bridge"))
            implementation(libs.compose.runtime)
            implementation(libs.compose.runtime.saveable)
            implementation(libs.compose.ui)
            implementation(libs.compose.foundation)
            implementation(libs.compose.material3)
            // Window size classes (currentWindowAdaptiveInfoV2, Adaptive.kt).
            implementation(libs.compose.material3.adaptive)
            implementation(libs.compose.components.resources)
            // collectAsStateWithLifecycle, LocalLifecycleOwner.
            implementation(libs.jetbrains.lifecycle.runtime.compose)
            // Public API (Event.toJson(): JsonElement, the wire types' serializers).
            api(libs.kotlinx.serialization.json)
            implementation(libs.kotlinx.coroutines.core)
            // Dates and time zones in common code; on Android it wraps java.time
            // (desugared below API 26), so dates read exactly as before.
            implementation(libs.kotlinx.datetime)
            // SynchronizedObject + synchronized(): a real lock on Kotlin/Native, where
            // @Synchronized does nothing; on the JVM it is the object's monitor.
            implementation(libs.atomicfu)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            // runTest for the boot sequence's suspend turns (AppBootTest).
            implementation(libs.kotlinx.coroutines.test)
        }
        iosMain.dependencies {
            implementation(libs.jetbrains.lifecycle.runtime.compose.ios)
            // BackHandler (PlatformUi.ios.kt): the UIKit back gesture.
            implementation(libs.compose.ui.backhandler)
        }
        androidMain.dependencies {
            implementation(libs.androidx.core.ktx)
            // Backported SplashScreen API: paints the theme-aware launch frame
            // (white/dark logo in light, black/white logo in dark) before Compose.
            implementation(libs.androidx.core.splashscreen)
            implementation(libs.androidx.activity.compose)
            // Declared rather than relied on transitively through activity-compose.
            implementation(libs.androidx.lifecycle.runtime.compose)
            implementation(project.dependencies.platform(libs.androidx.compose.bom))
            implementation(libs.androidx.compose.ui)
            implementation(libs.androidx.compose.material3)
            implementation(libs.androidx.compose.material3.adaptive)
            implementation(libs.kotlinx.coroutines.android)
            implementation(libs.osmdroid.android)
            implementation(libs.play.services.location)
            implementation(libs.androidx.glance.appwidget)
        }
        getByName("androidHostTest") {
            kotlin.srcDir(repoRootConstant("app.milestone"))
            dependencies {
                implementation(libs.junit)
                implementation(libs.kotlinx.coroutines.test)
                implementation(libs.robolectric)
                implementation(libs.androidx.test.core.ktx)
                implementation(project.dependencies.platform(libs.androidx.compose.bom))
                implementation(libs.androidx.compose.ui.test.junit4)
                // Declares the empty ComponentActivity that createComposeRule() launches;
                // the test manifest Robolectric reads must contain it.
                implementation(libs.androidx.compose.ui.test.manifest)
                implementation(libs.roborazzi)
                implementation(libs.androidx.glance.appwidget.testing)
            }
        }
    }
}

// enableCoreLibraryDesugaring needs the library on this module's configuration
// too (AGP checks it when writing the AAR metadata).
dependencies {
    "coreLibraryDesugaring"(libs.desugar.jdk.libs)
}

val generateComposeResources = tasks.register<GenerateComposeResources>("generateComposeResources") {
    group = "build"
    description = "Generate the CMP resources (strings, UI vectors) and the R id map from the Android res."
    sources.from(fileTree("src/androidMain/res/values") { include("strings*.xml") })
    drawables.from(
        fileTree("src/androidMain/res/drawable") {
            include("ic_*.xml")
            exclude("ic_launcher_*.xml", "ic_stat_mark.xml")
        },
    )
    outputDir.set(layout.buildDirectory.dir("generated/composeResources"))
    idsDir.set(layout.buildDirectory.dir("generated/androidResourceIds"))
}

kotlin.sourceSets.getByName("androidMain").kotlin.srcDir(generateComposeResources.flatMap { it.idsDir })

compose.resources {
    packageOfResClass = "app.milestone.resources"
    publicResClass = false
    generateResClass = always
    // The generated resources are commonMain's compose resources; the provider
    // carries the task dependency into every resource task.
    customDirectory(
        sourceSetName = "commonMain",
        directoryProvider = generateComposeResources.flatMap { it.outputDir },
    )
}

// The Robolectric UI/UX tests drive the real Rust core over JNI: the test JVM
// loads the host build of the core's JNI exports (the APK's core is :app's).
val qaJniDir: Provider<Directory> = layout.buildDirectory.dir("qa-jni")

val buildHostCore = rustCore.hostJniLibrary(project, "buildHostCore", qaJniDir)

tasks.withType<Test>().configureEach {
    dependsOn(buildHostCore)
    // System.loadLibrary("shared") (JniCore) finds the host core here and only here,
    // whatever library path the Gradle JVM runs with.
    systemProperty("java.library.path", qaJniDir.get().asFile.absolutePath)
    // Roborazzi records real pixels; NATIVE graphics also makes Compose layout
    // deterministic on the JVM.
    systemProperty("robolectric.graphicsMode", "NATIVE")
    // Native code in the Robolectric runtime logs straight to the terminal.
    // android-base's logger reads ANDROID_LOG_TAGS and accepts only
    // `*:<lowercase level>` (any other spec logs "unsupported ... in
    // ANDROID_LOG_TAGS"), so this sets one floor for every native tag.
    environment("ANDROID_LOG_TAGS", "*:w")
    // Two test JVMs, each with headroom for the real core + Compose +
    // Robolectric: 2 × 2 GB fits the 16 GB CI runner.
    maxParallelForks = 2
    maxHeapSize = "2g"
    // Robolectric's Android 16 (SDK 36) sandbox reaches into jdk.internal.access to
    // set up shared memory; without this export every @Config(sdk = [36]) test fails
    // at setup with "Failed to interact with raw FileDescriptor internals".
    jvmArgs("--add-exports=java.base/jdk.internal.access=ALL-UNNAMED")
    // A hang fails the task instead of holding CI until its job timeout: the full
    // suite runs in well under this.
    timeout.set(Duration.ofMinutes(30))
}

// scripts/emulator-scenarios.sh's 5-year log: LongHistorySeed is host-test code
// over the real core, so this runs on the host tests' classpath and host core.
tasks.register<JavaExec>("exportLongHistorySeed") {
    description = "Write the 5-year LongHistorySeed event log to --args=OUT.ndjson (scripts/emulator-scenarios.sh)."
    dependsOn(buildHostCore)
    classpath = tasks.getByName<Test>("testAndroidHostTest").classpath
    mainClass.set("app.milestone.qa.EmuSeedExportKt")
    systemProperty("java.library.path", qaJniDir.get().asFile.absolutePath)
}
