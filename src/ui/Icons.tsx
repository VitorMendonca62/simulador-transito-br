/** Ícones de traço (24×24, cor = currentColor), desenhados para o simulador. */
type P = { size?: number }

function Svg({ size = 20, children }: P & { children: React.ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  )
}

export const IconTurns = (p: P) => (
  <Svg {...p}>
    <path d="M7 21v-8a5 5 0 0 1 5-5h7" />
    <path d="M16 4l4 4-4 4" />
    <path d="M7 21V3" strokeDasharray="2 3" />
  </Svg>
)
export const IconLanes = (p: P) => (
  <Svg {...p}>
    <path d="M5 21L8.5 3M19 21L15.5 3" />
    <path d="M12 4v2.5M12 10.5v3M12 17.5V20" />
  </Svg>
)
export const IconWorks = (p: P) => (
  <Svg {...p}>
    <path d="M10 3.5h4L19 19H5z" />
    <path d="M8 11h8M6.7 15h10.6M3.5 21h17" />
  </Svg>
)
export const IconSignal = (p: P) => (
  <Svg {...p}>
    <rect x="8" y="2.5" width="8" height="19" rx="3" />
    <circle cx="12" cy="7" r="1.4" fill="currentColor" />
    <circle cx="12" cy="12" r="1.4" fill="currentColor" />
    <circle cx="12" cy="17" r="1.4" fill="currentColor" />
  </Svg>
)
export const IconZones = (p: P) => (
  <Svg {...p}>
    <path d="M12 3l8.5 5.5L17 20H7L3.5 8.5z" strokeDasharray="3 2.2" />
    <circle cx="12" cy="12" r="2" />
  </Svg>
)
export const IconSim = (p: P) => (
  <Svg {...p}>
    <path d="M5 15.5V12l1.8-4.6A2 2 0 0 1 8.7 6h6.6a2 2 0 0 1 1.9 1.4L19 12v3.5" />
    <path d="M3.5 12h17v4.5h-17z" />
    <path d="M6.5 16.5V19M17.5 16.5V19" />
    <circle cx="7.5" cy="14.2" r=".6" fill="currentColor" />
    <circle cx="16.5" cy="14.2" r=".6" fill="currentColor" />
  </Svg>
)
export const IconCompare = (p: P) => (
  <Svg {...p}>
    <rect x="3" y="5" width="7.5" height="14" rx="2" />
    <rect x="13.5" y="5" width="7.5" height="14" rx="2" />
    <path d="M5.2 15l1.55-6 1.55 6M5.8 12.8h1.9" strokeWidth={1.5} />
    <path d="M15.8 9h1.8a1.5 1.5 0 0 1 0 3h-1.8zM15.8 12h2.1a1.5 1.5 0 0 1 0 3h-2.1z" strokeWidth={1.5} />
  </Svg>
)
export const IconLayers = (p: P) => (
  <Svg {...p}>
    <path d="M12 3l9 4.8-9 4.8-9-4.8z" />
    <path d="M3 12.2l9 4.8 9-4.8" />
    <path d="M3 16.4l9 4.8 9-4.8" />
  </Svg>
)
export const IconUpload = (p: P) => (
  <Svg {...p}>
    <path d="M12 15V4M7.5 8.5L12 4l4.5 4.5" />
    <path d="M4 15v3.5A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V15" />
  </Svg>
)
export const IconDownload = (p: P) => (
  <Svg {...p}>
    <path d="M12 4v11M7.5 10.5L12 15l4.5-4.5" />
    <path d="M4 15v3.5A1.5 1.5 0 0 0 5.5 20h13a1.5 1.5 0 0 0 1.5-1.5V15" />
  </Svg>
)
export const IconPin = (p: P) => (
  <Svg {...p}>
    <path d="M12 21s-6.5-5.6-6.5-11a6.5 6.5 0 0 1 13 0c0 5.4-6.5 11-6.5 11z" />
    <circle cx="12" cy="10" r="2.3" />
  </Svg>
)
export const IconMap = (p: P) => (
  <Svg {...p}>
    <path d="M9 4L3.5 6v14L9 18l6 2 5.5-2V4L15 6z" />
    <path d="M9 4v14M15 6v14" />
  </Svg>
)
export const IconFile = (p: P) => (
  <Svg {...p}>
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
    <path d="M14 3v5h5M9 13h6M9 17h4" />
  </Svg>
)
export const IconPlus = (p: P) => (
  <Svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
)
export const IconMinus = (p: P) => (
  <Svg {...p}>
    <path d="M5 12h14" />
  </Svg>
)
export const IconFit = (p: P) => (
  <Svg {...p}>
    <path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15" />
  </Svg>
)
export const IconChevron = (p: P) => (
  <Svg {...p}>
    <path d="M6 9l6 6 6-6" />
  </Svg>
)
export const IconClose = (p: P) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6L6 18" />
  </Svg>
)
export const IconSearch = (p: P) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="M20 20l-4.2-4.2" />
  </Svg>
)
