import { useEffect, useMemo, useRef, useState } from 'react'
import { api, downloadDraftExport, exporting, type ExportTarget } from '../api/client'
import type {
  ExportFormat,
  ExportIssue,
  ExportOptions,
  ExportProfile,
  PreflightReport,
  Version,
} from '../api/types'
import { chapterName, sceneName } from '../naming'

interface Props {
  /** A frozen version to export; or, without one, the current draft of `workId`. */
  version?: Version
  workId?: string
  onClose: () => void
  /** Jumps to the place an issue is about, in the editor. */
  onGoTo?: (location: { chapterId: string; sceneId?: string | null }) => void
}

type Format = ExportFormat | 'project'

const FORMATS: { value: Format; label: string; note: string; draftOnly?: boolean }[] = [
  { value: 'docx', label: 'Word (.docx)', note: 'What agents and magazines ask for' },
  { value: 'pdf', label: 'PDF', note: 'A manuscript in Courier, or a book with embedded type' },
  { value: 'epub', label: 'EPUB', note: 'For beta readers and e-readers' },
  { value: 'html', label: 'HTML', note: 'A web page, clean markup, or a zip of pages' },
  { value: 'txt', label: 'Plain text', note: 'For pasting into a submission form' },
  {
    value: 'project',
    label: 'Project (.zip)',
    note:
      'The whole book: project.json, which rebuilds it in another Just Type Studio, and each scene as a Markdown file',
    draftOnly: true,
  },
]

const FONTS: [string, string][] = [
  ['georgia', 'Georgia'],
  ['garamond', 'Garamond'],
  ['palatino', 'Palatino'],
  ['times', 'Times New Roman'],
  ['baskerville', 'Baskerville'],
  ['helvetica', 'Helvetica'],
  ['verdana', 'Verdana'],
  ['courier', 'Courier'],
]

/** Which typography settings each format can carry. Anything else is marked, not hidden. */
const STYLE_SUPPORT: Record<ExportFormat, Set<string>> = {
  docx: new Set(['font', 'size', 'spacing', 'align', 'indent', 'paragraphSpacing', 'headingStyle', 'sceneBreak', 'hyphenate']),
  epub: new Set(['font', 'size', 'spacing', 'align', 'indent', 'paragraphSpacing', 'headingStyle', 'sceneBreak', 'dropCaps', 'hyphenate']),
  html: new Set(['font', 'size', 'spacing', 'align', 'indent', 'paragraphSpacing', 'headingStyle', 'sceneBreak', 'dropCaps', 'hyphenate']),
  txt: new Set(['sceneBreak']),
  pdf: new Set(['font', 'size', 'spacing', 'align', 'indent', 'paragraphSpacing', 'headingStyle', 'sceneBreak', 'hyphenate']),
}

const SCENE_BREAKS: [string, string][] = [
  ['⁂', '⁂'],
  ['* * *', '* * *'],
  ['#', '#'],
  ['', 'A blank line'],
]

interface Chapter {
  id: string
  title: string
  role: string
  scenes: { id: string; title: string }[]
}

const DEFAULTS: ExportOptions = {
  format: 'docx',
  content: { scope: { kind: 'all' }, frontMatter: true },
  style: { preset: 'manuscript' },
}

/** Options compared by what they say, not by which empty fields are spelt out. */
function canon(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canon)
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, v]) => v !== null && v !== undefined && v !== '')
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => [k, canon(v)]),
    )
  }
  return value
}

/** The part of the options a profile holds: how the file looks, not what is in it. */
function presentation(o: ExportOptions): string {
  const copy: ExportOptions = {
    ...o,
    content: { ...o.content, scope: undefined },
    metadata: undefined,
    profile: undefined,
  }
  return JSON.stringify(canon(copy))
}

/** For the per-format blocks that belong to one format only. */
function forFormat(o: ExportOptions, format: ExportFormat): ExportOptions {
  return {
    ...o,
    format,
    pdf: format === 'pdf' ? o.pdf : null,
    epub: format === 'epub' ? o.epub : null,
    html: format === 'html' ? o.html : null,
  }
}

