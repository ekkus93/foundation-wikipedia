pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // AI-003: the Kotlin half of rustls-platform-verifier (Android OS
        // certificate verification for the shared Rust HTTPS transport),
        // published by the rustls project as an on-disk Maven archive.
        // Restricted to its own group so nothing else resolves from it.
        exclusiveContent {
            forRepository {
                maven {
                    url = uri("https://raw.githubusercontent.com/rustls/rustls-platform-verifier/maven-archive/android-release-support/maven/")
                }
            }
            filter { includeGroup("org.rustls") }
        }
    }
}
rootProject.name = "FoundationWikipedia"
include(":app")
