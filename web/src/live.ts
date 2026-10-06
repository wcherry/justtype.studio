import { useEffect, useRef, useState } from 'react'
import { api, shareKeys, tokens } from './api/client'

/**
 * The live channel: one WebSocket for the tab, shared by everything that
 * wants notices. It carries "this changed", never the thing itself — a
 * component that hears comments changed fetches them through the API.
 *
 * Opened with a single-use ticket from `POST /api/live/ticket`, because a
 * browser cannot put an Authorization header on a WebSocket and a token in a
 * URL ends up in logs. Reconnects with a growing pause, and joins its rooms
 * again when it does.
 */
export interface Notice {
  type: string
  room?: string
  [key: string]: unknown
}

type Listener = (notice: Notice) => void

interface RoomState {
  listeners: Set<Listener>
  sceneId: string | null
}

const rooms = new Map<string, RoomState>()
let socket: WebSocket | null = null
let connecting = false
let retry = 0
let retryTimer: number | undefined

function send(command: object) {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(command))
}

async function connect() {
  if (connecting || socket || rooms.size === 0) return
  connecting = true
  try {
    const access = tokens.access()
    const res = await fetch('/api/live/ticket', {
      method: 'POST',
      // The ticket carries any share keys, so an unlisted draft's room opens.
      headers: { ...(access ? { authorization: `Bearer ${access}` } : {}), ...shareKeys.header() },
    })
    if (!res.ok) throw new Error(`ticket ${res.status}`)
    const { ticket } = (await res.json()) as { ticket: string }
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
    const next = new WebSocket(`${scheme}://${location.host}/api/live?ticket=${ticket}`)
    socket = next
    next.addEventListener('open', () => {
      retry = 0
      for (const [room, state] of rooms) send({ type: 'join', room, sceneId: state.sceneId })
    })
    next.addEventListener('message', (event) => {
      const notice = JSON.parse(event.data as string) as Notice
      const state = notice.room ? rooms.get(notice.room) : undefined
      state?.listeners.forEach((listener) => listener(notice))
    })
    next.addEventListener('close', () => {
      socket = null
      scheduleReconnect()
    })
  } catch {
    // The token may simply have expired; a normal request refreshes it.
    await api.auth.me().catch(() => undefined)
    scheduleReconnect()
  } finally {
    connecting = false
  }
}

function scheduleReconnect() {
  if (rooms.size === 0) return
  window.clearTimeout(retryTimer)
  const delay = Math.min(30_000, 1000 * 2 ** retry++)
  retryTimer = window.setTimeout(() => void connect(), delay)
}

/** Listens to a room until the returned function is called. */
export function join(room: string, listener: Listener, sceneId: string | null = null): () => void {
  let state = rooms.get(room)
  if (!state) {
    state = { listeners: new Set(), sceneId }
    rooms.set(room, state)
    send({ type: 'join', room, sceneId })
  }
  state.listeners.add(listener)
  void connect()
  return () => {
    const current = rooms.get(room)
    if (!current) return
    current.listeners.delete(listener)
    if (current.listeners.size === 0) {
      rooms.delete(room)
      send({ type: 'leave', room })
      if (rooms.size === 0) socket?.close()
    }
  }
}

/** Says which scene this tab is in, for the people writing the book with you. */
export function here(room: string, sceneId: string | null) {
  const state = rooms.get(room)
  if (!state || state.sceneId === sceneId) return
  state.sceneId = sceneId
  send({ type: 'here', room, sceneId })
}

/** Calls `onNotice` for each notice in a room, while the component is mounted. */
export function useLive(room: string | null, onNotice: Listener) {
  const handler = useRef(onNotice)
  handler.current = onNotice
  useEffect(() => {
    if (!room) return
    return join(room, (notice) => handler.current(notice))
  }, [room])
}

export interface Present {
  id: string
  displayName: string
  sceneId: string | null
}

/** Who else is in a work's room, and where. */
export function usePresence(room: string | null): Present[] {
  const [people, setPeople] = useState<Present[]>([])
  useLive(room, (notice) => {
    if (notice.type === 'presence' && Array.isArray(notice.people)) {
      setPeople(notice.people as Present[])
    }
  })
  return people
}
