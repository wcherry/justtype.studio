/** The few line icons the editor's toolbars need, drawn to one 24px grid. */
const PATHS = {
  find: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM20 20l-4.8-4.8',
  notebook: 'M6 3h11a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H6zM6 3v18M4 7h4M4 12h4M4 17h4M11 8h4',
  tasks: 'M10 6h10M10 12h10M10 18h10M3.5 6l1.5 1.5L7.5 5M3.5 12l1.5 1.5 2.5-2.5M3.5 18l1.5 1.5 2.5-2.5',
  room: 'M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5',
  menu: 'M4 6h16M4 12h16M4 18h16',
  info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5.5M12 7.6v.01',
  outline: 'M5 5h14M9 12h10M9 19h10M5 5v14M5 12h4M5 19h4',
  leave: 'M9 4v5H4M15 4v5h5M15 20v-5h5M9 20v-5H4',
  undo: 'M9 14L4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11',
  redo: 'M15 14l5-5-5-5M20 9H9.5a5.5 5.5 0 0 0 0 11H13',
} as const

export type IconName = keyof typeof PATHS

export function Icon({ name }: { name: IconName }) {
  return (
    <svg
      className={`icon icon--${name}`}
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  )
}
