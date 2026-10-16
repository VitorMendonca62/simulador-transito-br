import { Application, Container, Graphics } from 'pixi.js'
import type { NetworkData, SignalDef } from '../model/types'
import { C } from './colors'
import { LabelLayer } from './LabelLayer'
import { makeDots, makeMesh, pick } from './meshes'
import { SignalLayer } from './SignalLayer'
import { drawHover, drawTurns, nodeLanes, pickConn, pickDst, pickSrc, type NodeLanes } from './TurnOverlay'

export type LayerKey = 'roads' | 'markings' | 'labels' | 'nodes' | 'shapes' | 'signals' | 'turns' | 'grid'
export type LayerState = Record<LayerKey, boolean>

const MIN_Z = 0.04
const MAX_Z = 60
/** Zoom mínimo (px/m) para cada camada aparecer (LOD). */
/** Área livre (px) não coberta por painéis flutuantes, usada ao enquadrar o mapa. */
const insets = () => {
  const narrow = window.innerWidth < 760
  return { top: 70, right: narrow ? 12 : 344, bottom: 48, left: 12 }
}
/** Caixa envolvente (xmin,ymin,xmax,ymax) de cada trecho, para acelerar a seleção. */
function segBounds(d: NetworkData): Float32Array {
  const n = d.segKeys.length / 3
  const out = new Float32Array(n * 4)
  for (let i = 0; i < n; i++) {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
    for (let k = d.segPtsOff[i]; k < d.segPtsOff[i + 1]; k++) {
      const x = d.segPts[k * 2]
      const y = d.segPts[k * 2 + 1]
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
    out.set([x0, y0, x1, y1], i * 4)
  }
  return out
}
const LOD: Partial<Record<LayerKey, number>> = { markings: 0.35, nodes: 0.2, shapes: 2.2, signals: 0.3, labels: 0.3 }

export class MapView {
  readonly app = new Application()
  private world = new Container()
  private gridG = new Graphics()
  private roads = new Container()
  private markings = new Container()
  private nodes = new Container()
  private shapes = new Container()
  private turnsG = new Graphics()
  private hoverG = new Graphics()
  private labels!: LabelLayer
  private segG = new Graphics()
  private mode: 'nodes' | 'lanes' = 'nodes'
  private selectedSeg: number | null = null
  private hoverSeg: number | null = null
  private segBox: Float32Array = new Float32Array(0)
  onSegSelect: (seg: number | null) => void = () => {}
  private signals = new SignalLayer()
  private data: NetworkData | null = null
  private cam = { x: 0, y: 0, z: 1 }
  private user: LayerState = { roads: true, markings: true, labels: true, nodes: true, shapes: false, signals: true, turns: true, grid: true }
  private selectable = new Uint32Array(0)
  private selected: number | null = null
  private hover: number | null = null
  private dirty = true
  private raf = 0
  private labelTimer = 0
  private lastLabelRun = 0
  private overlayZoom = 0
  private playing = true
  private speed = 1
  private clock = 0
  private lastFrame = performance.now()
  private lastSignalDraw = 0
  private ro?: ResizeObserver
  private destroyed = false

  onSelect: (node: number | null) => void = () => {}
  onZoom: (z: number) => void = () => {}
  /** Chamado ao clicar numa curva de conversão do nó selecionado. */
  onConnClick: (conn: number) => void = () => {}
  /** Chamado ao soltar um arrasto de faixa de origem -> faixa de destino. */
  onConnEdit: (fromLane: number, toLane: number) => void = () => {}
  private hoverConn: number | null = null
  private lanes: NodeLanes | null = null
  private activeSrc: number | null = null
  private connDrag: { x: number; y: number; dst: number | null } | null = null

  constructor(private host: HTMLElement) {}

  async init() {
    const { app, host } = this
    await app.init({
      width: host.clientWidth || 800,
      height: host.clientHeight || 600,
      background: C.bg,
      antialias: true,
      autoDensity: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2),
      autoStart: false,
      powerPreference: 'high-performance',
    })
    if (this.destroyed) return app.destroy(true)
    host.appendChild(app.canvas)
    this.labels = new LabelLayer(host)
    app.canvas.style.display = 'block'
    app.canvas.style.touchAction = 'none'

    app.stage.addChild(this.gridG, this.world)
    this.world.addChild(this.roads, this.markings, this.segG, this.shapes, this.nodes, this.signals.g, this.turnsG, this.hoverG)
    this.bindEvents()

    this.ro = new ResizeObserver(() => {
      app.renderer.resize(host.clientWidth, host.clientHeight)
      this.camChanged()
    })
    this.ro.observe(host)
    this.loop()
  }

  destroy() {
    this.destroyed = true
    cancelAnimationFrame(this.raf)
    clearTimeout(this.labelTimer)
    this.ro?.disconnect()
    this.labels?.destroy()
    if (this.app.renderer) this.app.destroy(true, { children: true })
  }

  // ------------------------------------------------------------ dados

  private clearScene() {
    for (const c of [this.roads, this.markings, this.nodes, this.shapes]) {
      for (const ch of c.removeChildren()) ch.destroy(true)
    }
    this.labels.clear()
  }

  private buildScene(d: NetworkData) {
    this.roads.addChild(makeMesh(d.asphalt, C.asphalt), makeMesh(d.bus, C.bus))
    this.markings.addChild(makeMesh(d.white, C.white), makeMesh(d.yellow, C.yellow))

    const major: number[] = []
    const minor: number[] = []
    const sel: number[] = []
    for (let i = 0; i < d.nodeInfo.length / 2; i++) {
      const deg = d.nodeInfo[i * 2]
      const flags = d.nodeInfo[i * 2 + 1]
      if (deg >= 3) major.push(i)
      else minor.push(i)
      if (deg >= 3 || (flags & 1) === 1) sel.push(i)
    }
    this.selectable = Uint32Array.from(sel)
    const shapeDots = makeDots(d.shapeXY, 0.7, C.nodeShape, 8)
    const minorDots = makeDots(pick(d.nodeXY, minor), 1.25, C.nodeMinor, 12)
    const majorDots = makeDots(pick(d.nodeXY, major), 2.0, C.nodeJunction, 14)
    if (shapeDots) this.shapes.addChild(shapeDots)
    if (minorDots) this.nodes.addChild(minorDots)
    if (majorDots) this.nodes.addChild(majorDots)

    this.labels.setData(d.labels, d.meta.names)
    this.signals.setSignals(d.meta.signals)
    this.segBox = segBounds(d)
  }

  setNetwork(d: NetworkData | null) {
    this.clearScene()
    this.turnsG.clear()
    this.hoverG.clear()
    this.segG.clear()
    this.selected = null
    this.selectedSeg = null
    this.hoverSeg = null
    this.lanes = null
    this.activeSrc = null
    this.connDrag = null
    this.hover = null
    this.data = d
    if (!d) return this.requestRender()
    this.buildScene(d)
    this.fit()
    this.applyLayers()
  }

  /** Troca os dados (ex.: após editar faixas) mantendo câmera e seleção. */
  updateNetwork(d: NetworkData) {
    const sel = this.selected
    const seg = this.selectedSeg
    this.clearScene()
    this.data = d
    this.buildScene(d)
    this.connDrag = null
    this.activeSrc = null
    this.selectedSeg = seg != null && seg < d.segKeys.length / 3 ? seg : null
    this.hoverSeg = null
    this.selected = sel != null && sel < d.nodeInfo.length / 2 ? sel : null
    this.lanes = this.selected != null ? nodeLanes(d, this.selected) : null
    this.applyLayers()
    this.scheduleLabels()
    this.redrawOverlay(true)
    this.drawSeg()
  }

  /** Alterna entre editar conversões (nós) e faixas (trechos). */
  setMode(m: 'nodes' | 'lanes') {
    this.mode = m
    this.selected = null
    this.lanes = null
    this.activeSrc = null
    this.connDrag = null
    this.hoverConn = null
    this.hover = null
    this.hoverSeg = null
    this.selectedSeg = null
    this.turnsG.clear()
    this.hoverG.clear()
    this.drawSeg()
    this.app.canvas.style.cursor = 'grab'
    this.requestRender()
  }

  selectSeg(seg: number | null) {
    this.selectedSeg = seg
    this.drawSeg()
  }

  /** Trecho cujo corpo (asfalto) contém o ponto de tela, o mais próximo do eixo. */
  private pickSeg(sx: number, sy: number): number | null {
    const d = this.data
    if (!d) return null
    const { x, y, z } = this.cam
    const wx = (sx - x) / z
    const wy = (sy - y) / z
    const n = d.segKeys.length / 3
    let best: number | null = null
    let bd = Infinity
    for (let i = 0; i < n; i++) {
      const half = Math.max(((d.segU32[i * 9 + 2] + d.segU32[i * 9 + 3]) * d.segW[i]) / 2, 1.5) + 3 / z
      const bx = this.segBox
      if (wx < bx[i * 4] - half || wx > bx[i * 4 + 2] + half || wy < bx[i * 4 + 1] - half || wy > bx[i * 4 + 3] + half) continue
      let dm = Infinity
      for (let k = d.segPtsOff[i]; k < d.segPtsOff[i + 1] - 1; k++) {
        const ax = d.segPts[k * 2]
        const ay = d.segPts[k * 2 + 1]
        const vx = d.segPts[k * 2 + 2] - ax
        const vy = d.segPts[k * 2 + 3] - ay
        const l2 = vx * vx + vy * vy || 1
        const t = Math.max(0, Math.min(1, ((wx - ax) * vx + (wy - ay) * vy) / l2))
        dm = Math.min(dm, Math.hypot(ax + vx * t - wx, ay + vy * t - wy))
      }
      if (dm <= half && dm / half < bd) {
        bd = dm / half
        best = i
      }
    }
    return best
  }

  /** Realce do trecho selecionado/sob o mouse, com setas de sentido (A→B). */
  private drawSeg() {
    const g = this.segG
    g.clear()
    const d = this.data
    if (!d) return this.requestRender()
    const px = 1 / this.cam.z
    for (const [seg, selected] of [[this.hoverSeg, false], [this.selectedSeg, true]] as const) {
      if (seg == null || (!selected && seg === this.selectedSeg)) continue
      const o = seg * 9
      const two = (d.segU32[o + 6] & 1) === 0
      const total = (d.segU32[o + 2] + d.segU32[o + 3]) * d.segW[seg]
      const a = d.segPtsOff[seg]
      const b = d.segPtsOff[seg + 1]
      g.moveTo(d.segPts[a * 2], d.segPts[a * 2 + 1])
      for (let k = a + 1; k < b; k++) g.lineTo(d.segPts[k * 2], d.segPts[k * 2 + 1])
      g.stroke({ width: total + 1.4, color: C.nodeMinor, alpha: selected ? 0.3 : 0.16, cap: 'butt', join: 'round' })
      if (!selected) continue
      // chevrons de sentido ao longo do eixo (para quem trafega do início ao fim do trecho)
      const step = 46 * px
      let acc = step / 2
      for (let k = a; k < b - 1; k++) {
        let x0 = d.segPts[k * 2]
        let y0 = d.segPts[k * 2 + 1]
        const x1 = d.segPts[k * 2 + 2]
        const y1 = d.segPts[k * 2 + 3]
        const len = Math.hypot(x1 - x0, y1 - y0)
        if (len < 1e-6) continue
        const ux = (x1 - x0) / len
        const uy = (y1 - y0) / len
        let rem = len
        while (acc <= rem) {
          x0 += ux * acc
          y0 += uy * acc
          rem -= acc
          acc = step
          const s = 6 * px
          g.moveTo(x0 - ux * s - uy * s, y0 - uy * s + ux * s).lineTo(x0 + ux * s * 0.4, y0 + uy * s * 0.4).lineTo(x0 - ux * s + uy * s, y0 - uy * s - ux * s)
        }
        acc -= rem
      }
      g.stroke({ width: 2.2 * px, color: 0xffffff, alpha: 0.9, cap: 'round', join: 'round' })
      void two
    }
    this.requestRender()
  }

  setSignals(defs: SignalDef[]) {
    this.signals.setSignals(defs)
    this.lastSignalDraw = 0
    this.requestRender()
  }

  setPlaying(p: boolean) {
    this.playing = p
  }
  setSpeed(s: number) {
    this.speed = s
  }

  setLayer(key: LayerKey, on: boolean) {
    this.user[key] = on
    this.applyLayers()
    if (key === 'labels') this.scheduleLabels()
  }

  select(node: number | null, focus = false) {
    this.selected = node
    this.lastSignalDraw = 0
    this.hoverConn = null
    this.activeSrc = null
    this.connDrag = null
    this.lanes = node != null && this.data ? nodeLanes(this.data, node) : null
    this.redrawOverlay(true)
    if (node != null && focus && this.data) {
      const d = this.data
      const z = Math.max(this.cam.z, 4)
      this.cam.z = Math.min(z, 12)
      const f = insets()
      this.cam.x = (f.left + this.host.clientWidth - f.right) / 2 - d.nodeXY[node * 2] * this.cam.z
      this.cam.y = (f.top + this.host.clientHeight - f.bottom) / 2 - d.nodeXY[node * 2 + 1] * this.cam.z
      this.camChanged()
    }
    this.onSelect(node)
  }

  fit() {
    const d = this.data
    const w = this.host.clientWidth
    const h = this.host.clientHeight
    if (!d) return
    const [x0, y0, x1, y1] = d.meta.bounds
    const bw = Math.max(x1 - x0, 50)
    const bh = Math.max(y1 - y0, 50)
    const f = insets()
    const aw = Math.max(w - f.left - f.right, 100)
    const ah = Math.max(h - f.top - f.bottom, 100)
    const z = Math.min((aw - 40) / bw, (ah - 40) / bh)
    this.cam.z = Math.min(Math.max(z, MIN_Z), MAX_Z)
    this.cam.x = f.left + aw / 2 - ((x0 + x1) / 2) * this.cam.z
    this.cam.y = f.top + ah / 2 - ((y0 + y1) / 2) * this.cam.z
    this.camChanged()
  }

  zoomBy(f: number) {
    this.zoomAt(this.host.clientWidth / 2, this.host.clientHeight / 2, f)
  }

  // ------------------------------------------------------------ câmera e entrada

  private zoomAt(mx: number, my: number, f: number) {
    const z = Math.min(Math.max(this.cam.z * f, MIN_Z), MAX_Z)
    const k = z / this.cam.z
    this.cam.x = mx - (mx - this.cam.x) * k
    this.cam.y = my - (my - this.cam.y) * k
    this.cam.z = z
    this.camChanged()
  }

  private bindEvents() {
    const cv = this.app.canvas
    let drag: { x: number; y: number; moved: boolean; id: number } | null = null
    /** arrasto a partir de uma faixa de origem (criar/remover conversão) */
    let src: { lane: number; sx: number; sy: number; moved: boolean } | null = null
    const world = (e: PointerEvent): [number, number] => {
      const r = cv.getBoundingClientRect()
      return [(e.clientX - r.left - this.cam.x) / this.cam.z, (e.clientY - r.top - this.cam.y) / this.cam.z]
    }
    cv.addEventListener('wheel', (e) => {
      e.preventDefault()
      const r = cv.getBoundingClientRect()
      const f = Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015))
      this.zoomAt(e.clientX - r.left, e.clientY - r.top, f)
    }, { passive: false })
    cv.addEventListener('pointerdown', (e) => {
      cv.setPointerCapture(e.pointerId)
      if (this.selected != null && this.data && this.lanes && this.user.turns) {
        const [wx, wy] = world(e)
        const lane = pickSrc(this.data, this.lanes, wx, wy, 13 / this.cam.z)
        if (lane != null) {
          src = { lane, sx: e.clientX, sy: e.clientY, moved: false }
          this.activeSrc = lane
          this.connDrag = null
          this.redrawOverlay(true)
          cv.style.cursor = 'crosshair'
          return
        }
      }
      drag = { x: e.clientX, y: e.clientY, moved: false, id: e.pointerId }
    })
    cv.addEventListener('pointermove', (e) => {
      const r = cv.getBoundingClientRect()
      if (src) {
        if (!src.moved && Math.hypot(e.clientX - src.sx, e.clientY - src.sy) < 4) return
        src.moved = true
        const [wx, wy] = world(e)
        this.connDrag = { x: wx, y: wy, dst: pickDst(this.data!, this.lanes!, wx, wy, 15 / this.cam.z) }
        this.redrawOverlay(true)
        return
      }
      if (drag) {
        const dx = e.clientX - drag.x
        const dy = e.clientY - drag.y
        if (!drag.moved && Math.hypot(dx, dy) < 4) return
        drag.moved = true
        drag.x = e.clientX
        drag.y = e.clientY
        this.cam.x += dx
        this.cam.y += dy
        cv.style.cursor = 'grabbing'
        this.camChanged()
        return
      }
      if (this.mode === 'lanes') {
        const sg = this.pickSeg(e.clientX - r.left, e.clientY - r.top)
        if (sg !== this.hoverSeg) {
          this.hoverSeg = sg
          cv.style.cursor = sg != null ? 'pointer' : 'grab'
          this.drawSeg()
        }
        return
      }
      const overSrc = this.selected != null && this.data && this.lanes && this.user.turns && pickSrc(this.data, this.lanes, (e.clientX - r.left - this.cam.x) / this.cam.z, (e.clientY - r.top - this.cam.y) / this.cam.z, 13 / this.cam.z) != null
      const hc = overSrc ? null : this.connAt(e.clientX - r.left, e.clientY - r.top)
      if (hc !== this.hoverConn) {
        this.hoverConn = hc
        this.redrawOverlay(true)
      }
      const n = hc != null || overSrc ? null : this.pickNode(e.clientX - r.left, e.clientY - r.top)
      if (n !== this.hover || hc != null || overSrc) {
        this.hover = n
        cv.style.cursor = overSrc ? 'crosshair' : n != null || hc != null ? 'pointer' : 'grab'
        if (this.data) drawHover(this.hoverG, this.data, n === this.selected ? null : n, this.cam.z)
        this.requestRender()
      }
    })
    const end = (e: PointerEvent) => {
      if (src) {
        const s = src
        src = null
        cv.style.cursor = 'grab'
        if (s.moved && this.connDrag?.dst != null) this.onConnEdit(s.lane, this.connDrag.dst)
        this.connDrag = null
        this.redrawOverlay(true)
        return
      }
      if (!drag) return
      const wasClick = !drag.moved
      drag = null
      cv.style.cursor = 'grab'
      if (wasClick && this.mode === 'lanes') {
        const r = cv.getBoundingClientRect()
        const sg = this.pickSeg(e.clientX - r.left, e.clientY - r.top)
        this.selectedSeg = sg
        this.drawSeg()
        this.onSegSelect(sg)
        return
      }
      if (wasClick) {
        const r = cv.getBoundingClientRect()
        const hc = this.connAt(e.clientX - r.left, e.clientY - r.top)
        if (hc != null) this.onConnClick(hc)
        else if (this.activeSrc != null && this.pickNode(e.clientX - r.left, e.clientY - r.top) == null) {
          this.activeSrc = null // primeiro clique no vazio só solta a origem
          this.redrawOverlay(true)
        } else this.select(this.pickNode(e.clientX - r.left, e.clientY - r.top))
      }
    }
    cv.addEventListener('pointerup', end)
    cv.addEventListener('pointercancel', () => {
      drag = null
      if (src) {
        src = null
        this.connDrag = null
        this.redrawOverlay(true)
      }
    })
    cv.style.cursor = 'grab'
    window.addEventListener('keydown', this.onKey)
  }

  private onKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape') return
    if (this.mode === 'lanes') {
      this.selectedSeg = null
      this.drawSeg()
      this.onSegSelect(null)
      return
    }
    if (this.activeSrc != null) {
      this.activeSrc = null
      this.redrawOverlay(true)
    } else this.select(null)
  }

  private connAt(sx: number, sy: number): number | null {
    if (this.selected == null || !this.data || !this.user.turns) return null
    const { x, y, z } = this.cam
    return pickConn(this.data, this.selected, (sx - x) / z, (sy - y) / z, 7 / z)
  }

  /** Redesenha o overlay do nó selecionado (após editar conversões). */
  refreshOverlay() {
    this.redrawOverlay(true)
  }

  private pickNode(sx: number, sy: number): number | null {
    const d = this.data
    if (!d) return null
    const { x, y, z } = this.cam
    const wx = (sx - x) / z
    const wy = (sy - y) / z
    const r = 16 / z
    let best = -1
    let bd = r * r
    const xy = d.nodeXY
    for (let i = 0; i < this.selectable.length; i++) {
      const n = this.selectable[i]
      const dx = xy[n * 2] - wx
      const dy = xy[n * 2 + 1] - wy
      const dd = dx * dx + dy * dy
      if (dd < bd) {
        bd = dd
        best = n
      }
    }
    return best < 0 ? null : best
  }

  private camChanged() {
    const { x, y, z } = this.cam
    this.world.position.set(x, y)
    this.world.scale.set(z)
    this.applyLayers()
    this.drawGrid()
    this.lastSignalDraw = 0
    if (this.selectedSeg != null) this.drawSeg()
    this.labels?.reposition(this.cam)
    this.scheduleLabels()
    if (this.selected != null && Math.abs(z / (this.overlayZoom || z) - 1) > 0.02) this.redrawOverlay(false)
    if (this.hover != null && this.data) drawHover(this.hoverG, this.data, this.hover === this.selected ? null : this.hover, z)
    this.onZoom(z)
    this.requestRender()
  }

  private applyLayers() {
    const z = this.cam.z
    const on = (k: LayerKey) => this.user[k] && z >= (LOD[k] ?? 0)
    this.roads.visible = this.user.roads
    this.markings.visible = on('markings')
    this.nodes.visible = on('nodes')
    this.shapes.visible = on('shapes')
    this.labels?.setVisible(on('labels'))
    this.signals.g.visible = on('signals')
    this.turnsG.visible = this.user.turns
    this.gridG.visible = this.user.grid
    this.requestRender()
  }

  private drawGrid() {
    const g = this.gridG
    g.clear()
    if (!this.user.grid) return
    const { x, y, z } = this.cam
    const w = this.host.clientWidth
    const h = this.host.clientHeight
    let step = 10
    while (step * z < 40) step *= 5
    const draw = (s: number, color: number, alpha: number) => {
      const px = s * z
      const ox = ((x % px) + px) % px
      const oy = ((y % px) + px) % px
      for (let gx = ox; gx < w; gx += px) g.moveTo(gx, 0).lineTo(gx, h)
      for (let gy = oy; gy < h; gy += px) g.moveTo(0, gy).lineTo(w, gy)
      g.stroke({ width: 1, color, alpha })
    }
    draw(step, C.grid, 1)
    draw(step * 5, C.gridMajor, 1)
  }

  private scheduleLabels() {
    const run = () => {
      this.lastLabelRun = performance.now()
      this.labelTimer = 0
      if (!this.data) return
      this.labels.place(this.cam, this.host.clientWidth, this.host.clientHeight)
      this.requestRender()
    }
    if (this.labelTimer) return
    const wait = Math.max(0, 90 - (performance.now() - this.lastLabelRun))
    this.labelTimer = window.setTimeout(run, wait)
  }

  private redrawOverlay(_force: boolean) {
    if (!this.data) return
    this.overlayZoom = this.cam.z
    if (this.selected == null) this.turnsG.clear()
    else drawTurns(this.turnsG, this.data, this.selected, this.cam.z, { highlight: this.hoverConn, activeSrc: this.activeSrc, drag: this.connDrag })
    drawHover(this.hoverG, this.data, this.hover === this.selected ? null : this.hover, this.cam.z)
    this.requestRender()
  }

  // ------------------------------------------------------------ laço de render sob demanda

  requestRender() {
    this.dirty = true
  }

  private loop = () => {
    if (this.destroyed) return
    try {
      this.frame()
    } catch (e) {
      console.error('Erro no quadro de render', e)
      this.dirty = true
    }
    this.raf = requestAnimationFrame(this.loop)
  }

  private frame() {
    const now = performance.now()
    const dt = (now - this.lastFrame) / 1000
    this.lastFrame = now
    if (this.playing) this.clock += dt * this.speed
    if (this.signals.g.visible && (this.lastSignalDraw === 0 || (this.playing && now - this.lastSignalDraw > 120))) {
      this.lastSignalDraw = now
      this.signals.draw(this.clock, this.cam.z, this.selected)
      this.dirty = true
    }
    if (this.dirty) {
      this.dirty = false
      this.app.render()
    }
  }
}
