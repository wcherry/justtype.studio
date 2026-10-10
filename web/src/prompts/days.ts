/**
 * Days and times for Daily Prompts.
 *
 * A prompt day is a Pacific calendar day (`YYYY-MM-DD`); the server decides
 * which one it is. Here they are only shown: as a date in words, and the
 * prompt's opening and closing in the writer's own time.
 */

/** "Monday 12 October", from a `YYYY-MM-DD`. Read as a date, not an instant. */
export function dayName(day: string, withYear = false): string {
  const [y, m, d] = day.split('-').map(Number)
  const date = new Date(Date.UTC(y, m - 1, d))
  return date.toLocaleDateString(undefined, {
    weekday: 'long',
    day: 'numeric',
    month: 'long',
    year: withYear ? 'numeric' : undefined,
    timeZone: 'UTC',
  })
}

/** "14:00", in the reader's own timezone. */
export function localTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/** The browser's timezone, an IANA name such as `Europe/London`. */
export function browserTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'America/Los_Angeles'
  } catch {
    return 'America/Los_Angeles'
  }
}

/** Every IANA timezone the browser knows, for the picker, with the current one first. */
export function timezones(current: string): string[] {
  let all: string[] = []
  try {
    all = (Intl as unknown as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf?.('timeZone') ?? []
  } catch {
    all = []
  }
  if (!all.includes(current)) all = [current, ...all]
  return all
}

const minutes = (hhmm: string) => {
  const [h, m] = hhmm.split(':').map(Number)
  return h * 60 + m
}

/**
 * The note beside the email time, when the email would arrive in the last
 * six hours of the prompt's day: "Today's prompt closes at 08:00. You'll
 * have 3 hours." `opensAt` is when the prompt opens (and so closes) in the
 * same timezone as `chosen`.
 */
export function closingNote(opensAt: string, chosen: string): string | null {
  const left = (minutes(opensAt) - minutes(chosen) + 24 * 60) % (24 * 60)
  if (left === 0 || left > 6 * 60) return null
  const hours = Math.round(left / 6) / 10
  const said = Number.isInteger(hours) ? String(hours) : hours.toFixed(1)
  return `Today's prompt closes at ${opensAt}. You'll have ${said} hour${hours === 1 ? '' : 's'}.`
}
