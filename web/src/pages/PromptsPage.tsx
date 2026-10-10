import { useCallback, useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ApiError, prompts } from '../api/client'
import type { EnrollmentView, PromptView } from '../api/types'
import { EntryFeed } from '../prompts/EntryFeed'
import { PromptPreferences } from '../prompts/PromptPreferences'
import { browserTimezone, dayName, localTime } from '../prompts/days'

const STATE_LABEL: Record<string, string> = {
  none: 'Not started',
  draft: 'Draft',
  submitted: 'Submitted',
  withdrawn: 'Withdrawn',
}

function PromptCard({ prompt, enrolled, today }: { prompt: PromptView; enrolled: boolean; today: boolean }) {
  return (
    <article className="prompt-card">
      <p className="prompt-card__day">{today ? 'Today' : dayName(prompt.day, true)}</p>
      <blockquote className="prompt-card__body">{prompt.body}</blockquote>
      {enrolled && (
        <p className="prompt-card__status">
          <span className={`badge badge--${prompt.myState}`}>{STATE_LABEL[prompt.myState] ?? prompt.myState}</span>
          <Link className="button-link" to={`/prompts/${prompt.day}/write`}>
            {prompt.myState === 'none' ? 'Write' : 'Continue'}
          </Link>
          {prompt.myEntryId && prompt.myState === 'submitted' && (
            <Link to={`/prompts/${prompt.day}/entries/${prompt.myEntryId}`}>Read yours</Link>
          )}
          <span className="field__hint">
            {today ? `Open until ${localTime(prompt.closesAt)} your time` : 'You can still write on it'}
            {prompt.submissions > 0 && ` · ${prompt.submissions} submitted`}
          </span>
        </p>
      )}
    </article>
  )
}

function Archive() {
  const [days, setDays] = useState<PromptView[]>([])
  const [next, setNext] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)
  const load = useCallback(async (before?: string) => {
    const page = await prompts.archive(before, 14)
    setDays((current) => (before ? [...current, ...page.prompts] : page.prompts))
    setNext(page.nextBefore ?? null)
    setLoaded(true)
  }, [])
  useEffect(() => {
    void load().catch(() => setLoaded(true))
  }, [load])
  return (
    <section className="prompts__section" aria-labelledby="archive-heading">
      <h2 id="archive-heading">Archive</h2>
      {loaded && days.length === 0 && <p className="empty">No earlier prompts yet.</p>}
      <ul className="prompts__archive">
        {days.map((p) => (
          <li key={p.day}>
            <Link to={`/prompts/${p.day}`}>
              <span className="prompts__archive-day">{dayName(p.day, true)}</span>
              <span className="prompts__archive-body">{p.body}</span>
            </Link>
            <span className={`badge badge--${p.myState}`}>{p.myState === 'none' ? 'Not answered' : STATE_LABEL[p.myState]}</span>
          </li>
        ))}
      </ul>
      {next && <button type="button" onClick={() => void load(next)}>Earlier</button>}
    </section>
  )
}

/**
 * Daily Prompts: a prompt a day. Anyone signed in sees today's; joining opens
 * up writing an answer, everyone's answers, and the archive's.
 */
export function PromptsPage() {
  const [today, setToday] = useState<PromptView | null>(null)
  const [enrollment, setEnrollment] = useState<EnrollmentView | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    Promise.all([prompts.today(), prompts.enrollment()])
      .then(([t, e]) => {
        setToday(t.prompt)
        setEnrollment(e)
      })
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not load today’s prompt'))
  }, [])

  const enrolled = !!enrollment?.enrolled

  const join = async () => {
    setBusy(true)
    try {
      setEnrollment(await prompts.join(browserTimezone()))
      setToday(await prompts.day(today!.day))
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not join')
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="feed prompts">
      <header className="feed__header">
        <h1>Daily Prompts</h1>
      </header>
      {error && <p className="error">{error}</p>}
      {today && <PromptCard prompt={today} enrolled={enrolled} today />}
      {enrollment && !enrolled && (
        <section className="prompts__join">
          <p>
            A prompt every day, for whatever you are writing or for nothing in particular. Write an answer, keep it to
            yourself or share it with everyone else taking part, and read and critique theirs. An answer that wants to keep
            going can become a project of its own.
          </p>
          <p className="field__hint">Joining does not turn on the email; you can do that afterwards.</p>
          <button type="button" disabled={busy || !today} onClick={() => void join()}>Join Daily Prompts</button>
        </section>
      )}
      {enrolled && today && <EntryFeed day={today.day} heading="Today's submissions" />}
      {enrolled && <Archive />}
      {enrolled && enrollment && <PromptPreferences enrollment={enrollment} onChange={setEnrollment} />}
    </main>
  )
}

/** One past day: its prompt, the writer's own answer, and everyone's, late ones too. */
export function PromptDayPage() {
  const { day = '' } = useParams()
  const [prompt, setPrompt] = useState<PromptView | null>(null)
  const [enrolled, setEnrolled] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setPrompt(null)
    Promise.all([prompts.day(day), prompts.enrollment()])
      .then(([p, e]) => {
        setPrompt(p)
        setEnrolled(e.enrolled)
      })
      .catch((e) =>
        setError(e instanceof ApiError && e.status === 404 ? 'There is no prompt for that day.' : e.message),
      )
  }, [day])

  const isToday = prompt ? Date.parse(prompt.closesAt) > Date.now() && Date.parse(prompt.opensAt) <= Date.now() : false

  return (
    <main className="feed prompts">
      <p className="prompts__back"><Link to="/prompts">← Daily Prompts</Link></p>
      {error && <p className="error">{error}</p>}
      {prompt && <PromptCard prompt={prompt} enrolled={enrolled} today={isToday} />}
      {prompt && !enrolled && (
        <p className="field__hint">
          <Link to="/prompts">Join Daily Prompts</Link> to write on it and read the answers.
        </p>
      )}
      {prompt && enrolled && <EntryFeed day={prompt.day} heading={isToday ? "Today's submissions" : 'Submissions'} />}
    </main>
  )
}
