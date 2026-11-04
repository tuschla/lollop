pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "lollop"
include(":app", ":lollop", ":lollop-ios")
// The Rust core's Kotlin/Native bridge; kmp/ builds it on its own as well.
include(":core-bridge")
project(":core-bridge").projectDir = file("../kmp/core-bridge")
