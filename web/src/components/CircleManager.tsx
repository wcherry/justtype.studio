import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { CircleView } from '../api/types'

/**
 * Standing lists of readers, so inviting "my beta readers" is one action rather
 * than eleven.
 *
 * Members are email addresses rather than accounts, because the people a writer
 * trusts with a draft usually have an address and not yet an account here.
 */
export function CircleManager() {
  const [circles, setCircles] = useState<CircleView[]>([])
  // "No circles yet" is a claim about the server, so it waits for the server.
  // Rendering it while the first request is still out tells the writer their
  // circles are gone for as long as the network takes.
  const [loaded, setLoaded] = useState(false)
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [expanded, setExpanded] = useState<string | null>(null)

  const load = useCallback(() => {
    void api.circles
      .list()
      .then(setCircles)
      .catch(() => setCircles([]))
      .finally(() => setLoaded(true))
  }, [])

  useEffect(load, [load])

  return (
    <div className="circles">
      {loaded && circles.length === 0 && !adding && (
        <p className="empty">No circles yet. A circle is a list of people you send drafts to.</p>
      )}

      <ul className="circles__list">
        {circles.map((circle) => (
          <li key={circle.id}>
            <button
              type="button"
              className="circles__row"
              onClick={() => setExpanded(expanded === circle.id ? null : circle.id)}
            >
              <span>{circle.name}</span>
              <span className="bible__count">{circle.members.length}</span>
            </button>

            {expanded === circle.id && (
              <div className="circles__detail">
                <ul className="review-panel__list">
                  {circle.members.map((member) => (
                    <li key={member.id}>
                      <span>{member.name ?? member.email}</span>
                      {member.name && <span className="field__hint">{member.email}</span>}
                      <button
                        type="button"
                        onClick={async () => {
                          await api.circles.removeMember(circle.id, member.email)
                          load()
                        }}
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
                <AddMember circleId={circle.id} onAdded={load} />
                <button
                  type="button"
                  className="linklike"
                  onClick={async () => {
                    await api.circles.remove(circle.id)
                    load()
                  }}
                >
                  Delete this circle
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>

      {adding ? (
        <form
          className="circles__new"
          onSubmit={async (event) => {
            event.preventDefault()
            await api.circles.create(name)
            setName('')
            setAdding(false)
            load()
          }}
        >
          <input autoFocus value={name} placeholder="First readers" onChange={(e) => setName(e.target.value)} />
          <button type="submit" disabled={!name.trim()}>Create</button>
          <button type="button" onClick={() => setAdding(false)}>Cancel</button>
        </form>
      ) : (
        <button type="button" className="linklike" onClick={() => setAdding(true)}>+ New circle</button>
      )}
    </div>
  )
}

function AddMember({ circleId, onAdded }: { circleId: string; onAdded: () => void }) {
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')

  return (
    <form
      className="circles__member"
      onSubmit={async (event) => {
        event.preventDefault()
        await api.circles.addMember(circleId, email, name || undefined)
        setEmail('')
        setName('')
        onAdded()
      }}
    >
      <input value={email} placeholder="reader@example.com" onChange={(e) => setEmail(e.target.value)} />
      <input value={name} placeholder="name" onChange={(e) => setName(e.target.value)} />
      <button type="submit" disabled={!email.trim()}>Add</button>
    </form>
  )
}
