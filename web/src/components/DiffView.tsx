import type { DiffResponse } from '../api/types'

const KIND_LABEL: Record<string, string> = {
  added: 'new scene',
  removed: 'removed',
  modified: 'revised',
  moved: 'moved',
  unchanged: 'unchanged',
}

export function DiffView({ diff }: { diff: DiffResponse }) {
  const changed = diff.scenes.filter((s) => s.kind !== 'unchanged')

  return (
    <div className="diff">
      <header className="diff__header">
        <h3>
          {diff.from.label} <span aria-hidden>→</span> {diff.to.label}
        </h3>
        <p className="diff__summary">
          <span className="diff__stat diff__stat--add">+{diff.summary.wordsAdded.toLocaleString()}</span>
          <span className="diff__stat diff__stat--del">−{diff.summary.wordsRemoved.toLocaleString()}</span>
          <span>
            {diff.summary.scenesModified} revised · {diff.summary.scenesAdded} added ·{' '}
            {diff.summary.scenesRemoved} removed · {diff.summary.scenesMoved} moved
          </span>
        </p>
      </header>

      {changed.length === 0 && <p className="empty">Nothing changed between these versions.</p>}

      {changed.map((scene) => (
        <article key={scene.sceneId} className="diff__scene">
          <h4>
            {scene.chapterTitle} · {scene.sceneTitle}
            <span className={`badge badge--${scene.kind}`}>{KIND_LABEL[scene.kind]}</span>
          </h4>
          {scene.changes && scene.changes.length > 0 ? (
            <p className="diff__text">
              {scene.changes.map((change, index) => (
                <span key={index} className={`diff__${change.op}`}>
                  {change.value}
                </span>
              ))}
            </p>
          ) : (
            <p className="diff__text diff__text--meta">
              {scene.wordsBefore.toLocaleString()} → {scene.wordsAfter.toLocaleString()} words
            </p>
          )}
        </article>
      ))}
    </div>
  )
}
