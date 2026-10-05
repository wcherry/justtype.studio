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

# The newest link of one kind mailed to an address, read back from the outbox
# the server writes instead of sending mail.
link_token() { python3 -c "
import json,re,sys
rows=[json.loads(l) for l in open('$TMP/outbox.jsonl')]
rows=[r for r in rows if r['to']=='$1' and '/$2?token=' in r['body']]
print(re.search(r'token=([0-9a-f]+)', rows[-1]['body']).group(1))"; }
# Proves an address the way a person would: by following the mailed link.
confirm() { curl -s -X POST "$API/auth/verify" -H 'content-type: application/json' \
  -d "{\"token\":\"$(link_token "$1" verify)\"}" >/dev/null; }

step "Building"
cargo build -q -p scriptorium-backend --manifest-path "$ROOT/Cargo.toml"

step "Starting a server on port $PORT"
SCRIPTORIUM_BIND_ADDR="127.0.0.1:${PORT}" \
SCRIPTORIUM_DATABASE_URL="$TMP/scriptorium.db" \
SCRIPTORIUM_DATA_DIR="$TMP/works" \
SCRIPTORIUM_JWT_SECRET="smoke-test-secret-value-1234567890" \
SCRIPTORIUM_VOTE_RATE=3 \
SCRIPTORIUM_SECOND_FACTOR_RATE=20 \
SCRIPTORIUM_MAIL=outbox \
SCRIPTORIUM_BACKUP_DIR="$TMP/backups" \
SCRIPTORIUM_OUTBOX="$TMP/outbox.jsonl" \
SCRIPTORIUM_PUBLIC_URL="http://scriptorium.test" \
RUST_LOG=warn,scriptorium_backend::ops=info \
  "$ROOT/target/debug/scriptorium-backend" >"$TMP/server.log" 2>&1 &
SERVER_PID=$!

for _ in {1..40}; do
  curl -sf "$API/health" >/dev/null 2>&1 && break
  sleep 0.25
done
curl -sf "$API/health" | field status

step "Registering an author"
TOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correcthorse","displayName":"Ada","acceptTerms":true}' | field accessToken)
confirm ada@example.com
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
  -d '{"email":"grace@example.com","password":"correcthorse","displayName":"Grace","acceptTerms":true}' | field accessToken)
confirm grace@example.com
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
  -d '{"email":"stranger@example.com","password":"correcthorse","displayName":"Stranger","acceptTerms":true}' | field accessToken)
confirm stranger@example.com
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

