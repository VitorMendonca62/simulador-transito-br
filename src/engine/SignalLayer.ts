import { Graphics } from 'pixi.js'
import type { SignalDef } from '../model/types'
import { C } from './colors'

export type LightState = 'green' | 'yellow' | 'red'

/** Estado do semáforo no instante t (s): fase ativa e cor dela; as demais ficam vermelhas. */
/** Cores que identificam cada fase (no painel e nos marcadores do semáforo selecionado). */
export const PHASE_COLORS = [0x60a5fa, 0xf472b6, 0xfbbf24, 0xa78bfa, 0x2dd4bf, 0xfb923c, 0x94a3b8, 0xf87171]

export function signalState(def: SignalDef, t0: number): { phase: number; state: LightState } {
  const t = t0 + def.offset
  const cycle = def.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
  if (cycle <= 0) return { phase: 0, state: 'red' }
  let r = ((t % cycle) + cycle) % cycle
  for (let i = 0; i < def.phases.length; i++) {
    const p = def.phases[i]
    if (r < p.green) return { phase: i, state: 'green' }
    r -= p.green
    if (r < p.yellow) return { phase: i, state: 'yellow' }
    r -= p.yellow
    if (r < p.allRed) return { phase: -1, state: 'red' }
    r -= p.allRed
  }
  return { phase: -1, state: 'red' }
}

export class SignalLayer {
  readonly g = new Graphics()
  private defs: SignalDef[] = []

  setSignals(defs: SignalDef[]) {
    this.defs = defs
  }

  draw(t: number, zoom: number, selectedNode: number | null = null) {
    const g = this.g
    g.clear()
    const r = Math.max(1.0, 3.5 / zoom)
    for (const def of this.defs) {
      const { phase, state } = signalState(def, t)
      const owner = new Map<number, number>()
      def.phases.forEach((p, i) => p.ends.forEach((e) => owner.set(e, i)))
      const selected = def.node === selectedNode
      for (const e of def.ends) {
        const s: LightState = owner.get(e.end) === phase ? state : 'red'
        if (selected) {
          // anel na cor da fase a que a aproximação pertence (liga o painel ao mapa)
          const ph = owner.get(e.end)
          const ring = ph == null ? 0x64748b : PHASE_COLORS[ph % PHASE_COLORS.length]
          g.circle(e.x, e.y, r * 2.6).stroke({ width: Math.max(0.35, 2.2 / zoom), color: ring, alpha: 0.95 })
        }
        const col = s === 'green' ? C.signalGreen : s === 'yellow' ? C.signalYellow : C.signalRed
        g.circle(e.x, e.y, r * 1.9).fill({ color: col, alpha: 0.18 })
        g.circle(e.x, e.y, r * 1.35).fill(C.signalHousing)
        g.circle(e.x, e.y, r).fill(col)
      }
    }
  }
}
