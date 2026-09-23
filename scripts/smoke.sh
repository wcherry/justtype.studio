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

step "Refusing a save whose base has moved"
# Two writers on one scene: the second one's base hash is stale, so the save is
# refused rather than quietly overwriting the first.
STALE="$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)"
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"content\":{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"Someone else got here first.\"}]}]},\"baseHash\":\"$STALE\"}" >/dev/null
CONFLICT=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"content\":{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"And I clobbered them.\"}]}]},\"baseHash\":\"$STALE\"}")
[ "$CONFLICT" = "409" ] && echo "  stale save rejected with 409" || { echo "  EXPECTED 409, got $CONFLICT"; exit 1; }
# Re-sending with the hash the 409 reported is how a client says "I have seen
# theirs and still mean this".
CURRENT="$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)"
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"content\":{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"The field went on past where the light gave out.\"}]}]},\"baseHash\":\"$CURRENT\"}" >/dev/null
echo "  resolved save accepted"

step "Restoring an earlier revision"
# Pick a revision whose text actually differs from what is on screen now, so the
# restore has something to do. Newest first.
pick_revision() { curl -s "$API/scenes/$SCENE/revisions" -H "$AUTH" \
  | python3 -c "import sys,json
revs=json.load(sys.stdin)
print(next(r['id'] for r in revs if r['wordCount']==int('$1')))"; }

REV_OTHER="$(pick_revision 5)"
curl -s -X POST "$API/scenes/$SCENE/revisions/$REV_OTHER/restore" -H "$AUTH" \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print('  rolled back to:', d['scene']['wordCount'], 'words, superseded text kept as a revision:', bool(d['revisionId']))"

# ... and back again, which is the point of writing a revision on the way in:
# a restore is itself undoable, and the history is never rewritten.
REV_BACK="$(pick_revision 10)"
curl -s -X POST "$API/scenes/$SCENE/revisions/$REV_BACK/restore" -H "$AUTH" \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print('  and forward again:', d['scene']['wordCount'], 'words')"

step "Daily goal and streak"
TODAY=$(date +%F)
curl -s -X PATCH "$API/me/goals" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"dailyWordGoal":50}' | field dailyWordGoal | sed 's/^/  daily goal: /'
# The save above carried no localDay, so nothing is counted yet; one that does
# gets recorded against the writer's own day.
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"content\":{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"The field went on past where the light gave out, and she went on with it.\"}]}]},\"localDay\":\"$TODAY\"}" >/dev/null
curl -s "$API/me/writing?today=$TODAY" -H "$AUTH" | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(f\"  today: {d['wordsToday']} words over {d['savesToday']} save(s), goal {d['dailyWordGoal']}, streak {d['streak']}\")"

step "Splitting a scene, then merging it back"
# The property that matters: prose must survive the round trip exactly.
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"First paragraph, before the cut."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Second paragraph, after it."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Third, also after."}]}]}}' >/dev/null
BEFORE_TEXT=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
print(text(json.load(sys.stdin)['content']))")
HASH=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)
SPLIT=$(curl -s -X POST "$API/scenes/$SCENE/split" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"atBlock\":1,\"baseHash\":\"$HASH\"}")
SECOND=$(echo "$SPLIT" | field second.id)
echo "  split: $(echo "$SPLIT" | field first.wordCount) + $(echo "$SPLIT" | field second.wordCount) words, new scene \"$(echo "$SPLIT" | field second.title)\""

# A stale hash must be refused — splitting rewrites prose in two places.
STALE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/scenes/$SCENE/split" -H "$AUTH" \
  -H 'content-type: application/json' -d "{\"atBlock\":1,\"baseHash\":\"$HASH\"}")
[ "$STALE" = "409" ] && echo "  stale split rejected with 409" || { echo "  EXPECTED 409, got $STALE"; exit 1; }

H1=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)
H2=$(curl -s "$API/scenes/$SECOND" -H "$AUTH" | field contentHash)
curl -s -X POST "$API/scenes/$SCENE/merge" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"with\":\"$SECOND\",\"baseHash\":\"$H1\",\"withBaseHash\":\"$H2\"}" >/dev/null
AFTER_TEXT=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
print(text(json.load(sys.stdin)['content']))")
[ "$BEFORE_TEXT" = "$AFTER_TEXT" ] && echo "  merged back, text identical" || { echo "  TEXT CHANGED:"; echo "   before: $BEFORE_TEXT"; echo "   after:  $AFTER_TEXT"; exit 1; }

step "Tracked changes, applied"
# Twenty numbered words, and twenty proposals to renumber them. Accepting all
# of them must produce exactly what the proposals asked for — which only works
# if applying one does not disturb where the others point.
python3 - > "$TMP/twenty.json" <<'PYEOF'
import json
words = " ".join(f"word{i}" for i in range(1, 21))
print(json.dumps({"content": {"type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text": words}]}]}}))
PYEOF
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' -d @"$TMP/twenty.json" >/dev/null
HASH=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)

python3 - "$API" "$SCENE" "$TOKEN" "$HASH" <<'PYEOF'
import json, subprocess, sys
api, scene, token, base = sys.argv[1:5]
# Positions are ProseMirror's: the paragraph opens at 0, so its text starts at 1.
offset = 1
for i in range(1, 21):
    word = f"word{i}"
    body = {
        "kind": "replace",
        "anchorFrom": offset,
        "anchorTo": offset + len(word),
        "quote": word,
        "payload": f"WORD{i}",
        "note": f"renumber {i}",
        "baseHash": base,
    }
    subprocess.run(["curl", "-s", "-o", "/dev/null", "-X", "POST",
                    f"{api}/scenes/{scene}/changes",
                    "-H", f"Authorization: Bearer {token}",
                    "-H", "content-type: application/json",
                    "-d", json.dumps(body)], check=True)
    offset += len(word) + 1
