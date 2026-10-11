import { useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { staff } from '../../api/client'
import type { RestrictionView, UserDetail } from '../../api/types'
import { useAuth } from '../../state/auth'
import { Pill, REASON_LABEL, ReasonButton, StatusPill, day, errorText, useLoad, when } from './common'

/** The whole of an account in one place: who they are, what they've done, and what's been done to them. */
export function AdminUserPage() {
  const { userId = '' } = useParams()
  const [params] = useSearchParams()
  const { data, error, reload } = useLoad(() => staff.user(userId), [userId])
  const [message, setMessage] = useState<string | null>(null)
  const [restricting, setRestricting] = useState(params.has('restrict'))
  const [editing, setEditing] = useState(false)
  const [note, setNote] = useState('')

  if (error && !data) return <main className="feed"><p className="error">{error}</p></main>
  if (!data) return <main className="empty">Loading…</main>
  const can = (a: string) => data.actions.includes(a)
  const s = data.summary
  const done = async (work: Promise<unknown>, said: string) => {
    setMessage(null)
    try {
      await work
      setMessage(said)
      await reload()
    } catch (e) {
      setMessage(errorText(e))
    }
  }

  return (
    <main className="feed admin">
      <header className="feed__header">
        <h1>{s.displayName}</h1>
        <StatusPill status={s.status} ends={s.statusEnds} />
        {s.role && <Pill>{s.role}</Pill>}
      </header>
      {message && <p className="banner" role="status">{message}</p>}

      <section className="prompts__section">
        <h2>Summary</h2>
        <dl className="admin__facts">
          {s.email !== null && s.email !== undefined && (
            <><dt>Email</dt><dd>{s.email} {s.emailVerified ? '(confirmed)' : '(not confirmed)'}</dd></>
          )}
          {s.legalName && <><dt>Legal name</dt><dd>{s.legalName}</dd></>}
          {s.mailingAddress && <><dt>Address</dt><dd>{s.mailingAddress}</dd></>}
          {s.phone && <><dt>Phone</dt><dd>{s.phone}</dd></>}
          <dt>Joined</dt><dd>{when(s.createdAt)}</dd>
          <dt>Last seen</dt><dd>{when(s.lastSeenAt)}</dd>
          <dt>Two-factor</dt><dd>{s.twoFactor ? 'on' : 'off'}</dd>
          <dt>Terms</dt><dd>{s.termsVersion ? `version ${s.termsVersion}` : 'not agreed'}</dd>
          {s.setupPending && <><dt>Setup</dt><dd>waiting for them to choose a password</dd></>}
          {s.deactivatedAt && <><dt>Deactivated</dt><dd>{when(s.deactivatedAt)}</dd></>}
          {s.purgeAt && <><dt>Deleted on</dt><dd><strong>{day(s.purgeAt)}</strong></dd></>}
        </dl>
        {s.bio && <p><em>Bio:</em> {s.bio}</p>}
        <div className="admin__actions">
          {can('restrict') && <button type="button" onClick={() => setRestricting((r) => !r)}>Restrict…</button>}
          {can('edit') && <button type="button" onClick={() => setEditing((e) => !e)}>Edit…</button>}
          {can('edit_profile_text') && !can('edit') && <button type="button" onClick={() => setEditing((e) => !e)}>Fix name or bio…</button>}
          {can('password_reset') && (
            <>
              <button type="button" onClick={() => done(staff.sendReset(userId, false), 'Reset email sent.')}>Send password reset</button>
              <button type="button" onClick={() => done(staff.sendReset(userId, true), 'Reset email sent, and every device signed out.')}>Reset and sign out everywhere</button>
            </>
          )}
          {can('revoke_sessions') && <button type="button" onClick={() => done(staff.revokeAll(userId), 'Every device signed out.')}>Sign out every device</button>}
          {can('setup_email') && s.setupPending && <button type="button" onClick={() => done(staff.resendSetup(userId), 'Setup email sent again.')}>Resend setup email</button>}
          {can('deactivate') && !s.deactivatedAt && (
            <ReasonButton label="Deactivate" onDo={(r) => done(staff.deactivate(userId, r), 'Deactivated.')} />
          )}
          {can('reactivate') && s.deactivatedAt && (
            <ReasonButton label="Reactivate" onDo={(r) => done(staff.reactivate(userId, r), 'Reactivated.')} />
          )}
          {can('delete_content') && (
            <ReasonButton label="Remove all their public posts" danger onDo={(r) => done(staff.deleteContent(userId, r), 'Their public posts were removed.')} />
          )}
          {can('moderator') && (
            <ReasonButton
              label={s.role === 'moderator' ? 'Unmake moderator' : 'Make moderator'}
              onDo={(r) => done(staff.setModerator(userId, s.role !== 'moderator', r), s.role === 'moderator' ? 'No longer a moderator.' : 'Now a moderator.')}
            />
          )}
          {can('delete') && <DeleteAccount name={s.displayName} id={userId} />}
        </div>
        {can('password_reset') && (
          <p className="field__hint">Staff never see or set a password, and never see the link — only the normal email is sent.</p>
        )}
      </section>

      {restricting && can('restrict') && (
        <RestrictForm
          detail={data}
          itemId={params.get('item')}
          onDone={async () => {
            setRestricting(false)
            setMessage('Restriction set. The writer has been told.')
            await reload()
          }}
        />
      )}
      {editing && <EditForm detail={data} canAccount={can('edit')} onDone={async () => { setEditing(false); await reload() }} />}

      <section className="prompts__section">
        <h2>Activity</h2>
        <p>
          {data.activity.works} works · {data.activity.publications} publications · {data.activity.comments} comments ·{' '}
          {data.activity.critiques} critiques · {data.activity.submissions} Daily Prompts answers ·{' '}
          {data.activity.invitesSent} invites sent. <Link to={`/people/${userId}`}>Public profile</Link>
        </p>
        <p className="field__hint">Private writing is never shown to staff.</p>
      </section>

      <section className="prompts__section">
        <h2>Restrictions</h2>
        {data.restrictions.length === 0 && <p className="empty">None, ever.</p>}
        <ul className="admin__list">
          {data.restrictions.map((r) => (
            <RestrictionRow key={r.id} r={r} canLift={can('lift')} onChanged={async (said) => { setMessage(said); await reload() }} />
          ))}
        </ul>
      </section>

      <section className="prompts__section">
        <h2>Reports</h2>
        <h3>About this account</h3>
        {data.reportsAbout.length === 0 && <p className="empty">None.</p>}
        <ul className="admin__list">
          {data.reportsAbout.map((r) => (
            <li key={r.itemId}>
              <Link to={`/admin/queue?tab=done&focus=${r.itemId}`}>{r.subjectType}</Link> · {r.reportCount} report{r.reportCount === 1 ? '' : 's'}
              {r.topReason && ` · ${REASON_LABEL[r.topReason] ?? r.topReason}`} · {r.state} · {when(r.lastFlaggedAt)}
            </li>
          ))}
        </ul>
        <h3>By this account</h3>
        {data.reportsBy.length === 0 && <p className="empty">None.</p>}
        <ul className="admin__list">
          {data.reportsBy.map((r, i) => <li key={i}>{r.kind} · {r.reason} · {when(r.createdAt)}</li>)}
        </ul>
      </section>

      {data.sessions && (
        <section className="prompts__section">
          <h2>Signed in on</h2>
          {data.sessions.length === 0 && <p className="empty">Nowhere.</p>}
          <ul className="admin__list">
            {data.sessions.map((sess) => (
              <li key={sess.id}>
                {sess.userAgent ?? 'unknown device'} · {sess.ip ?? 'unknown address'} · since {when(sess.createdAt)}, last seen {when(sess.lastSeenAt)}{' '}
                {can('revoke_sessions') && (
                  <button type="button" className="linklike" onClick={() => done(staff.revokeOne(userId, sess.id), 'Signed out.')}>Sign out</button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="prompts__section">
        <h2>Notes</h2>
        <p className="field__hint">Only staff see these. They can be added to, never edited or removed.</p>
        <form
          className="admin__add"
          onSubmit={async (event) => {
            event.preventDefault()
            await done(staff.addNote(userId, note), 'Note added.')
            setNote('')
          }}
        >
          <textarea aria-label="New note" rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
          <button type="submit" disabled={!note.trim()}>Add note</button>
        </form>
        <ul className="admin__list">
          {data.notes.map((n) => (
            <li key={n.id}><strong>{n.author.displayName}</strong> · {when(n.createdAt)}<br />{n.body}</li>
          ))}
        </ul>
      </section>

      <section className="prompts__section">
        <h2>Log</h2>
        <ul className="admin__list">
          {data.audit.map((a) => (
            <li key={a.id}>
              {when(a.createdAt)} · {a.actor?.displayName ?? 'system'} · <strong>{a.action.replace(/_/g, ' ')}</strong>
              {a.detail && ` — ${a.detail}`}
              {a.reason && <em> ({a.reason})</em>}
            </li>
          ))}
        </ul>
      </section>
    </main>
  )
}

function RestrictionRow({ r, canLift, onChanged }: { r: RestrictionView; canLift: boolean; onChanged: (said: string) => Promise<void> }) {
  const where = r.scope === 'discussion' ? 'in discussions' : 'whole site'
  return (
    <li className={r.inForce ? 'admin__restriction is-in-force' : 'admin__restriction'}>
      <p>
        <strong>{r.kind.replace('_', '-')}</strong> · {where} ·{' '}
        {r.kind === 'warning' ? 'warning' : r.endsAt ? `until ${day(r.endsAt)}` : 'permanent'}
        {r.inForce && <Pill tone="warn">in force</Pill>}
        {r.liftedAt && <Pill>lifted {day(r.liftedAt)}</Pill>}
      </p>
      <p>
        Guideline {r.reasonCode}: {r.reasonText}
        {r.internalNote && <><br /><em>Internal: {r.internalNote}</em></>}
      </p>
      <p className="field__hint">Set {when(r.createdAt)} by {r.createdByName ?? r.createdBy}</p>
      {r.appeal && (
        <p>
          <strong>Appeal ({r.appeal.state}):</strong> {r.appeal.body}
          {r.appeal.decisionText && <><br />Decision: {r.appeal.decisionText}</>}
        </p>
      )}
      {r.inForce && canLift && r.kind !== 'warning' && (
        <div className="admin__actions">
          <ReasonButton label="Lift" onDo={async (reason) => { await staff.lift(r.id, reason); await onChanged('Lifted. The writer has been told.') }} />
          {r.endsAt && <ChangeLength r={r} onDone={onChanged} />}
        </div>
      )}
    </li>
  )
}

function ChangeLength({ r, onDone }: { r: RestrictionView; onDone: (said: string) => Promise<void> }) {
  const [days, setDays] = useState('7')
  return (
    <ReasonButton
      label="Change length"
      extra={
        <label>
          New length
          <select value={days} onChange={(e) => setDays(e.target.value)}>
            <option value="3">3 days</option>
            <option value="7">7 days</option>
            <option value="30">30 days</option>
            <option value="permanent">permanent</option>
          </select>
        </label>
      }
      onDo={async (reason) => {
        await staff.changeRestriction(r.id, { days: days === 'permanent' ? null : Number(days), reason })
        await onDone('Changed. The writer has been told.')
      }}
    />
  )
}

function RestrictForm({ detail, itemId, onDone }: { detail: UserDetail; itemId: string | null; onDone: () => void }) {
  const { user } = useAuth()
  const isAdmin = !!user?.isAdmin
  const [kind, setKind] = useState<'warning' | 'ban' | 'suspend' | 'read_only'>('suspend')
  const [scope, setScope] = useState<'site' | 'discussion'>('discussion')
  const [days, setDays] = useState('3')
  const [code, setCode] = useState('')
  const [text, setText] = useState('')
  const [note, setNote] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const name = detail.summary.displayName

  const readOnlySite = kind === 'read_only' && scope === 'site'
  const permanent = readOnlySite ? false : days === 'permanent'
  const closing = readOnlySite || (kind === 'ban' && scope === 'site' && permanent)
  const permanentSite = scope === 'site' && permanent && kind !== 'warning'
  const deleteDate = new Date(Date.now() + 30 * 86_400_000).toLocaleDateString(undefined, { day: 'numeric', month: 'long', year: 'numeric' })
  // A moderator can set any 3-, 7- or 30-day restriction and permanent ones
  // in discussions; the two closing ones are for administrators.
  const notAllowed = !isAdmin && (closing || permanentSite)

  return (
    <section className="prompts__section admin__restrict">
      <h2>Restrict {name}</h2>
      {detail.ladder.length > 0 && (
        <p className="field__hint">
          Suggested ladder: {detail.ladder.map((s) => s.replace(/:/g, ' ')).join(' → ')}.
          {detail.restrictions.length > 0 && ` They have ${detail.restrictions.length} on record.`} It is only a hint.
        </p>
      )}
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await staff.restrict(detail.summary.id, {
              kind,
              scope,
              days: kind === 'warning' || readOnlySite || permanent ? null : Number(days),
              reasonCode: code,
              reasonText: text,
              internalNote: note || undefined,
              itemIds: itemId ? [itemId] : undefined,
              confirmName: closing ? confirm : undefined,
            })
            onDone()
          } catch (e) {
            setError(errorText(e))
          } finally {
            setBusy(false)
          }
        }}
      >
        <div className="settings__row">
          <label>
            Kind
            <select value={kind} onChange={(e) => setKind(e.target.value as typeof kind)}>
              <option value="warning">Warning (no effect)</option>
              <option value="suspend">Suspend — can read, can't post or publish</option>
              <option value="read_only">Read-only — can read and export only</option>
              <option value="ban">Ban — can't sign in</option>
            </select>
          </label>
          <label>
            Where
            <select value={scope} onChange={(e) => setScope(e.target.value as typeof scope)} disabled={kind === 'warning'}>
              <option value="discussion">Discussions only</option>
              <option value="site">Whole site</option>
            </select>
          </label>
          <label>
            Length
            <select value={readOnlySite ? '30' : days} onChange={(e) => setDays(e.target.value)} disabled={kind === 'warning' || readOnlySite}>
              <option value="3">3 days</option>
              <option value="7">7 days</option>
              <option value="30">30 days</option>
              <option value="permanent">Permanent</option>
            </select>
          </label>
        </div>
        {closing && (
          <div className="admin__danger">
            <p className="error">
              This account, all of its writing and all of its comments will be deleted on {deleteDate}.
              {kind === 'ban' && ' The address goes on the permanent ban list and can never sign up again.'}
              {readOnlySite && ' Until then they can read and export their work.'}
            </p>
            <label>
              Type {name} to confirm
              <input value={confirm} onChange={(e) => setConfirm(e.target.value)} />
            </label>
          </div>
        )}
        {notAllowed && <p className="error">Only an administrator can set this one.</p>}
        <label>
          Community guidelines section
          <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="e.g. 3.2" required />
        </label>
        <label>
          Message to the writer (they will read this)
          <textarea rows={3} value={text} onChange={(e) => setText(e.target.value)} required />
        </label>
        <label>
          Internal note (staff only)
          <textarea rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
        </label>
        {itemId && <p className="field__hint">Linked to the queue item that prompted it.</p>}
        {error && <p className="error">{error}</p>}
        <button
          type="submit"
          className={closing ? 'is-danger' : undefined}
          disabled={busy || notAllowed || !code.trim() || !text.trim() || (closing && confirm.trim() !== name.trim())}
        >
          {kind === 'warning' ? 'Send warning' : 'Set restriction'}
        </button>
      </form>
    </section>
  )
}

function EditForm({ detail, canAccount, onDone }: { detail: UserDetail; canAccount: boolean; onDone: () => void }) {
  const s = detail.summary
  const [displayName, setDisplayName] = useState(s.displayName)
  const [bio, setBio] = useState(s.bio ?? '')
  const [about, setAbout] = useState(s.aboutAuthor ?? '')
  const [email, setEmail] = useState('')
  const [markVerified, setMarkVerified] = useState(false)
  const [resetTwoFactor, setResetTwoFactor] = useState(false)
  const [reason, setReason] = useState('')
  const [error, setError] = useState<string | null>(null)
  return (
    <section className="prompts__section">
      <h2>Edit</h2>
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setError(null)
          try {
            await staff.editUser(s.id, {
              displayName: displayName !== s.displayName ? displayName : undefined,
              bio: bio !== (s.bio ?? '') ? bio : undefined,
              aboutAuthor: about !== (s.aboutAuthor ?? '') ? about : undefined,
              email: email || undefined,
              markVerified: markVerified || undefined,
              resetTwoFactor: resetTwoFactor || undefined,
              reason,
            })
            onDone()
          } catch (e) {
            setError(errorText(e))
          }
        }}
      >
        <label>Display name<input value={displayName} onChange={(e) => setDisplayName(e.target.value)} /></label>
        <label>Bio<textarea rows={2} value={bio} onChange={(e) => setBio(e.target.value)} /></label>
        <label>About the author<textarea rows={2} value={about} onChange={(e) => setAbout(e.target.value)} /></label>
        {canAccount && (
          <>
            <label>
              New email address
              <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
              <span className="field__hint">Both addresses are told, and the account is unconfirmed until the new one is.</span>
            </label>
            <label className="auth__check"><input type="checkbox" checked={markVerified} onChange={(e) => setMarkVerified(e.target.checked)} /><span>Mark the address as confirmed (proved some other way)</span></label>
            {s.twoFactor && (
              <label className="auth__check">
                <input type="checkbox" checked={resetTwoFactor} onChange={(e) => setResetTwoFactor(e.target.checked)} />
                <span>Turn off two-factor sign-in</span>
              </label>
            )}
            {resetTwoFactor && (
              <p className="error">
                This is the one action that lets someone into an account without the second factor. It signs out every device and emails the account.
              </p>
            )}
          </>
        )}
        <label>Reason (required, goes in the log)<textarea rows={2} value={reason} onChange={(e) => setReason(e.target.value)} required /></label>
        {error && <p className="error">{error}</p>}
        <button type="submit" disabled={!reason.trim()}>Save</button>
      </form>
    </section>
  )
}

function DeleteAccount({ name, id }: { name: string; id: string }) {
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [confirm, setConfirm] = useState('')
  const [reason, setReason] = useState('')
  const [shared, setShared] = useState(false)
  const [skip, setSkip] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sharedWorks, setSharedWorks] = useState<string[]>([])
  if (!open) return <button type="button" className="is-danger" onClick={() => setOpen(true)}>Delete…</button>
  return (
    <form
      className="admin__reason admin__danger"
      onSubmit={async (event) => {
        event.preventDefault()
        setError(null)
        try {
          await staff.deleteUser(id, { confirmName: confirm, reason, deleteSharedWorks: shared, skipExport: skip })
          navigate('/admin/accounts')
        } catch (e) {
          setError(errorText(e))
          const details = (e as { details?: { sharedWorks?: string[] } }).details
          if (details?.sharedWorks) setSharedWorks(details.sharedWorks)
        }
      }}
    >
      <p className="error">This deletes the account and everything it owns, now. They are emailed a copy of their data first unless you skip it.</p>
      {sharedWorks.length > 0 && (
        <label className="auth__check">
          <input type="checkbox" checked={shared} onChange={(e) => setShared(e.target.checked)} />
          <span>Other people are working on {sharedWorks.join(', ')}. Delete {sharedWorks.length === 1 ? 'it' : 'them'} too.</span>
        </label>
      )}
      <label className="auth__check"><input type="checkbox" checked={skip} onChange={(e) => setSkip(e.target.checked)} /><span>Don't email them a copy first (say why below)</span></label>
      <label>Type {name} to confirm<input value={confirm} onChange={(e) => setConfirm(e.target.value)} /></label>
      <label>Reason<textarea rows={2} value={reason} onChange={(e) => setReason(e.target.value)} required /></label>
      {error && <p className="error">{error}</p>}
      <div className="critique__actions">
        <button type="submit" className="is-danger" disabled={confirm.trim() !== name.trim() || !reason.trim()}>Delete the account</button>
        <button type="button" onClick={() => setOpen(false)}>Cancel</button>
      </div>
    </form>
  )
}
