#!/usr/bin/env bash
# End-to-end walkthrough of the API against a throwaway database.
# Starts its own server on a spare port, exercises the whole flow, tears down.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${SMOKE_PORT:-8099}"
API="http://127.0.0.1:${PORT}/api"
TMP="$(mktemp -d)"
SERVER_PID=""

cleanup() {
  [[ -n "$SERVER_PID" ]] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

# Pull one field out of a JSON response.
field() { python3 -c "import sys,json;d=json.load(sys.stdin)
for k in '$1'.split('.'): d = d[int(k)] if k.isdigit() else d[k]
print(d)"; }

step() { printf '\n\033[1m%s\033[0m\n' "$*"; }

step "Building"
cargo build -q -p scriptorium-backend --manifest-path "$ROOT/Cargo.toml"

step "Starting a server on port $PORT"
SCRIPTORIUM_BIND_ADDR="127.0.0.1:${PORT}" \
SCRIPTORIUM_DATABASE_URL="$TMP/scriptorium.db" \
SCRIPTORIUM_DATA_DIR="$TMP/works" \
SCRIPTORIUM_JWT_SECRET="smoke-test-secret-value-1234567890" \
RUST_LOG=warn \
  "$ROOT/target/debug/scriptorium-backend" >"$TMP/server.log" 2>&1 &
SERVER_PID=$!

for _ in {1..40}; do
  curl -sf "$API/health" >/dev/null 2>&1 && break
  sleep 0.25
done
curl -sf "$API/health" | field status

step "Registering an author"
TOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correcthorse","displayName":"Ada"}' | field accessToken)
AUTH="Authorization: Bearer $TOKEN"

step "Creating a novel from the three-act template"
WORK=$(curl -s -X POST "$API/works" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"title":"The Long Field","kind":"novel","templateId":"tpl-three-act"}')
WID=$(echo "$WORK" | field id)
SCENE=$(echo "$WORK" | field chapters.0.scenes.0.id)
echo "$WORK" | python3 -c "import sys,json;d=json.load(sys.stdin)
print(' ', ' / '.join(f\"{c['title']} ({len(c['scenes'])})\" for c in d['chapters']))"

step "Writing a scene"
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"The field went on past where the light gave out."}]}]}}' \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print(' ', d['scene']['wordCount'],'words, revision kept:', bool(d['revisionId']))"

step "Snapshotting, revising, snapshotting again"
V1=$(curl -s -X POST "$API/works/$WID/versions" -H "$AUTH" -H 'content-type: application/json' -d '{"label":"First pass"}' | field id)
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"The field went on well past where the light gave out, and kept going."}]}]}}' >/dev/null
V2=$(curl -s -X POST "$API/works/$WID/versions" -H "$AUTH" -H 'content-type: application/json' -d '{"label":"Second pass"}' | field id)

step "Diffing the two versions"
curl -s "$API/works/$WID/versions/diff?from=$V1&to=$V2" -H "$AUTH" | python3 -c "
import sys,json
d=json.load(sys.stdin)
s=d['summary']
print(f\"  +{s['wordsAdded']} / -{s['wordsRemoved']} words, {s['scenesModified']} scene(s) revised\")
for scene in d['scenes']:
    if scene['kind']=='modified':
        rendered=''.join({'insert':'[+%s]','delete':'[-%s]','equal':'%s'}[c['op']] % c['value'] for c in scene['changes'])
        print('  ', scene['sceneTitle'], '->', rendered)
        break"

step "Publishing and offering it for review"
curl -s -X POST "$API/versions/$V2/publish" -H "$AUTH" | field state
PUB=$(curl -s -X POST "$API/versions/$V2/publications" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"visibility":"platform","scope":"work","reviewPrompt":"Does the opening hold?"}')
PID=$(echo "$PUB" | field id)
echo "  excerpt: $(echo "$PUB" | field excerpt)"

step "A second writer reviews it"
RTOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"grace@example.com","password":"correcthorse","displayName":"Grace"}' | field accessToken)
RAUTH="Authorization: Bearer $RTOKEN"
curl -s "$API/publications" -H "$RAUTH" | python3 -c "import sys,json;print('  feed:',[(c['workTitle'],c['author']['displayName']) for c in json.load(sys.stdin)])"

CID=$(curl -s -X POST "$API/publications/$PID/comments" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"body\":\"This line lands.\",\"sceneId\":\"$SCENE\",\"anchor\":{\"from\":1,\"to\":18},\"anchorQuote\":\"The field went on\",\"category\":\"prose\"}" | field id)
curl -s -X POST "$API/publications/$PID/comments" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"body\":\"Agreed, though 'kept going' is doing a lot of work.\",\"parentId\":\"$CID\"}" >/dev/null
curl -s -X POST "$API/comments/$CID/vote" -H "$AUTH" -H 'content-type: application/json' -d '{"value":1}' >/dev/null

step "Threaded feedback"
curl -s "$API/publications/$PID/comments" -H "$AUTH" | python3 -c "
import sys,json
for c in json.load(sys.stdin):
    print(f\"  • {c['author']['displayName']} (score {c['score']}) on '{c['anchorQuote']}': {c['body']}\")
    for r in c['replies']:
        print(f\"      ↳ {r['author']['displayName']}: {r['body']}\")"

printf '\n\033[32mAll good.\033[0m\n'
