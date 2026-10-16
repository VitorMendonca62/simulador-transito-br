import { Container, Mesh, MeshGeometry, Texture } from 'pixi.js'

/** Cores por faixa de velocidade (km/h): parado, lento, médio, livre; e ônibus. */
const COLORS = [0xef4444, 0xfb923c, 0xfacc15, 0x34d399, 0x60a5fa]

/**
 * Veículos como retângulos orientados. Cada quadro da simulação gera uma malha por faixa de velocidade
 * (5 desenhos no total, qualquer que seja o nº de veículos). Em zoom baixo os retângulos são ampliados
 * para continuarem visíveis.
 */
export class VehicleLayer {
  readonly container = new Container()
  private meshes: Mesh[] = COLORS.map((c) => {
    const m = new Mesh({ geometry: new MeshGeometry({ positions: new Float32Array(8), uvs: new Float32Array(8), indices: new Uint32Array([0, 1, 2, 0, 2, 3]) }), texture: Texture.WHITE })
    m.tint = c
    m.visible = false
    return m
  })
  private last: Float32Array | null = null
  private lastZoom = 0

  constructor() {
    for (const m of this.meshes) this.container.addChild(m)
  }

  get count() {
    return this.last ? this.last.length / 7 : 0
  }

  /** Substitui o quadro atual (formato: x, y, rumo, velocidade, tipo, comprimento, id por veículo). */
  set(buf: Float32Array | null, zoom: number) {
    this.last = buf
    this.lastZoom = zoom
    this.build(zoom)
  }

  /** Reconstrói com o quadro atual quando o zoom mudou o bastante. */
  rezoom(zoom: number) {
    if (this.last && Math.abs(zoom / (this.lastZoom || zoom) - 1) > 0.06) {
      this.lastZoom = zoom
      this.build(zoom)
    }
  }

  /** Id do veículo mais próximo de (wx, wy) dentro de `r` metros (centro do retângulo), ou null. */
  pick(wx: number, wy: number, r: number): number | null {
    const buf = this.last
    if (!buf) return null
    let best: number | null = null
    let bd = r * r
    for (let i = 0; i < buf.length; i += 7) {
      const L = buf[i + 5]
      const cx = buf[i] - Math.cos(buf[i + 2]) * L * 0.5
      const cy = buf[i + 1] - Math.sin(buf[i + 2]) * L * 0.5
      const d = (cx - wx) ** 2 + (cy - wy) ** 2
      if (d < bd) {
        bd = d
        best = buf[i + 6]
      }
    }
    return best
  }

  /** Posição (centro) e rumo do veículo `id` no quadro atual. */
  find(id: number): { x: number; y: number; a: number; len: number } | null {
    const buf = this.last
    if (!buf) return null
    for (let i = 0; i < buf.length; i += 7) {
      if (buf[i + 6] === id) {
        const a = buf[i + 2]
        const L = buf[i + 5]
        return { x: buf[i] - Math.cos(a) * L * 0.5, y: buf[i + 1] - Math.sin(a) * L * 0.5, a, len: L }
      }
    }
    return null
  }

  private build(zoom: number) {
    const buf = this.last
    if (!buf || buf.length === 0) {
      for (const m of this.meshes) m.visible = false
      return
    }
    const n = buf.length / 7
    const bucket = new Uint8Array(n)
    const counts = [0, 0, 0, 0, 0]
    for (let i = 0; i < n; i++) {
      const kmh = buf[i * 7 + 3] * 3.6
      const b = buf[i * 7 + 4] === 1 ? 4 : kmh < 5 ? 0 : kmh < 20 ? 1 : kmh < 40 ? 2 : 3
      bucket[i] = b
      counts[b]++
    }
    const pos = counts.map((c) => new Float32Array(c * 8))
    const fill = [0, 0, 0, 0, 0]
    const minPx = 3.2
    for (let i = 0; i < n; i++) {
      const o = i * 7
      const b = bucket[i]
      const w0 = buf[o + 4] === 1 ? 2.5 : 1.9
      const f = Math.min(6, Math.max(1, minPx / (zoom * w0)))
      const L = buf[o + 5] * f
      const W = w0 * f
      const ca = Math.cos(buf[o + 2])
      const sa = Math.sin(buf[o + 2])
      // (x, y) é a frente do veículo; o centro fica meio comprimento atrás
      const cx = buf[o] - ca * L * 0.5
      const cy = buf[o + 1] - sa * L * 0.5
      const fx = ca * L * 0.5
      const fy = sa * L * 0.5
      const rx = -sa * W * 0.5
      const ry = ca * W * 0.5
      const p = pos[b]
      let k = fill[b]
      p[k++] = cx + fx + rx
      p[k++] = cy + fy + ry
      p[k++] = cx + fx - rx
      p[k++] = cy + fy - ry
      p[k++] = cx - fx - rx
      p[k++] = cy - fy - ry
      p[k++] = cx - fx + rx
      p[k++] = cy - fy + ry
      fill[b] = k
    }
    for (let b = 0; b < 5; b++) {
      const m = this.meshes[b]
      const c = counts[b]
      m.visible = c > 0
      if (c === 0) continue
      const idx = new Uint32Array(c * 6)
      for (let q = 0; q < c; q++) {
        const v = q * 4
        idx.set([v, v + 1, v + 2, v, v + 2, v + 3], q * 6)
      }
      const old = m.geometry
      m.geometry = new MeshGeometry({ positions: pos[b], uvs: new Float32Array(pos[b].length), indices: idx })
      old.destroy()
    }
  }
}
