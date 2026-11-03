// Kotlin Multiplatform build for the iOS port (Compose Multiplatform). Its own
// Gradle build, separate from android/, until the Android app moves in here.
// See ios/README.md.
pluginManagement {
    repositories {
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        mavenCentral()
    }
}

rootProject.name = "lollop-kmp"
include(":core-bridge")
