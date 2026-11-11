import org.gradle.api.DefaultTask
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
 */
fun Project.repoRootConstant(pkg: String): TaskProvider<GenerateRepoRoot> =
    tasks.register("generateRepoRoot", GenerateRepoRoot::class.java) {
        packageName.set(pkg)
        root.set(rootDir.parentFile.absolutePath)
        outputDir.set(layout.buildDirectory.dir("generated/repoRoot"))
    }
