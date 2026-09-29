// The phone's side of the protocol in plain Kotlin: TLS, connections, reconnection and saved state around the
// shared core. Android-free, so it runs and is tested on the JVM against a real clipsyncd.

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.kotlinter)
}

val rustDir: File = rootDir.resolve("../linux").canonicalFile

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

dependencies {
    api(project(":bindings"))
    implementation(libs.kotlinx.serialization.json)
    // The app brings JNA's Android AAR; the JVM tests bring the jar.
    compileOnly(libs.jna)
    testImplementation(libs.jna)
    testImplementation(libs.junit)
}

// The interop tests run the real daemon and CLI. Cargo decides what is up to date.
val cargoBuildDaemon = tasks.register<Exec>("cargoBuildDaemon") {
    description = "Builds clipsyncd and clipsync for the interop tests."
    workingDir = rustDir
    commandLine("cargo", "build", "--locked", "-p", "clipsyncd")
}

tasks.test {
    dependsOn(":bindings:cargoBuildHost", cargoBuildDaemon)
    val debugDir = rustDir.resolve("target/debug").path
    systemProperty("jna.library.path", debugDir)
    systemProperty("clipsync.bin.dir", debugDir)
    testLogging {
        events("failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}
