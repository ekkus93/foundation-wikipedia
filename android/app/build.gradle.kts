plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.foundation.wikipedia"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.foundation.wikipedia"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
}

// Explicit opt-in native build. Do not make assembleDebug depend on this until
// UniFFI-generated Kotlin bindings and a tested JNI contract are available.
tasks.register<Exec>("buildRustArm64") {
    group = "build"
    description = "Cross-compile wiki-ffi as an Android arm64-v8a native library"
    workingDir = rootProject.projectDir
    commandLine("bash", "scripts/build-rust-android.sh")
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