print("  proposed 20 changes")
PYEOF

curl -s -X POST "$API/scenes/$SCENE/changes/resolve" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"action":"accept"}' | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f\"  accepted {d['applied']}, stale {len(d['stale'])}\")
for s in d['stale']:
    print('   stale:', s['reason'])"

RESULT=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
print(text(json.load(sys.stdin)['content']))")
EXPECTED=$(python3 -c "print(' '.join(f'WORD{i}' for i in range(1,21)))")
[ "$RESULT" = "$EXPECTED" ] && echo "  document is exactly what the proposals asked for" || {
  echo "  MISMATCH"; echo "   got:      $RESULT"; echo "   expected: $EXPECTED"; exit 1; }

# A proposal whose text has moved on must be refused, not guessed at.
H=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)
curl -s -X POST "$API/scenes/$SCENE/changes" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"kind\":\"replace\",\"anchorFrom\":1,\"anchorTo\":6,\"quote\":\"vanished\",\"payload\":\"x\",\"baseHash\":\"$H\"}" >/dev/null
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Entirely different text now."}]}]}}' >/dev/null
curl -s -X POST "$API/scenes/$SCENE/changes/resolve" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"action":"accept"}' | python3 -c "
import sys, json
d = json.load(sys.stdin)
print(f\"  after the scene moved on: applied {d['applied']}, left pending {len(d['stale'])}\")
for s in d['stale']: print('   ', s['reason'])"

step "Story bible"
ENTRY=$(curl -s -X POST "$API/works/$WID/bible" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"kind":"character","name":"Ada","summary":"Walks into the dark on purpose."}')
EID=$(echo "$ENTRY" | field id)
curl -s -X POST "$API/bible/$EID/links" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"sceneId\":\"$SCENE\"}" >/dev/null
# Linking twice is what someone does when unsure whether they already did.
curl -s -X POST "$API/bible/$EID/links" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"sceneId\":\"$SCENE\"}" >/dev/null
curl -s "$API/works/$WID/bible" -H "$AUTH" | python3 -c "
import sys,json
for e in json.load(sys.stdin):
    print(f\"  {e['kind']}: {e['name']} — in {len(e['sceneIds'])} scene(s)\")"

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
print(f\"  {d['from']} -> {d['to']}: +{s['wordsAdded']} / -{s['wordsRemoved']} words, {s['scenesModified']} scene(s), {s['paragraphsChanged']} paragraph(s)\")
for scene in d['scenes']:
    if scene['kind'] != 'modified': continue
    for b in scene.get('blocks', []):
        if b['kind'] == 'unchanged': continue
        if b['kind'] == 'modified':
            rendered=''.join({'insert':'[+%s]','delete':'[-%s]','equal':'%s'}[c['op']] % c['value'] for c in b['changes'])
            print('   modified:', rendered[:110])
        else:
            print(f\"   {b['kind']}: {(b['after'] or b['before'])[:70]}\")
    break"

step "Paragraph alignment and moves"
# Three paragraphs, then the same three reordered with one edited: alignment
# should report one move and one modification, not a wall of word changes.
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"Alpha paragraph, entirely unchanged throughout."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Beta paragraph, which will be edited slightly later on."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Gamma paragraph, which will be moved to the top."}]}]}}' >/dev/null
VA=$(curl -s -X POST "$API/works/$WID/versions" -H "$AUTH" -H 'content-type: application/json' -d '{"label":"Before"}' | field id)
curl -s -X POST "$API/versions/$VA/publish" -H "$AUTH" >/dev/null
curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"Gamma paragraph, which will be moved to the top."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Alpha paragraph, entirely unchanged throughout."}]},
    {"type":"paragraph","content":[{"type":"text","text":"Beta paragraph, which was edited considerably later on."}]}]}}' >/dev/null

# ... and compared against the working draft, not another snapshot.
curl -s "$API/works/$WID/versions/diff?from=$VA&to=working" -H "$AUTH" | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(f\"  {d['from']} -> {d['to']}\")
for scene in d['scenes']:
    for b in scene.get('blocks', []):
        if b['kind'] == 'unchanged': continue
        print(f\"   {b['kind']}: {(b['after'] or b['before'])[:60]}\")"

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

step "Exporting the published version"
for fmt in txt docx epub pdf; do
  out="$TMP/export.$fmt"
  code=$(curl -s -o "$out" -w '%{http_code}' "$API/versions/$V2/export?format=$fmt&style=manuscript" -H "$AUTH")
  [ "$code" = "200" ] || { echo "  $fmt: HTTP $code"; exit 1; }
  printf '  %-5s %7s bytes  %s\n' "$fmt" "$(wc -c < "$out" | tr -d ' ')" "$(python3 "$ROOT/scripts/check_export.py" "$out")"
done
# A style the server does not know has to be refused, not quietly ignored.
bad=$(curl -s -o /dev/null -w '%{http_code}' "$API/versions/$V2/export?format=docx&style=fancy" -H "$AUTH")
[ "$bad" = "400" ] && echo "  unknown style rejected with 400" || { echo "  EXPECTED 400, got $bad"; exit 1; }

printf '\n\033[32mAll good.\033[0m\n'
