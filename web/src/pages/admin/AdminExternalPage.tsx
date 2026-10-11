import { useEffect, useState } from 'react'
import { staff } from '../../api/client'
import type { ExternalTestResult, ModerationSettings } from '../../api/types'
import { errorText, useLoad } from './common'

const PROVIDERS: [string, string][] = [
  ['none', 'None — nothing is sent anywhere'],
  ['stream', 'Stream-style hosted moderation'],
  ['stub', 'Local stub (development and tests)'],
]

const SURFACE_LABEL: Record<string, string> = {
  comment: 'comments',
  question: 'reader questions',
  answer: 'reader answers',
  entry: 'Daily Prompts answers',
  critique: 'critiques',
  display_name: 'display names',
  bio: 'bios',
  about_author: 'about the author',
  publication_title: 'publication titles',
  blurb: 'blurbs and prompts',
}

/** The hosted moderation platform: which, what is sent to it, and how its verdicts are used. */
export function AdminExternalPage() {
  const { data, error, reload } = useLoad(() => staff.external(), [])
  const settings = useLoad(() => staff.settings(), [])
  const [provider, setProvider] = useState('none')
  const [baseUrl, setBaseUrl] = useState('')
  const [surfaces, setSurfaces] = useState<string[]>([])
  const [removeAs, setRemoveAs] = useState('hold')
  const [banScope, setBanScope] = useState('discussion')
  const [apiKey, setApiKey] = useState('')
  const [webhookSecret, setWebhookSecret] = useState('')
  const [message, setMessage] = useState<string | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [text, setText] = useState('')
  const [verdict, setVerdict] = useState<ExternalTestResult | null>(null)
  const [local, setLocal] = useState<ModerationSettings | null>(null)

  useEffect(() => {
    if (!data) return
    setProvider(data.config.provider)
    setBaseUrl(data.config.baseUrl ?? '')
    setSurfaces(data.config.surfaces ?? [])
    setRemoveAs(data.config.removeAs ?? 'hold')
    setBanScope(data.config.banScope ?? 'discussion')
  }, [data])
  useEffect(() => { if (settings.data) setLocal(settings.data) }, [settings.data])

  return (
    <main className="feed admin">
      <header className="feed__header"><h1>External moderation</h1></header>
      <p className="field__hint">
        A hosted platform classifies public text with AI and its own blocklists and recommends an action. Only the
        text, the kind of thing it is, and an opaque author reference are sent — never an email or a name, and never
        private writing. A platform that is down never stops anyone posting.
      </p>
      {(error || problem) && <p className="error">{problem ?? error}</p>}
      {message && <p className="banner" role="status">{message}</p>}
      {data && (
        <p className="field__hint">
          {data.backlog > 0 ? `${data.backlog} post${data.backlog === 1 ? '' : 's'} waiting for the platform to come back. ` : ''}
          Webhook URL: <code>{data.webhookUrl}</code>
        </p>
      )}
      <form
        className="admin__reason"
        onSubmit={async (event) => {
          event.preventDefault()
          setProblem(null)
          try {
            await staff.saveExternal({
              provider, baseUrl, surfaces, removeAs, banScope,
              apiKey: apiKey || undefined,
              webhookSecret: webhookSecret || undefined,
            })
            setApiKey('')
            setWebhookSecret('')
            setMessage('Saved.')
            await reload()
          } catch (e) {
            setProblem(errorText(e))
          }
        }}
      >
        <label>
          Platform
          <select value={provider} onChange={(e) => setProvider(e.target.value)}>
            {PROVIDERS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
        </label>
        <label>Base URL<input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://…" /></label>
        <label>
          API key {data?.apiKeySet && <em>(set; leave blank to keep it)</em>}
          <input type="password" autoComplete="off" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
        </label>
        <label>
          Webhook secret {data?.webhookSecretSet && <em>(set; leave blank to keep it)</em>}
          <input type="password" autoComplete="off" value={webhookSecret} onChange={(e) => setWebhookSecret(e.target.value)} />
        </label>
        <p className="field__hint">Both are stored encrypted and never shown again.</p>
        <fieldset className="admin__surfaces">
          <legend>Checked</legend>
          {(data?.surfaces ?? []).map((s) => (
            <label key={s} className="auth__check">
              <input type="checkbox" checked={surfaces.includes(s)} onChange={(e) => setSurfaces(e.target.checked ? [...surfaces, s] : surfaces.filter((x) => x !== s))} />
              <span>{SURFACE_LABEL[s] ?? s}</span>
            </label>
          ))}
        </fieldset>
        <div className="settings__row">
          <label>
            When the platform says remove
            <select value={removeAs} onChange={(e) => setRemoveAs(e.target.value)}>
              <option value="hold">Hold it for review (default)</option>
              <option value="block">Refuse the post</option>
            </select>
          </label>
          <label>
            A temporary ban from the platform applies to
            <select value={banScope} onChange={(e) => setBanScope(e.target.value)}>
              <option value="discussion">Discussions only (default)</option>
              <option value="site">The whole site</option>
            </select>
          </label>
        </div>
        <p className="field__hint">A permanent ban from the platform is never applied automatically: it becomes a queue item for an administrator.</p>
        <button type="submit">Save</button>
      </form>

      <section className="prompts__section">
        <h2>Test a check</h2>
        <textarea aria-label="Text to check" rows={3} value={text} onChange={(e) => setText(e.target.value)} />
        <button type="button" disabled={!text.trim() || provider === 'none'} onClick={() => void staff.testExternal(text).then(setVerdict).catch((e) => setProblem(errorText(e)))}>
          Ask the platform
        </button>
        {verdict && <p>It recommends <strong>{verdict.recommended}</strong>{verdict.labels.length > 0 && ` (${verdict.labels.join(', ')})`}.</p>}
      </section>

      {local && (
        <section className="prompts__section">
          <h2>Thresholds and the escalation ladder</h2>
          <form
            className="admin__reason"
            onSubmit={async (event) => {
              event.preventDefault()
              setProblem(null)
              try {
                await staff.saveSettings(local)
                setMessage('Saved.')
              } catch (e) {
                setProblem(errorText(e))
              }
            }}
          >
            <p className="field__hint">An account is flagged when it crosses any of these.</p>
            <div className="settings__row">
              <label>Reporters<input type="number" min={1} value={local.thresholds.reporters} onChange={(e) => setLocal({ ...local, thresholds: { ...local.thresholds, reporters: Number(e.target.value) } })} /></label>
              <label>…in days<input type="number" min={1} value={local.thresholds.reporterDays} onChange={(e) => setLocal({ ...local, thresholds: { ...local.thresholds, reporterDays: Number(e.target.value) } })} /></label>
            </div>
            <div className="settings__row">
              <label>Removals<input type="number" min={1} value={local.thresholds.removals} onChange={(e) => setLocal({ ...local, thresholds: { ...local.thresholds, removals: Number(e.target.value) } })} /></label>
              <label>…in days<input type="number" min={1} value={local.thresholds.removalDays} onChange={(e) => setLocal({ ...local, thresholds: { ...local.thresholds, removalDays: Number(e.target.value) } })} /></label>
            </div>
            <label>Comments in an account's first hour<input type="number" min={1} value={local.thresholds.commentsFirstHour} onChange={(e) => setLocal({ ...local, thresholds: { ...local.thresholds, commentsFirstHour: Number(e.target.value) } })} /></label>
            <label>
              Ladder, one step per line (warning, suspend:discussion:3, ban:site:permanent…)
              <textarea rows={5} value={local.ladder.join('\n')} onChange={(e) => setLocal({ ...local, ladder: e.target.value.split('\n').map((l) => l.trim()).filter(Boolean) })} />
            </label>
            <button type="submit">Save</button>
          </form>
        </section>
      )}
    </main>
  )
}
