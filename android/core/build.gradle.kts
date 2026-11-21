// Everything that is not a screen: the generated API types, the client, the
// copy of the manuscript kept on the phone, the save queue and the sync engine
// between them. Plain Kotlin/JVM, so all of it is tested in seconds without an
// emulator — the same split as ios/JustTypeKit's JustTypeKit target.
plugins {
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.kotlin.serialization)
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    api(libs.kotlinx.coroutines.core)
    api(libs.kotlinx.serialization.json)
    api(libs.okhttp)

    testImplementation(libs.junit)
    testImplementation(libs.okhttp.mockwebserver)
    testImplementation(libs.kotlinx.coroutines.test)
}

tasks.test {
    testLogging {
        events("failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

// The fixtures iOS tests against, read from where they live rather than copied: one scene document
// both clients must decode the same way.
sourceSets.test {
    resources.srcDir("../../ios/JustTypeKit/Tests/JustTypeKitTests/Fixtures")
}
