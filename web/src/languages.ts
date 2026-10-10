/**
 * Languages a book is commonly written in, as BCP 47 tags with their names.
 * The picker searches these, but any well-formed tag can still be typed.
 */
export const LANGUAGES: [tag: string, name: string][] = [
  ['af', 'Afrikaans'],
  ['ar', 'Arabic'],
  ['bg', 'Bulgarian'],
  ['ca', 'Catalan'],
  ['cs', 'Czech'],
  ['cy', 'Welsh'],
  ['da', 'Danish'],
  ['de', 'German'],
  ['de-AT', 'German (Austria)'],
  ['de-CH', 'German (Switzerland)'],
  ['el', 'Greek'],
  ['en', 'English'],
  ['en-AU', 'English (Australia)'],
  ['en-CA', 'English (Canada)'],
  ['en-GB', 'English (United Kingdom)'],
  ['en-IE', 'English (Ireland)'],
  ['en-NZ', 'English (New Zealand)'],
  ['en-US', 'English (United States)'],
  ['eo', 'Esperanto'],
  ['es', 'Spanish'],
  ['es-419', 'Spanish (Latin America)'],
  ['es-MX', 'Spanish (Mexico)'],
  ['et', 'Estonian'],
  ['eu', 'Basque'],
  ['fa', 'Persian'],
  ['fi', 'Finnish'],
  ['fr', 'French'],
  ['fr-CA', 'French (Canada)'],
  ['ga', 'Irish'],
  ['gd', 'Scottish Gaelic'],
  ['gl', 'Galician'],
  ['he', 'Hebrew'],
  ['hi', 'Hindi'],
  ['hr', 'Croatian'],
  ['hu', 'Hungarian'],
  ['hy', 'Armenian'],
  ['id', 'Indonesian'],
  ['is', 'Icelandic'],
  ['it', 'Italian'],
  ['ja', 'Japanese'],
  ['ka', 'Georgian'],
  ['ko', 'Korean'],
  ['la', 'Latin'],
  ['lt', 'Lithuanian'],
  ['lv', 'Latvian'],
  ['mk', 'Macedonian'],
  ['nb', 'Norwegian (Bokmål)'],
  ['nl', 'Dutch'],
  ['nn', 'Norwegian (Nynorsk)'],
  ['pl', 'Polish'],
  ['pt', 'Portuguese'],
  ['pt-BR', 'Portuguese (Brazil)'],
  ['pt-PT', 'Portuguese (Portugal)'],
  ['ro', 'Romanian'],
  ['ru', 'Russian'],
  ['sk', 'Slovak'],
  ['sl', 'Slovenian'],
  ['sq', 'Albanian'],
  ['sr', 'Serbian'],
  ['sv', 'Swedish'],
  ['sw', 'Swahili'],
  ['th', 'Thai'],
  ['tr', 'Turkish'],
  ['uk', 'Ukrainian'],
  ['ur', 'Urdu'],
  ['vi', 'Vietnamese'],
  ['yi', 'Yiddish'],
  ['zh-Hans', 'Chinese (Simplified)'],
  ['zh-Hant', 'Chinese (Traditional)'],
]

/** The name for a tag, or nothing when it is not one of ours. */
export function languageName(tag: string): string | undefined {
  const lower = tag.trim().toLowerCase()
  return LANGUAGES.find(([t]) => t.toLowerCase() === lower)?.[1]
}

/** The writer's own language, from the browser, as the default for a new book. */
export function uiLanguage(): string {
  if (typeof navigator === 'undefined' || !navigator.language) return 'en'
  return navigator.language
}
