# Just Type Studio

A SaaS writing tool for long-form fiction. Authors draft in chapters and scenes,
snapshot versions as they go, publish a version for review, and get anchored,
votable feedback from other writers and readers.

- **Backend** — Rust: Axum + Diesel + SQLite, with manuscript content on disk
- **Web** — React + TypeScript + Vite, TipTap (ProseMirror) editor
- **iOS** — SwiftUI, sharing a Swift package with the app target
- **Android** — Kotlin and Jetpack Compose, the same editor page as iOS

See [ROADMAP.md](ROADMAP.md) for what is built and what comes next, and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for why it is shaped this way.

## Layout

```
backend/    Rust API server, migrations, domain logic
web/        React client
ios/        Swift package (JustTypeKit + JustTypeUI) and the app target
android/    Gradle project: core (JVM, tested without an emulator) and the app
docs/       Architecture, data model, API reference
data/       Local SQLite database and manuscript tree (git-ignored)
```

## Running it

Prerequisites: Rust 1.75+, Node 20+ with pnpm, Xcode 15+ for iOS, JDK 17 and
the Android SDK (API 36) for Android.

```bash
cp .env.example .env          # then set JUSTTYPE_JWT_SECRET
make backend                  # http://127.0.0.1:8088
make web                      # http://localhost:5183, proxies /api to the backend
make ios                      # generates the Xcode project and opens it
cargo android --local         # builds the Android app and installs it on the emulator
```

`make test` runs the Rust unit tests, the Swift package tests, the Android
core tests, and the web type-check. `make smoke` exercises the whole API end to end against a throwaway
database. Every target is a thin wrapper over `cargo xtask` — run `cargo xtask`
with no arguments for the full list.

### The Android app

See [android/README.md](android/README.md): `cargo android` builds and installs
it against production, `--local` against `make backend` on this computer.

### The iOS app

```bash
cargo sim                              # build, install and launch on a simulator
cargo sim --device "iPad Pro 13-inch (M4)"
cargo sim --reset --screenshot         # fresh install, then a PNG
cargo sim --physical --console         # a paired iPhone, attached to its output
cargo xtask testflight                 # bump the build number, archive, upload
cargo xtask testflight 7 1.1.0         # build 7, marketing version 1.1.0
cargo xtask testflight --no-upload     # archive and verify the .ipa, stop there
```

`ios/project.yml` is the source of truth for the bundle id and both version
numbers; the scripts read and rewrite it there and regenerate the Xcode project.

Signing is not in the repo. The Apple Developer team is a property of whoever is
building, so it lives in `scripts/.env` (gitignored) alongside the App Store
Connect credentials — copy `scripts/.env.example` and fill it in:

| Variable | Needed for |
|---|---|
| `DEVELOPMENT_TEAM` | device builds, archives, uploads |
| `ASC_KEY_ID` / `ASC_ISSUER_ID` / `ASC_KEY_PATH` | uploading (preferred) |
| `ASC_APPLE_ID` / `ASC_APP_PASSWORD` | uploading (alternative) |

`scripts/ios_signing.sh` turns `DEVELOPMENT_TEAM` into `ios/Signing.xcconfig`,
which the generated project points both build configurations at — so xcodebuild
and Xcode-opened-by-hand read the same value. Anything that runs `xcodegen`
writes it first. A simulator build needs no team at all; the paths that do say
so plainly rather than failing inside xcodebuild.

## The shape of the thing

A **work** has **chapters**, which have **scenes**. A scene's body is a TipTap
JSON document stored as a file; everything else lives in SQLite.

Taking a **version** freezes the whole work: the scene files are copied into an
immutable snapshot and a flat manifest records what was in it. A version moves
`draft → published → final`. Publishing a version makes it eligible to be
offered for review; a **publication** is that offer, carrying its visibility,
the slice of the work on offer, and the question the author wants answered.

