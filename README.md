# Scriptorium

A SaaS writing tool for long-form fiction. Authors draft in chapters and scenes,
snapshot versions as they go, publish a version for review, and get anchored,
votable feedback from other writers and readers.

- **Backend** — Rust: Axum + Diesel + SQLite, with manuscript content on disk
- **Web** — React + TypeScript + Vite, TipTap (ProseMirror) editor
- **iOS** — SwiftUI, sharing a Swift package with the app target

See [ROADMAP.md](ROADMAP.md) for what is built and what comes next, and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for why it is shaped this way.

## Layout

```
backend/    Rust API server, migrations, domain logic
web/        React client
ios/        Swift package (ScriptoriumKit + ScriptoriumUI) and the app target
docs/       Architecture, data model, API reference
data/       Local SQLite database and manuscript tree (git-ignored)
```

## Running it

Prerequisites: Rust 1.75+, Node 20+ with pnpm, Xcode 15+ for iOS.

```bash
cp .env.example .env          # then set SCRIPTORIUM_JWT_SECRET
make backend                  # http://127.0.0.1:8088
make web                      # http://localhost:5183, proxies /api to the backend
make ios                      # generates the Xcode project and opens it
```

`make test` runs the Rust unit tests, the Swift package tests, and the web
type-check. `make smoke` exercises the whole API end to end against a throwaway
database. Every target is a thin wrapper over `cargo xtask` — run `cargo xtask`
with no arguments for the full list.

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

Generated files — `web/src/api/schema.ts` and
`ios/ScriptoriumKit/Sources/ScriptoriumKit/Generated/Schemas.swift` — are
checked in and must not be hand-edited. Behaviour that hangs off those types
lives beside them, in `web/src/api/types.ts` and
`ios/.../Model+Helpers.swift`, so regenerating never clobbers hand-written code.

A running server also serves the document at `/api/openapi.json`.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `SCRIPTORIUM_BIND_ADDR` | `127.0.0.1:8088` | Listen address |
| `SCRIPTORIUM_DATABASE_URL` | `./data/scriptorium.db` | SQLite file |
| `SCRIPTORIUM_DATA_DIR` | `./data/works` | Manuscript tree |
| `SCRIPTORIUM_JWT_SECRET` | *(required, 16+ chars)* | Access-token signing key |
| `SCRIPTORIUM_ACCESS_TTL_MINUTES` | `30` | Access-token lifetime |
| `SCRIPTORIUM_REFRESH_TTL_DAYS` | `30` | Refresh-token lifetime |
| `SCRIPTORIUM_CORS_ORIGINS` | `http://localhost:5183` | Comma-separated allowed origins |
| `SCRIPTORIUM_PUBLIC_URL` | `http://localhost:5183` | Where the web client lives, for links in mail |
| `SCRIPTORIUM_MAIL` | `outbox` | `outbox` writes mail to a file; `smtp` sends it |
| `SCRIPTORIUM_OUTBOX` | `<data dir>/../outbox.jsonl` | The outbox file, one JSON message per line |
| `SCRIPTORIUM_SMTP_HOST` | *(required for smtp)* | Relay, spoken to with STARTTLS |
| `SCRIPTORIUM_SMTP_PORT` | `587` | |
| `SCRIPTORIUM_SMTP_USER`, `SCRIPTORIUM_SMTP_PASSWORD` | *(none)* | Relay credentials, if it wants them |
| `SCRIPTORIUM_MAIL_FROM` | *(required for smtp)* | The sender address |
| `SCRIPTORIUM_SECOND_FACTOR_RATE` | `5` | Two-factor tries per account per minute |
| `SCRIPTORIUM_LOG_FORMAT` | `text` | `json` for one JSON object per log line |
| `SCRIPTORIUM_BACKUP_DIR` | *(none)* | Where `cargo backup` puts backups; `/api/status` checks them |
| `SCRIPTORIUM_BACKUP_KEEP` | `14` | How many backups to keep |
| `SCRIPTORIUM_METRICS_TOKEN` | *(none)* | Turns on `/api/metrics` (Prometheus text) behind this bearer token |

In development nothing is ever sent: a verification or reset link lands in the
outbox file, and following it from there is the same as following it from an
inbox. The SMTP path compiles and is configured as above, but has not been run
against a real relay — that needs a sending domain and credentials.

Backups, the restore drill, request ids and the status check are in
[`docs/OPERATIONS.md`](docs/OPERATIONS.md).
