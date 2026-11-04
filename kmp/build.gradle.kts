// Plugin versions for the standalone bridge build. The app's Gradle build
// (android/) includes :core-bridge too and declares the same versions there.
plugins {
    kotlin("multiplatform") version "2.4.21" apply false
    kotlin("plugin.serialization") version "2.4.21" apply false
}
