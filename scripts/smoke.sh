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

step "Naming, tagging and pinning a snapshot"
curl -s -X PATCH "$API/versions/$VA" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"label":"Before the agent read it","notes":"What went out in March."}' \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print(f\"  renamed to '{d['label']}'\")"
curl -s -X POST "$API/versions/$VA/tags" -H "$AUTH" -H 'content-type: application/json' -d '{"tag":"Submitted"}' >/dev/null
curl -s -X POST "$API/versions/$VA/tags" -H "$AUTH" -H 'content-type: application/json' -d '{"tag":"submitted"}' >/dev/null
curl -s -X POST "$API/versions/$VA/tags" -H "$AUTH" -H 'content-type: application/json' -d '{"tag":"march"}' >/dev/null
curl -s "$API/versions/$VA" -H "$AUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print('  tags:', d['tags'], '(case-folded, so tagging twice is once)')"
curl -s -X POST "$API/versions/$VA/pin" -H "$AUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin);print('  pinned:', d['pinnedVersionId'] is not None)"

step "Snapshotting one chapter rather than the whole book"
CH=$(curl -s "$API/works/$WID/outline" -H "$AUTH" | field chapters.0.id)
curl -s -X POST "$API/works/$WID/versions" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"label\":\"Chapter one only\",\"scope\":\"chapter\",\"scopeRefId\":\"$CH\"}" \
  | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  {d['label']}: scope={d['scope']}, {len(d['entries'])} scene(s)\")"
BAD=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/works/$WID/versions" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"scope":"chapter"}')
[ "$BAD" = "400" ] && echo "  a scoped snapshot without a target is refused" || { echo "  EXPECTED 400, got $BAD"; exit 1; }

step "Restoring a version, without losing the draft it replaces"
# The working copy says one thing; the snapshot says another.
DRAFT_TEXT=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
print(text(json.load(sys.stdin)['content'])[:60])")
RESTORE=$(curl -s -X POST "$API/versions/$VA/restore" -H "$AUTH")
echo "$RESTORE" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  restored {d['scenesRestored']} scene(s), removed {d['scenesRemoved']}\")
print(f\"  safety snapshot: '{d['safetyVersion']['label']}'\")"
AFTER_TEXT=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
print(text(json.load(sys.stdin)['content'])[:60])")
[ "$DRAFT_TEXT" != "$AFTER_TEXT" ] && echo "  working copy now holds the restored text" || { echo "  RESTORE DID NOTHING"; exit 1; }

# The draft that was replaced has to still be reachable.
SAFETY=$(echo "$RESTORE" | field safetyVersion.id)
SAFE_TEXT=$(curl -s "$API/versions/$SAFETY/content" -H "$AUTH" | python3 -c "
import sys,json
def text(n): return ''.join(c['text'] if c.get('type')=='text' else text(c) for c in n.get('content',[]))
d=json.load(sys.stdin)
for ch in d['chapters']:
    for sc in ch['scenes']:
        t = text(sc['content'])
        if t: print(t[:60]); break
    else: continue
    break")
[ "$SAFE_TEXT" = "$DRAFT_TEXT" ] && echo "  the replaced draft is safe in the safety snapshot" || {
  echo "  DRAFT LOST"; echo "   was:  $DRAFT_TEXT"; echo "   safe: $SAFE_TEXT"; exit 1; }

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

step "Invited readers only"
# A third writer nobody invited, to check the door is actually locked.
STOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"stranger@example.com","password":"correcthorse","displayName":"Stranger"}' | field accessToken)
SAUTH="Authorization: Bearer $STOKEN"

INV=$(curl -s -X POST "$API/versions/$V2/publications" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"visibility":"invited","scope":"work","reviewPrompt":"Just for the circle.","maxReviewers":2}' | field id)
curl -s -X POST "$API/publications/$INV/invites" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"emails":["Grace@Example.com","grace@example.com"]}' | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  invited {len(d)} reader(s) — the duplicate and the capitalisation collapsed\")"

CODE=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$INV" -H "$SAUTH")
[ "$CODE" = "403" ] && echo "  an uninvited reader is refused (403)" || { echo "  EXPECTED 403, got $CODE"; exit 1; }
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$INV" -H "$RAUTH")
[ "$CODE" = "200" ] && echo "  the invited reader gets in" || { echo "  EXPECTED 200, got $CODE"; exit 1; }

