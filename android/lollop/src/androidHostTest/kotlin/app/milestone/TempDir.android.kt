package app.milestone

import java.io.File
import java.nio.file.Files

actual fun newTempDir(): AppPath = Files.createTempDirectory("lollop-log").toFile()

actual fun fileIn(dir: AppPath, name: String): AppPath = File(dir, name)
