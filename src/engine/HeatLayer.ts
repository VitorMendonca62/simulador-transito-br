import { Container, Mesh, MeshGeometry, Texture } from 'pixi.js'
import type { NetworkData } from '../model/types'

/** Paleta do pior ao melhor (nível 0..4). */
export const HEAT_PALETTE = [0xef4444, 0xfb923c, 0xfacc15, 0xa3e635, 0x34d399]
/** Paleta da diferença entre cenários: muito pior, pior, (igual = oculto), melhor, muito melhor. */
export const DIFF_PALETTE = [0xdc2626, 0xfb923c, 0x94a3b8, 0x7dd3fc, 0x2563eb]

/** Nível 0..4 (ou -1 = sem dado) da razão de velocidade 0..254. */
export function speedLevels(v: Uint8Array): Int8Array {
  const out = new Int8Array(v.length)
  for (let i = 0; i < v.length; i++) {
    const x = v[i]
    out[i] = x === 255 ? -1 : x < 64 ? 0 : x < 127 ? 1 : x < 178 ? 2 : x < 224 ? 3 : 4
  }
  return out
}
/** Fração parada 0..254 -> nível (muita fila = vermelho). Oculta o que está praticamente livre. */
export function queueLevels(v: Uint8Array): Int8Array {
  const out = new Int8Array(v.length)
  for (let i = 0; i < v.length; i++) {
    const x = v[i]
    out[i] = x === 255 ? -1 : x > 152 ? 0 : x > 89 ? 1 : x > 38 ? 2 : x > 13 ? 3 : -1
  }
  return out
}
/** Diferença de razão de velocidade (B − A) em [-1, 1] -> nível; NaN/pequena = oculto. */
export function diffLevels(d: Float32Array): Int8Array {
  const out = new Int8Array(d.length)
  for (let i = 0; i < d.length; i++) {
    const x = d[i]
    out[i] = Number.isNaN(x) ? -1 : x < -0.3 ? 0 : x < -0.1 ? 1 : x > 0.3 ? 4 : x > 0.1 ? 3 : -1
  }
  return out
}

/**
 * Mapa de calor: uma fita por trecho direcionado (a metade da pista daquele sentido, sem os recuos
 * dos cruzamentos), pintada por nível. A geometria é única; a cada atualização só os índices mudam
 * (5 malhas, uma por cor), então o custo não depende do nº de trechos desenhados.
 */
export class HeatLayer {
  readonly container = new Container()
  private meshes: Mesh[] = []
  private vstart = new Uint32Array(0)
  private npts = new Uint16Array(0)
  private built = false
  private palette = HEAT_PALETTE

  invalidate() {
    this.built = false
  }

  private build(d: NetworkData) {
    const nseg = d.segKeys.length / 3
    const nd = nseg * 2
    this.vstart = new Uint32Array(nd)
    this.npts = new Uint16Array(nd)
    const verts: number[] = []
    for (let i = 0; i < nseg; i++) {
      const lf = d.segU32[i * 9 + 2]
      const lb = d.segU32[i * 9 + 3]
      const oneway = (d.segU32[i * 9 + 6] & 1) === 1
      const w = d.segW[i]
      const a0 = d.segPtsOff[i]
      const a1 = d.segPtsOff[i + 1]
      // trecho do eixo sem os recuos
      const pts: [number, number][] = []
      let total = 0
      const cum: number[] = [0]
      for (let k = a0; k < a1; k++) {
        pts.push([d.segPts[k * 2], d.segPts[k * 2 + 1]])
        if (k > a0) {
          total += Math.hypot(pts[pts.length - 1][0] - pts[pts.length - 2][0], pts[pts.length - 1][1] - pts[pts.length - 2][1])
          cum.push(total)
        }
      }
      const s0 = d.segCuts[i * 2]
      const s1 = total - d.segCuts[i * 2 + 1]
      if (pts.length < 2 || s1 - s0 < 1) continue
      const sl = sliceLine(pts, cum, s0, s1)
      const norm = normals(sl)
      for (const dir of [0, 1]) {
        const n = dir === 0 ? lf : lb
        if (n === 0) continue
        let a: number
        let b: number
        if (dir === 0) {
          const p0 = oneway ? (-lf * w) / 2 : 0
          a = p0
          b = p0 + lf * w
        } else {
          a = -lb * w
          b = 0
        }
        a += 0.25
        b -= 0.25
        const d2 = i * 2 + dir
        this.vstart[d2] = verts.length / 2
        this.npts[d2] = sl.length
        for (let k = 0; k < sl.length; k++) {
          verts.push(sl[k][0] + norm[k][0] * a, sl[k][1] + norm[k][1] * a, sl[k][0] + norm[k][0] * b, sl[k][1] + norm[k][1] * b)
        }
      }
    }
    const positions = new Float32Array(verts)
    for (const m of this.meshes) {
      m.destroy(true)
    }
    this.meshes = this.palette.map((color) => {
      const m = new Mesh({ geometry: new MeshGeometry({ positions, uvs: new Float32Array(positions.length), indices: new Uint32Array(0) }), texture: Texture.WHITE })
      m.tint = color
      m.alpha = 0.7
      return m
    })
    this.container.removeChildren()
    for (const m of this.meshes) this.container.addChild(m)
    this.built = true
  }

