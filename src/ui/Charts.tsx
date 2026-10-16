import { useId } from 'react'

export interface Line {
  values: number[]
  color: string
  label?: string
}

/**
 * Gráfico de linhas leve em SVG. Várias séries compartilham a escala; `cursor` marca o índice em
 * reprodução e `onPick` permite escolher um ponto clicando.
 */
export function LineChart({ lines, unit = '', height = 54, cursor, onPick, title }: { lines: Line[]; unit?: string; height?: number; cursor?: number | null; onPick?: (i: number) => void; title: string }) {
  const id = useId()
  const n = Math.max(...lines.map((l) => l.values.length), 0)
  if (n < 2) return <div className="chart empty">{title}: aguardando dados…</div>
  const all = lines.flatMap((l) => l.values)
  const max = Math.max(...all, 1e-9)
  const min = Math.min(0, ...all)
  const W = 200
  const x = (i: number) => (i / (n - 1)) * W
  const y = (v: number) => height - 4 - ((v - min) / (max - min || 1)) * (height - 10)
  const fmt = (v: number) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(1))
  return (
    <figure className="chart">
      <figcaption>
        <span>{title}</span>
        <span className="chart-vals">
          {lines.map((l, k) => (
            <b key={k} style={{ color: l.color }}>
              {l.label ? `${l.label} ` : ''}
              {fmt(l.values[l.values.length - 1] ?? 0)}
              {unit}
            </b>
          ))}
        </span>
      </figcaption>
      <svg
        viewBox={`0 0 ${W} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-labelledby={id}
        onClick={(e) => {
          if (!onPick) return
          const r = e.currentTarget.getBoundingClientRect()
          onPick(Math.round(((e.clientX - r.left) / r.width) * (n - 1)))
        }}
        style={{ cursor: onPick ? 'pointer' : 'default' }}
      >
        <title id={id}>{title}</title>
        <line x1={0} x2={W} y1={y(0)} y2={y(0)} className="axis" />
        {lines.map((l, k) => (
          <polyline key={k} fill="none" stroke={l.color} strokeWidth={1.6} vectorEffect="non-scaling-stroke" points={l.values.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')} />
        ))}
        {cursor != null && cursor >= 0 && cursor < n && <line x1={x(cursor)} x2={x(cursor)} y1={0} y2={height} className="cursor" vectorEffect="non-scaling-stroke" />}
      </svg>
      <div className="chart-range">
        <span>0</span>
        <span>máx {fmt(max)}{unit}</span>
      </div>
    </figure>
  )
}

export const col = (series: Float32Array | null, c: number): number[] => {
  if (!series) return []
  const out: number[] = []
  for (let i = 0; i < series.length / 6; i++) out.push(series[i * 6 + c])
  return out
}
