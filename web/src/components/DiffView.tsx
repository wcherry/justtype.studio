import { useState } from 'react'
import type { BlockDiff, DiffResponse, SceneDiff, TextChange } from '../api/types'

const SCENE_LABEL: Record<SceneDiff['kind'], string> = {
  added: 'new scene',
  removed: 'removed',
  modified: 'revised',
  moved: 'moved',
  unchanged: 'unchanged',
}

const BLOCK_LABEL: Record<BlockDiff['kind'], string> = {
  added: 'new',
  removed: 'cut',
  modified: 'edited',
  moved: 'moved',
  unchanged: '',
}

/** Word-level detail, rendered inline. */
function Words({ changes }: { changes: TextChange[] }) {
  return (
    <>
      {changes.map((change, index) => (
        <span key={index} className={`diff__${change.op}`}>
          {change.value}
        </span>
      ))}
    </>
  )
}

/**
 * What changed between two versions, or between a version and the draft.
 *
 * Paragraph-aligned rather than one run of words across the whole scene: a
 * reflowed sentence used to make everything after it look edited, and a moved
 * paragraph read as a deletion and an unrelated insertion.
 */
export function DiffView({ diff }: { diff: DiffResponse }) {
  const [mode, setMode] = useState<'inline' | 'side'>('inline')
  const changed = diff.scenes.filter((scene) => scene.kind !== 'unchanged')

  return (
    <div className="diff">
      <header className="diff__header">
        <h3>
          {diff.from} <span aria-hidden>→</span> {diff.to}
        </h3>
        <p className="diff__summary">
          <span className="diff__stat diff__stat--add">+{diff.summary.wordsAdded.toLocaleString()}</span>
          <span className="diff__stat diff__stat--del">−{diff.summary.wordsRemoved.toLocaleString()}</span>
          <span>
            {diff.summary.scenesModified} revised · {diff.summary.scenesAdded} added ·{' '}
            {diff.summary.scenesRemoved} removed · {diff.summary.scenesMoved} moved
            {diff.summary.paragraphsChanged > 0 && ` · ${diff.summary.paragraphsChanged} paragraphs`}
          </span>
        </p>
        <div className="diff__modes">
          <button
            type="button"
            className={mode === 'inline' ? 'is-active' : undefined}
            onClick={() => setMode('inline')}
          >
            Inline
          </button>
          <button
            type="button"
            className={mode === 'side' ? 'is-active' : undefined}
            onClick={() => setMode('side')}
          >
            Side by side
          </button>
        </div>
      </header>

      {changed.length === 0 && <p className="empty">Nothing changed between these.</p>}

      {changed.map((scene) => (
        <article key={scene.sceneId} className="diff__scene">
          <h4>
            {scene.chapterTitle} · {scene.sceneTitle}
            <span className={`badge badge--${scene.kind}`}>{SCENE_LABEL[scene.kind]}</span>
          </h4>

          {scene.blocks && scene.blocks.length > 0 ? (
            <ol className="diff__blocks">
              {scene.blocks
                .filter((block) => block.kind !== 'unchanged')
                .map((block, index) => (
                  <li key={index} className={`diff__block diff__block--${block.kind}`}>
                    <span className="diff__block-kind">{BLOCK_LABEL[block.kind]}</span>

                    {mode === 'side' && block.kind === 'modified' ? (
                      <div className="diff__columns">
                        <p className="diff__text diff__text--before">{block.before}</p>
                        <p className="diff__text diff__text--after">{block.after}</p>
                      </div>
                    ) : (
                      <p className="diff__text">
                        {block.kind === 'modified' && <Words changes={block.changes ?? []} />}
                        {block.kind === 'added' && <span className="diff__insert">{block.after}</span>}
                        {block.kind === 'removed' && <span className="diff__delete">{block.before}</span>}
                        {block.kind === 'moved' && (
                          <span className="diff__moved">
                            {block.after}
                            <span className="diff__block-kind">
                              {' '}
                              (was paragraph {(block.beforeIndex ?? 0) + 1}, now{' '}
                              {(block.afterIndex ?? 0) + 1})
                            </span>
                          </span>
                        )}
                      </p>
                    )}
                  </li>
                ))}
            </ol>
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
