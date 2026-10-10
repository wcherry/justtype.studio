// The little of nspell this app uses. Its published types reach for Node's
// Buffer, which a browser build has no business knowing about.
declare module 'nspell' {
  interface NSpell {
    correct(word: string): boolean
    suggest(word: string): string[]
    add(word: string, model?: string): NSpell
    remove(word: string): NSpell
  }
  export default function nspell(aff: string, dic?: string): NSpell
}
