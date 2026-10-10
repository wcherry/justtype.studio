/** What the page and the spelling worker say to each other. */

export type SpellingLanguage = 'en-US' | 'en-GB'

export type ToWorker =
  | { type: 'load'; language: SpellingLanguage }
  | { type: 'add'; words: string[] }
  | { type: 'remove'; words: string[] }
  | { type: 'check'; id: number; words: string[] }
  | { type: 'suggest'; id: number; word: string }

export type FromWorker =
  | { type: 'ready' }
  | { type: 'failed'; message: string }
  | { type: 'checked'; id: number; wrong: string[] }
  | { type: 'suggested'; id: number; suggestions: string[] }
