import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { AnnotationInput } from '../api/client'
import type { Annotation } from '../api/types'

/** Where the marks belong: the working draft of a book, or one publication. */
export type AnnotationSource =
  | { kind: 'draft'; workId: string }
  | { kind: 'publication'; publicationId: string }

/** One person's highlights and bookmarks on one text, and the ways to change them. */
export function useAnnotations(source: AnnotationSource | null) {
  const [list, setList] = useState<Annotation[]>([])
  const id = source ? (source.kind === 'draft' ? source.workId : source.publicationId) : null
  const kind = source?.kind ?? null

  useEffect(() => {
    if (!id || !kind) return
    let live = true
    const load = kind === 'draft' ? api.annotations.draft(id) : api.annotations.publication(id)
    // A reader without an account, or a closed window, simply has none.
    load.then((rows) => live && setList(rows)).catch(() => live && setList([]))
    return () => {
      live = false
    }
  }, [id, kind])

  const add = useCallback(
    async (input: AnnotationInput) => {
      if (!id) return
      const row = kind === 'draft'
        ? await api.annotations.markDraft(id, input)
        : await api.annotations.markPublication(id, input)
      setList((current) => [...current, row])
      return row
    },
    [id, kind],
  )

  const remove = useCallback(async (annotationId: string) => {
    await api.annotations.remove(annotationId)
    setList((current) => current.filter((a) => a.id !== annotationId))
  }, [])

  const update = useCallback(
    async (annotationId: string, input: { from?: number; to?: number; quote?: string; note?: string }) => {
      const row = await api.annotations.update(annotationId, input)
      setList((current) => current.map((a) => (a.id === row.id ? row : a)))
      return row
    },
    [],
  )

  return { list, add, remove, update }
}
