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
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    sourceSets {
        // MOB-001 instrumented tests read the committed synthetic fixture
        // pack directly from the repository's provenance-tracked fixtures.
        getByName("androidTest").assets.srcDir(rootProject.file("../fixtures/synthetic/wpack"))
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

// Explicit opt-in native build (MOB-001). assembleDebug does not depend on it,
// so the app still builds without a Rust toolchain; the committed Kotlin
// bindings call into libwiki_ffi.so only when it is packaged in jniLibs.
// RUST_ANDROID_ABIS selects ABIs (default arm64-v8a; CI adds x86_64).
tasks.register<Exec>("buildRustArm64") {
    group = "build"
    description = "Cross-compile wiki-ffi as Android native libraries (RUST_ANDROID_ABIS)"
    workingDir = rootProject.projectDir
    commandLine("bash", "scripts/build-rust-android.sh")
}

tasks.register<Exec>("generateUniFfiKotlin") {
    group = "build"
    description = "Regenerate the committed UniFFI Kotlin bindings from the wiki-ffi exports"
    workingDir = rootProject.projectDir
    commandLine("bash", "scripts/generate-uniffi-kotlin.sh")
    inputs.dir(rootProject.file("../crates/wiki-ffi/src"))
    outputs.file(file("src/main/java/uniffi/wiki_ffi/wiki_ffi.kt"))
}

dependencies {
    // UniFFI Kotlin bindings load libwiki_ffi.so through JNA (AAR variant
    // bundles the Android JNA native dispatch library).
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    // Must equal the rustls-platform-verifier-android version in Cargo.lock
    // (guarded by scripts/test_android_verifier_pin.py). Its AAR adds the
    // INTERNET permission and a network security config that allows
    // cleartext only to CA revocation-list hosts.
    implementation("org.rustls:rustls-platform-verifier:0.2.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
