import type { FromWorker, SpellingLanguage, ToWorker } from './protocol'

/**
 * The page's side of the spellchecker: what it already knows about each word,
 * and a worker to ask about the rest.
 *
 * Checking is asynchronous, but the editor paints synchronously, so this
 * keeps an answer per word and paints from that. A word it has not heard
 * about yet is treated as right — never underlined on a guess — and asked
 * about in a batch; when the answers come back, whoever is listening repaints.
 * A book uses the same few thousand words over and over, so after the first
 * page nearly every check is a lookup.
 */
export class Speller {
  private worker: Worker
  /** word -> right? Absent means not asked yet, or waiting for the answer. */
  private verdicts = new Map<string, boolean>()
  private asking = new Set<string>()
  private queued = new Set<string>()
  private flushTimer: number | null = null
  private nextId = 1
  private waiting = new Map<number, (message: FromWorker) => void>()
  private listeners = new Set<() => void>()
  /** Words passed over for now — this page, this session — not taught. */
  private ignored = new Set<string>()
  ready = false
  failed: string | null = null

  constructor(readonly language: SpellingLanguage) {
    this.worker = new Worker(new URL('./spell.worker.ts', import.meta.url), { type: 'module' })
    this.worker.addEventListener('message', (event: MessageEvent<FromWorker>) => this.receive(event.data))
    this.send({ type: 'load', language })
  }

  /** `true` right, `false` wrong, `undefined` not known yet (and now asked). */
  check(word: string): boolean | undefined {
    const key = normalise(word)
    if (this.ignored.has(key)) return true
    const verdict = this.verdicts.get(key)
    if (verdict === undefined && !this.asking.has(key)) {
      this.queued.add(key)
      this.scheduleFlush()
    }
    return verdict
  }

  async suggest(word: string): Promise<string[]> {
    if (!this.ready) return []
    const answer = await this.ask({ type: 'suggest', id: this.nextId++, word: normalise(word) })
    return answer.type === 'suggested' ? answer.suggestions : []
  }

  /** Teach it words: the writer's dictionary, the names in the notebook. */
  add(words: string[]) {
    const keys = words.map(normalise).filter(Boolean)
    if (keys.length === 0) return
    this.send({ type: 'add', words: keys })
    // "Ysolde" being right makes "Ysolde's" right too, so a word that was
    // wrong before anything was taught is asked about again.
    for (const [word, right] of this.verdicts) {
      if (!right) this.verdicts.delete(word)
    }
    keys.forEach((k) => this.verdicts.set(k, true))
    this.notify()
  }

  remove(words: string[]) {
    const keys = words.map(normalise)
    this.send({ type: 'remove', words: keys })
    keys.forEach((k) => this.verdicts.delete(k))
    this.notify()
  }

  ignore(word: string) {
    this.ignored.add(normalise(word))
    this.notify()
  }

  /** Called when answers arrive that change what should be underlined. */
  subscribe(listener: () => void) {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  dispose() {
    this.worker.terminate()
    this.listeners.clear()
  }

  private notify() {
    this.listeners.forEach((l) => l())
  }

  private send(message: ToWorker) {
    this.worker.postMessage(message)
  }

  private ask(message: ToWorker & { id: number }): Promise<FromWorker> {
    return new Promise((resolve) => {
      this.waiting.set(message.id, resolve)
      this.send(message)
    })
  }

  private scheduleFlush() {
    if (this.flushTimer !== null || !this.ready) return
    // Gathered over a frame, so a whole scene opening is one message.
    this.flushTimer = window.setTimeout(() => {
      this.flushTimer = null
      void this.flush()
    }, 16)
  }

  private async flush() {
    const words = [...this.queued]
    this.queued.clear()
    if (words.length === 0) return
    words.forEach((w) => this.asking.add(w))
    const answer = await this.ask({ type: 'check', id: this.nextId++, words })
    if (answer.type !== 'checked') return
    const wrong = new Set(answer.wrong)
    words.forEach((w) => {
      this.asking.delete(w)
      this.verdicts.set(w, !wrong.has(w))
    })
    // Only a wrong word changes what is painted; right is the default.
    if (wrong.size > 0) this.notify()
  }

  private receive(message: FromWorker) {
    if (message.type === 'ready') {
      this.ready = true
      this.scheduleFlush()
      this.notify()
    } else if (message.type === 'failed') {
      this.failed = message.message
      this.notify()
    } else {
      this.waiting.get(message.id)?.(message)
      this.waiting.delete(message.id)
    }
  }
}

/** Curly apostrophes are the same apostrophe to a dictionary. */
export function normalise(word: string) {
  return word.replace(/[’‘]/g, "'").trim()
}