  /** Aplica níveis por trecho direcionado (índice = trecho*2 + sentido); null esconde tudo. */
  set(d: NetworkData | null, levels: Int8Array | null, palette: number[] = HEAT_PALETTE) {
    if (!d || !levels) {
      this.container.visible = false
      return
    }
    if (palette !== this.palette) {
      this.palette = palette
      this.built = false
    }
    if (!this.built) this.build(d)
    this.container.visible = true
    const lists: number[][] = [[], [], [], [], []]
    for (let dseg = 0; dseg < levels.length && dseg < this.npts.length; dseg++) {
      const lv = levels[dseg]
      const n = this.npts[dseg]
      if (lv < 0 || n < 2) continue
      const out = lists[lv]
      const v = this.vstart[dseg]
      for (let k = 0; k < n - 1; k++) {
        const b = v + k * 2
        out.push(b, b + 1, b + 2, b + 1, b + 3, b + 2)
      }
    }
    for (let l = 0; l < 5; l++) {
      this.meshes[l].geometry.indices = new Uint32Array(lists[l])
      this.meshes[l].visible = lists[l].length > 0
    }
  }
}

function sliceLine(pts: [number, number][], cum: number[], s0: number, s1: number): [number, number][] {
  const at = (s: number): [number, number] => {
    let i = 0
    while (i < cum.length - 2 && cum[i + 1] < s) i++
    const l = cum[i + 1] - cum[i] || 1
    const t = Math.min(1, Math.max(0, (s - cum[i]) / l))
    return [pts[i][0] + (pts[i + 1][0] - pts[i][0]) * t, pts[i][1] + (pts[i + 1][1] - pts[i][1]) * t]
  }
  const out: [number, number][] = [at(s0)]
  for (let i = 0; i < pts.length; i++) if (cum[i] > s0 && cum[i] < s1) out.push(pts[i])
  out.push(at(s1))
  return out
}

/** Normal à direita por vértice (média das normais dos segmentos vizinhos, com correção de quina limitada). */
function normals(p: [number, number][]): [number, number][] {
  const n = p.length
  const dir = (a: [number, number], b: [number, number]): [number, number] => {
    const dx = b[0] - a[0]
    const dy = b[1] - a[1]
    const l = Math.hypot(dx, dy) || 1
    return [-dy / l, dx / l]
  }
  const out: [number, number][] = []
  for (let i = 0; i < n; i++) {
    const a = i > 0 ? dir(p[i - 1], p[i]) : dir(p[0], p[1])
    const b = i + 1 < n ? dir(p[i], p[i + 1]) : a
    let mx = a[0] + b[0]
    let my = a[1] + b[1]
    const ml = Math.hypot(mx, my)
    if (ml < 1e-9) {
      out.push(a)
      continue
    }
    mx /= ml
    my /= ml
    const c = Math.max(0.4, mx * a[0] + my * a[1])
    out.push([mx / c, my / c])
  }
  return out
}
