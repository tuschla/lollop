import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget
import org.jetbrains.kotlin.gradle.plugin.mpp.NativeBinary
import org.jetbrains.kotlin.gradle.plugin.mpp.TestExecutable
import org.jetbrains.kotlin.gradle.targets.native.tasks.KotlinNativeSimulatorTest
import org.jetbrains.kotlin.konan.target.Family
import org.jetbrains.kotlin.konan.target.KonanTarget

// The Rust core over its C ABI (shared/include/lollop.h) for Kotlin/Native:
// `CoreBridge` in commonMain is the call set the app's engine makes, and
// nativeMain implements it with cinterop. linuxX64 exists only to run the
// bridge tests on a Linux host; the iOS targets compile and link on macOS only
// (CI job `bridge-macos`). Two builds include this module: kmp/ (the bridge
// alone) and android/ (with the app's iOS framework, :lollop-ios). See
// ios/README.md.
plugins {
    kotlin("multiplatform")
    kotlin("plugin.serialization")
}

val repoRoot: File = rootDir.parentFile

// Honor CARGO_TARGET_DIR like android/app's buildHostCore does.
val cargoTargetDir: File =
    System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: File(repoRoot, "target")

fun rustTriple(target: KonanTarget): String = when (target) {
    KonanTarget.LINUX_X64 -> "x86_64-unknown-linux-gnu"
    KonanTarget.IOS_ARM64 -> "aarch64-apple-ios"
    KonanTarget.IOS_SIMULATOR_ARM64 -> "aarch64-apple-ios-sim"
    else -> throw GradleException("no Rust target for $target")
}

/** The system libraries the Rust staticlib needs, as printed by
 *  `cargo rustc ... -- --print native-static-libs` (CoreFoundation: chrono's
 *  time-zone lookup on Apple targets). */
fun rustSystemLibs(target: KonanTarget): List<String> = when (target.family) {
    Family.LINUX -> listOf("-lgcc_s", "-lutil", "-lrt", "-lpthread", "-lm", "-ldl", "-lc")
    else -> listOf("-framework", "CoreFoundation", "-lSystem", "-lc", "-lm")
}

/** One cargo build of libshared.a per (triple, flavour): test binaries link a
 *  debug build with the `lollop_*_for_test` hooks, everything else the release
 *  build without them. */
fun cargoTask(triple: String, testHooks: Boolean): TaskProvider<Exec> {
    val name = "cargo" + (if (testHooks) "TestHooks" else "Release") +
        triple.split('-', '_').joinToString("") { it.replaceFirstChar(Char::uppercase) }
    return tasks.findByName(name)?.let { tasks.named<Exec>(name) }
        ?: tasks.register<Exec>(name) {
            group = "build"
            description = "Build libshared.a (C ABI) for $triple."
            workingDir = repoRoot
            val args = if (testHooks) listOf("--test-hooks", triple) else listOf(triple)
            commandLine(listOf(File(repoRoot, "scripts/build-ios-core.sh").path) + args)
            inputs.dir(File(repoRoot, "shared/src"))
            inputs.file(File(repoRoot, "shared/Cargo.toml"))
            inputs.file(File(repoRoot, "Cargo.toml"))
            inputs.file(File(repoRoot, "Cargo.lock"))
            outputs.file(rustLibDir(triple, testHooks).resolve("libshared.a"))
        }
}

fun rustLibDir(triple: String, testHooks: Boolean): File =
    cargoTargetDir.resolve(triple).resolve(if (testHooks) "debug" else "release")

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
        binaries.all { linkRustCore(this@configureEach, this) }
    }

    sourceSets {
        commonTest {
            kotlin.srcDir(generateTestPaths)
            dependencies {
                implementation(kotlin("test"))
                implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
            }
        }
    }
}

/** Link the Rust core into [binary] of [target]: test binaries get the debug core
 *  with the test hooks, every other binary the release core. A module that links
 *  this one into its own binaries needs the same (the cinterop klib doesn't carry
 *  the archive), so it is shared as `extra["linkRustCore"]` (android/lollop-ios). */
fun linkRustCore(target: KotlinNativeTarget, binary: NativeBinary) {
    val triple = rustTriple(target.konanTarget)
    val testHooks = binary is TestExecutable
    val cargo = cargoTask(triple, testHooks)
    val rustLib = rustLibDir(triple, testHooks).resolve("libshared.a")
    // The archive is a link input: a rebuilt core must relink the
    // binary, not leave an up-to-date one with the old core.
    binary.linkTaskProvider.configure {
        dependsOn(cargo)
        inputs.file(rustLib)
    }
    binary.linkerOpts(listOf(rustLib.path) + rustSystemLibs(target.konanTarget))
}

extra["linkRustCore"] = ::linkRustCore

// `-Plollop.iosSimulator=<udid>` picks the simulator the iOS tests boot (CI
// passes an iPhone the runner image has); unset keeps the plugin's default.
tasks.withType<KotlinNativeSimulatorTest>().configureEach {
    providers.gradleProperty("lollop.iosSimulator").orNull?.let { device.set(it) }
}
