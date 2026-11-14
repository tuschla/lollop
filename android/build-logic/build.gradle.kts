plugins {
    `kotlin-dsl`
}

dependencies {
    // The build applies the plugin itself (root build.gradle.kts), so at run time
    // these classes come from there.
    compileOnly(libs.kotlin.gradle.plugin)
    compileOnly(libs.android.gradle.api)
}

gradlePlugin {
    plugins {
        register("rustCore") {
            id = "lollo.rust-core"
            implementationClass = "RustCorePlugin"
        }
    }
}
