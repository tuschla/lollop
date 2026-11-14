package app.milestone

/** A new, empty directory under the platform's temporary directory. */
expect fun newTempDir(): AppPath

/** The file [name] inside [dir]. */
expect fun fileIn(dir: AppPath, name: String): AppPath
