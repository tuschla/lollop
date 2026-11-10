plugins {
    `kotlin-dsl`
}

dependencies {
    // The build applies the plugin itself (root build.gradle.kts), so at run time
    // these classes come from there.
    compileOnly(libs.kotlin.gradle.plugin)
}

gradlePlugin {
    plugins {
        register("rustCore") {
            id = "lollop.rust-core"
            implementationClass = "RustCorePlugin"
        }
    }
}