**Comments** hang off a publication and anchor into the snapshot by ProseMirror
position. Because a snapshot never changes, an anchor can never drift — which is
the whole reason comments attach to versions rather than to the working copy.

## The API contract

`docs/openapi.json` is generated from the Rust handlers, and the client model
types are generated from it:

```bash
cargo xtask codegen          # regenerate after any change to a handler or DTO
cargo xtask codegen --check  # what CI runs; fails if anything is stale
```

Generated files — `web/src/api/schema.ts`,
`ios/JustTypeKit/Sources/JustTypeKit/Generated/Schemas.swift` and
`android/core/src/main/kotlin/studio/justtype/core/api/Schemas.kt` — are
checked in and must not be hand-edited. Behaviour that hangs off those types
lives beside them, in `web/src/api/types.ts` and
`ios/.../Model+Helpers.swift`, so regenerating never clobbers hand-written code.

A running server also serves the document at `/api/openapi.json`.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `JUSTTYPE_BIND_ADDR` | `127.0.0.1:8088` | Listen address |
| `JUSTTYPE_DATABASE_URL` | `./data/justtype.db` | SQLite file |
| `JUSTTYPE_DATA_DIR` | `./data/works` | Manuscript tree |
| `JUSTTYPE_JWT_SECRET` | *(required, 16+ chars)* | Access-token signing key |
| `JUSTTYPE_ACCESS_TTL_MINUTES` | `30` | Access-token lifetime |
| `JUSTTYPE_REFRESH_TTL_DAYS` | `30` | Refresh-token lifetime |
| `JUSTTYPE_CORS_ORIGINS` | `http://localhost:5183` | Comma-separated allowed origins |
| `JUSTTYPE_PUBLIC_URL` | `http://localhost:5183` | Where the web client lives, for links in mail |
| `JUSTTYPE_MAIL` | `outbox` | `outbox` writes mail to a file; `smtp` or `resend` sends it |
| `JUSTTYPE_OUTBOX` | `<data dir>/../outbox.jsonl` | The outbox file, one JSON message per line |
| `JUSTTYPE_SMTP_HOST` | *(required for smtp)* | Relay, spoken to with STARTTLS |
| `JUSTTYPE_SMTP_PORT` | `587` | |
| `JUSTTYPE_SMTP_USER`, `JUSTTYPE_SMTP_PASSWORD` | *(none)* | Relay credentials, if it wants them |
| `JUSTTYPE_RESEND_API_KEY` | *(required for resend)* | Resend API key (`re_…`) |
| `JUSTTYPE_MAIL_FROM` | *(required for smtp, resend)* | The sender address, on a domain the provider has verified |
| `JUSTTYPE_SECOND_FACTOR_RATE` | `5` | Two-factor tries per account per minute |
| `JUSTTYPE_LOG_FORMAT` | `text` | `json` for one JSON object per log line |
| `JUSTTYPE_BACKUP_DIR` | *(none)* | Where `cargo backup` puts backups; `/api/status` checks them |
| `JUSTTYPE_BACKUP_KEEP` | `14` | How many backups to keep |
| `JUSTTYPE_ARGON2_MEMORY_KIB` | `65536` | Password hashing memory; `--tune-argon2` measures what this machine affords |
| `JUSTTYPE_ARGON2_ITERATIONS`, `JUSTTYPE_ARGON2_LANES` | `2`, `1` | |
| `JUSTTYPE_IP_RATE` | `600` | Requests per minute from one address; `0` turns it off |
| `JUSTTYPE_AUTH_IP_RATE` | `30` | Sign-in, sign-up and reset requests per minute from one address |
| `JUSTTYPE_TRUST_PROXY` | `false` | Read the client address from the proxy's last `X-Forwarded-For` entry |
| `JUSTTYPE_SIGNUP_IP_HOURLY`, `JUSTTYPE_SIGNUP_IP_DAILY` | `3`, `10` | Sign-ups from one address an hour, a day; `0` turns one off |
| `JUSTTYPE_SIGNUP_GLOBAL_HOURLY` | `60` | Sign-ups across the site an hour |
| `JUSTTYPE_TURNSTILE_SITE_KEY`, `JUSTTYPE_TURNSTILE_SECRET_KEY` | *(none)* | Cloudflare Turnstile on the sign-up form; set both or neither |
| `JUSTTYPE_SIGNUP_UNVERIFIED_IP_DAILY`, `JUSTTYPE_SIGNUP_UNVERIFIED_GLOBAL_HOURLY` | `2`, `10` | With Turnstile on: sign-ups without it, per address a day and site-wide an hour |
| `ANTHROPIC_API_KEY` | *(none)* | Turns on summaries; unset, the feature is off and hidden |
| `JUSTTYPE_SUMMARY_MODEL` | `claude-opus-5-5` | The model that writes them |
| `JUSTTYPE_ANTHROPIC_URL` | `https://api.anthropic.com` | Where to send them (the smoke points it at a stub) |
| `JUSTTYPE_CACHE_MB` | `64` | Memory for snapshot scene bodies |
| `JUSTTYPE_METRICS_TOKEN` | *(none)* | Turns on `/api/metrics` (Prometheus text) behind this bearer token |
| `JUSTTYPE_WEB_DIR` | *(none)* | A built web app (`web/dist`) to serve beside the API; unset, the API alone |

