/**
 * A search snippet. The server marks matched words with U+0002 … U+0003
 * rather than HTML, so text from a manuscript is only ever text here.
 */
export function Snippet({ text }: { text: string }) {
  const parts = text.split(/(\u0002[^\u0003]*\u0003)/)
  return (
    <span className="snippet">
      {parts.map((part, i) =>
        part.startsWith('\u0002')
          ? <mark key={i}>{part.slice(1, -1)}</mark>
          : <span key={i}>{part}</span>,
      )}
    </span>
  )
}
