# Just Type Studio for Android

The Android app: Kotlin and Jetpack Compose, a port of the iOS app screen for
screen — the desk, a book's outline, the scene editor, versions and offering
for review, the reader with anchored comments, Daily Prompts, the inbox, and
the account's settings — plus a "Today's words" home-screen widget and a
"Start writing" launcher shortcut.

```
core/   Plain Kotlin/JVM: the generated API types, the client, the copy of the
        manuscript kept on the phone, the save queue and the sync engine.
app/    The Android app: Compose screens around the shared editor page.
```

Like `ios/JustTypeKit`, everything that is not a screen lives in `core/`, and is
tested on the JVM in seconds without an emulator.

## Build and run

Needs JDK 17 (Gradle does not run on 25) and the Android SDK, API 36.

```bash
cargo android                # build, install and open it; talks to production
cargo android --local        # …to `make backend` on this computer (10.0.2.2:8088 from the emulator)
cargo android --api <url>    # …to any other server

cd android
./gradlew :core:test         # what `cargo xtask test` runs
./gradlew :app:assembleDebug # app/build/outputs/apk/debug/app-debug.apk
./gradlew :app:installDebug -PapiBaseUrl=http://10.0.2.2:8088/api
```

`cargo android` finds a JDK 17 itself when `JAVA_HOME` is unset; by hand,
`export JAVA_HOME=$(/usr/libexec/java_home -v 17)` first.

## How it is put together

**The editor is the iOS app's.** The page in
`ios/JustTypeKit/Sources/JustTypeUI/Resources/Editor` — the same TipTap the
browser runs — is copied into the app's assets at build time and hosted in a
`WebView` (`app/.../editor/ProseEditor.kt`). One line is added to its
`<head>`: iOS hands the page `window.webkit.messageHandlers.justtype`, and on
Android that object is a shim over a `JustTypeAndroid` JavaScript interface.
So there is one editor and one set of ProseMirror positions for every client;
a comment anchored on Android lands on the same words in a browser.
`DocNodeTest` checks Kotlin's arithmetic against the numbers iOS captured from
the real editor.

**The model types are generated.** `core/.../api/Schemas.kt` comes from
`docs/openapi.json` with the TypeScript and Swift types
(`cargo xtask codegen`; CI fails if it is stale). Do not edit it; behaviour
on those types lives in `core/.../model/`.

**Offline is the iOS rules, rule for rule.** Reads prefer the server and fall
back to the phone; writes go to the phone first; a queued edit always beats the
server's copy; a conflicting save waits for the writer to choose. The queue is
sent when the app comes back to the front. `OfflineTest` is iOS's
`OfflineTests` against a real HTTP server that can be switched off.
Unlike iOS, losing signal never signs the writer out: the session is cleared
only when the server refuses it, and the last-known account is kept so the app
opens to the desk on a plane.

**The session** is sealed with an AES-GCM key in the Android Keystore
(`SecureTokenStore`), in `noBackupFilesDir`, and nothing is backed up.

## Identifiers

- Application id `studio.justtype.app`. Unlike the iOS bundle ids it carries
  the new name: there was never a Scriptorium Android app to stay compatible with.
- The daily email's links (`https://www.justtype.studio/prompts/...`) open the
  app once the site serves `/.well-known/assetlinks.json` with the release
  certificate's SHA-256; until then Android offers the app in the chooser.
- Release builds are minified and unsigned until a release key exists.
