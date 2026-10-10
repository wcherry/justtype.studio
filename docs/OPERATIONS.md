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

`JUSTTYPE_LOG_FORMAT=json` writes one JSON object per line, with the
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
| jobs | the scheduled jobs are on time: the runner ticked in the last 5 minutes, and today's `open_prompt` and `build_digests` and tomorrow's `prepare_prompt` (from noon Pacific) have run. Which one is late is at `/api/admin/jobs`, for admins |
| mail | always; says whether mail goes to the outbox or out through SMTP |

`/api/health` still exists and only says the process is up.

## Backups

```sh
JUSTTYPE_BACKUP_DIR=/var/backups/justtype cargo backup
# or, on a server without cargo:
JUSTTYPE_BACKUP_DIR=… JUSTTYPE_BIN=/usr/local/bin/justtype-backend scripts/backup.sh
```

One run takes a backup and then restores it to prove it. A backup is a
timestamped folder:

```
<backup dir>/20261005T220617Z/
  justtype.db   a consistent copy, made with VACUUM INTO while the server runs
  works/           the manuscript tree, file for file
  manifest.json    when, which version, row counts, and a SHA-256 of every file
  drill.json       what the restore drill found
```

The newest `JUSTTYPE_BACKUP_KEEP` (default 14) are kept.

The database is copied first, then the files. Scene files are only ever
replaced whole and snapshot files never change, so each file is consistent;
the one gap is a scene deleted in the seconds between the two steps, which the
drill reports as a missing file rather than letting it pass.

### The restore drill

`justtype-backend --restore-drill [<backup>]` (default: the newest) copies
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
15 3 * * *  cd /srv/justtype && scripts/backup.sh >> /var/log/justtype-backup.log 2>&1
```

On macOS, a launchd job in `~/Library/LaunchAgents/app.justtype.backup.plist`:

```xml
<plist version="1.0"><dict>
  <key>Label</key><string>app.justtype.backup</string>
  <key>ProgramArguments</key>
  <array><string>/bin/bash</string><string>/path/to/justtype/scripts/backup.sh</string></array>
  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>3</integer><key>Minute</key><integer>15</integer></dict>
  <key>StandardOutPath</key><string>/tmp/justtype-backup.log</string>
  <key>StandardErrorPath</key><string>/tmp/justtype-backup.log</string>
</dict></plist>
```

The backup folder should be on a different disk from the data, and copied off
the machine; Litestream replication to object storage is *7.1*.

### Restoring for real

Stop the server. Move the current database and `works/` aside rather than
deleting them. Copy `justtype.db` from the backup to
`JUSTTYPE_DATABASE_URL` and `works/` to `JUSTTYPE_DATA_DIR`. Start the
server; it applies any newer migrations on boot. Check `/status`.

## Replication (Litestream)

Nightly backups lose up to a day. Litestream streams every change to the
SQLite file into object storage as it happens, so a dead disk loses about a
second, and the database can be restored to any moment in the retention
window. `deploy/litestream.yml` is the configuration; every value in it comes
from the environment.

**It has not been run.** It needs a bucket and credentials, which this
repository does not have. What follows is the procedure; the first time it is
pointed at a real bucket, the restore at the end is the check that it works.

```sh
export JUSTTYPE_DATABASE_URL=/srv/justtype/data/justtype.db
export LITESTREAM_BUCKET=… LITESTREAM_PATH=justtype LITESTREAM_REGION=…
export LITESTREAM_ACCESS_KEY_ID=… LITESTREAM_SECRET_ACCESS_KEY=…
# LITESTREAM_ENDPOINT=https://… for a store that is not AWS S3