step "Review windows"
# A window that closed yesterday is shut, even to someone who was invited.
PAST=$(python3 -c "import datetime;print((datetime.datetime.now(datetime.UTC)-datetime.timedelta(days=1)).isoformat().replace('+00:00','Z'))")
curl -s -X PATCH "$API/publications/$INV" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"closesAt\":\"$PAST\"}" >/dev/null
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$INV" -H "$RAUTH")
[ "$CODE" = "403" ] && echo "  a closed window shuts out an invited reader too" || { echo "  EXPECTED 403, got $CODE"; exit 1; }
CODE=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$INV" -H "$AUTH")
[ "$CODE" = "200" ] && echo "  the author can still see their own" || { echo "  EXPECTED 200, got $CODE"; exit 1; }
FUTURE=$(python3 -c "import datetime;print((datetime.datetime.now(datetime.UTC)+datetime.timedelta(days=7)).isoformat().replace('+00:00','Z'))")
curl -s -X PATCH "$API/publications/$INV" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"closesAt\":\"$FUTURE\"}" >/dev/null

step "Beta-reader circles"
CIRCLE=$(curl -s -X POST "$API/circles" -H "$AUTH" -H 'content-type: application/json' -d '{"name":"First readers"}' | field id)
for who in kay@example.com sam@example.com; do
  curl -s -X POST "$API/circles/$CIRCLE/members" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"email\":\"$who\",\"name\":\"${who%%@*}\"}" >/dev/null
done
curl -s -X POST "$API/publications/$INV/invites" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"circleId\":\"$CIRCLE\"}" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print('  after inviting the circle:', ', '.join(sorted(i['email'] for i in d)))"

