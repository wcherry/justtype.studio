// The live channel, end to end, for the smoke walkthrough: two people on one
// book seeing each other and each other's saves, a reader hearing a new note,
// and the doors that should stay shut.
//
//   node scripts/live_check.mjs  (reads API, ADA, KIT, HAL, WID, PID, SCENE)
const { API, ADA, KIT, HAL, WID, PID, SCENE } = process.env
const WS = API.replace(/^http/, 'ws')

const fail = (message) => {
  console.log(`  FAILED: ${message}`)
  process.exit(1)
}

async function ticket(token) {
  const res = await fetch(`${API}/live/ticket`, {
    method: 'POST',
    headers: token ? { authorization: `Bearer ${token}` } : {},
  })
  return (await res.json()).ticket
}

/** A connection that keeps every notice, and can wait for one. */
async function open(token) {
  const t = await ticket(token)
  const socket = new WebSocket(`${WS}/live?ticket=${t}`)
  const seen = []
  const waiters = []
  socket.addEventListener('message', (event) => {
    const notice = JSON.parse(event.data)
    seen.push(notice)
    for (const w of [...waiters]) {
      if (w.test(notice)) {
        waiters.splice(waiters.indexOf(w), 1)
        w.resolve(notice)
      }
    }
  })
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true })
    socket.addEventListener('error', () => reject(new Error('could not connect')), { once: true })
  })
  return {
    t,
    send: (command) => socket.send(JSON.stringify(command)),
    close: () => socket.close(),
    waitFor(test, what, ms = 3000) {
      const already = seen.find(test)
      if (already) {
        seen.splice(seen.indexOf(already), 1)
        return Promise.resolve(already)
      }
      return new Promise((resolve) => {
        const entry = { test, resolve }
        waiters.push(entry)
        setTimeout(() => {
          if (waiters.includes(entry)) fail(`no ${what} within ${ms}ms`)
        }, ms)
      })
    },
  }
}

const ada = await open(ADA)
const kit = await open(KIT)

// Two people on one book.
ada.send({ type: 'join', room: `work:${WID}`, sceneId: SCENE })
await ada.waitFor((n) => n.type === 'joined', 'join for Ada')
kit.send({ type: 'join', room: `work:${WID}` })
await kit.waitFor((n) => n.type === 'joined', 'join for Kit')
const together = await ada.waitFor(
  (n) => n.type === 'presence' && n.people?.length === 2,
  'presence with both writers',
)
console.log(`  on the book together: ${together.people.map((p) => p.displayName).sort().join(' and ')}`)
kit.send({ type: 'here', room: `work:${WID}`, sceneId: SCENE })
await ada.waitFor(
  (n) => n.type === 'presence' && n.people?.some((p) => p.displayName === 'Kit' && p.sceneId === SCENE),
  'Kit moving into the scene',
)
console.log('  and Ada can see which scene Kit is in')

// A save Kit makes reaches Ada, by name.
const scene = await (await fetch(`${API}/scenes/${SCENE}`, { headers: { authorization: `Bearer ${KIT}` } })).json()
const saved = await fetch(`${API}/scenes/${SCENE}/content`, {
  method: 'PUT',
  headers: { authorization: `Bearer ${KIT}`, 'content-type': 'application/json' },
  body: JSON.stringify({
    baseHash: scene.contentHash,
    content: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Kit was here, live.' }] }] },
  }),
})
if (!saved.ok) fail(`Kit could not save: ${saved.status}`)
const notice = await ada.waitFor((n) => n.type === 'scene_saved' && n.sceneId === SCENE, 'save notice')
console.log(`  Kit saves; Ada hears it at once: "${notice.by.displayName} saved this scene"`)

// A reader in a review hears a new note, and is counted rather than named.
ada.send({ type: 'join', room: `publication:${PID}` })
kit.send({ type: 'join', room: `publication:${PID}` })
await ada.waitFor((n) => n.type === 'presence' && n.readers === 2, 'two readers counted')
console.log('  two in the review: shown as a number, not a list of names')
await fetch(`${API}/publications/${PID}/comments`, {
  method: 'POST',
  headers: { authorization: `Bearer ${KIT}`, 'content-type': 'application/json' },
  body: JSON.stringify({ body: 'Live from the review.' }),
})
const heard = await ada.waitFor((n) => n.type === 'comments' && n.publicationId === PID, 'comment notice')
if ('comment' in heard || 'body' in heard) fail('the notice carried the note itself')
console.log('  a new note is announced; the note itself is fetched through the API')

// Doors that stay shut.
const hal = await open(HAL)
hal.send({ type: 'join', room: `work:${WID}` })
const denied = await hal.waitFor((n) => n.type === 'denied', 'denial for a stranger')
console.log(`  someone not on the book is refused: ${denied.message}`)
const anonymous = await open(null)
anonymous.send({ type: 'join', room: `publication:${PID}` })
await anonymous.waitFor((n) => n.type === 'denied', 'denial for signed-out on a platform review')
console.log('  signed out, a members-only review is refused too')

const reusedOpened = await new Promise((resolve) => {
  const again = new WebSocket(`${WS}/live?ticket=${ada.t}`)
  again.addEventListener('open', () => { again.close(); resolve(true) }, { once: true })
  again.addEventListener('error', () => resolve(false), { once: true })
})
if (reusedOpened) fail('a spent ticket opened a second socket')
console.log('  a ticket opens one socket, once')

for (const c of [ada, kit, hal, anonymous]) c.close()
process.exit(0)
