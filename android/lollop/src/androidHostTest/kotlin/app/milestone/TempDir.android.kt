package app.milestone

import java.nio.file.Files

actual fun newTempDir(): AppPath = Files.createTempDirectory("lollop-log").toFile()
