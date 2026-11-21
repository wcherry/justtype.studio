plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

/**
 * Every build talks to production, as the iOS app does. For a local backend:
 *   ./gradlew :app:installDebug -PapiBaseUrl=http://10.0.2.2:8088/api   (emulator)
 * 10.0.2.2 is the emulator's name for the computer it runs on.
 */
val apiBaseUrl = (findProperty("apiBaseUrl") as String?) ?: "https://www.justtype.studio/api"

android {
    namespace = "studio.justtype.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "studio.justtype.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        buildConfigField("String", "API_BASE_URL", "\"$apiBaseUrl\"")
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Unsigned until a release key exists.
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        resources.excludes += setOf("META-INF/versions/9/OSGI-INF/MANIFEST.MF", "META-INF/*.kotlin_module")
    }
}

kotlin {
    jvmToolchain(17)
}

/**
 * The editor is the page the iOS app ships — the same TipTap build, so there is one editor and one
 * set of ProseMirror positions for every client. It is copied from where iOS keeps it rather than
 * duplicated, with one line added to its <head>: iOS's WKWebView hands the page
 * `window.webkit.messageHandlers.justtype`, and on Android that object is a shim over the
 * `JustTypeAndroid` JavaScript interface. Nothing else in the page changes.
 */
val editorSource = rootProject.file("../ios/JustTypeKit/Sources/JustTypeUI/Resources/Editor")
val editorAssets = layout.buildDirectory.dir("generated/editor-assets")
val copyEditor by tasks.registering(Copy::class) {
    from(editorSource)
    into(editorAssets.map { it.dir("editor") })
    filesMatching("index.html") {
        filter { line ->
            if (line.trim() == "<head>") line + "\n    <script>window.webkit = { messageHandlers: { justtype: { postMessage: function (m) { JustTypeAndroid.post(JSON.stringify(m)) } } } }</script>"
            else line
        }
    }
}
android.sourceSets["main"].assets.srcDir(editorAssets)
tasks.named("preBuild") { dependsOn(copyEditor) }

dependencies {
    implementation(project(":core"))

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.webkit)
    implementation(libs.androidx.glance.appwidget)
    implementation(libs.kotlinx.coroutines.android)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
}
