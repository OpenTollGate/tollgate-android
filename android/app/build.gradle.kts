plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

/**
 * Resolve the short (7-char) git SHA of this repo at configuration time, used to
 * stamp BuildConfig.GIT_HASH for the Settings → About card. Git auto-discovers
 * the repo root from any subdirectory, so this works regardless of which module
 * gradle is configuring. Returns "dev" if git is missing (source export / CI
 * without VCS) so the build never fails for want of a hash.
 */
fun gitShortHash(): String = try {
    val proc = Runtime.getRuntime().exec(
        arrayOf("git", "rev-parse", "--short=7", "HEAD"),
        emptyArray(),
        rootProject.projectDir,
    )
    proc.inputStream.bufferedReader().readText().trim().ifEmpty { "dev" }
} catch (e: Exception) {
    "dev"
}

android {
    namespace = "org.opentollgate.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.opentollgate.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
        // Short git SHA baked into the APK at build time so the Settings → About
        // screen can show a build hash. Resolved eagerly from the repo root; if
        // git is unavailable (CI without VCS, source tarball) it degrades to
        // "dev" rather than failing the build.
        buildConfigField("String", "GIT_HASH", "\"${gitShortHash()}\"")
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
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
        // Emit org.opentollgate.android.BuildConfig (VERSION_NAME + GIT_HASH),
        // read by the Settings → About card.
        buildConfig = true
    }
    // The UniFFI-generated Kotlin bindings live under this source set (produced
    // by `just bindings` / the uniffi-bindgen step in justfile).
    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/kotlin")
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons.extended)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.lifecycle.viewmodel.compose)
    implementation(libs.lifecycle.runtime.compose)
    // UniFFI's generated Foreign Callback + pointer plumbing uses JNA on the JVM.
    // Must force the AAR variant — the plain JAR only has desktop native libs,
    // causing UnsatisfiedLinkError for libjnidispatch.so on Android.
    // Using explicit @aar notation — the artifact{} block syntax is unreliable
    // in some AGP versions and silently falls back to the JAR.
    implementation("net.java.dev.jna:jna:5.16.0@aar")
    debugImplementation(libs.compose.ui.tooling)
}
