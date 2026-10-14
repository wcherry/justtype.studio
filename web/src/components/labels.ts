import type { FeedSort, Genre, LengthBand, Visibility, WindowFilter } from '../api/types'

/** What each closed set is called on screen. The values come from the server. */
export const GENRES: { value: Genre; label: string }[] = [
  { value: 'literary', label: 'Literary' },
  { value: 'fantasy', label: 'Fantasy' },
  { value: 'science_fiction', label: 'Science fiction' },
  { value: 'mystery', label: 'Mystery' },
  { value: 'thriller', label: 'Thriller' },
  { value: 'romance', label: 'Romance' },
  { value: 'horror', label: 'Horror' },
  { value: 'historical', label: 'Historical' },
  { value: 'young_adult', label: 'Young adult' },
  { value: 'other', label: 'Something else' },
]

export const LENGTHS: { value: LengthBand; label: string }[] = [
  { value: 'short', label: 'Short (under 7,500)' },
  { value: 'novelette', label: 'Novelette (to 17,500)' },
  { value: 'novella', label: 'Novella (to 40,000)' },
  { value: 'novel', label: 'Novel (40,000+)' },
]

export const WINDOWS: { value: WindowFilter; label: string }[] = [
  { value: 'closing_soon', label: 'Closing within three days' },
  { value: 'open_ended', label: 'No closing date' },
]

export const SORTS: { value: FeedSort; label: string }[] = [
  { value: 'newest', label: 'Newest' },
  { value: 'needs_readers', label: 'Needs readers' },
]

export const genreLabel = (genre: Genre | null | undefined) =>
  GENRES.find((g) => g.value === genre)?.label ?? null

/**
 * A publication's audience, as its badge names it. "Public" is the signed-in
 * readers here, which is what a writer means by it; the level that also lets
 * in people who are not signed in is the wider one, and says so.
 */
export const VISIBILITY_BADGE: Record<Visibility, string> = {
  invited: 'Invited',
  unlisted: 'Unlisted',
  platform: 'Public',
  public: 'Open to all',
}
