import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget

// The Rust core over its C ABI (shared/include/lollop.h) for Kotlin/Native:
// `CoreBridge` in commonMain is the call set the app's engine makes, and
// nativeMain implements it with cinterop. linuxX64 exists only to run the
// bridge tests on a Linux host; the iOS targets compile and link on macOS only
// (CI job `bridge-macos`). :lollop-ios links it into the app's framework. See
// ios/README.md.
plugins {
    id("org.jetbrains.kotlin.multiplatform")
    id("org.jetbrains.kotlin.plugin.serialization")
}

val repoRoot: File = rootDir.parentFile

/** The repo root as a test constant, so the tests read the core's fixture logs. */
val generateTestPaths = tasks.register("generateTestPaths") {
    val out = layout.buildDirectory.dir("generated/testPaths")
    val root = repoRoot.absolutePath
    inputs.property("root", root)
    outputs.dir(out)
    doLast {
        val file = out.get().file("app/milestone/core/TestPaths.kt").asFile
        file.parentFile.mkdirs()
        file.writeText(
            "package app.milestone.core\n\ninternal const val REPO_ROOT = \"" +
                root.replace("\\", "\\\\").replace("\"", "\\\"") + "\"\n",
        )
    }
}

kotlin {
    linuxX64()
    iosArm64()
    iosSimulatorArm64()

    targets.withType<KotlinNativeTarget>().configureEach {
        compilations.getByName("main").cinterops.create("lollop") {
            definitionFile.set(project.file("src/nativeInterop/cinterop/lollop.def"))
            includeDirs(File(repoRoot, "shared/include"))
            // Declare the test hooks on every target; only test binaries call
            // them, and only those link the hook-enabled library.
            compilerOpts("-DLOLLOP_TEST_HOOKS")
        }
        binaries.all { rustCore.link(this@configureEach, this) }
    }

    sourceSets {
        commonTest {
            kotlin.srcDir(generateTestPaths)
            dependencies {
                implementation(kotlin("test"))
                implementation(libs.kotlinx.serialization.json)
            }
        }
    }
}
