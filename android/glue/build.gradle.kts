// Loss' Kotlin glue as an Android library. dx copies this folder into its generated Gradle
// project as :plugins:lossglue (declared from Rust in app/src/glue.rs) and the app depends on it.
// The manifest in src/main is merged into the app's manifest by Gradle.
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.loss.glue"
    compileSdk = 34
    defaultConfig {
        minSdk = 26
        // Rust reaches these classes only through JNI, so R8 must keep them.
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/kotlin")
        }
    }
}
