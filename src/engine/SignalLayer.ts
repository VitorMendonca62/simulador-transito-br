import { Container, Graphics, Mesh, MeshGeometry, Texture } from 'pixi.js'
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

const SIDES = 12
const VERTS = SIDES + 1
const STATE_IDX: Record<LightState, number> = { green: 0, yellow: 1, red: 2 }
const STATE_COLORS = [C.signalGreen, C.signalYellow, C.signalRed]

const makeMesh = (positions: Float32Array, color: number, alpha = 1): Mesh => {
  const m = new Mesh({ geometry: new MeshGeometry({ positions, uvs: new Float32Array(positions.length), indices: new Uint32Array(0) }), texture: Texture.WHITE })
  m.tint = color
  m.alpha = alpha
  return m
}

/**
 * Semáforos como malhas estáticas (um disco por aproximação para o halo, a caixa e a luz). A geometria é
 * montada uma vez; o zoom só reescreve as posições quando o raio muda de verdade (acima de 3,5 px/m o raio
 * é constante) e a cor é trocada trocando listas de índices, só quando alguma luz muda de estado.
 */
export class SignalLayer {
  /** contêiner raiz (visibilidade); contém as malhas e o anel do semáforo selecionado */
  readonly g = new Container()
  private ring = new Graphics()
  private defs: SignalDef[] = []
  private cx = new Float32Array(0)
  private cy = new Float32Array(0)
  /** por aproximação: semáforo (índice em defs), fase dona (-1 = nenhuma) */
  private endDef = new Uint32Array(0)
  private endPhase = new Int16Array(0)
  private defStart = new Uint32Array(0)
  private halo: Mesh[] = []
  private housing: Mesh | null = null
  private light: Mesh[] = []
  private state = new Int8Array(0)
  private radius = 0
  private lastRingKey = ''

  constructor() {
    this.g.addChild(this.ring)
  }

  setSignals(defs: SignalDef[]) {
    this.defs = defs
    for (const m of [...this.halo, ...(this.housing ? [this.housing] : []), ...this.light]) m.destroy(true)
    this.halo = []
    this.light = []
    this.housing = null
    let n = 0
    for (const d of defs) n += d.ends.length
    this.cx = new Float32Array(n)
    this.cy = new Float32Array(n)
    this.endDef = new Uint32Array(n)
    this.endPhase = new Int16Array(n).fill(-1)
    this.defStart = new Uint32Array(defs.length + 1)
    let k = 0
    defs.forEach((d, di) => {
      this.defStart[di] = k
      const owner = new Map<number, number>()
      d.phases.forEach((p, i) => p.ends.forEach((e) => owner.set(e, i)))
      for (const e of d.ends) {
        this.cx[k] = e.x
        this.cy[k] = e.y
        this.endDef[k] = di
        this.endPhase[k] = owner.get(e.end) ?? -1
        k++
      }
    })
    this.defStart[defs.length] = k
    this.state = new Int8Array(n).fill(-1)
    this.radius = 0
    this.lastRingKey = ''
    if (n === 0) return
    const zero = () => new Float32Array(n * VERTS * 2)
    this.halo = STATE_COLORS.map((c) => makeMesh(zero(), c, 0.18))
    this.light = STATE_COLORS.map((c) => makeMesh(zero(), c))
    this.housing = makeMesh(zero(), C.signalHousing)
    const idx = new Uint32Array(n * SIDES * 3)
    for (let e = 0, o = 0; e < n; e++) {
      const b = e * VERTS
      for (let s = 0; s < SIDES; s++) {
        idx[o++] = b
        idx[o++] = b + 1 + s
        idx[o++] = b + 1 + ((s + 1) % SIDES)
      }
    }
    this.housing.geometry.indices = idx
    this.g.addChildAt(this.housing, 0)
    for (const m of [...this.halo, ...this.light]) this.g.addChild(m)
    this.g.addChild(this.ring) // o anel fica por cima
  }

  /** Escreve as posições dos discos para o raio `r` (m). */
  private layout(r: number) {
    const n = this.cx.length
    const fill = (radius: number) => {
      const pos = new Float32Array(n * VERTS * 2)
      let p = 0
      for (let e = 0; e < n; e++) {
        pos[p++] = this.cx[e]
        pos[p++] = this.cy[e]
        for (let s = 0; s < SIDES; s++) {
          const a = (s / SIDES) * Math.PI * 2
          pos[p++] = this.cx[e] + Math.cos(a) * radius
          pos[p++] = this.cy[e] + Math.sin(a) * radius
        }
      }
      return pos
    }
    const halo = fill(r * 1.9)
    for (const m of this.halo) m.geometry.positions = halo
    if (this.housing) this.housing.geometry.positions = fill(r * 1.35)
    const light = fill(r)
    for (const m of this.light) m.geometry.positions = light
    this.radius = r
  }

  /** Estado do relógio `t`: raio pelo zoom, cores por estado e anel do semáforo selecionado. */
  draw(t: number, zoom: number, selectedNode: number | null = null) {
    const n = this.cx.length
    const r = Math.max(1.0, 3.5 / zoom)
    if (n > 0 && (this.radius === 0 || Math.abs(r / this.radius - 1) > 0.03)) this.layout(r)
    let changed = false
    const st = this.state
    for (let di = 0; di < this.defs.length; di++) {
      const { phase, state } = signalState(this.defs[di], t)
      const si = STATE_IDX[state]
      for (let e = this.defStart[di]; e < this.defStart[di + 1]; e++) {
        const s = this.endPhase[e] === phase ? si : 2
        if (st[e] !== s) {
          st[e] = s
          changed = true
        }
      }
    }
    if (changed && n > 0) {
      const lists: number[][] = [[], [], []]
      for (let e = 0; e < n; e++) {
        const out = lists[st[e]]
        const b = e * VERTS
        for (let s = 0; s < SIDES; s++) out.push(b, b + 1 + s, b + 1 + ((s + 1) % SIDES))
      }
      for (let k = 0; k < 3; k++) {
        const idx = new Uint32Array(lists[k])
        this.halo[k].geometry.indices = idx
        this.light[k].geometry.indices = idx
        this.halo[k].visible = this.light[k].visible = idx.length > 0
      }
    }
    this.drawRing(zoom, r, selectedNode)
  }

  /** Anéis (cor da fase) só do semáforo selecionado. */
  private drawRing(zoom: number, r: number, selectedNode: number | null) {
    const di = selectedNode == null ? -1 : this.defs.findIndex((d) => d.node === selectedNode)
    const key = `${di}|${zoom.toFixed(3)}|${di >= 0 ? this.defs[di].phases.length : 0}`
    if (key === this.lastRingKey) return
    this.lastRingKey = key
    const g = this.ring
    g.clear()
    if (di < 0) return
    for (let e = this.defStart[di]; e < this.defStart[di + 1]; e++) {
      const ph = this.endPhase[e]
      const color = ph < 0 ? 0x64748b : PHASE_COLORS[ph % PHASE_COLORS.length]
      g.circle(this.cx[e], this.cy[e], r * 2.6).stroke({ width: Math.max(0.35, 2.2 / zoom), color, alpha: 0.95 })
    }
  }
}