step "What changed since they read it"
# The question an author actually has in front of a pile of feedback is not
# "what did I change" but "which of these have I already dealt with".
#
# Grace's note is anchored to the opening line. Rewrite that paragraph in the
# working draft and it should come back as answered, while a note on a
# paragraph nobody touched still stands.
READ_TEXT="The field went on well past where the light gave out, and kept going."
rewrite() {
  HASH=$(curl -s "$API/scenes/$SCENE" -H "$AUTH" | field contentHash)
  curl -s -X PUT "$API/scenes/$SCENE/content" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"baseHash\":\"$HASH\",\"content\":{\"type\":\"doc\",\"content\":[
      {\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"$1\"}]},
      {\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"A paragraph nobody has said anything about.\"}]}]}}" >/dev/null
}
status_of() { curl -s "$API/publications/$PID/since" -H "$AUTH" | python3 -c "
import sys,json
for n in json.load(sys.stdin)['notes']:
    if n['quote']: print(n['status']); break"; }

# Writing around a paragraph is not writing it: the note still stands.
rewrite "$READ_TEXT"
STILL=$(status_of)
[ "$STILL" = "untouched" ] && echo "  a note on a paragraph you wrote around still stands" \
  || { echo "  EXPECTED untouched, got $STILL"; exit 1; }

rewrite "The field stopped well short of the light, and so did she."
curl -s "$API/publications/$PID/since" -H "$AUTH" | python3 -c "
import sys,json
d=json.load(sys.stdin)
c=d['counts']
print(f\"  against the {d['to'].lower()}: {d['summary']['scenesModified']} scene(s), {d['summary']['paragraphsChanged']} paragraph(s)\")
print(f\"  notes: {c['untouched']} still stand, {c['edited']} answered, {c['gone']} on text that has gone, {c['unanchored']} general\")
for n in d['notes']:
    if n['status'] != 'edited': continue
    rendered=''.join({'insert':'[+%s]','delete':'[-%s]','equal':'%s'}[ch['op']] % ch['value'] for ch in n.get('changes', []))
    print(f\"   {n['author']['displayName']} on '{n['quote']}' -> {rendered[:80]}\")
assert c['edited'] >= 1, 'a note on rewritten text should read as answered'
"
# A reviewer has no business seeing the working draft, or everyone's names.
PEEK=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$PID/since" -H "$RAUTH")
[ "$PEEK" = "403" ] && echo "  a reviewer cannot look at the author's draft this way" || { echo "  EXPECTED 403, got $PEEK"; exit 1; }

step "Sorting through the notes"
count() { curl -s "$API/publications/$PID/comments?$1" -H "$AUTH" | python3 -c "
import sys,json;print(len(json.load(sys.stdin)))"; }
echo "  everything: $(count '') · prose only: $(count 'category=prose') · from Grace: $(count "authorId=$GRACE")"
UNVOTED=$(count 'minScore=1')
echo "  worth at least one vote: $UNVOTED"
NONE=$(count 'category=typo')
[ "$NONE" = "0" ] && echo "  a category nobody used comes back empty" || { echo "  EXPECTED 0, got $NONE"; exit 1; }

step "Resolving a pile at once"
# A bulk call with nothing to narrow it would resolve the whole review.
WILD=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/publications/$PID/comments/state" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"state":"resolved"}')
[ "$WILD" = "400" ] && echo "  a bulk resolve with no filter is refused" || { echo "  EXPECTED 400, got $WILD"; exit 1; }
curl -s -X POST "$API/publications/$PID/comments/state" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"state\":\"resolved\",\"authorId\":\"$GRACE\"}" | python3 -c "
import sys,json;print('  resolved', json.load(sys.stdin)['changed'], \"of Grace's notes at once\")"
curl -s -X POST "$API/publications/$PID/comments/state" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"state\":\"open\",\"commentIds\":[\"$CID\"]}" >/dev/null

step "Turning a note into work"
TASK=$(curl -s -X POST "$API/comments/$CID/task" -H "$AUTH")
TID=$(echo "$TASK" | field id)
echo "$TASK" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(f\"  from {d['source']['author']['displayName']}: {d['body'][:48]}\")"
AGAIN=$(curl -s -X POST "$API/comments/$CID/task" -H "$AUTH" | field id)
[ "$AGAIN" = "$TID" ] && echo "  the same note twice is the same task" || { echo "  DUPLICATED"; exit 1; }
curl -s -X POST "$API/works/$WID/tasks" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"body":"Chapter two needs to be colder."}' >/dev/null
curl -s -X PATCH "$API/tasks/$TID" -H "$AUTH" -H 'content-type: application/json' -d '{"state":"done"}' >/dev/null
curl -s "$API/works/$WID/tasks?state=all" -H "$AUTH" | python3 -c "
import sys,json
for t in json.load(sys.stdin):
    print(f\"  [{'x' if t['state'] == 'done' else ' '}] {t['body'][:56]}\")"

step "The review as a document"
for fmt in txt docx; do
  out="$TMP/feedback.$fmt"
  code=$(curl -s -o "$out" -w '%{http_code}' "$API/publications/$PID/feedback/export?format=$fmt" -H "$AUTH")
  [ "$code" = "200" ] || { echo "  $fmt: HTTP $code"; exit 1; }
  printf '  %-4s %7s bytes  %s\n' "$fmt" "$(wc -c < "$out" | tr -d ' ')" "$(python3 "$ROOT/scripts/check_export.py" "$out")"
done
grep -q "This line lands" "$TMP/feedback.txt" && echo "  the notes are in it" || { echo "  NOTES MISSING"; exit 1; }
MINE=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$PID/feedback/export?format=txt" -H "$RAUTH")
[ "$MINE" = "403" ] && echo "  a reviewer cannot export everyone else's feedback" || { echo "  EXPECTED 403, got $MINE"; exit 1; }

step "Reporting a comment"
curl -s -X POST "$API/comments/$CID/report" -H "$SAUTH" -H 'content-type: application/json' \
  -d '{"reason":"off_topic","detail":"Not about the writing."}' >/dev/null
# Twice is the same report, not a louder one.
curl -s -X POST "$API/comments/$CID/report" -H "$SAUTH" -H 'content-type: application/json' \
  -d '{"reason":"spam"}' >/dev/null
curl -s "$API/publications/$PID/reports" -H "$AUTH" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
assert len(rows) == 1, f'expected one report, got {len(rows)}'
for r in rows:
    print(f\"  {r['reporter']['displayName']} flagged {r['commentAuthor']['displayName']}: {r['reason']}\")"
OWN=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/comments/$CID/report" -H "$RAUTH" \
  -H 'content-type: application/json' -d '{"reason":"spam"}')
[ "$OWN" = "400" ] && echo "  reporting your own comment is refused" || { echo "  EXPECTED 400, got $OWN"; exit 1; }

step "Spoiler-safe serials"
SCENES2=$(curl -s "$API/publications/$PID/content" -H "$AUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
print(' '.join(s['sceneId'] for ch in d['chapters'] for s in ch['scenes']))")
LATER=$(echo $SCENES2 | cut -d' ' -f3)
curl -s -X POST "$API/publications/$PID/comments" -H "$RAUTH" -H 'content-type: application/json' \
  -d "{\"body\":\"The ending gives it away.\",\"sceneId\":\"$LATER\"}" >/dev/null
OPEN=$(count '')
curl -s -X PATCH "$API/publications/$PID" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"spoilerSafe":true}' >/dev/null
BEFORE=$(curl -s "$API/publications/$PID/comments" -H "$SAUTH" | python3 -c "
import sys,json;print(len(json.load(sys.stdin)))")
curl -s -X PUT "$API/publications/$PID/progress" -H "$SAUTH" -H 'content-type: application/json' \
  -d "{\"sceneId\":\"$LATER\"}" >/dev/null
AFTER=$(curl -s "$API/publications/$PID/comments" -H "$SAUTH" | python3 -c "
import sys,json;print(len(json.load(sys.stdin)))")
[ "$AFTER" -gt "$BEFORE" ] && echo "  a reader at the start sees $BEFORE of $OPEN notes; once they reach scene 3, $AFTER" \
  || { echo "  EXPECTED more notes after reading on: $BEFORE -> $AFTER"; exit 1; }
curl -s -X PATCH "$API/publications/$PID" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"spoilerSafe":false}' >/dev/null

step "Rate limits"
# The server runs with a vote limit of 3 a minute; Ada has already used one.
for _ in 1 2; do
  curl -s -X POST "$API/comments/$CID/vote" -H "$AUTH" -H 'content-type: application/json' -d '{"value":1}' >/dev/null
done
LIMITED=$(curl -s -X POST "$API/comments/$CID/vote" -H "$AUTH" -H 'content-type: application/json' -d '{"value":1}')
echo "$LIMITED" | python3 -c "
import sys,json;d=json.load(sys.stdin)['error']
assert d['code'] == 'too_many_requests', d
print(f\"  refused after three: {d['message']}\")"

step "An author's own door"
curl -s -X POST "$API/me/blocks" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"userId\":\"$GRACE\"}" | python3 -c "
import sys,json
print('  blocked:', ', '.join(b['reader']['displayName'] for b in json.load(sys.stdin)))"
SHUT=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$PID" -H "$RAUTH")
[ "$SHUT" = "403" ] && echo "  a blocked reader cannot open the work at all" || { echo "  EXPECTED 403, got $SHUT"; exit 1; }
curl -s -X DELETE "$API/me/blocks/$GRACE" -H "$AUTH" >/dev/null
BACK=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$PID" -H "$RAUTH")
[ "$BACK" = "200" ] && echo "  and is let back in when the block goes" || { echo "  EXPECTED 200, got $BACK"; exit 1; }

step "Profiles"
ADA=$(curl -s "$API/auth/me" -H "$AUTH" | field id)
curl -s -X PATCH "$API/me/profile" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"bio":"Writes about fields, mostly."}' | python3 -c "
import sys,json;d=json.load(sys.stdin);assert d['bio']=='Writes about fields, mostly.', d;print('  bio set')"
EMPTY=$(curl -s -o /dev/null -w '%{http_code}' -X PATCH "$API/me/profile" -H "$AUTH" \
  -H 'content-type: application/json' -d '{"displayName":"   "}')
[ "$EMPTY" = "400" ] && echo "  a blank name is refused" || { echo "  EXPECTED 400, got $EMPTY"; exit 1; }
curl -s "$API/users/$ADA/profile" -H "$SAUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
assert 'email' not in d, 'a profile must not carry an address'
assert all(p['visibility'] != 'invited' for p in d['publications']), 'an invited draft leaked onto a profile'
print(f\"  {d['displayName']}: {len(d['publications'])} on offer to a stranger, bio {d['bio']!r}\")"
curl -s "$API/users/$GRACE/profile" -H "$SAUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin)
assert d['reviews'], 'Grace has reviewed something'
r=d['reviews'][0]
print(f\"  {d['displayName']} reviewed {r['workTitle']} by {r['author']['displayName']}: {r['notes']} notes, {r['resolved']} acted on\")"

step "Following"
curl -s -X PUT "$API/users/$ADA/follow" -H "$SAUTH" | field followers | sed 's/^/  followers after one follow: /'
curl -s -X PUT "$API/users/$ADA/follow" -H "$SAUTH" | field followers | sed 's/^/  and after following twice: /'
SELF=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$API/users/$ADA/follow" -H "$AUTH")
[ "$SELF" = "400" ] && echo "  following yourself is refused" || { echo "  EXPECTED 400, got $SELF"; exit 1; }
curl -s "$API/publications?following=true" -H "$SAUTH" | python3 -c "
import sys,json;rows=json.load(sys.stdin)
assert rows and all(r['author']['displayName']=='Ada' for r in rows), rows
print(f\"  the following feed: {len(rows)} from Ada and nothing else\")"
curl -s "$API/publications?following=true" -H "$RAUTH" | python3 -c "
import sys,json;assert json.load(sys.stdin)==[], 'Grace follows nobody';print('  someone who follows nobody gets an empty feed')"
curl -s -X POST "$API/me/blocks" -H "$AUTH" -H 'content-type: application/json' -d "{\"userId\":\"$GRACE\"}" >/dev/null
curl -s "$API/publications" -H "$RAUTH" | python3 -c "
import sys,json;assert all(r['author']['displayName']!='Ada' for r in json.load(sys.stdin)), 'a blocked reader is still shown the work'
print('  a blocked reader is no longer shown the author\\'s work in the feed')"
SHUTF=$(curl -s -o /dev/null -w '%{http_code}' -X PUT "$API/users/$ADA/follow" -H "$RAUTH")
[ "$SHUTF" = "403" ] && echo "  nor can they follow her" || { echo "  EXPECTED 403, got $SHUTF"; exit 1; }
curl -s -X DELETE "$API/me/blocks/$GRACE" -H "$AUTH" >/dev/null
curl -s -X DELETE "$API/users/$ADA/follow" -H "$SAUTH" | field following | sed 's/^/  unfollowed, following: /'

step "Browsing by genre, length and window"
curl -s -X PATCH "$API/works/$WID" -H "$AUTH" -H 'content-type: application/json' -d '{"genre":"literary"}' | field genre | sed 's/^/  genre set: /'
has() { curl -s "$API/publications?$1" -H "$SAUTH" | python3 -c "
import sys,json;print(sum(1 for r in json.load(sys.stdin) if r['id']=='$PID'))"; }
[ "$(has genre=literary)" = "1" ] && [ "$(has genre=horror)" = "0" ] && echo "  literary finds it, horror does not" || { echo "  GENRE FILTER WRONG"; exit 1; }
[ "$(has length=short)" = "1" ] && [ "$(has length=novel)" = "0" ] && echo "  a few hundred words is short, not a novel" || { echo "  LENGTH FILTER WRONG"; exit 1; }
[ "$(has window=open_ended)" = "1" ] && [ "$(has window=closing_soon)" = "0" ] && echo "  no closing date is open-ended" || { echo "  WINDOW FILTER WRONG"; exit 1; }
SOON=$(python3 -c "import datetime;print((datetime.datetime.now(datetime.UTC)+datetime.timedelta(days=1)).isoformat().replace('+00:00','Z'))")
curl -s -X PATCH "$API/publications/$PID" -H "$AUTH" -H 'content-type: application/json' -d "{\"closesAt\":\"$SOON\"}" >/dev/null
[ "$(has window=closing_soon)" = "1" ] && echo "  and closing tomorrow is closing soon" || { echo "  CLOSING SOON WRONG"; exit 1; }
BADG=$(curl -s -o /dev/null -w '%{http_code}' "$API/publications?genre=space_opera" -H "$SAUTH")
[ "$BADG" = "400" ] && echo "  an unknown genre is refused rather than matching nothing" || { echo "  EXPECTED 400, got $BADG"; exit 1; }

step "The needs-readers queue"
# A second author with something nobody has read yet.
SW=$(curl -s -X POST "$API/works" -H "$SAUTH" -H 'content-type: application/json' -d '{"title":"Low Tide","kind":"short_story"}')
SWID=$(echo "$SW" | field id)
SSCENE=$(echo "$SW" | field chapters.0.scenes.0.id)
curl -s -X PUT "$API/scenes/$SSCENE/content" -H "$SAUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"The water went out further than anyone remembered."}]}]}}' >/dev/null
SV=$(curl -s -X POST "$API/works/$SWID/versions" -H "$SAUTH" -H 'content-type: application/json' -d '{}' | field id)
curl -s -X POST "$API/versions/$SV/publish" -H "$SAUTH" >/dev/null
SPID=$(curl -s -X POST "$API/versions/$SV/publications" -H "$SAUTH" -H 'content-type: application/json' -d '{"visibility":"platform"}' | field id)
HTOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"hal@example.com","password":"correcthorse","displayName":"Hal","acceptTerms":true}' | field accessToken)
confirm hal@example.com
HAUTH="Authorization: Bearer $HTOKEN"
queue() { curl -s "$API/publications?sort=needs_readers" -H "$1" | python3 -c "
import sys,json;print(', '.join(f\"{r['workTitle']} ({r['readerCount']} read)\" for r in json.load(sys.stdin)) or 'nothing')"; }
Q=$(queue "$HAUTH")
echo "  a newcomer's queue: $Q"
[[ "$Q" == "Low Tide"* ]] || { echo "  EXPECTED the unread story first"; exit 1; }
QS=$(queue "$SAUTH")
echo "  the stranger's own queue: $QS"
[[ "$QS" != *"Low Tide"* && "$QS" != *"The Long Field"* ]] || { echo "  EXPECTED neither their own work nor one they have read"; exit 1; }
# Reading for someone else is what lifts your own work.
curl -s -X POST "$API/publications/$SPID/comments" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"body":"The first line is the story."}' >/dev/null
curl -s "$API/publications/$PID" -H "$HAUTH" | python3 -c "
import sys,json;d=json.load(sys.stdin);assert d['authorReviewsGiven']==1, d['authorReviewsGiven']
print(f\"  Ada has now read for {d['authorReviewsGiven']} other writer, which her card carries\")"

step "Co-authors and their roles"
OUT=$(curl -s "$API/works/$WID/outline" -H "$AUTH")
CH1=$(echo "$OUT" | field chapters.0.id)
CH2=$(echo "$OUT" | field chapters.1.id)
S1=$(echo "$OUT" | field chapters.0.scenes.0.id)
S2=$(echo "$OUT" | field chapters.1.scenes.0.id)
collab() { curl -s -o /dev/null -w '%{http_code}' -X POST "$API/works/$WID/collaborators" -H "$AUTH" \
  -H 'content-type: application/json' -d "$1"; }
[ "$(collab '{"email":"ADA@example.com","role":"writer"}')" = "400" ] && echo "  the creator cannot be added to her own book" || { echo "  EXPECTED 400"; exit 1; }
[ "$(collab "{\"email\":\"x@example.com\",\"role\":\"owner\",\"chapterIds\":[\"$CH2\"]}")" = "400" ] && echo "  an owner cannot be narrowed to a chapter" || { echo "  EXPECTED 400"; exit 1; }
curl -s -X POST "$API/works/$WID/collaborators" -H "$AUTH" -H 'content-type: application/json' \
  -d '{"email":"hal@example.com","role":"editor"}' >/dev/null
# Ivy has no account yet: the invitation waits for her address.
curl -s -X POST "$API/works/$WID/collaborators" -H "$AUTH" -H 'content-type: application/json' \
  -d "{\"email\":\"ivy@example.com\",\"role\":\"writer\",\"chapterIds\":[\"$CH2\"]}" | python3 -c "
import sys,json
for c in json.load(sys.stdin):
    who = c['person']['displayName'] if c['person'] else c['email'] + ' (no account yet)'
    print(f\"  {who}: {c['role']}{' on ' + str(len(c['chapterIds'])) + ' chapter' if c['chapterIds'] is not None else ''}\")"
ITOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"ivy@example.com","password":"correcthorse","displayName":"Ivy","acceptTerms":true}' | field accessToken)
confirm ivy@example.com
IAUTH="Authorization: Bearer $ITOKEN"
curl -s "$API/works" -H "$HAUTH" | python3 -c "
import sys,json;rows=json.load(sys.stdin);assert any(w['title']=='The Long Field' for w in rows), rows
print('  the shared work is on the editor\\'s desk')"
curl -s "$API/works/$WID/outline" -H "$IAUTH" | python3 -c "
import sys,json;a=json.load(sys.stdin)['access']
assert a['role']=='writer' and len(a['chapterIds'])==1 and not a['creator'], a
print(f\"  Ivy's outline says: {a['role']}, {len(a['chapterIds'])} chapter, owned by {a['owner']['displayName']}\")"

code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
expect() { [ "$2" = "$3" ] && echo "  $1" || { echo "  $1 — EXPECTED $3, got $2"; exit 1; }; }
DOC='{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Ivy was here."}]}]}}'
expect "an editor cannot change the text" \
  "$(code -X PUT "$API/scenes/$S1/content" -H "$HAUTH" -H 'content-type: application/json' -d "$DOC")" 403
expect "but can propose a change" \
  "$(code -X POST "$API/scenes/$S1/changes" -H "$HAUTH" -H 'content-type: application/json' -d '{"kind":"insert","anchorFrom":1,"anchorTo":1,"payload":{"text":"Still, "}}')" 200
expect "and add to the to-do list" \
  "$(code -X POST "$API/works/$WID/tasks" -H "$HAUTH" -H 'content-type: application/json' -d '{"body":"The middle sags."}')" 200
expect "but not snapshot the book" \
  "$(code -X POST "$API/works/$WID/versions" -H "$HAUTH" -H 'content-type: application/json' -d '{}')" 403
expect "a writer handed chapter two can write in it" \
  "$(code -X PUT "$API/scenes/$S2/content" -H "$IAUTH" -H 'content-type: application/json' -d "$DOC")" 200
curl -s -X PUT "$API/scenes/$S2/content" -H "$IAUTH" -H 'content-type: application/json' \
  -d '{"content":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Ivy was here twice."}]}]}}' >/dev/null
expect "not in chapter one" \
  "$(code -X PUT "$API/scenes/$S1/content" -H "$IAUTH" -H 'content-type: application/json' -d "$DOC")" 403
expect "though she can read it" "$(code "$API/scenes/$S1" -H "$IAUTH")" 200
expect "and cannot add a chapter, which changes the book's shape" \
  "$(code -X POST "$API/works/$WID/chapters" -H "$IAUTH" -H 'content-type: application/json' -d '{"title":"Mine now"}')" 403
expect "a stranger still gets nothing" "$(code "$API/works/$WID/outline" -H "$SAUTH")" 403
curl -s "$API/works/$WID/collaborators" -H "$IAUTH" | python3 -c "
import sys,json;assert all(c['email'] is None for c in json.load(sys.stdin)), 'addresses leaked to a non-owner'
print('  a writer sees who else is on the book, but not their addresses')"
HALC=$(curl -s "$API/works/$WID/collaborators" -H "$AUTH" | python3 -c "
import sys,json;print(next(c['id'] for c in json.load(sys.stdin) if c['email']=='hal@example.com'))")
curl -s -X PATCH "$API/works/$WID/collaborators/$HALC" -H "$AUTH" -H 'content-type: application/json' -d '{"role":"owner"}' >/dev/null
expect "promoted to owner, Hal can snapshot" \
  "$(code -X POST "$API/works/$WID/versions" -H "$HAUTH" -H 'content-type: application/json' -d '{"label":"Hal was here"}')" 200
expect "but not delete the book out from under the person who started it" "$(code -X DELETE "$API/works/$WID" -H "$HAUTH")" 403

step "The work's activity"
curl -s "$API/works/$WID/activity" -H "$IAUTH" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
for r in rows[:6]:
    times = f\" x{r['times']}\" if r['times'] > 1 else ''
    print(f\"  {r['actor']['displayName']:5} {r['kind']:22} {r['subject']}{times}\")
edits=[r for r in rows if r['kind']=='scene_edited' and r['actor']['displayName']=='Ivy']
assert len(edits)==1 and edits[0]['times']==2, 'two saves in a sitting should be one line'
assert not any('@' in r['subject'] for r in rows), 'an address leaked into the activity feed'"
BEFORE_R=$(curl -s "$API/publications/$PID" -H "$AUTH" | field readerCount)
curl -s -X PUT "$API/publications/$PID/progress" -H "$IAUTH" -H 'content-type: application/json' -d "{\"sceneId\":\"$S2\"}" >/dev/null
AFTER_R=$(curl -s "$API/publications/$PID" -H "$AUTH" | field readerCount)
[ "$BEFORE_R" = "$AFTER_R" ] && echo "  a co-author opening the book is not counted as a reader ($AFTER_R)" || { echo "  READER COUNT MOVED: $BEFORE_R -> $AFTER_R"; exit 1; }
IVYC=$(curl -s "$API/works/$WID/collaborators" -H "$AUTH" | python3 -c "
import sys,json;print(next(c['id'] for c in json.load(sys.stdin) if c['email']=='ivy@example.com'))")
expect "Ivy steps off the book" "$(code -X DELETE "$API/works/$WID/collaborators/$IVYC" -H "$IAUTH")" 200
expect "and is shut out of it" "$(code "$API/works/$WID/outline" -H "$IAUTH")" 403
expect "and an owner can take someone else off" \
  "$(code -X DELETE "$API/works/$WID/collaborators/$HALC" -H "$AUTH")" 200

step "Terms"
curl -s "$API/terms" | python3 -c "
import sys,json;d=json.load(sys.stdin);assert 'belongs to you' in d['text'] and not d['text'].startswith('<!--'), d['text'][:80]
print(f\"  version {d['version']}, and it says the writing belongs to the writer\")"
NOTERMS=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"nope@example.com","password":"correcthorse","displayName":"Nope"}')
[ "$NOTERMS" = "400" ] && echo "  an account without agreement is refused" || { echo "  EXPECTED 400, got $NOTERMS"; exit 1; }

step "Proving an address"
KTOKEN=$(curl -s -X POST "$API/auth/register" -H 'content-type: application/json' \
  -d '{"email":"kit@example.com","password":"correcthorse","displayName":"Kit","acceptTerms":true}' | field accessToken)
KAUTH="Authorization: Bearer $KTOKEN"
curl -s -X POST "$API/publications/$INV/invites" -H "$AUTH" -H 'content-type: application/json' -d '{"emails":["kit@example.com"]}' >/dev/null
UNPROVED=$(curl -s "$API/publications/$INV" -H "$KAUTH")
echo "$UNPROVED" | python3 -c "
import sys,json;e=json.load(sys.stdin)['error'];assert e['code']=='forbidden' and 'confirm' in e['message'], e
print('  invited, but not let in until the address is proved:', e['message'])"
FIRST=$(link_token kit@example.com verify)
curl -s -X POST "$API/me/verification" -H "$KAUTH" >/dev/null
SECOND=$(link_token kit@example.com verify)
STALE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/auth/verify" -H 'content-type: application/json' -d "{\"token\":\"$FIRST\"}")
[ "$STALE" = "400" ] && echo "  asking again retires the first link" || { echo "  EXPECTED 400, got $STALE"; exit 1; }
curl -s -X POST "$API/auth/verify" -H 'content-type: application/json' -d "{\"token\":\"$SECOND\"}" >/dev/null
[ "$(curl -s -o /dev/null -w '%{http_code}' "$API/publications/$INV" -H "$KAUTH")" = "200" ] \
  && echo "  following the mailed link opens the draft, with the same session" || { echo "  STILL SHUT OUT"; exit 1; }
AGAIN=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/auth/verify" -H 'content-type: application/json' -d "{\"token\":\"$SECOND\"}")
[ "$AGAIN" = "400" ] && echo "  a link works once" || { echo "  EXPECTED 400, got $AGAIN"; exit 1; }

step "Getting back in"
curl -s -X POST "$API/auth/password/forgot" -H 'content-type: application/json' -d '{"email":"nobody@example.com"}' | field ok | sed 's/^/  an unknown address gets the same answer: ok=/'
curl -s -X POST "$API/auth/password/forgot" -H 'content-type: application/json' -d '{"email":"kit@example.com"}' >/dev/null
RESET=$(link_token kit@example.com reset)
curl -s -X POST "$API/auth/password/reset" -H 'content-type: application/json' \
  -d "{\"token\":\"$RESET\",\"password\":\"batterystaple\"}" >/dev/null
[ "$(curl -s -o /dev/null -w '%{http_code}' "$API/auth/me" -H "$KAUTH")" = "401" ] \
  && echo "  the reset signed out the session that was open" || { echo "  OLD SESSION SURVIVED"; exit 1; }
[ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/auth/login" -H 'content-type: application/json' -d '{"email":"kit@example.com","password":"correcthorse"}')" = "401" ] \
  && echo "  the old password no longer works" || { echo "  OLD PASSWORD WORKS"; exit 1; }
KTOKEN=$(curl -s -X POST "$API/auth/login" -H 'content-type: application/json' -d '{"email":"kit@example.com","password":"batterystaple"}' | field accessToken)
KAUTH="Authorization: Bearer $KTOKEN"
echo "  the new one does"

step "Two-factor sign-in"
totp() { python3 -c "
import hmac,hashlib,struct,time,base64
s='$1';k=base64.b32decode(s+'='*(-len(s)%8));c=int(time.time()//30)+${2:-0}
h=hmac.new(k,struct.pack('>q',c),hashlib.sha1).digest();o=h[-1]&15
print('%06d'%((struct.unpack('>I',h[o:o+4])[0]&0x7fffffff)%1000000))"; }
SECRET=$(curl -s -X POST "$API/me/two-factor/setup" -H "$KAUTH" | field secret)
CODES=$(curl -s -X POST "$API/me/two-factor/enable" -H "$KAUTH" -H 'content-type: application/json' -d "{\"code\":\"$(totp $SECRET)\"}")
RECOVERY=$(echo "$CODES" | field codes.0)
RECOVERY2=$(echo "$CODES" | field codes.1)
echo "  on, with $(echo "$CODES" | python3 -c 'import sys,json;print(len(json.load(sys.stdin)["codes"]))') recovery codes"
login() { curl -s -X POST "$API/auth/login" -H 'content-type: application/json' -d "{\"email\":\"kit@example.com\",\"password\":\"batterystaple\"$1}"; }
login '' | python3 -c "
import sys,json;e=json.load(sys.stdin)['error'];assert e['code']=='two_factor_required', e
print('  the right password alone now asks for a code')"
REPLAY=$(login ",\"code\":\"$(totp $SECRET)\"" | python3 -c "import sys,json;print(json.load(sys.stdin).get('error',{}).get('code','ok'))")
[ "$REPLAY" = "unauthorized" ] && echo "  the code that switched it on cannot be used again" || { echo "  EXPECTED a refused replay, got $REPLAY"; exit 1; }
login ",\"code\":\"$(totp $SECRET 1)\"" | field accessToken >/dev/null && echo "  the next code signs in"
login ",\"code\":\"$RECOVERY\"" | field accessToken >/dev/null && echo "  so does a recovery code"
SPENT=$(login ",\"code\":\"$RECOVERY\"" | python3 -c "import sys,json;print(json.load(sys.stdin).get('error',{}).get('code','ok'))")
[ "$SPENT" = "unauthorized" ] && echo "  once" || { echo "  EXPECTED a spent recovery code, got $SPENT"; exit 1; }

step "Sessions"
OTHER=$(login ",\"code\":\"$RECOVERY2\"" 2>/dev/null | python3 -c "import sys,json;print(json.load(sys.stdin).get('accessToken',''))")
curl -s "$API/me/sessions" -H "$KAUTH" | python3 -c "
import sys,json;rows=json.load(sys.stdin);assert sum(r['current'] for r in rows)==1
print(f'  {len(rows)} devices signed in, one of them this one')"
VICTIM=$(curl -s "$API/me/sessions" -H "$KAUTH" | python3 -c "
import sys,json;print(next(r['id'] for r in json.load(sys.stdin) if not r['current']))")
curl -s -X DELETE "$API/me/sessions/$VICTIM" -H "$KAUTH" >/dev/null
curl -s -X POST "$API/me/sessions/sign-out-others" -H "$KAUTH" >/dev/null
[ -n "$OTHER" ] && [ "$(curl -s -o /dev/null -w '%{http_code}' "$API/auth/me" -H "Authorization: Bearer $OTHER")" = "401" ] \
  && echo "  signing the others out works at once, not when their tokens run out" || { echo "  A REVOKED SESSION STILL WORKS"; exit 1; }
curl -s "$API/me/sessions" -H "$KAUTH" | python3 -c "
import sys,json;rows=json.load(sys.stdin);assert len(rows)==1 and rows[0]['current'], rows
print('  this one is left')"

step "The audit log"
curl -s "$API/me/audit" -H "$AUTH" | python3 -c "
import sys,json
seen={r['action'] for r in json.load(sys.stdin)}
for want in ['publication_created','publication_changed','reader_invited','collaborator_added','collaborator_removed','reader_blocked']:
    assert want in seen, (want, seen)
print('  Ada can see who published, who was let in and who was shut out:', ', '.join(sorted(seen)))"

step "A copy of everything"
curl -s -o "$TMP/export.zip" "$API/me/export" -H "$AUTH"
python3 - "$TMP/export.zip" <<'PY'
import sys, zipfile, json
z = zipfile.ZipFile(sys.argv[1])
names = z.namelist()
for want in ['README.txt', 'account.json', 'works.json', 'contributions.json', 'audit.json']:
    assert want in names, (want, names[:10])
scenes = [n for n in names if n.startswith('manuscripts/') and '/working/' in n]
assert scenes, 'no manuscript files'
everything = b''.join(z.read(n) for n in names)
assert b'argon2' not in everything and b'passwordHash' not in everything, 'a password hash got into the export'
account = json.loads(z.read('account.json'))
print(f"  {len(names)} files: the records, and {len(scenes)} working scenes exactly as stored — no password hash")
PY

step "Deleting an account"
curl -s -X POST "$API/works/$SWID/collaborators" -H "$SAUTH" -H 'content-type: application/json' -d '{"email":"hal@example.com","role":"reader"}' >/dev/null
SHARED=$(curl -s -X DELETE "$API/me" -H "$SAUTH" -H 'content-type: application/json' -d '{"password":"correcthorse"}')
echo "$SHARED" | python3 -c "
import sys,json;e=json.load(sys.stdin)['error'];assert e['details']['sharedWorks']==['Low Tide'], e
print('  refused until told:', e['message'])"
WRONG=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "$API/me" -H "$SAUTH" -H 'content-type: application/json' -d '{"password":"wrong-password","deleteSharedWorks":true}')
[ "$WRONG" = "401" ] && echo "  and never without the password" || { echo "  EXPECTED 401, got $WRONG"; exit 1; }
[ -d "$TMP/works/$SWID" ] || { echo "  EXPECTED Low Tide's files before deletion"; exit 1; }
curl -s -X DELETE "$API/me" -H "$SAUTH" -H 'content-type: application/json' -d '{"password":"correcthorse","deleteSharedWorks":true}' | field worksDeleted | sed 's/^/  deleted, with works: /'
[ ! -d "$TMP/works/$SWID" ] && echo "  and Low Tide's files are gone from disk" || { echo "  FILES LEFT BEHIND"; exit 1; }
[ "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$API/auth/login" -H 'content-type: application/json' -d '{"email":"stranger@example.com","password":"correcthorse"}')" = "401" ] \
  && echo "  the account no longer signs in" || { echo "  STILL SIGNS IN"; exit 1; }
# Someone else answers one of Grace's notes; deleting her must not take
# their reply with it.
REPLYRAW=$(curl -s -X POST "$API/publications/$PID/comments" -H "$KAUTH" -H 'content-type: application/json' \
  -d "{\"body\":\"I read it the other way.\",\"parentId\":\"$CID\"}")
REPLY=$(echo "$REPLYRAW" | field id 2>/dev/null) || { echo "  COULD NOT REPLY: $REPLYRAW"; exit 1; }
curl -s -X DELETE "$API/me" -H "$RAUTH" -H 'content-type: application/json' -d '{"password":"correcthorse"}' >/dev/null
curl -s "$API/publications/$PID/comments" -H "$AUTH" | python3 -c "
import sys,json
rows=json.load(sys.stdin)
def every(cs):
    for c in cs:
        yield c
        yield from every(c['replies'])
flat=list(every(rows))
assert not any(c['author']['displayName']=='Grace' for c in flat), 'Grace is still named'
root=[c for c in rows if c['id']=='$CID']
assert root and root[0]['state']=='deleted' and root[0]['body']=='', 'her note should be a blank placeholder'
assert any(r['id']=='$REPLY' for r in root[0]['replies']), 'the reply under her note went with her'
print(\"  Grace's notes are blanked, and Kit's reply under one of them is still there\")"
curl -s "$API/me/audit" -H "$AUTH" | python3 -c "
import sys,json;assert any(r['action']=='collaborator_added' for r in json.load(sys.stdin))
print('  and the audit trail outlives the accounts it mentions')"

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

step "Request ids"
HDR=$(curl -s -D - -o /dev/null "$API/works" -H "$AUTH" | tr -d '\r' | awk -F': ' 'tolower($1)=="x-request-id"{print $2}')
[ ${#HDR} -ge 8 ] && echo "  every response carries one: $HDR" || { echo "  NO REQUEST ID"; exit 1; }
curl -s "$API/works/not-a-work" -H "$AUTH" -H 'x-request-id: smoke-trace-0001' | python3 -c "
import sys,json;e=json.load(sys.stdin)['error'];assert e['requestId']=='smoke-trace-0001', e
print('  a client\'s own id comes back in the error, to quote:', e['requestId'])"
grep -q 'smoke-trace-0001' "$TMP/server.log" && echo "  and is on the server's log line for it" || { echo "  NOT IN THE LOG"; exit 1; }

step "Status, backups and a restore drill"
status() { curl -s "$API/status" | python3 -c "
import sys,json;d=json.load(sys.stdin)
b=next(c for c in d['checks'] if c['name']=='backups')
print(d['status'], '|', b['detail'])"; }
echo "  before any backup: $(status)"
BENV=(env SCRIPTORIUM_DATABASE_URL="$TMP/scriptorium.db" SCRIPTORIUM_DATA_DIR="$TMP/works" \
  SCRIPTORIUM_BACKUP_DIR="$TMP/backups" SCRIPTORIUM_JWT_SECRET="smoke-test-secret-value-1234567890" RUST_LOG=warn)
TAKEN=$("${BENV[@]}" "$ROOT/target/debug/scriptorium-backend" --backup)
echo "  taken while the server runs: $(basename "$TAKEN")"
echo "  backed up but not yet restored: $(status)"
"${BENV[@]}" "$ROOT/target/debug/scriptorium-backend" --restore-drill "$TAKEN" | sed 's/^/  /'
S=$(status); echo "  after the drill: $S"
[[ "$S" == ok* ]] || { echo "  EXPECTED ok"; exit 1; }
# A backup that has lost a file has to fail its drill, or the drill proves nothing.
rm "$(find "$TAKEN/works" -name '*.json' -path '*working*' | head -1)"
if "${BENV[@]}" "$ROOT/target/debug/scriptorium-backend" --restore-drill "$TAKEN" >/dev/null 2>&1; then
  echo "  A DAMAGED BACKUP PASSED"; exit 1
fi
echo "  a backup missing one scene fails its drill: $(status)"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$API/status")" = "503" ] && echo "  and the status check says so with a 503" || { echo "  EXPECTED 503"; exit 1; }

printf '\n\033[32mAll good.\033[0m\n'
