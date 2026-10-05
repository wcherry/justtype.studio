import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { api } from '../api/client'
import type { CollaboratorRole, CollaboratorView, Outline } from '../api/types'
import { useAuth } from '../state/auth'
import type { Permissions } from '../state/permissions'

const ROLES: { value: CollaboratorRole; label: string; hint: string }[] = [
  { value: 'writer', label: 'Writer', hint: 'Changes the text and the structure' },
  { value: 'editor', label: 'Editor', hint: 'Proposes changes and keeps the to-do list' },
  { value: 'reader', label: 'Reader', hint: 'Reads the working draft' },
  { value: 'owner', label: 'Owner', hint: 'Everything but deleting the work' },
]

/**
 * Who is on the book. Owners add people, change what they may do, and hand a
 * ghostwriter or an editor only some chapters; everyone else can see the list
 * and step off it.
 */
export function CollaboratorsPanel({
  outline,
  permissions,
}: {
  outline: Outline
  permissions: Permissions
}) {
  const { user } = useAuth()
  const navigate = useNavigate()
  const [people, setPeople] = useState<CollaboratorView[]>([])
  const [email, setEmail] = useState('')
  const [role, setRole] = useState<CollaboratorRole>('writer')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    api.collaborators.list(outline.id).then(setPeople).catch((e) => setError(e.message))
  }, [outline.id])

  useEffect(load, [load])

  const run = async (work: () => Promise<CollaboratorView[] | void>) => {
    setBusy(true)
    setError(null)
    try {
      const next = await work()
      if (next) setPeople(next)
      else load()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }

  const owner = outline.access?.owner

  return (
    <div className="people">
      {error && <p className="error">{error}</p>}

      <ul className="people__list">
        {owner && (
          <li className="person">
            <div className="person__head">
              <strong>{owner.displayName}</strong>
              <span className="badge">started it</span>
            </div>
          </li>
        )}
        {people.map((person) => {
          const me = person.person?.id === user?.id
          return (
            <li key={person.id} className="person">
              <div className="person__head">
                <strong>
                  {person.person?.displayName ?? person.email ?? 'Invited'}
                  {me && ' (you)'}
                </strong>
                {!person.acceptedAt && <span className="field__hint">hasn’t opened it yet</span>}
              </div>
              {person.person && person.email && (
                <p className="field__hint">{person.email}</p>
              )}

              {permissions.isOwner ? (
                <div className="person__controls">
                  <select
                    value={person.role}
                    disabled={busy}
                    onChange={(e) =>
                      run(() =>
                        api.collaborators.update(outline.id, person.id, {
                          role: e.target.value as CollaboratorRole,
                        }),
                      )
                    }
                  >
                    {ROLES.map((r) => <option key={r.value} value={r.value}>{r.label}</option>)}
                  </select>
                  {(person.role === 'writer' || person.role === 'editor') && (
                    <ChapterGrant
                      outline={outline}
                      chapterIds={person.chapterIds ?? null}
                      disabled={busy}
                      onChange={(chapterIds) =>
                        run(() =>
                          api.collaborators.update(
                            outline.id,
                            person.id,
                            chapterIds === null ? { wholeWork: true } : { chapterIds },
                          ),
                        )
                      }
                    />
                  )}
                </div>
              ) : (
                <p className="person__role">
                  {ROLES.find((r) => r.value === person.role)?.label}
                  {person.chapterIds && ` · ${chapterNames(outline, person.chapterIds)}`}
                </p>
              )}

              {(permissions.isOwner || me) && (
                <button
                  type="button"
                  className="linklike"
                  disabled={busy}
                  onClick={() =>
                    run(async () => {
                      await api.collaborators.remove(outline.id, person.id)
                      // Stepping off a book means it is no longer yours to look at.
                      if (me) navigate('/')
                    })
                  }
                >
                  {me ? 'Leave this work' : 'Take off this work'}
                </button>
              )}
            </li>
          )
        })}
      </ul>

      {people.length === 0 && (
        <p className="empty">Nobody else is on this work yet.</p>
      )}

      {permissions.isOwner && (
        <form
          className="people__add"
          onSubmit={(event) => {
            event.preventDefault()
            void run(async () => {
              const next = await api.collaborators.add(outline.id, { email, role })
              setEmail('')
              return next
            })
          }}
        >
          <label>
            Add someone by email
            <input
              type="email"
              value={email}
              required
              placeholder="co-author@example.com"
              onChange={(e) => setEmail(e.target.value)}
            />
          </label>
          <label>
            As
            <select value={role} onChange={(e) => setRole(e.target.value as CollaboratorRole)}>
              {ROLES.map((r) => <option key={r.value} value={r.value}>{r.label}</option>)}
            </select>
            <span className="field__hint">{ROLES.find((r) => r.value === role)?.hint}</span>
          </label>
          <button type="submit" disabled={busy || !email.trim()}>Add</button>
          <p className="field__hint">
            They do not need an account yet. The work appears on their desk when they sign up
            with this address.
          </p>
        </form>
      )}
    </div>
  )
}

function chapterNames(outline: Outline, ids: string[]): string {
  if (ids.length === 0) return 'no chapters'
  return outline.chapters
    .filter((c) => ids.includes(c.id))
    .map((c) => c.title)
    .join(', ')
}

/**
 * The whole work, or only some chapters of it. Handing someone no chapters at
 * all is allowed and means nowhere to write — never everywhere.
 */
function ChapterGrant({
  outline,
  chapterIds,
  disabled,
  onChange,
}: {
  outline: Outline
  chapterIds: string[] | null
  disabled: boolean
  onChange: (chapterIds: string[] | null) => void
}) {
  const [open, setOpen] = useState(false)
  const whole = chapterIds === null
  return (
    <div className="grant">
      <button type="button" className="linklike" onClick={() => setOpen((o) => !o)}>
        {whole ? 'Whole work' : chapterNames(outline, chapterIds)} ▾
      </button>
      {open && (
        <div className="grant__options">
          <label className="grant__option">
            <input
              type="checkbox"
              checked={whole}
              disabled={disabled}
              onChange={() => onChange(whole ? outline.chapters.map((c) => c.id) : null)}
            />
            The whole work
          </label>
          {!whole &&
            outline.chapters.map((chapter) => (
              <label key={chapter.id} className="grant__option">
                <input
                  type="checkbox"
                  checked={chapterIds.includes(chapter.id)}
                  disabled={disabled}
                  onChange={(e) =>
                    onChange(
                      e.target.checked
                        ? [...chapterIds, chapter.id]
                        : chapterIds.filter((id) => id !== chapter.id),
                    )
                  }
                />
                {chapter.title}
              </label>
            ))}
        </div>
      )}
    </div>
  )
}
