import { useId } from 'react'
import { LANGUAGES, languageName } from '../languages'

interface Props {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  'aria-label'?: string
}

/**
 * A language tag, chosen by name or typed. The browser's own suggestion list
 * searches the names as the writer types ("port" finds Portuguese); a tag that
 * isn't listed, such as `gsw`, can still be entered.
 */
export function LanguageInput({ value, onChange, placeholder, ...rest }: Props) {
  const list = useId()
  const name = value ? languageName(value) : undefined
  return (
    <>
      <input
        list={list}
        value={value}
        placeholder={placeholder}
        aria-label={rest['aria-label']}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => {
          // Picking a suggestion by its name fills in the tag.
          const typed = e.target.value
          const byName = LANGUAGES.find(([, n]) => n.toLowerCase() === typed.trim().toLowerCase())
          onChange(byName ? byName[0] : typed)
        }}
      />
      <datalist id={list}>
        {LANGUAGES.map(([tag, label]) => (
          <option key={tag} value={tag}>{label}</option>
        ))}
      </datalist>
      {name && <span className="field__hint">{name}</span>}
    </>
  )
}