/**
 * Exports of a version come from a frozen copy — a file you send someone has to
 * be a thing that stays the same. The current draft can be exported too, as it
 * reads at this moment, for when you want the book out now — or as a project,
 * to keep or to move to another instance.
 *
 * Every option here is a field of one `ExportOptions` document; the dialog
 * edits it, asks the server what is wrong with it as it changes, and sends the
 * same document to build the file.
 */
export function ExportDialog({ version, workId: workIdProp, onClose, onGoTo }: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const workId = version?.workId ?? workIdProp
  const target: ExportTarget | null = version ? { version: version.id } : workId ? { work: workId } : null

  const [options, setOptions] = useState<ExportOptions>(DEFAULTS)
  const [project, setProject] = useState(!version)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const [chapters, setChapters] = useState<Chapter[]>([])
  const [ticked, setTicked] = useState<Set<string>>(new Set())
  const [profiles, setProfiles] = useState<{ builtin: ExportProfile[]; custom: ExportProfile[] }>({
    builtin: [],
    custom: [],
  })
  const [profileId, setProfileId] = useState('')
  const [lastByFormat, setLastByFormat] = useState<Record<string, ExportOptions>>({})
  const [report, setReport] = useState<PreflightReport | null>(null)
  const [more, setMore] = useState<ExportFormat[]>([])

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  // The book's shape, for choosing what goes in.
  useEffect(() => {
    let live = true
    const done = (list: Chapter[]) => {
      if (!live) return
      setChapters(list)
      setTicked(new Set(list.flatMap((c) => c.scenes.map((s) => s.id))))
    }
    if (version) {
      void api.versions.get(version.id).then((detail) => {
        const grouped: Chapter[] = []
        for (const entry of detail.entries) {
          let chapter = grouped.find((c) => c.id === entry.chapterId)
          if (!chapter) {
            chapter = { id: entry.chapterId, title: entry.chapterTitle, role: entry.chapterRole, scenes: [] }
            grouped.push(chapter)
          }
          chapter.scenes.push({ id: entry.sceneId, title: entry.sceneTitle })
        }
        done(grouped)
      }).catch(() => undefined)
    } else if (workId) {
      void api.works.outline(workId).then((outline) => {
        let numbered = 0
        done(
          outline.chapters.map((c) => ({
            id: c.id,
            role: c.role,
            title: c.role === 'chapter' ? chapterName(c.title, ++numbered) : c.title || (c.role === 'front' ? 'Front matter' : 'Back matter'),
            scenes: c.scenes.map((s, i) => ({ id: s.id, title: sceneName(s.title, i + 1) })),
          })),
        )
      }).catch(() => undefined)
    }
    return () => {
      live = false
    }
  }, [version, workId])

  // Profiles, and what was used last time.
  useEffect(() => {
    let live = true
    void exporting.profiles().then((p) => live && setProfiles(p)).catch(() => undefined)
    if (workId && !version) {
      void exporting.last(workId).then((last) => {
        if (!live) return
        const byFormat: Record<string, ExportOptions> = {}
        for (const item of last.byFormat) byFormat[item.format] = item.options
        setLastByFormat(byFormat)
        if (last.latest) {
          // Open with the last settings, but not the last selection: the book
          // may have changed shape since.
          const o = last.latest.options
          setOptions({
            ...o,
            content: { ...o.content, scope: { kind: 'all' } },
            metadata: o.metadata,
          })
          setProfileId('')
          setProject(false)
        }
      }).catch(() => undefined)
    }
    return () => {
      live = false
    }
  }, [workId, version])

  const content = options.content ?? {}
  const style = options.style ?? { preset: 'manuscript' }
  const format = (options.format ?? 'docx') as ExportFormat
  const book = style.preset === 'book'
  const supported = STYLE_SUPPORT[format]

  const patch = (change: Partial<ExportOptions>) => setOptions((o) => ({ ...o, ...change }))
  const patchContent = (change: NonNullable<ExportOptions['content']>) =>
    setOptions((o) => ({ ...o, content: { ...o.content, ...change } }))
  const patchStyle = (change: NonNullable<ExportOptions['style']>) =>
    setOptions((o) => ({ ...o, style: { ...o.style, ...change } }))
  const patchMeta = (change: NonNullable<ExportOptions['metadata']>) =>
    setOptions((o) => ({ ...o, metadata: { ...o.metadata, ...change } }))

  // What goes out, from the ticks: a whole chapter as its id, otherwise scene by scene.
  const everything = chapters.every((c) => c.scenes.every((s) => ticked.has(s.id)))
  const scopeOf = (): NonNullable<ExportOptions['content']>['scope'] => {
    if (content.scope?.kind !== 'selected') return { kind: 'all' }
    const chapterIds: string[] = []
    const sceneIds: string[] = []
    for (const c of chapters) {
      const mine = c.scenes.filter((s) => ticked.has(s.id))
      if (mine.length === c.scenes.length && mine.length > 0) chapterIds.push(c.id)
      else sceneIds.push(...mine.map((s) => s.id))
    }
    return { kind: 'selected', chapterIds, sceneIds }
  }
  const sendable: ExportOptions = useMemo(
    () => forFormat({ ...options, content: { ...options.content, scope: scopeOf() } }, format),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [options, ticked, chapters, format],
  )

  // Ask the server what is wrong, once typing settles.
  const key = JSON.stringify(sendable)
  useEffect(() => {
    if (!target || project) return
    let live = true
    const timer = setTimeout(() => {
      void exporting.preflight(target, sendable).then((r) => live && setReport(r)).catch(() => live && setReport(null))
    }, 350)
    return () => {
      live = false
      clearTimeout(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, project])

  const chosenProfile = [...profiles.builtin, ...profiles.custom].find((p) => p.id === profileId)
  const modified = !!chosenProfile && presentation(chosenProfile.options) !== presentation(sendable)

  const applyProfile = (profile: ExportProfile) => {
    setOptions((o) => ({
      ...profile.options,
      content: { ...profile.options.content, scope: o.content?.scope },
      metadata: o.metadata,
      profile: profile.name,
    }))
    if (profile.notes.length) setNotice(profile.notes.join(' '))
    else setNotice(null)
    setProject(false)
  }

  const chooseFormat = (value: Format) => {
    if (value === 'project') {
      setProject(true)
      return
    }
    setProject(false)
    // The settings last used for this format, when there are some; otherwise
    // the current ones, carried over.
    const last = lastByFormat[value]
    setOptions((o) => {
      if (last) return { ...last, content: { ...last.content, scope: o.content?.scope }, metadata: o.metadata }
      return forFormat(o, value)
    })
    setProfileId('')
  }

  const blocked = !project && (!report || !report.ok)
  const errors = report?.issues.filter((i) => i.severity === 'error') ?? []

  const run = async (job: () => Promise<{ filename: string; warnings: number } | void>) => {
    setBusy(true)
    setError(null)
    try {
      const saved = await job()
      if (saved && saved.warnings > 0) {
        setNotice(`${saved.filename} was saved with ${saved.warnings} warning${saved.warnings === 1 ? '' : 's'}.`)
      } else {
        ref.current?.close()
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not build that file')
    } finally {
      setBusy(false)
    }
  }

  const issue = (i: ExportIssue, index: number) => (
    <li key={`${i.code}-${index}`} className={`export-issue export-issue--${i.severity}`}>
      <span aria-hidden>{i.severity === 'error' ? '⛔' : i.severity === 'warning' ? '⚠' : 'ℹ'}</span>{' '}
      {i.message}
      {i.location && onGoTo && (
        <>
          {' '}
          <button
            type="button"
            className="linklike"
            onClick={() => {
              onGoTo({ chapterId: i.location!.chapterId, sceneId: i.location!.sceneId })
              ref.current?.close()
            }}
          >
            Go there
          </button>
        </>
      )}
    </li>
  )

  const tickChapter = (chapter: Chapter, on: boolean) =>
    setTicked((t) => {
      const next = new Set(t)
      for (const s of chapter.scenes) on ? next.add(s.id) : next.delete(s.id)
      return next
    })

  const note = (field: string) =>
    !supported.has(field) && book ? (
      <span className="field__hint">Not applied in {format.toUpperCase()}.</span>
    ) : null

  const hasComments = (content.comments ?? 'off') !== 'off'
  const chosen = FORMATS.find((f) => f.value === (project ? 'project' : format)) ?? FORMATS[0]
  const formats = FORMATS.filter((f) => !f.draftOnly || !version)

  return (
    <dialog
      ref={ref}
      className="dialog dialog--export"
      aria-labelledby="export-title"
      onClose={onClose}
      // A click on the backdrop is a click on the dialog element itself.
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="export-title">{version ? `Export ${version.label}` : 'Export the current draft'}</h2>

      {!project && (
        <label>
          Profile
          <select
            value={profileId}
            onChange={(e) => {
              setProfileId(e.target.value)
              const p = [...profiles.builtin, ...profiles.custom].find((x) => x.id === e.target.value)
              if (p) applyProfile(p)
            }}
          >
            <option value="">Custom settings</option>
            <optgroup label="Built in">
              {profiles.builtin.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </optgroup>
            {profiles.custom.length > 0 && (
              <optgroup label="Mine">
                {profiles.custom.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </optgroup>
            )}
          </select>
          <span className="field__hint">
            {chosenProfile?.description ?? 'A profile is how the file looks, not what is in it.'}
            {modified && ' (modified) '}
            {modified && chosenProfile && (
              <button type="button" className="linklike" onClick={() => applyProfile(chosenProfile)}>Restore</button>
            )}
          </span>
        </label>
      )}

      <label>
        Type
        <select value={chosen.value} onChange={(e) => chooseFormat(e.target.value as Format)}>
          {formats.map((option) => (
            <option key={option.value} value={option.value}>{option.label}</option>
          ))}
        </select>
        <span className="field__hint">{chosen.note}</span>
      </label>

      {project && (
        <p className="field__hint">A project has no settings: it is the whole book, as it stands.</p>
      )}

      {!project && (
        <>
          <fieldset className="export-group">
            <legend>Layout</legend>
            <label>
              Laid out as
              <select
                value={style.preset}
                onChange={(e) => patchStyle({ preset: e.target.value as 'manuscript' | 'book' })}
              >
                <option value="manuscript">Standard manuscript format</option>
                <option value="book">A finished book</option>
              </select>
              <span className="field__hint">
                {book
                  ? 'Proportional type, chapter openings, a glyph for scene breaks.'
                  : 'Courier, double spaced, # scene breaks, contact block and running header. Standard manuscript format fixes the typography.'}
              </span>
            </label>

            {format === 'pdf' && book && (
              <div className="export-grid">
                <label>
                  Page size
                  <select
                    value={options.pdf?.pageSize ?? 'letter'}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), pageSize: e.target.value as NonNullable<ExportOptions['pdf']>['pageSize'] } })}
                  >
                    <option value="letter">Letter</option>
                    <option value="a4">A4</option>
                    <option value="trade5x8">5 × 8 in</option>
                    <option value="trade5_5x8_5">5.5 × 8.5 in</option>
                    <option value="trade6x9">6 × 9 in</option>
                  </select>
                </label>
                <label>
                  Margin (inches)
                  <input
                    type="number" min={0.25} max={3} step={0.05}
                    value={options.pdf?.marginIn ?? 1}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), marginIn: Number(e.target.value) } })}
                  />
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={options.pdf?.pageNumbers ?? true}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), pageNumbers: e.target.checked } })} />
                  Page numbers
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={!!options.pdf?.romanFrontMatter}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), romanFrontMatter: e.target.checked } })} />
                  Roman numerals for front matter
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={!!options.pdf?.chaptersOnRight}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), chaptersOnRight: e.target.checked } })} />
                  Chapters start on right-hand pages
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={!!options.pdf?.landscape}
                    onChange={(e) => patch({ pdf: { ...(options.pdf ?? {}), landscape: e.target.checked } })} />
                  Landscape
                </label>
              </div>
            )}

            <div className="export-grid" aria-disabled={!book}>
              <label>
                Typeface
                <select
                  disabled={!book}
                  value={book ? style.font ?? 'georgia' : 'courier'}
                  onChange={(e) => patchStyle({ font: e.target.value })}
                >
                  {FONTS.map(([k, v]) => <option key={k} value={k}>{v}</option>)}
                </select>
                {note('font')}
              </label>
              <label>
                Size
                <input
                  type="number" min={8} max={24} step={0.5} disabled={!book}
                  value={book ? style.sizePt ?? 12 : 12}
                  onChange={(e) => patchStyle({ sizePt: Number(e.target.value) })}
                />
                {note('size')}
              </label>
              <label>
                Line spacing
                <select
                  disabled={!book}
                  value={book ? style.lineSpacing ?? '1.15' : 'double'}
                  onChange={(e) => patchStyle({ lineSpacing: e.target.value as NonNullable<ExportOptions['style']>['lineSpacing'] })}
                >
                  <option value="single">Single</option>
                  <option value="1.15">1.15</option>
                  <option value="1.5">1.5</option>
                  <option value="double">Double</option>
                </select>
                {note('spacing')}
              </label>
              <label>
                Alignment
                <select
                  disabled={!book}
                  value={book ? style.align ?? 'justify' : 'left'}
                  onChange={(e) => patchStyle({ align: e.target.value as 'left' | 'justify' })}
                >
                  <option value="left">Left</option>
                  <option value="justify">Justified</option>
                </select>
                {note('align')}
              </label>
            </div>
            {!book && <p className="field__hint">Standard manuscript format fixes these.</p>}

            {book && (
              <details>
                <summary>Advanced typography</summary>
                <div className="export-grid">
                  <label>
                    Paragraph indent (em)
                    <input
                      type="number" min={0} max={4} step={0.25}
                      value={style.paragraphIndentEm ?? 1.5}
                      onChange={(e) => patchStyle({ paragraphIndentEm: Number(e.target.value) })}
                    />
                    {note('indent')}
                  </label>
                  <label>
                    Space between paragraphs (em)
                    <input
                      type="number" min={0} max={4} step={0.25}
                      value={style.paragraphSpacingEm ?? 0}
                      onChange={(e) => patchStyle({ paragraphSpacingEm: Number(e.target.value) })}
                    />
                    {note('paragraphSpacing')}
                  </label>
                  <label>
                    Chapter headings
                    <select
                      value={style.headingStyle ?? 'plain'}
                      onChange={(e) => patchStyle({ headingStyle: e.target.value as NonNullable<ExportOptions['style']>['headingStyle'] })}
                    >
                      <option value="plain">Plain</option>
                      <option value="smallCaps">Small capitals</option>
                      <option value="centred">Centred</option>
                    </select>
                    {note('headingStyle')}
                  </label>
                  <label>
                    Scene break
                    <select
                      value={SCENE_BREAKS.some(([v]) => v === (style.sceneBreak ?? '⁂')) ? style.sceneBreak ?? '⁂' : '__custom'}
                      onChange={(e) => patchStyle({ sceneBreak: e.target.value === '__custom' ? 'custom' : e.target.value })}
                    >
                      {SCENE_BREAKS.map(([v, l]) => <option key={l} value={v}>{l}</option>)}
                      <option value="__custom">Custom text…</option>
                    </select>
                    {!SCENE_BREAKS.some(([v]) => v === (style.sceneBreak ?? '⁂')) && (
                      <input
                        aria-label="Custom scene break"
                        maxLength={40}
                        value={style.sceneBreak ?? ''}
                        onChange={(e) => patchStyle({ sceneBreak: e.target.value })}
                      />
                    )}
                    {note('sceneBreak')}
                  </label>
                </div>
                <label className="export-check">
                  <input type="checkbox" checked={!!style.dropCaps} onChange={(e) => patchStyle({ dropCaps: e.target.checked })} />
                  Drop caps on chapter openings {note('dropCaps')}
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={!!style.hyphenate} onChange={(e) => patchStyle({ hyphenate: e.target.checked })} />
                  Hyphenate {note('hyphenate')}
                </label>
              </details>
            )}

            {format === 'epub' && (
              <label className="export-check">
                <input
                  type="checkbox"
                  checked={options.epub?.splitChapters ?? true}
                  onChange={(e) => patch({ epub: { ...(options.epub ?? {}), splitChapters: e.target.checked } })}
                />
                One file per chapter (kinder to small e-readers)
              </label>
            )}
            {format === 'html' && (
              <label>
                HTML as
                <select
                  value={options.html?.mode ?? 'styled'}
                  onChange={(e) => patch({ html: { mode: e.target.value as 'styled' | 'clean' | 'package' } })}
                >
                  <option value="styled">A styled page</option>
                  <option value="clean">Clean markup, no styles (to paste into a site)</option>
                  <option value="package">A zip of pages, one per chapter</option>
                </select>
              </label>
            )}
          </fieldset>

          <fieldset className="export-group">
            <legend>Content</legend>
            <div className="export-choice" role="radiogroup" aria-label="What to export">
              <label className="export-check">
                <input
                  type="radio" name="export-scope"
                  checked={content.scope?.kind !== 'selected'}
                  onChange={() => patchContent({ scope: { kind: 'all' } })}
                />
                Entire book
              </label>
              <label className="export-check">
                <input
                  type="radio" name="export-scope"
                  checked={content.scope?.kind === 'selected'}
                  onChange={() => patchContent({ scope: { kind: 'selected' } })}
                />
                Selected chapters and scenes
              </label>
            </div>

            {content.scope?.kind === 'selected' && (
              <div className="export-tree">
                <p className="export-tree__tools">
                  <button type="button" className="linklike" onClick={() => setTicked(new Set(chapters.flatMap((c) => c.scenes.map((s) => s.id))))}>Select all</button>
                  {' · '}
                  <button type="button" className="linklike" onClick={() => setTicked(new Set())}>Select none</button>
                </p>
                <ul>
                  {chapters.map((chapter) => {
                    const count = chapter.scenes.filter((s) => ticked.has(s.id)).length
                    const all = count === chapter.scenes.length && count > 0
                    return (
                      <li key={chapter.id}>
                        <label className="export-check">
                          <input
                            type="checkbox"
                            checked={all}
                            ref={(el) => { if (el) el.indeterminate = count > 0 && !all }}
                            onChange={(e) => tickChapter(chapter, e.target.checked)}
                          />
                          {chapter.title}
                          {chapter.role !== 'chapter' && <span className="outline__badge">{chapter.role === 'front' ? 'front matter' : 'back matter'}</span>}
                        </label>
                        <ul>
                          {chapter.scenes.map((scene) => (
                            <li key={scene.id}>
                              <label className="export-check">
                                <input
                                  type="checkbox"
                                  checked={ticked.has(scene.id)}
                                  onChange={(e) =>
                                    setTicked((t) => {
                                      const next = new Set(t)
                                      e.target.checked ? next.add(scene.id) : next.delete(scene.id)
                                      return next
                                    })
                                  }
                                />
                                {scene.title}
                              </label>
                            </li>
                          ))}
                        </ul>
                      </li>
                    )
                  })}
                </ul>
                {!everything && ticked.size === 0 && <p className="error">Nothing is selected.</p>}
                <p className="field__hint">The title page still shows the whole book's word count, which is what an agent wants for a partial submission.</p>
              </div>
            )}

            <label className="export-check">
              <input type="checkbox" checked={content.frontMatter ?? true} onChange={(e) => patchContent({ frontMatter: e.target.checked })} />
              Include front matter (title page, dedication, epigraph, copyright line)
            </label>
            <label className="export-check">
              <input
                type="checkbox"
                checked={content.backMatter ?? book}
                onChange={(e) => patchContent({ backMatter: e.target.checked })}
              />
              Include back matter (acknowledgments, afterword, appendix)
            </label>
            <label className="export-check">
              <input
                type="checkbox"
                checked={content.toc ?? format === 'epub'}
                onChange={(e) => patchContent({ toc: e.target.checked })}
              />
              Include a table of contents
            </label>
            <label className="export-check">
              <input type="checkbox" checked={!!content.aboutAuthor} onChange={(e) => patchContent({ aboutAuthor: e.target.checked })} />
              Add “About the author” as back matter
            </label>

            <label>
              Your comments
              <select
                value={content.comments ?? 'off'}
                onChange={(e) => patchContent({ comments: e.target.value as NonNullable<ExportOptions['content']>['comments'] })}
              >
                <option value="off">Leave out</option>
                <option value="footnotes">As footnotes</option>
                <option value="endnotes">As endnotes</option>
              </select>
            </label>
            {hasComments && (
              <div className="export-subgroup">
                {content.comments === 'endnotes' && (
                  <label>
                    Collect notes
                    <select
                      value={content.notePlacement ?? 'chapter'}
                      onChange={(e) => patchContent({ notePlacement: e.target.value as 'chapter' | 'book' })}
                    >
                      <option value="chapter">At the end of each chapter</option>
                      <option value="book">At the end of the book</option>
                    </select>
                  </label>
                )}
                <label>
                  Numbering
                  <select
                    value={content.commentNumbering ?? 'continuous'}
                    onChange={(e) => patchContent({ commentNumbering: e.target.value as 'continuous' | 'chapter' })}
                  >
                    <option value="continuous">Continuous through the book</option>
                    <option value="chapter">Restart each chapter</option>
                  </select>
                </label>
                <label className="export-check">
                  <input
                    type="checkbox"
                    checked={content.quotePassage ?? content.comments === 'endnotes'}
                    onChange={(e) => patchContent({ quotePassage: e.target.checked })}
                  />
                  Quote the passage in the note
                </label>
                <label className="export-check">
                  <input type="checkbox" checked={!!content.commentLabel} onChange={(e) => patchContent({ commentLabel: e.target.checked })} />
                  Begin each note with “Comment:”
                </label>
                {format === 'pdf' && content.comments === 'footnotes' && (
                  <p className="field__hint">PDF puts notes at the end of each chapter. Use Word for true footnotes.</p>
                )}
                <p className="export-privacy">
                  Your comments will be in this file, readable by anyone you send it to. Only yours are included.
                </p>
              </div>
            )}
          </fieldset>

          <details className="export-group">
            <summary>Details for this file only</summary>
            <p className="field__hint">Filled from the book; changes here apply to this export and are not saved to the book.</p>
            <div className="export-grid">
              <label>Title<input value={options.metadata?.title ?? ''} onChange={(e) => patchMeta({ title: e.target.value || null })} /></label>
              <label>Subtitle<input value={options.metadata?.subtitle ?? ''} onChange={(e) => patchMeta({ subtitle: e.target.value || null })} /></label>
              <label>Author<input value={options.metadata?.author ?? ''} onChange={(e) => patchMeta({ author: e.target.value || null })} placeholder="Pen name, for this file" /></label>
              <label>Language<input value={options.metadata?.language ?? ''} onChange={(e) => patchMeta({ language: e.target.value || null })} placeholder="en, fr, pt-BR…" /></label>
            </div>
            <label>Description<textarea rows={3} value={options.metadata?.description ?? ''} onChange={(e) => patchMeta({ description: e.target.value || null })} /></label>
          </details>

          <label>
            File name
            <input
              value={options.filenameTemplate ?? ''}
              placeholder="{title}"
              onChange={(e) => patch({ filenameTemplate: e.target.value || null })}
            />
            <span className="field__hint">
              Use {'{title} {short_title} {author} {date} {version} {format}'}. {report && <>The file will be <strong>{report.filename}</strong>.</>}
            </span>
          </label>

          {report && (
            <section className="export-report" aria-live="polite">
              <p>
                <strong>{report.summary.chapters}</strong> chapter{report.summary.chapters === 1 ? '' : 's'},{' '}
                <strong>{report.summary.scenes}</strong> scene{report.summary.scenes === 1 ? '' : 's'},{' '}
                <strong>{report.summary.words.toLocaleString()}</strong> words included
                {report.summary.comments > 0 && <>, {report.summary.comments} comment{report.summary.comments === 1 ? '' : 's'}</>}.
              </p>
              {report.issues.length > 0 && <ul>{report.issues.map(issue)}</ul>}
            </section>
          )}

          <details className="export-group">
            <summary>Export several formats at once</summary>
            <div className="export-choice">
              {(['docx', 'pdf', 'epub', 'html', 'txt'] as ExportFormat[]).map((f) => (
                <label key={f} className="export-check">
                  <input
                    type="checkbox"
                    checked={more.includes(f)}
                    onChange={(e) => setMore((m) => (e.target.checked ? [...m, f] : m.filter((x) => x !== f)))}
                  />
                  {f.toUpperCase()}
                </label>
              ))}
            </div>
            <button
              type="button"
              disabled={busy || !target || more.length === 0 || errors.length > 0}
              onClick={() => target && run(() => exporting.bundle(target, more, sendable))}
            >
              Download them as one zip
            </button>
          </details>
        </>
      )}

      {notice && <p className="field__hint">{notice}</p>}
      {error && <p className="error">{error}</p>}

      <div className="dialog__actions">
        {!project && workId && (
          <button
            type="button"
            disabled={busy}
            onClick={async () => {
              const name = window.prompt('Name this profile', chosenProfile && !chosenProfile.builtin ? chosenProfile.name : '')
              if (!name?.trim()) return
              try {
                const saved = await exporting.saveProfile(name.trim(), sendable)
                setProfiles((p) => ({ ...p, custom: [...p.custom, saved] }))
                setProfileId(saved.id)
                setNotice(`Saved as “${saved.name}”. A profile keeps the look, not the selection.`)
              } catch (e) {
                setError(e instanceof Error ? e.message : 'Could not save the profile')
              }
            }}
          >
            Save as profile…
          </button>
        )}
        {!project && chosenProfile && !chosenProfile.builtin && (
          <button
            type="button"
            disabled={busy}
            onClick={async () => {
              if (!window.confirm(`Delete the profile “${chosenProfile.name}”?`)) return
              await exporting.deleteProfile(chosenProfile.id).catch(() => undefined)
              setProfiles((p) => ({ ...p, custom: p.custom.filter((x) => x.id !== chosenProfile.id) }))
              setProfileId('')
            }}
          >
            Delete profile
          </button>
        )}
        {!project && target && (
          <button
            type="button"
            disabled={busy || errors.length > 0}
            onClick={() => run(async () => { await exporting.preview(target, sendable) })}
          >
            Preview
          </button>
        )}
        <button
          type="button"
          disabled={busy || blocked || (!project && !target)}
          title={blocked && errors[0] ? errors[0].message : undefined}
          onClick={() =>
            run(async () => {
              if (project) {
                if (workId) await downloadDraftExport(workId, 'project', 'manuscript')
                return
              }
              if (target) return exporting.download(target, sendable)
            })
          }
        >
          {busy ? 'Building…' : 'Download'}
        </button>
        <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}
