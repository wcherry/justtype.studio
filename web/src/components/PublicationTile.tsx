import { Link } from 'react-router-dom'
import type { PublicationCard } from '../api/types'

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
        {card.workKind.replace('_', ' ')} · {card.wordCount.toLocaleString()} words ·{' '}
        {card.versionLabel}
        <span className="feed__count">
          {card.commentCount} comment{card.commentCount === 1 ? '' : 's'}
        </span>
      </p>
      {card.reviewPrompt && <p className="feed__prompt">“{card.reviewPrompt}”</p>}
      {card.excerpt && <p className="feed__excerpt">{card.excerpt}</p>}
    </div>
  )
}
