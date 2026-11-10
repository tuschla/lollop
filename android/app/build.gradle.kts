// The application shell: packages :lollop (all code, resources and manifest
// components) with the Rust core's jniLibs into the APK. Application id,
// signing inputs, ABI filters, minification and versioning live here.
plugins {
    id("com.android.application")
}

val appVersion: Map<String, String> =
    rootDir.resolve("../app-version.xcconfig").readLines()
        .mapNotNull { Regex("""^(\w+)\s*=\s*(.+)$""").find(it.trim())?.destructured }
        .associate { (key, value) -> key to value.trim() }

// The ABIs per build type: the Rust core is built for these and the APK keeps
// only these. Without a filter androidx transitive .so's would add
// armeabi-v7a/x86 slices that install on those devices and crash at the first
// JNI call. Release ships phones only (arm64-v8a, ~3 MB smaller); debug adds
// x86_64 so it runs on the emulator.
// QA only: `-PqaX86Release` (or `=true`) adds x86_64 to release so the minified
// APK runs on the x86_64 emulator (scripts/emulator-scenarios.sh --apk); `=false`
// leaves it out.
val abis: Map<String, List<String>> = mapOf(
    "debug" to listOf("arm64-v8a", "x86_64"),
    "release" to listOf("arm64-v8a") +
        if (providers.gradleProperty("qaX86Release").map { it != "false" }.getOrElse(false)) listOf("x86_64") else emptyList(),
)

android {
    // The library owns `app.milestone` (its R class and every FQCN); the shell
    // only needs a namespace of its own.
    namespace = "app.milestone.shell"
    compileSdk = libs.versions.android.compileSdk.get().toInt()
    buildToolsVersion = libs.versions.android.buildTools.get()

    defaultConfig {
        applicationId = "app.milestone"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = appVersion.getValue("CURRENT_PROJECT_VERSION").toInt()
        versionName = appVersion.getValue("MARKETING_VERSION")
    }

    buildTypes {
        debug {
            ndk { abiFilters += abis.getValue("debug") }
        }
        release {
            ndk { abiFilters += abis.getValue("release") }
            isMinifyEnabled = true
            isShrinkResources = true
            // The app's own keep rules ship with :lollop as consumer rules
            // (lollop/consumer-rules.pro).
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }

    // Distribution shape:
    //   • F-Droid + GitHub direct-download → `./gradlew assembleRelease` = one
    //     arm64-v8a APK. F-Droid builds APKs from source (no AAB support).
    //   • Play Store → `./gradlew bundleRelease` = an AAB; Play splits it
    //     per-device (density + language), no manual ABI splits needed.
    // Any published build MUST be signed with a real keystore supplied through
    // MILESTONE_KEYSTORE (see scripts/build.sh); the debug-key fallback is for
    // local sideload/testing only and is not distributable.

    compileOptions {
        // Core-library desugaring backports the java.time API (kotlinx-datetime
        // wraps it on Android: every date in the UI, the export file names, the
        // import time stamps) to the minSdk-24 range. Without it, an Android 7
        // (API 24/25) device hits NoClassDefFoundError on the first frame:
        // java.time.* only exists natively from API 26.
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.toVersion(libs.versions.jvm.target.get())
        targetCompatibility = JavaVersion.toVersion(libs.versions.jvm.target.get())
    }
}

androidComponents {
    onVariants { variant ->
        val jniLibs = rustCore.androidJniLibs(
            project,
            "rustJniLibs" + variant.name.replaceFirstChar(Char::uppercase),
            abis.getValue(variant.buildType!!),
        )
        variant.sources.jniLibs?.addGeneratedSourceDirectory(jniLibs, CargoNdk::outputDir)
    }
}

dependencies {
    implementation(project(":lollop"))
    coreLibraryDesugaring(libs.desugar.jdk.libs)
}
