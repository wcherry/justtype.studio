import { Link } from 'react-router-dom'
import type { PublicationCard } from '../api/types'
import { genreLabel } from './labels'

/**
 * One publication as the feed and a profile both show it.
 *
 * The title opens the manuscript and the author's name opens their profile, so
 * the card is not one big link: a link inside a link is two places to go and
 * the browser picks neither reliably.
 */
export function PublicationTile({ card }: { card: PublicationCard }) {
  return (
    <div className="feed__card">
      <h2><Link to={`/review/${card.id}`}>{card.workTitle}</Link></h2>
      <p className="feed__meta">
        <Link to={`/people/${card.author.id}`}>{card.author.displayName}</Link> ·{' '}
        {genreLabel(card.genre) && <>{genreLabel(card.genre)} · </>}
        {card.workKind.replace('_', ' ')} · {card.wordCount.toLocaleString()} words ·{' '}
        {card.versionLabel}
        <span className="feed__count">
          {card.readerCount === 0
            ? 'no readers yet'
            : `${card.readerCount} reader${card.readerCount === 1 ? '' : 's'}`}
          {' · '}
          {card.commentCount} comment{card.commentCount === 1 ? '' : 's'}
        </span>
      </p>
      {card.reviewPrompt && <p className="feed__prompt">“{card.reviewPrompt}”</p>}
      {card.excerpt && <p className="feed__excerpt">{card.excerpt}</p>}
      {card.authorReviewsGiven > 0 && (
        <p className="feed__reciprocity">
          {card.author.displayName} has read for {card.authorReviewsGiven} other writer
          {card.authorReviewsGiven === 1 ? '' : 's'}
        </p>
      )}
    </div>
  )
}
