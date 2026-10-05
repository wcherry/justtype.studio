# Operations

One process, one SQLite file, one folder of manuscripts. Running it well comes
down to four things: knowing what a failed request was, knowing whether the
service is fit to serve, keeping copies, and proving the copies restore.

## Request ids and logs

Every response carries an `x-request-id`. The web client sends its own
(`web-…`) and the server keeps it if it looks like an id, so the id a browser
saw is the id in the logs. Every log line a request writes is inside a span
with that id, and every error envelope repeats it as `requestId`. A failure on
the server's side (5xx) shows the writer `(reference …)`, which is the thing to
ask them for.

`SCRIPTORIUM_LOG_FORMAT=json` writes one JSON object per line, with the
request's span (`request_id`, `method`, `path`) on each, for a log shipper to
index. Anything else writes text for a person. Each request ends with one
`served` line (or `failed`, at error level, for a 5xx) with its status and
milliseconds. A handler that panics answers with the usual JSON 500, and the
panic is logged inside the request's span.

## Status

`GET /api/status` is public, and answers `200` when every check passes and
`503` when any fails — point an uptime monitor at it. `/status` in the web
client shows the same checks to a person.

| Check | Passes when |
|---|---|
| database | it answers, and no migration is pending |
| manuscripts | the data folder takes a write and a rename — what a save does |
| backups | the newest backup is under 26 hours old **and its restore drill passed** |
| mail | always; says whether mail goes to the outbox or out through SMTP |

`/api/health` still exists and only says the process is up.

## Backups

```sh
SCRIPTORIUM_BACKUP_DIR=/var/backups/scriptorium cargo backup
# or, on a server without cargo:
SCRIPTORIUM_BACKUP_DIR=… SCRIPTORIUM_BIN=/usr/local/bin/scriptorium-backend scripts/backup.sh
```

One run takes a backup and then restores it to prove it. A backup is a
timestamped folder:

```
<backup dir>/20261005T220617Z/
  scriptorium.db   a consistent copy, made with VACUUM INTO while the server runs
  works/           the manuscript tree, file for file
  manifest.json    when, which version, row counts, and a SHA-256 of every file
  drill.json       what the restore drill found
```

The newest `SCRIPTORIUM_BACKUP_KEEP` (default 14) are kept.

The database is copied first, then the files. Scene files are only ever
replaced whole and snapshot files never change, so each file is consistent;
the one gap is a scene deleted in the seconds between the two steps, which the
drill reports as a missing file rather than letting it pass.

### The restore drill

`scriptorium-backend --restore-drill [<backup>]` (default: the newest) copies
the backup somewhere disposable and checks that it would serve:

- the database file matches its digest and passes `PRAGMA integrity_check`;
- this build's migrations apply to it;
- the row counts match the manifest;
- every manuscript file is present and matches its digest;
- every scene the database knows about — every draft and every scene in every
  snapshot — has its file.

It exits non-zero on any failure and writes `drill.json`, which is what the
status check reads. A backup that has never been drilled shows as failing:
a backup nobody has restored is a hope.

### Nightly

Nothing is installed for you. On Linux, with cron:

```
15 3 * * *  cd /srv/scriptorium && scripts/backup.sh >> /var/log/scriptorium-backup.log 2>&1
```

On macOS, a launchd job in `~/Library/LaunchAgents/app.scriptorium.backup.plist`:

```xml
<plist version="1.0"><dict>
  <key>Label</key><string>app.scriptorium.backup</string>
  <key>ProgramArguments</key>
  <array><string>/bin/bash</string><string>/path/to/scriptorium/scripts/backup.sh</string></array>
  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>3</integer><key>Minute</key><integer>15</integer></dict>
  <key>StandardOutPath</key><string>/tmp/scriptorium-backup.log</string>
  <key>StandardErrorPath</key><string>/tmp/scriptorium-backup.log</string>
</dict></plist>
```

The backup folder should be on a different disk from the data, and copied off
the machine; Litestream replication to object storage is *7.1*.

### Restoring for real

Stop the server. Move the current database and `works/` aside rather than
deleting them. Copy `scriptorium.db` from the backup to
`SCRIPTORIUM_DATABASE_URL` and `works/` to `SCRIPTORIUM_DATA_DIR`. Start the
server; it applies any newer migrations on boot. Check `/status`.

## Not built here

- **Hosted error tracking** (Sentry or similar) needs an account and a DSN.
  Until then, 5xx responses are logged at error level with their request id.
- **External uptime checks and a hosted status page** need a provider. The
  endpoint they would poll is `/api/status`.
