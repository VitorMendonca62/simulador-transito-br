import { Container, Graphics } from 'pixi.js'
import { lineColor, lineOn, type TransitData, type TransitRecord } from '../model/transit'

/** Zoom mínimo (px/m) para as paradas aparecerem. */
const STOPS_Z = 0.5

/**
 * Camada Ônibus: itinerários das linhas ligadas (uma cor por linha, na faixa da direita do caminho encaixado) e as
 * paradas. As larguras são em pixels de tela, então o desenho é refeito quando o zoom muda de faixa (fator 1,6), não a
 * cada quadro; a linha e a parada selecionadas vão por cima.
 */
export class BusLayer {
  readonly container = new Container()
  private routes = new Graphics()
  private sel = new Graphics()
  private stops = new Graphics()
  private data: TransitData | null = null
  private rec: TransitRecord | null = null
  private selLine: string | null = null
  private selStop: string | null = null
  private drawnZ = 0

  constructor() {
    this.container.addChild(this.routes, this.sel, this.stops)
  }

  set(data: TransitData | null, rec: TransitRecord, selLine: string | null, selStop: string | null, zoom: number) {
    this.data = data
    this.rec = rec
    this.selLine = selLine
    this.selStop = selStop
    this.draw(zoom)
  }

  /** Redesenha se o zoom mudou de faixa desde o último desenho. */
  rezoom(zoom: number) {
    if (!this.data || (zoom / this.drawnZ < 1.6 && this.drawnZ / zoom < 1.6)) {
      this.stops.visible = zoom >= STOPS_Z
      return
    }
    this.draw(zoom)
  }

  /** Parada mais próxima de (x, y) a até `r` m (só as visíveis). */
  pickStop(x: number, y: number, r: number): string | null {
    if (!this.data || !this.stops.visible) return null
    let best: string | null = null
    let bd = r * r
    for (const s of this.data.stops) {
      const d = (s.x - x) ** 2 + (s.y - y) ** 2
      if (d < bd) {
        bd = d
        best = s.id
      }
    }
    return best
  }

  private draw(zoom: number) {
    const { data, rec } = this
    this.routes.clear()
    this.sel.clear()
    this.stops.clear()
    this.drawnZ = zoom
    if (!data || !rec) return
    const px = 1 / zoom
    // itinerários: um traço por cor (as linhas de mesma cor vão juntas)
    const byColor = new Map<number, number[]>()
    let selPolys: number[] = []
    for (const l of data.lines) {
      const on = lineOn(rec, l.id)
      const polys: number[] = []
      for (const p of l.padroes) for (const t of p.trechos) if (t.pts >= 0) polys.push(t.pts)
      if (l.id === this.selLine) selPolys = polys
      if (!on) continue
      const c = lineColor(l.numero, rec.lines[l.id])
      byColor.set(c, [...(byColor.get(c) ?? []), ...polys])
    }
    const path = (g: Graphics, k: number) => {
      const a = data.off[k]
      const b = data.off[k + 1]
      if (b - a < 2) return
      g.moveTo(data.pts[a * 2], data.pts[a * 2 + 1])
      for (let i = a + 1; i < b; i++) g.lineTo(data.pts[i * 2], data.pts[i * 2 + 1])
    }
    const w = Math.max(1.6, 2.2 * px)
    for (const [c, polys] of byColor) {
      for (const k of polys) path(this.routes, k)
      this.routes.stroke({ width: w, color: c, alpha: this.selLine ? 0.25 : 0.7, join: 'round', cap: 'round' })
    }
    if (selPolys.length) {
      const l = data.lines.find((x) => x.id === this.selLine)
      const c = l ? lineColor(l.numero, rec.lines[l.id]) : 0xffffff
      for (const k of selPolys) path(this.sel, k)
      this.sel.stroke({ width: w + 4 * px, color: 0x0b1220, alpha: 0.9, join: 'round', cap: 'round' })
      for (const k of selPolys) path(this.sel, k)
      this.sel.stroke({ width: w + 1.5 * px, color: c, alpha: 1, join: 'round', cap: 'round' })
    }
    // paradas: disco claro com borda escura; baia com borda laranja; editadas/criadas em laranja; a da linha
    // selecionada maior
    const selNum = this.selLine ? data.lines.find((x) => x.id === this.selLine)?.numero : undefined
    const r = Math.max(1.5, 4.5 * px)
    for (const s of data.stops) {
      const onSel = selNum != null && s.linhas.includes(selNum)
      const rr = s.id === this.selStop ? r * 1.8 : onSel ? r * 1.3 : r
      this.stops.circle(s.x, s.y, rr).fill({ color: s.editada || s.nova ? 0xff9a3d : 0xf8fafc }).stroke({ width: 1.2 * px, color: s.baia ? 0xff9a3d : 0x0b1220 })
      if (s.id === this.selStop) this.stops.circle(s.x, s.y, rr + 3 * px).stroke({ width: 2 * px, color: 0x44d492 })
    }
    this.stops.visible = zoom >= STOPS_Z
  }
}
