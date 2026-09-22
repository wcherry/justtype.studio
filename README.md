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
database.

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
