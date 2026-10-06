#!/usr/bin/env bash
# Takes a backup and then restores it to prove it, the way the nightly job
# does. Exits non-zero if either step fails, so whatever runs it (cron,
# launchd, a CI schedule) can raise the alarm.
#
#   scripts/backup.sh              back up, then drill the backup just taken
#   scripts/backup.sh --drill-only drill the newest backup again
#
# Reads the same .env as the server. JUSTTYPE_BACKUP_DIR must be set.
# JUSTTYPE_BIN picks the binary; otherwise the release build, then debug.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# .env fills in what the environment does not say, never the reverse — the
# same rule the server follows — so a variable set by the caller always wins.
if [[ -f .env ]]; then
  while IFS='=' read -r key value; do
    [[ "$key" =~ ^[A-Z_][A-Z0-9_]*$ ]] || continue
    [[ -z "${!key+set}" ]] && export "$key=$value"
  done < .env
fi
# Settings from before the rename (SCRIPTORIUM_*) count as their JUSTTYPE_*
# names when those are not set; the server does the same.
while IFS='=' read -r key value; do
  new="JUSTTYPE_${key#SCRIPTORIUM_}"
  [[ -z "${!new+set}" ]] && export "$new=$value"
done < <(env | grep '^SCRIPTORIUM_')
: "${JUSTTYPE_BACKUP_DIR:?set JUSTTYPE_BACKUP_DIR to say where backups go}"

BIN="${JUSTTYPE_BIN:-}"
if [[ -z "$BIN" ]]; then
  for candidate in target/release/justtype-backend target/debug/justtype-backend; do
    [[ -x "$candidate" ]] && BIN="$candidate" && break
  done
fi
[[ -n "$BIN" ]] || { echo "no backend binary — build one, or set JUSTTYPE_BIN" >&2; exit 1; }

if [[ "${1:-}" != "--drill-only" ]]; then
  TAKEN=$("$BIN" --backup)
  echo "backed up to $TAKEN"
  "$BIN" --restore-drill "$TAKEN"
else
  "$BIN" --restore-drill
fi