step "Reader progress"
SCENES=$(curl -s "$API/publications/$INV/content" -H "$RAUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(' '.join(s['sceneId'] for ch in d['chapters'] for s in ch['scenes']))")
FIRST=$(echo $SCENES | cut -d' ' -f1)
SECOND=$(echo $SCENES | cut -d' ' -f2)
curl -s -X PUT "$API/publications/$INV/progress" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"sceneId\":\"$SECOND\"}" | python3 -c "
import sys,json;d=json.load(sys.stdin);print(f\"  {d['reader']['displayName']} reached {d['percent']}%\")"
# Going back does not move the furthest mark: giving up in chapter three and
# later skimming the ending still means they stopped in chapter three.
curl -s -X PUT "$API/publications/$INV/progress" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"sceneId\":\"$FIRST\"}" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  after turning back: now on scene 1, furthest still {d['percent']}%\")"
curl -s "$API/publications/$INV/progress" -H "$AUTH" | python3 -c "
import sys,json
for r in json.load(sys.stdin):
    print(f\"  author sees: {r['reader']['displayName']} — {r['percent']}% read\")"

step "Reviewer cap"
# Beta readers are scarce; twenty overlapping opinions are worse than five
# considered ones. Grace has already commented on $PID, so a cap of one is full.
curl -s -X PATCH "$API/publications/$PID" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"maxReviewers":1}' >/dev/null
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/publications/$PID/comments" -H "$SAUTH" \
  -H 'content-type: application/json' -d '{"body":"Third opinion nobody asked for."}')
[ "$CODE" = "403" ] && echo "  a new voice is turned away once the cap is full" || { echo "  EXPECTED 403, got $CODE"; exit 1; }
# But someone already inside may say more than one thing.
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/publications/$PID/comments" -H "$RAUTH" \
  -H 'content-type: application/json' -d '{"body":"One more thought, from someone already here."}')
[ "$CODE" = "200" ] && echo "  a reader already inside may still speak" || { echo "  EXPECTED 200, got $CODE"; exit 1; }
curl -s -X PATCH "$API/publications/$PID" -H "$AUTH" -H 'content-type: application/json' -d '{"maxReviewers":0}' >/dev/null

step "What the feed shows"
# There is no mail going out yet, so an invited reader who cannot see the draft
# in their feed cannot reach it at all without the author pasting a link.
titles() { curl -s "$API/publications" -H "$1" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
print(', '.join(r['workTitle']+' ('+r['visibility']+')' for r in rows) or 'nothing')"; }
echo "  the invited reader sees:  $(titles "$RAUTH")"
echo "  an uninvited writer sees: $(titles "$SAUTH")"
curl -s "$API/publications" -H "$SAUTH" | python3 -c "
import sys,json
assert all(r['visibility'] != 'invited' for r in json.load(sys.stdin)), 'an invited draft leaked into the open feed'"
# A window that refuses the click should not be sitting in the feed inviting it.
curl -s -X PATCH "$API/publications/$INV" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"closesAt\":\"$PAST\"}" >/dev/null
echo "  once the window closes:   $(titles "$RAUTH")"
echo "  the author still sees:    $(titles "$AUTH")"
curl -s -X PATCH "$API/publications/$INV" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"closesAt\":\"$FUTURE\"}" >/dev/null

step "One-tap reactions"
# A reader who will not write a paragraph about your paragraph will still tell
# you, in one tap, where they got lost.
react() {
  curl -s -X PUT "$API/publications/$PID/reactions" -H "$1" -H 'content-type: application/json' \
    -d "{\"sceneId\":\"$SCENE\",\"blockIndex\":$2,\"kind\":\"$3\"}"
}
react "$RAUTH" 0 loved >/dev/null
react "$SAUTH" 0 confusing >/dev/null
curl -s "$API/publications/$PID/reactions" -H "$AUTH" | python3 -c "
import sys,json
for b in json.load(sys.stdin):
    tally = ', '.join(f\"{c['count']} {c['kind']}\" for c in b['counts'])
    print(f\"  paragraph {b['blockIndex']} — {tally}: {b['quote'][:40]}\")"
# A paragraph that is not there is not a signal.
GONE=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$API/publications/$PID/reactions" -H "$SAUTH" \
  -H 'content-type: application/json' -d "{\"sceneId\":\"$SCENE\",\"blockIndex\":99,\"kind\":\"dragging\"}")
[ "$GONE" = "400" ] && echo "  a reaction on a paragraph that is not there is refused" || { echo "  EXPECTED 400, got $GONE"; exit 1; }
# The same face twice takes it back.
LEFT=$(react "$SAUTH" 0 confusing | python3 -c "import sys,json;print(json.load(sys.stdin)['mine'])")
[ "$LEFT" = "None" ] && echo "  tapping the same face again clears it" || { echo "  EXPECTED cleared, got $LEFT"; exit 1; }
# A different face replaces rather than piles up: the tally counts people.
react "$RAUTH" 0 confusing >/dev/null
MINE=$(curl -s "$API/publications/$PID/reactions" -H "$RAUTH" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
assert all(not r['counts'] for r in rows), 'a reader was shown the totals'
print(' '.join(f\"{r['blockIndex']}:{r['mine']}\" for r in rows))")
echo "  a reader sees only their own marks: $MINE"

step "The author's own questions"
Q1=$(curl -s -X POST "$API/publications/$PID/questions" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"prompt":"Did this chapter drag?","kind":"rating","scope":"chapter"}' | field id)
Q2=$(curl -s -X POST "$API/publications/$PID/questions" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"prompt":"Who did you think the villain was?","kind":"text","scope":"work"}' | field id)
CH1=$(curl -s "$API/publications/$PID/content" -H "$RAUTH" | field chapters.0.chapterId)
curl -s -X PUT "$API/questions/$Q1/answers" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"chapterId\":\"$CH1\",\"rating\":2}" >/dev/null
curl -s -X PUT "$API/questions/$Q2/answers" -H "$RAUTH" -H 'content-type: application/json' \
  -d '{"body":"The house."}' >/dev/null
# Answering twice corrects the answer rather than adding a second one.
curl -s -X PUT "$API/questions/$Q1/answers" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"chapterId\":\"$CH1\",\"rating\":4}" >/dev/null
curl -s "$API/publications/$PID/answers" -H "$AUTH" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
assert len(rows) == 2, f'expected 2 answers, got {len(rows)}'
for a in rows:
    said = a['rating'] if a['rating'] is not None else a['body']
    print(f\"  {a['reader']['displayName']}: {said}\")"
BAD=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$API/questions/$Q1/answers" -H "$RAUTH" \
  -H 'content-type: application/json' -d "{\"chapterId\":\"$CH1\",\"rating\":9}")
[ "$BAD" = "400" ] && echo "  a rating outside 1-5 is refused" || { echo "  EXPECTED 400, got $BAD"; exit 1; }
MINE=$(curl -s "$API/publications/$PID/answers" -H "$SAUTH" | python3 -c "
import sys,json;print(len(json.load(sys.stdin)))")
[ "$MINE" = "0" ] && echo "  a reader does not see anyone else's answers" || { echo "  EXPECTED 0, got $MINE"; exit 1; }

step "Reviewer standing"
# Votes say the other readers agreed; a resolve says the author acted.
GRACE=$(curl -s "$API/publications/$PID/comments" -H "$AUTH" | field 0.author.id)
curl -s -X PATCH "$API/comments/$CID" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"state":"resolved"}' >/dev/null
curl -s "$API/users/$GRACE/standing" -H "$AUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  {d['comments']} notes, {d['votes']} net votes, {d['resolved']} acted on — standing {d['score']}\")
assert d['score'] == d['votes'] + 3 * d['resolved']"

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
