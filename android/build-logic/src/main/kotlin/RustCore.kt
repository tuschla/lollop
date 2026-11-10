import java.io.File
import java.util.Properties
import javax.inject.Inject
import org.gradle.api.GradleException
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.file.Directory
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Provider
import org.gradle.api.tasks.Exec
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskProvider
import org.jetbrains.kotlin.gradle.plugin.mpp.KotlinNativeTarget
import org.jetbrains.kotlin.gradle.plugin.mpp.NativeBinary
import org.jetbrains.kotlin.gradle.plugin.mpp.TestExecutable

/** Applied to the root project; every project reaches it as [rustCore]. */
class RustCorePlugin : Plugin<Project> {
    override fun apply(project: Project) {
        check(project == project.rootProject) { "lollop.rust-core is applied to the root project" }
        project.extensions.create("rustCore", RustCore::class.java, project)
    }
}

val Project.rustCore: RustCore
    get() = rootProject.extensions.getByType(RustCore::class.java)

enum class CargoProfile(val cargoName: String, val dir: String, val flags: List<String>) {
    DEV("dev", "debug", listOf("--profile", "dev")),
    RELEASE("release", "release", listOf("--release")),
}

abstract class CargoNdk : Exec() {
    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty
}

/**
 * Every cargo build of the Rust core (the `shared` crate) in this Gradle build:
 * where cargo writes, which profile and triple each product uses (from
 * scripts/lib/rust-core.env, which scripts/build-ios-core.sh reads too), and what
 * a build reads. The C ABI staticlibs are root-project tasks because more than
 * one module links them.
 */
open class RustCore @Inject constructor(private val root: Project) {
    val repoRoot: File = root.rootDir.parentFile

    private val facts = Properties().apply {
        repoRoot.resolve(FACTS).reader().use { load(it) }
    }

    val appProfile: CargoProfile = profile("RUST_PROFILE_APP")
    val testProfile: CargoProfile = profile("RUST_PROFILE_TESTS")

    /** CARGO_TARGET_DIR, which cargo resolves against the repo root it runs in. */
    val targetDir: File = repoRoot.resolve(System.getenv("CARGO_TARGET_DIR") ?: "target")

    init {
        for (key in facts.stringPropertyNames().filter { it.startsWith(TRIPLE) }) {
            val triple = facts.getProperty(key)
            for (testHooks in listOf(false, true)) {
                root.tasks.register(staticLibTask(triple, testHooks), Exec::class.java) {
                    group = "build"
                    description = "Build the core's C ABI staticlib for $triple" +
                        if (testHooks) " with the test hooks." else "."
                    val script = repoRoot.resolve("scripts/build-ios-core.sh")
                    runsCargo(listOf(script.path) + (if (testHooks) listOf("--test-hooks") else emptyList()) + triple)
                    inputs.file(script)
                    outputs.file(staticLib(triple, testHooks))
                }
            }
        }
    }

    fun libDir(triple: String?, profile: CargoProfile): File =
        (if (triple == null) targetDir else targetDir.resolve(triple)).resolve(profile.dir)

    /** The core's JNI exports for the host JVM (feature `jni-host`), copied to [into]. */
    fun hostJniLibrary(project: Project, name: String, into: Provider<Directory>): TaskProvider<Exec> =
        project.tasks.register(name, Exec::class.java) {
            val built = libDir(null, testProfile).resolve(System.mapLibraryName("shared"))
            runsCargo(listOf("cargo", "build", "-p", "shared", "--features", "jni-host") + testProfile.flags + "--locked")
            val out = into.map { it.file(built.name) }
            outputs.file(out)
            doLast { built.copyTo(out.get().asFile, overwrite = true) }
            group = "build"
            description = "Build the core's JNI exports (jni-host) for the host JVM's tests."
        }

    /** The core for each Android ABI in [abis] (cargo-ndk), laid out as a jniLibs directory. */
    fun androidJniLibs(project: Project, name: String, abis: List<String>): TaskProvider<CargoNdk> =
        project.tasks.register(name, CargoNdk::class.java) {
            group = "build"
            description = "Build the core for the Android ABIs $abis."
            outputDir.set(project.layout.buildDirectory.dir("rustJniLibs/$name"))
            val out = outputDir.get().asFile
            runsCargo(
                listOf("cargo", "ndk", "-o", out.path) + abis.flatMap { listOf("-t", it) } +
                    listOf("build", "-p", "shared") + appProfile.flags + "--locked",
            )
            inputs.property("abis", abis)
            // cargo-ndk only adds files; an ABI dropped from the list must not stay.
            doFirst { out.deleteRecursively() }
        }

    /**
     * Links the core's C ABI staticlib into [binary]: test executables get the
     * test profile with the `lollop_*_for_test` hooks, every other binary the app
     * profile without them. The cinterop klib doesn't carry the archive, so every
     * module that links a binary over it calls this.
     */
    fun link(target: KotlinNativeTarget, binary: NativeBinary) {
        val triple = facts.getProperty(TRIPLE + target.name)
            ?: throw GradleException("$FACTS has no $TRIPLE${target.name}")
        val testHooks = binary is TestExecutable
        val lib = staticLib(triple, testHooks)
        val cargo = root.tasks.named(staticLibTask(triple, testHooks))
        binary.linkTaskProvider.configure {
            dependsOn(cargo)
            // A rebuilt core must relink the binary.
            inputs.file(lib)
        }
        binary.linkerOpts(listOf(lib.path) + systemLibs(triple))
    }

    private fun Exec.runsCargo(command: List<String>) {
        workingDir = repoRoot
        commandLine(command)
        inputs.dir(repoRoot.resolve("shared/src")).withPathSensitivity(PathSensitivity.RELATIVE)
        inputs.files(
            listOf("shared/Cargo.toml", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", FACTS).map(repoRoot::resolve),
        ).withPathSensitivity(PathSensitivity.RELATIVE)
    }

    private fun staticLib(triple: String, testHooks: Boolean): File =
        libDir(triple, if (testHooks) testProfile else appProfile).resolve("libshared.a")

    private fun staticLibTask(triple: String, testHooks: Boolean): String =
        "rustStaticLib" + triple.split('-', '_').joinToString("") { it.replaceFirstChar(Char::uppercase) } +
            if (testHooks) "TestHooks" else ""

    private fun profile(key: String): CargoProfile {
        val name = facts.getProperty(key)
        return CargoProfile.entries.find { it.cargoName == name }
            ?: throw GradleException("$FACTS: $key=$name is not one of ${CargoProfile.entries.map { it.cargoName }}")
    }

    /** The system libraries the staticlib needs, as printed by `cargo rustc ...
     *  -- --print native-static-libs` (CoreFoundation: chrono's time-zone lookup
     *  on Apple targets). */
    private fun systemLibs(triple: String): List<String> = when {
        "-apple-" in triple -> listOf("-framework", "CoreFoundation", "-lSystem", "-lc", "-lm")
        "-linux-" in triple -> listOf("-lgcc_s", "-lutil", "-lrt", "-lpthread", "-lm", "-ldl", "-lc")
        else -> throw GradleException("no system libraries known for $triple")
    }

    private companion object {
        const val FACTS = "scripts/lib/rust-core.env"
        const val TRIPLE = "RUST_TRIPLE_"
    }
}
