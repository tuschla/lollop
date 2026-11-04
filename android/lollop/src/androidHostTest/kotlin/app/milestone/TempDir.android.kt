package app.milestone

import java.nio.file.Files

actual fun newTempDir(): LogPath = Files.createTempDirectory("lollop-log").toFile()
