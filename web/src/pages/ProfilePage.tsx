import { useCallback, useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { api } from '../api/client'
import type { Profile } from '../api/types'
import { PublicationTile } from '../components/PublicationTile'
import { ReportButton } from '../components/ReportButton'
import { useAuth } from '../state/auth'
import { useStanding } from '../state/standing'

export function ProfilePage() {
  const { userId = '' } = useParams()
  const { user } = useAuth()
  const { blocked } = useStanding()
  const [profile, setProfile] = useState<Profile | null>(null)
  const [editing, setEditing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const mine = user?.id === userId

  const load = useCallback(() => {
    api.profiles
      .get(userId)
      .then((next) => {
        setProfile(next)
        setError(null)
      })
      .catch((e) => setError(e.message))
  }, [userId])

  useEffect(() => {
    load()
  }, [load])

  if (error) return <main className="feed"><p className="error">{error}</p></main>
  if (!profile) return <main className="feed"><p className="empty">Opening…</p></main>

  const toggleFollow = async () => {
    try {
      const next = profile.viewerFollows
        ? await api.profiles.unfollow(profile.id)
        : await api.profiles.follow(profile.id)
      setProfile({ ...profile, viewerFollows: next.following, followers: next.followers })
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not change that')
    }
  }

  return (
    <main className="feed profile">
      <header className="feed__header">
        <div>
          <h1>{profile.displayName}</h1>
          <p className="feed__meta">
            {profile.followers} follower{profile.followers === 1 ? '' : 's'} · following{' '}
            {profile.following} · here since {new Date(profile.joinedAt).toLocaleDateString()}
          </p>
        </div>
        <div className="feed__actions">
          {mine ? (
            <button type="button" onClick={() => setEditing((e) => !e)}>
              {editing ? 'Done' : 'Edit profile'}
            </button>
          ) : (
            <>
              <button
                type="button"
                className={profile.viewerFollows ? 'is-active' : undefined}
                disabled={!!blocked('follow')}
                title={blocked('follow') ?? undefined}
                onClick={() => void toggleFollow()}
              >
                {profile.viewerFollows ? 'Following' : 'Follow'}
              </button>
              <ReportButton subjectType="profile" subjectId={profile.id} />
            </>
          )}
        </div>
      </header>

      {editing && mine ? (
        <ProfileForm
          profile={profile}
          onSaved={() => {
            setEditing(false)
            load()
          }}
        />
      ) : (
        profile.bio && <p className="profile__bio">{profile.bio}</p>
      )}

      <section>
        <h2 className="profile__heading">On offer</h2>
        {profile.publications.length === 0 && (
          <p className="empty">Nothing open for readers right now.</p>
        )}
        <ul className="feed__list">
          {profile.publications.map((card) => (
            <li key={card.id}><PublicationTile card={card} /></li>
          ))}
        </ul>
      </section>

      <section>
        <h2 className="profile__heading">As a reviewer</h2>
        <p className="feed__meta">
          {profile.standing.comments} note{profile.standing.comments === 1 ? '' : 's'} ·{' '}
          {profile.standing.resolved} acted on by the author · {profile.standing.votes} net votes
        </p>
        {profile.reviews.length === 0 ? (
          <p className="empty">No notes on anything you can see.</p>
        ) : (
          <ul className="profile__reviews">
            {profile.reviews.map((review) => (
              <li key={review.publicationId}>
                <Link to={`/review/${review.publicationId}`}>{review.workTitle}</Link>
                {' by '}
                <Link to={`/people/${review.author.id}`}>{review.author.displayName}</Link>
                <span className="feed__meta">
                  {' '}· {review.notes} note{review.notes === 1 ? '' : 's'}
                  {review.resolved > 0 && `, ${review.resolved} acted on`} ·{' '}
                  {new Date(review.lastAt).toLocaleDateString()}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  )
}

function ProfileForm({ profile, onSaved }: { profile: Profile; onSaved: () => void }) {
  const { user, updateUser } = useAuth()
  const [displayName, setDisplayName] = useState(profile.displayName)
  const [bio, setBio] = useState(profile.bio ?? '')
  const [about, setAbout] = useState(user?.aboutAuthor ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  return (
    <form
      className="offer"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          updateUser(await api.profiles.update({ displayName, bio, aboutAuthor: about }))
          onSaved()
        } catch (e) {
          setError(e instanceof Error ? e.message : 'Could not save that')
        } finally {
          setBusy(false)
        }
      }}
    >
      <label>
        Name
        <input value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
      </label>
      <label>
        About you
        <textarea
          rows={5}
          value={bio}
          maxLength={2000}
          onChange={(e) => setBio(e.target.value)}
          placeholder="What you write, and what you like to read."
        />
      </label>
      <label>
        About the author
        <textarea
          rows={5}
          value={about}
          maxLength={3000}
          onChange={(e) => setAbout(e.target.value)}
          placeholder="A paragraph for the back of your books."
        />
        <span className="field__hint">
          Private to you. You can add it as back matter when you export any book.
        </span>
      </label>
      {error && <p className="error">{error}</p>}
      <button type="submit" disabled={busy || !displayName.trim()}>Save</button>
    </form>
  )
}