In development nothing is ever sent: a verification or reset link lands in the
outbox file, and following it from there is the same as following it from an
inbox. The SMTP path compiles and is configured as above, but has not been run
against a real relay — that needs a sending domain and credentials.

Backups, the restore drill, request ids and the status check are in
[`docs/OPERATIONS.md`](docs/OPERATIONS.md).

## Renamed from Scriptorium

What was named for the old name keeps working:

- `SCRIPTORIUM_*` settings are read as their `JUSTTYPE_*` names (the server
  warns once at start), and `cargo setup` renames them in `.env`, values kept.
- `./data/scriptorium.db` is used if there is no `./data/justtype.db` yet, and
  backups holding `scriptorium.db` still restore.
- The browser moves its `scriptorium.*` storage, and saves still queued
  offline, to the new names on first load; the iOS app moves its offline
  folder the same way.

Kept on purpose: the iOS bundle ids, app group, keychain service and widget
kind (`app.scriptorium.*`), which are registered with Apple and tied to what
is on people's phones; and the label share links are signed with, so links
already sent keep opening. Migrations already applied are left as written.

## Docker

One image holds the whole service: the API, and the web app it serves beside
it (`JUSTTYPE_WEB_DIR`). Every push to `main` builds it and publishes it to
GitHub's registry (`.github/workflows/ghcr-publish.yml`) as
`ghcr.io/<owner>/justtype-studio`, tagged `latest`, `main`, the commit's short
sha and a version (`0.1.<run>`), which `/api/health` and `/api/status` report.

To try the image locally, `cargo docker` builds it from the working tree and
runs it on <http://localhost:8080>, with the repo's JWT secret and its own data
volume (`--port <n>`, `--no-build`, `--detach`, `--fresh`; `cargo xtask` lists
them). By hand:

```sh
docker build -t justtype-studio .
docker run -p 8080:8080 -v justtype-data:/usr/local/data \
  -e JUSTTYPE_JWT_SECRET=… justtype-studio
```

The image listens on 8080 as an unprivileged user, keeps the database, the
manuscript files and the mail outbox in the `/usr/local/data` volume, logs
JSON to stderr, and checks its own `/api/health`. It defaults
`JUSTTYPE_PUBLIC_URL` and `JUSTTYPE_CORS_ORIGINS` to
`https://www.justtype.studio`; set `JUSTTYPE_TRUST_PROXY=true` behind a
reverse proxy (otherwise every sign-up counts against the proxy's address, and
the per-address sign-up limit acts as a site-wide one), and `JUSTTYPE_MAIL=smtp` or `resend` with its settings to send real mail.
