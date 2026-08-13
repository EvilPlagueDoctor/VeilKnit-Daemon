import org.gradle.api.tasks.Exec

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.example.veilknit_deamon"
    compileSdk {
        version = release(36) {
            minorApiLevel = 1
        }
    }

    defaultConfig {
        applicationId = "com.example.veilknit_deamon"
        minSdk = 29
        targetSdk = 36
        versionCode = 7
        versionName = "1.5.0-multilingual"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        manifestPlaceholders["apiPermission"] =
            "com.example.veilknit_deamon.permission.BIND_VEILKNIT_API"
        manifestPlaceholders["apiAction"] =
            "com.example.veilknit_deamon.BIND_LOCAL_API"
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isDebuggable = false
            isJniDebuggable = false
            isMinifyEnabled = true
            isShrinkResources = true
            ndk {
                debugSymbolLevel = "none"
            }
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

    buildFeatures {
        compose = true
        aidl = true
        buildConfig = true
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation("androidx.lifecycle:lifecycle-service:2.10.0")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")

    testImplementation(libs.junit)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(libs.androidx.junit)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
    debugImplementation(libs.androidx.compose.ui.tooling)
}

androidComponents {
    onVariants(selector().all()) { variant ->
        variant.outputs.forEach { output ->
            output.outputFileName.set("VeilKnitDaemon-${variant.name}.apk")
        }
    }
}

val rustManifest = rootProject.file("native/veilknit-daemon/Cargo.toml")
val rustOutput = layout.projectDirectory.dir("src/main/jniLibs")

fun Exec.configureCargoNdk(release: Boolean) {
    group = "rust"
    description = "Build the VeilKnit Rust daemon for Android"
    val rustRoot = rootProject.file("native/veilknit-daemon")
    workingDir(rustRoot)

    if (release) {
        val separator = "\u001f"
        val flags = listOf(
            "--remap-path-prefix=${rootProject.projectDir.absolutePath}=/_/veilknit-daemon/android",
            "--remap-path-prefix=${System.getProperty("user.home")}=/_/home",
            "-C", "debuginfo=0",
            "-C", "strip=symbols"
        )
        environment("CARGO_ENCODED_RUSTFLAGS", flags.joinToString(separator))
        environment("CARGO_INCREMENTAL", "0")
    }

    val args = mutableListOf(
        "cargo", "ndk",
        "--platform", "29",
        "-t", "arm64-v8a",
        "-t", "x86_64",
        "-o", rustOutput.asFile.absolutePath,
        "build", "--lib"
    )
    if (release) args.add("--release")
    commandLine(args)

    inputs.dir(rootProject.file("native/veilknit-daemon/src"))
    inputs.file(rustManifest)
    outputs.dir(rustOutput)
}

val buildRustDebug = tasks.register<Exec>("buildRustDebug") {
    configureCargoNdk(release = false)
}
val buildRustRelease = tasks.register<Exec>("buildRustRelease") {
    configureCargoNdk(release = true)
}

afterEvaluate {
    tasks.matching { it.name == "preDebugBuild" }
        .configureEach { dependsOn(buildRustDebug) }
    tasks.matching { it.name == "preReleaseBuild" }
        .configureEach { dependsOn(buildRustRelease) }
}
