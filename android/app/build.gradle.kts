// The Android app: platform code (Keystore, NSD, the foreground service, the clipboard, the UI) around the shared
// core, whose native library cargo-ndk builds for each ABI (D2).

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlinter)
}

android {
    namespace = "io.github.josevini.clipsync"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.josevini.clipsync"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // 64-bit only: arm64 for phones, x86_64 for the emulator and ChromeOS.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    lint {
        warningsAsErrors = true
        abortOnError = true
        // targetSdk 37 opts into Android 17's behaviour changes, such as its local network permission, which the app
        // does not handle yet. Newer library versions are proposed by hand, not by every build.
        disable += listOf("OldTargetApi", "GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion")
    }

    packaging {
        // Android 15+ maps native libraries straight from the APK, which needs them uncompressed and 16 KB aligned.
        jniLibs.useLegacyPackaging = false
    }
}

/** Builds the clipsync-ffi native library for every ABI with cargo-ndk. Cargo decides what is up to date. */
abstract class CargoNdk : DefaultTask() {
    @get:Inject abstract val exec: ExecOperations

    @get:Internal abstract val rustDir: DirectoryProperty

    @get:Internal abstract val ndkDir: DirectoryProperty

    @get:Input abstract val abis: ListProperty<String>

    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    init {
        outputs.upToDateWhen { false }
    }

    @TaskAction
    fun build() {
        exec.exec {
            workingDir = rustDir.get().asFile
            environment("ANDROID_NDK_HOME", ndkDir.get().asFile.path)
            // 16 KB pages (Android 15+): keep every load segment aligned for them.
            environment("RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384")
            commandLine(
                listOf("cargo", "ndk") + abis.get().flatMap { listOf("-t", it) } +
                    listOf("--platform", "29", "-o", outputDir.get().asFile.path) +
                    listOf("build", "--locked", "--profile", "android", "-p", "clipsync-ffi"),
            )
        }
    }
}

val cargoNdkBuild =
    tasks.register<CargoNdk>("cargoNdkBuild") {
        rustDir.set(rootDir.resolve("../linux"))
        ndkDir.set(androidComponents.sdkComponents.ndkDirectory)
        abis.set(android.defaultConfig.ndk.abiFilters.toList())
        outputDir.set(layout.buildDirectory.dir("rustJniLibs"))
    }

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdkBuild, CargoNdk::outputDir)
    }
}

dependencies {
    implementation(project(":session"))
    implementation("${libs.jna.get()}@aar")
    implementation(libs.core.ktx)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.lifecycle.service)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.zxing.core)

    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
}