# Run the server under Litestream, so replication starts before the first
# write and stops after the last:
litestream replicate -config deploy/litestream.yml -exec "justtype-backend"
```

Restoring, to the latest state or to a moment:

```sh
litestream restore -config deploy/litestream.yml -o /tmp/restored.db "$JUSTTYPE_DATABASE_URL"
litestream restore -config deploy/litestream.yml -timestamp 2026-10-05T14:00:00Z -o /tmp/restored.db "$JUSTTYPE_DATABASE_URL"
```

Then prove the restored file before trusting it: put it in a folder beside a
`works/` copy and a `manifest.json` from the latest nightly backup, and run
`justtype-backend --restore-drill <that folder>`, or open it with
`sqlite3 /tmp/restored.db 'PRAGMA integrity_check'` at the least.

Litestream covers the database only. The manuscript tree is files, and the
nightly backup is still what keeps them; the two together are the recovery
plan. Things that would break replication: replacing the database file under
a running server, or running `VACUUM` (not `VACUUM INTO`) against it. The
nightly backup uses `VACUUM INTO`, which only reads.

## The write path

The architecture rests on an assumption — a few writes a second, which one
SQLite file absorbs without noticing — and it is measured rather than
believed:

| Number | What it is |
|---|---|
| writes/s | requests that change something, and the database calls they made |
| write latency | each of those calls, waiting for a pooled connection included |
| lock wait | how long a writer waits for SQLite's write lock, **sampled**: every five seconds a probe takes the lock (`BEGIN IMMEDIATE`) and times it |
| busy | writes that gave up because the five-second busy timeout ran out |

Lock wait is sampled because SQLite does that waiting inside its busy
handler, which Diesel does not let the application hook. A probe that takes
the same lock sees the same wait a write would have at that moment.

The last minute and last fifteen minutes are on the `/status` page, and the
status check fails when the p95 lock wait reaches 250 ms — the point at which
writes are queueing, which the expected load should never cause. For a real
dashboard, set `JUSTTYPE_METRICS_TOKEN` (16+ characters) and scrape
`/api/metrics` with it as a bearer token; it is Prometheus text. Without the
token set, the endpoint does not exist.

If the numbers ever say SQLite is the bottleneck, the contingency is Postgres
behind Diesel (see `ROADMAP.md`, *7.1*). They are there so that decision is
made from a graph.

## Scheduled jobs

The server runs its own jobs: one task ticks every minute
(`JUSTTYPE_JOB_TICK_SECONDS`) and runs what is due. Each daily run is claimed
in `job_runs` under its day before it starts and marked when it ends, so a
restart, a missed tick or a second process cannot run it twice, and a run
missed while the server was down happens on the first tick back.

| Job | When | Does |
|---|---|---|
| `open_prompt` | from midnight Pacific | gives today a prompt if it has none: pinned, else queued, else written by the model, else the built-in list |
| `prepare_prompt` | from noon Pacific | gives tomorrow its prompt (pinned, queued, or the model's), so admins can see and change it; a model failure is retried after 5 minutes |
| `build_digests` | from midnight Pacific | yesterday's inbox summary for each writer with activity |
| prompt emails | every tick | queues today's prompt for each writer whose chosen time has come, once per day |
| mail delivery | every tick | sends `mail_queue`, retrying with backoff (1 minute, doubling, at most an hour) until the message's day is over |

Settings:

- `JUSTTYPE_PROMPT_TZ`: the prompt day's timezone (default `America/Los_Angeles`).
- `JUSTTYPE_APPLE_APP_ID`: `TEAMID.app.scriptorium.ios`, to serve the
  `apple-app-site-association` that lets the email's links open the iOS app.
- `JUSTTYPE_TEST_CLOCK=1`: **for the smoke walkthrough only.** Mounts
  `POST /api/test/clock {now}`, which moves the server's clock and runs a tick.
  Never set it on a server people use.

Going live with the email needs `JUSTTYPE_MAIL=smtp` (or `resend`, with
`JUSTTYPE_RESEND_API_KEY`) and a sending domain verified with the provider;
until then it goes to the outbox file like the rest of the mail.

## Site administrators

```sh
cargo xtask admin grant ada@example.com    # or: justtype-backend --admin grant …
cargo xtask admin revoke ada@example.com
```

Run against the same environment (or `.env`) as the server. Both are recorded
in the audit log. No request body can change who is an admin.

## Not built here

- **Hosted error tracking** (Sentry or similar) needs an account and a DSN.
  Until then, 5xx responses are logged at error level with their request id.
- **External uptime checks and a hosted status page** need a provider. The
  endpoint they would poll is `/api/status`.
