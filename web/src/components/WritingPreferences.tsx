import type { WritingPreferences as Preferences } from '../editor/preferences'

interface Props {
  preferences: Preferences
  onChange: <K extends keyof Preferences>(field: K, value: Preferences[K]) => void
  onReset: () => void
}

const FAMILIES: { value: Preferences['family']; label: string }[] = [
  { value: 'serif', label: 'Serif' },
  { value: 'sans', label: 'Sans' },
  { value: 'mono', label: 'Mono' },
]

export function WritingPreferencesPanel({ preferences, onChange, onReset }: Props) {
  return (
    <div className="prefs">
      <label className="prefs__toggle">
        <input
          type="checkbox"
          checked={preferences.focusMode}
          onChange={(e) => onChange('focusMode', e.target.checked)}
        />
        Focus mode
        <span className="prefs__hint">Dim everything but the paragraph you are in</span>
      </label>

      <label className="prefs__toggle">
        <input
          type="checkbox"
          checked={preferences.typewriter}
          onChange={(e) => onChange('typewriter', e.target.checked)}
        />
        Typewriter scrolling
        <span className="prefs__hint">Hold the line you are writing at a fixed height</span>
      </label>

      <label>
        Typeface
        <select
          value={preferences.family}
          onChange={(e) => onChange('family', e.target.value as Preferences['family'])}
        >
          {FAMILIES.map((f) => (
            <option key={f.value} value={f.value}>{f.label}</option>
          ))}
        </select>
      </label>

      <label>
        Size <span className="prefs__value">{preferences.fontSize}px</span>
        <input
          type="range" min={14} max={28} step={1}
          value={preferences.fontSize}
          onChange={(e) => onChange('fontSize', Number(e.target.value))}
        />
      </label>

      <label>
        Measure <span className="prefs__value">{preferences.measure} characters</span>
        <input
          type="range" min={24} max={60} step={1}
          value={preferences.measure}
          onChange={(e) => onChange('measure', Number(e.target.value))}
        />
      </label>

      <label>
        Line height <span className="prefs__value">{preferences.lineHeight.toFixed(1)}</span>
        <input
          type="range" min={1.2} max={2.4} step={0.1}
          value={preferences.lineHeight}
          onChange={(e) => onChange('lineHeight', Number(e.target.value))}
        />
      </label>

      <button type="button" className="linklike" onClick={onReset}>Reset to defaults</button>
    </div>
  )
}
