import com.android.build.api.variant.HostTestBuilder
import com.android.build.api.variant.KotlinMultiplatformAndroidComponentsExtension
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.Project
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.api.tasks.TaskProvider

abstract class GenerateRepoRoot : DefaultTask() {
    @get:Input
    abstract val packageName: Property<String>

    @get:Input
    abstract val root: Property<String>

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val dir = outputDir.get().asFile
        dir.deleteRecursively()
        val pkg = packageName.get()
        val path = root.get().replace("\\", "\\\\").replace("\"", "\\\"")
        dir.resolve(pkg.replace('.', '/')).apply { mkdirs() }.resolve("RepoRoot.kt")
            .writeText("package $pkg\n\ninternal const val REPO_ROOT = \"$path\"\n")
    }
}

/**
 * A source directory declaring `internal const val REPO_ROOT` (the repository's
 * absolute path) in [pkg], for a test source set: the one way tests read repo
 * files (fixtures, generated iOS sources). Kotlin/Native and simulator test
 * runs have no classpath resources and no fixed working directory.
 * The Android host tests take it through [androidHostTestRepoRootConstant].
 */
fun Project.repoRootConstant(pkg: String): TaskProvider<GenerateRepoRoot> =
    tasks.register("generateRepoRoot", GenerateRepoRoot::class.java) {
        packageName.set(pkg)
        root.set(rootDir.parentFile.absolutePath)
        outputDir.set(layout.buildDirectory.dir("generated/repoRoot"))
    }

/**
 * [repoRootConstant] for the Android host tests of a
 * `com.android.kotlin.multiplatform.library` module. AGP's lint model tasks read
 * the host-test source directories as plain paths, so a directory added with
 * `kotlin.srcDir` reaches them without its task dependency; added through the
 * variant API, every task that reads the host-test sources depends on the task.
 */
fun Project.androidHostTestRepoRootConstant(pkg: String) {
    val task = repoRootConstant(pkg)
    extensions.getByType(KotlinMultiplatformAndroidComponentsExtension::class.java).onVariants { variant ->
        val hostTest = variant.hostTests[HostTestBuilder.UNIT_TEST_TYPE]
            ?: throw GradleException("$path has no Android host tests (withHostTest)")
        hostTest.sources.kotlin!!.addGeneratedSourceDirectory(task, GenerateRepoRoot::outputDir)
    }
}
