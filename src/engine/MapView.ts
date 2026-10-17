import { Application, Container, Graphics, type Mesh } from 'pixi.js'
import type { Zone } from '../model/demand'
import type { NewPt } from '../model/persistence'
import { presetWidth, type RoadPreset, ROAD_PRESETS } from '../model/presets'
import type { NetworkData, NodeConns, SignalDef, TileUpdate } from '../model/types'
import { C } from './colors'
import { GpuTimer, perf } from './perf'
import { LabelLayer } from './LabelLayer'
import { makeDots, makeMesh } from './meshes'
import { SignalLayer } from './SignalLayer'
import { HEAT_PALETTE, HeatLayer } from './HeatLayer'
import { VehicleLayer } from './VehicleLayer'
import { SignLayer, type Approach } from './SignLayer'
import { drawHover, drawTurns, nodeLanes, pickConn, pickDst, pickSrc, type NodeLanes } from './TurnOverlay'

export type LayerKey = 'vehicles' | 'roads' | 'markings' | 'labels' | 'nodes' | 'shapes' | 'signals' | 'turns' | 'grid' | 'zones'
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
const LOD: Partial<Record<LayerKey, number>> = { markings: 0.6, nodes: 0.4, shapes: 2.2, signals: 0.3, labels: 0.3 }

type V = [number, number]
/** Alvo de encaixe do cursor: nó existente, corpo de uma via (divide) ou ponto livre. */
interface Snap {
  x: number
  y: number
  kind: 'node' | 'way' | 'free'
  node?: number
  way?: number
}
interface DrawState {
  start: Snap | null
  /** ponto de controle da curva (modo curva, 1º trecho) */
  ctrl: V | null
  /** tangente ao fim do último trecho (continuação suave da curva) */
  lastDir: V | null
  newChain: boolean
}

const quad = (a: V, c: V, b: V): V[] => {
  const n = Math.min(40, Math.max(4, Math.ceil((Math.hypot(c[0] - a[0], c[1] - a[1]) + Math.hypot(b[0] - c[0], b[1] - c[1])) / 6)))
  const out: V[] = []
  for (let i = 0; i <= n; i++) {
    const t = i / n
    const u = 1 - t
    out.push([u * u * a[0] + 2 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2 * u * t * c[1] + t * t * b[1]])
  }
  return out
}
const polyLen = (p: V[]) => p.reduce((a, q, i) => (i ? a + Math.hypot(q[0] - p[i - 1][0], q[1] - p[i - 1][1]) : 0), 0)
const MIN_PIECE = 6
/** Ângulo máximo (graus) entre o chord e cada tangente da curva; acima disso a curva faz laço e é recusada. */
const MAX_BEND = 100
const angleBetween = (a: V, b: V) => {
  const l = Math.hypot(a[0], a[1]) * Math.hypot(b[0], b[1])
  if (l < 1e-9) return 0
  return (Math.acos(Math.max(-1, Math.min(1, (a[0] * b[0] + a[1] * b[1]) / l))) * 180) / Math.PI
}
/** Curva quadrática (a, c, b) é aceitável se as duas tangentes não passam de MAX_BEND em relação à corda. */
const curveOk = (a: V, c: V, b: V) => {
  const chord: V = [b[0] - a[0], b[1] - a[1]]
  return angleBetween([c[0] - a[0], c[1] - a[1]], chord) <= MAX_BEND && angleBetween([b[0] - c[0], b[1] - c[1]], chord) <= MAX_BEND
}

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
  private trackG = new Graphics()
  private trackId: number | null = null
  private trackPts: Float32Array | null = null
  private trackBlocker: number | null = null
  private follow = false
  private labels!: LabelLayer
  private signs!: SignLayer
  private vehicles = new VehicleLayer()
  private heat = new HeatLayer()
  private segG = new Graphics()
  private buildG = new Graphics()
  private badge!: HTMLDivElement
  private buildPreset: RoadPreset = ROAD_PRESETS[0]
  private buildCurve = false
  private draw: DrawState = { start: null, ctrl: null, lastDir: null, newChain: true }
  private cursorSnap: Snap | null = null
  private mode: 'nodes' | 'lanes' | 'build' | 'erase' | 'zones' | 'signal' = 'nodes'
  private zonesG = new Graphics()
  private zoneEl!: HTMLDivElement
  private zones: Zone[] = []
  private selZone: number | null = null
  private zoneDraw: V[] = []
  private zoneCursor: V | null = null
  private lastZoneClick = { t: 0, x: 0, y: 0 }
  onZoneCreate: (poly: [number, number][]) => void = () => {}
  onZoneSelect: (id: number | null) => void = () => {}
  onZoneEdit: (id: number, poly: [number, number][]) => void = () => {}
  private selectedSeg: number | null = null
  private hoverSeg: number | null = null
  private segBox: Float32Array = new Float32Array(0)
  onSegSelect: (seg: number | null) => void = () => {}
  /** Trecho desenhado confirmado (pontos em metros locais). */
  onBuildCommit: (pts: NewPt[], newChain: boolean) => void = () => {}
  /** clique no modo Semáforo: nó existente ou ponto no corpo de uma via */
  onSignalPlace: (p: { kind: 'node' | 'way'; x: number; y: number; node?: number; way?: number }) => void = () => {}
  onErase: (seg: number) => void = () => {}
  private signals = new SignalLayer()
  private data: NetworkData | null = null
  private cam = { x: 0, y: 0, z: 1 }
  private user: LayerState = { roads: true, markings: true, labels: true, nodes: true, shapes: false, signals: true, turns: true, grid: true, zones: true, vehicles: true }
  private selectable = new Uint32Array(0)
  private selected: number | null = null
  private hover: number | null = null
  private dirty = true
  private raf = 0
  private labelTimer = 0
  private lastLabelRun = 0
  private overlayZoom = 0
  /** relógio da simulação (s); sem simulação os semáforos ficam parados no instante 0 */
  private simClock: number | null = null
  private simClockDrawn = -1
  private lastSignalDraw = 0
  private ro?: ResizeObserver
  private destroyed = false
  private gpu: GpuTimer | null = null

  onSelect: (node: number | null) => void = () => {}
  /** O usuário escolheu uma placa de preferência para um ramo do nó selecionado. */
  onPriority: (key: string, level: number) => void = () => {}
  onZoom: (z: number) => void = () => {}
  /** Chamado ao clicar numa curva de conversão do nó selecionado. */
  onConnClick: (conn: number) => void = () => {}
  /** clique num veículo (id do núcleo) ou fora dele (null) enquanto havia um selecionado */
  onVehiclePick: (id: number | null) => void = () => {}
  /** Chamado ao soltar um arrasto de faixa de origem -> faixa de destino. */
  onConnEdit: (fromLane: number, toLane: number) => void = () => {}
  private hoverConn: number | null = null
  private lanes: NodeLanes | null = null
  private nodeConns: NodeConns | null = null
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
    this.gpu = new GpuTimer(app.renderer)
    host.appendChild(app.canvas)
    this.labels = new LabelLayer(host)
    this.signs = new SignLayer(host)
    this.signs.onPick = (k, lv) => this.onPriority(k, lv)
    this.zoneEl = document.createElement('div')
    this.zoneEl.className = 'zone-labels'
    host.appendChild(this.zoneEl)
    this.badge = document.createElement('div')
    this.badge.className = 'build-badge'
    this.badge.hidden = true
    host.appendChild(this.badge)
    app.canvas.style.display = 'block'
    app.canvas.style.touchAction = 'none'

    app.stage.addChild(this.gridG, this.world)
    this.world.addChild(this.zonesG, this.roads, this.markings, this.heat.container, this.vehicles.container, this.trackG, this.segG, this.buildG, this.shapes, this.nodes, this.signals.g, this.turnsG, this.hoverG)
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
    this.signs?.destroy()
    this.badge?.remove()
    this.zoneEl?.remove()
    if (this.app.renderer) this.app.destroy(true, { children: true })
  }

  // ------------------------------------------------------------ dados

  private tileObjs = new Map<number, Mesh[]>()
  /** caixa (xmin,ymin,xmax,ymax) de cada ladrilho em metros, para não enviar à GPU o que está fora da tela */
  private tileBox = new Map<number, [number, number, number, number]>()
  /** coordenadas dos pontos de cada ladrilho; os círculos só são gerados quando a camada aparece naquele ladrilho */
  private tileDots = new Map<number, { shape: Float32Array; minor: Float32Array; major: Float32Array; shapeDone: boolean; nodesDone: boolean }>()

  private clearScene() {
    for (const meshes of this.tileObjs.values()) for (const m of meshes) m.destroy(true)
    this.tileObjs.clear()
    this.tileBox.clear()
    this.tileDots.clear()
    this.labels.clear()
  }

  /** Aplica os ladrilhos regenerados (e remove os esvaziados); o resto da cena fica intocado. */
  private applyTiles(u: TileUpdate) {
    for (const id of u.removed) this.dropTile(id)
    for (const t of u.dirty) {
      this.dropTile(t.id)
      const meshes: Mesh[] = []
      const add = (parent: Container, m: Mesh | null) => {
        if (m) {
          parent.addChild(m)
          meshes.push(m)
        }
      }
      // cópias (slice): os buffers do worker são visões de blocos grandes compartilhados
      const mesh = (m: { pos: Float32Array; idx: Uint32Array }, color: number) =>
        m.idx.length ? makeMesh({ pos: m.pos.slice(), idx: m.idx.slice() }, color) : null
      add(this.roads, mesh(t.asphalt, C.asphalt))
      add(this.roads, mesh(t.bus, C.bus))
      add(this.markings, mesh(t.white, C.white))
      add(this.markings, mesh(t.yellow, C.yellow))
      // círculos dos nós/pontos de forma são caros de gerar: criados sob demanda (ver cullTiles)
      this.tileDots.set(t.id, { shape: t.shape.slice(), minor: t.minor.slice(), major: t.major.slice(), shapeDone: false, nodesDone: false })
      this.tileObjs.set(t.id, meshes)
      const box: [number, number, number, number] = [Infinity, Infinity, -Infinity, -Infinity]
      for (const pos of [t.asphalt.pos, t.bus.pos, t.white.pos, t.yellow.pos, t.shape, t.minor, t.major]) {
        for (let i = 0; i < pos.length; i += 2) {
          const x = pos[i]
          const y = pos[i + 1]
          if (x < box[0]) box[0] = x
          if (x > box[2]) box[2] = x
          if (y < box[1]) box[1] = y
          if (y > box[3]) box[3] = y
        }
      }
      this.tileBox.set(t.id, box)
    }
  }

  /** Mostra só os ladrilhos que cruzam a tela (com folga): corta milhares de draw calls por quadro. */
  private cullTiles() {
    const { x, y, z } = this.cam
    const pad = 20 / z
    const x0 = -x / z - pad
    const y0 = -y / z - pad
    const x1 = (this.host.clientWidth - x) / z + pad
    const y1 = (this.host.clientHeight - y) / z + pad
    const shapesOn = this.user.shapes && z >= (LOD.shapes ?? 0)
    const nodesOn = this.user.nodes && z >= (LOD.nodes ?? 0)
    for (const [id, meshes] of this.tileObjs) {
      const b = this.tileBox.get(id)
      const vis = !b || (b[2] >= x0 && b[0] <= x1 && b[3] >= y0 && b[1] <= y1)
      const dots = this.tileDots.get(id)
      if (vis && dots) {
        const add = (parent: Container, m: Mesh | null) => {
          if (m) {
            parent.addChild(m)
            meshes.push(m)
          }
        }
        if (shapesOn && !dots.shapeDone) {
          dots.shapeDone = true
          add(this.shapes, makeDots(dots.shape, 0.7, C.nodeShape, 6))
        }
        if (nodesOn && !dots.nodesDone) {
          dots.nodesDone = true
          add(this.nodes, makeDots(dots.minor, 1.25, C.nodeMinor, 8))
          add(this.nodes, makeDots(dots.major, 2.0, C.nodeJunction, 10))
        }
      }
      for (const m of meshes) m.visible = vis
    }
  }

  private dropTile(id: number) {
    const old = this.tileObjs.get(id)
    if (old) for (const m of old) m.destroy(true)
    this.tileObjs.delete(id)
    this.tileBox.delete(id)
    this.tileDots.delete(id)
  }

  /** Estruturas derivadas da rede inteira (seleção, rótulos, caixas): leves, refeitas a cada atualização. */
  private buildScene(d: NetworkData) {
    const sel: number[] = []
    for (let i = 0; i < d.nodeInfo.length / 2; i++) {
      const deg = d.nodeInfo[i * 2]
      const flags = d.nodeInfo[i * 2 + 1]
      if (deg >= 3 || (flags & 1) === 1) sel.push(i)
    }
    this.selectable = Uint32Array.from(sel)
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
    this.heat.invalidate()
    this.heat.set(null, null)
    this.applyTiles(d.tiles)
    this.buildScene(d)
    this.nodeConns = null
    this.fit()
    this.applyLayers()
  }

  /** Troca os dados (ex.: após editar faixas) mantendo câmera e seleção. */
  updateNetwork(d: NetworkData) {
    const sel = this.selected
    const seg = this.selectedSeg
    this.labels.clear()
    this.heat.invalidate()
    this.heat.set(null, null)
    this.data = d
    this.applyTiles(d.tiles)
    this.buildScene(d)
    this.connDrag = null
    this.activeSrc = null
    this.selectedSeg = seg != null && seg < d.segKeys.length / 3 ? seg : null
    this.hoverSeg = null
    this.selected = sel != null && sel < d.nodeInfo.length / 2 ? sel : null
    this.nodeConns = null // o store pede de novo as conexões do nó selecionado
    this.lanes = null
    this.applyLayers()
    this.scheduleLabels()
    this.redrawOverlay(true)
    this.drawSeg()
    this.drawBuild()
  }

  /** Alterna entre editar conversões (nós) e faixas (trechos). */
  setMode(m: 'nodes' | 'lanes' | 'build' | 'erase' | 'zones' | 'signal') {
    this.mode = m
    this.selected = null
    this.lanes = null
    this.activeSrc = null
    this.connDrag = null
    this.hoverConn = null
    this.hover = null
    this.hoverSeg = null
    this.selectedSeg = null
    this.resetDraw()
    this.zoneDraw = []
    this.zoneCursor = null
    if (m !== 'zones') this.selZone = null
    this.drawZones()
    this.turnsG.clear()
    this.signs.clear()
    this.hoverG.clear()
    this.drawSeg()
    this.app.canvas.style.cursor = m === 'build' || m === 'zones' ? 'crosshair' : 'grab'
    this.requestRender()
  }

  /** Tipo de via e modo reta/curva do construtor. */
  setBuild(preset: RoadPreset, curve: boolean) {
    this.buildPreset = preset
    this.buildCurve = curve
    if (curve && this.draw.start && this.draw.ctrl == null && this.draw.lastDir == null) this.draw.ctrl = null
    this.drawBuild()
  }

  // ------------------------------------------------------------ zonas de demanda

  setZones(zones: Zone[], selected: number | null) {
    this.zones = zones
    this.selZone = selected
    this.drawZones()
  }

  private zoneAt(wx: number, wy: number): Zone | null {
    for (let i = this.zones.length - 1; i >= 0; i--) {
      const poly = this.zones[i].poly
      let inside = false
      for (let a = 0, b = poly.length - 1; a < poly.length; b = a++) {
        const [ax, ay] = poly[a]
        const [bx, by] = poly[b]
        if (ay > wy !== by > wy && wx < ((bx - ax) * (wy - ay)) / (by - ay) + ax) inside = !inside
      }
      if (inside) return this.zones[i]
    }
    return null
  }

  private zoneVertexAt(sx: number, sy: number): { id: number; idx: number } | null {
    const z = this.zones.find((q) => q.id === this.selZone)
    if (!z || this.mode !== 'zones') return null
    const { x, y, zoom } = { x: this.cam.x, y: this.cam.y, zoom: this.cam.z }
    for (let i = 0; i < z.poly.length; i++) {
      if (Math.hypot(z.poly[i][0] * zoom + x - sx, z.poly[i][1] * zoom + y - sy) <= 10) return { id: z.id, idx: i }
    }
    return null
  }

  private drawZones() {
    const g = this.zonesG
    g.clear()
    const px = 1 / this.cam.z
    const labels: string[] = []
    this.zoneEl?.replaceChildren()
    for (const z of this.zones) {
      if (z.poly.length < 3) continue
      const sel = z.id === this.selZone
      g.poly(z.poly.flat()).fill({ color: z.color, alpha: sel ? 0.26 : 0.14 })
      g.poly(z.poly.flat()).stroke({ width: (sel ? 2.6 : 1.6) * px, color: sel ? 0xffffff : z.color, alpha: 0.9, join: 'round' })
      if (sel && this.mode === 'zones') for (const p of z.poly) g.circle(p[0], p[1], 6 * px).fill(0xffffff).stroke({ width: 2 * px, color: z.color })
      const el = document.createElement('span')
      el.textContent = z.name
      el.style.setProperty('--zc', `#${z.color.toString(16).padStart(6, '0')}`)
      el.dataset.id = String(z.id)
      this.zoneEl?.appendChild(el)
      labels.push(z.name)
    }
    // polígono em desenho
    if (this.mode === 'zones' && this.zoneDraw.length) {
      const pts = this.zoneCursor ? [...this.zoneDraw, this.zoneCursor] : this.zoneDraw
      g.moveTo(pts[0][0], pts[0][1])
      for (let i = 1; i < pts.length; i++) g.lineTo(pts[i][0], pts[i][1])
      if (pts.length >= 3) g.lineTo(pts[0][0], pts[0][1])
      g.stroke({ width: 2 * px, color: 0xffffff, alpha: 0.9, join: 'round' })
      for (const p of this.zoneDraw) g.circle(p[0], p[1], 4 * px).fill(0xffffff)
      if (this.zoneDraw.length >= 3) g.circle(this.zoneDraw[0][0], this.zoneDraw[0][1], 9 * px).stroke({ width: 2 * px, color: 0x4ade80 })
    }
    this.positionZoneLabels()
    this.requestRender()
  }

  private positionZoneLabels() {
    if (!this.zoneEl) return
    const spans = this.zoneEl.children
    let k = 0
    for (const z of this.zones) {
      if (z.poly.length < 3) continue
      const el = spans[k++] as HTMLElement | undefined
      if (!el) break
      let cx = 0
      let cy = 0
      for (const p of z.poly) {
        cx += p[0]
        cy += p[1]
      }
      cx /= z.poly.length
      cy /= z.poly.length
      el.style.transform = `translate(${cx * this.cam.z + this.cam.x}px, ${cy * this.cam.z + this.cam.y}px) translate(-50%, -50%)`
    }
  }

  private zoneClick(sx: number, sy: number) {
    const wx = (sx - this.cam.x) / this.cam.z
    const wy = (sy - this.cam.y) / this.cam.z
    const now = performance.now()
    const last = this.lastZoneClick
    this.lastZoneClick = { t: now, x: sx, y: sy }
    if (this.zoneDraw.length) {
      const first = this.zoneDraw[0]
      const nearFirst = Math.hypot(first[0] * this.cam.z + this.cam.x - sx, first[1] * this.cam.z + this.cam.y - sy) < 12
      const dbl = now - last.t < 350 && Math.hypot(last.x - sx, last.y - sy) < 6
      if (this.zoneDraw.length >= 3 && (nearFirst || dbl)) return this.finishZone()
      if (!dbl) this.zoneDraw.push([wx, wy])
      return this.drawZones()
    }
    const hit = this.zoneAt(wx, wy)
    if (hit) {
      this.selZone = hit.id
      this.onZoneSelect(hit.id)
      return this.drawZones()
    }
    if (this.selZone != null) {
      this.selZone = null
      this.onZoneSelect(null)
    }
    this.zoneDraw = [[wx, wy]]
    this.drawZones()
  }

  private finishZone() {
    const poly = this.zoneDraw.map((p): [number, number] => [Math.round(p[0] * 10) / 10, Math.round(p[1] * 10) / 10])
    this.zoneDraw = []
    this.zoneCursor = null
    if (poly.length >= 3) this.onZoneCreate(poly)
    this.drawZones()
  }

  private resetDraw() {
    this.draw = { start: null, ctrl: null, lastDir: null, newChain: true }
    this.cursorSnap = null
    this.badge.hidden = true
    this.buildG.clear()
    this.requestRender()
  }

  selectSeg(seg: number | null) {
    this.selectedSeg = seg
    this.drawSeg()
  }

  /** Trecho cujo corpo (asfalto) contém o ponto de tela, o mais próximo do eixo. */
  private pickSeg(sx: number, sy: number): number | null {
    return this.nearestSeg(sx, sy)?.seg ?? null
  }

  /** Trecho sob o ponto de tela e o ponto mais próximo do seu eixo. */
  private nearestSeg(sx: number, sy: number): { seg: number; x: number; y: number } | null {
    const d = this.data
    if (!d) return null
    const { x, y, z } = this.cam
    const wx = (sx - x) / z
    const wy = (sy - y) / z
    const n = d.segKeys.length / 3
    let best: { seg: number; x: number; y: number } | null = null
    let bd = Infinity
    for (let i = 0; i < n; i++) {
      const half = Math.max(((d.segU32[i * 9 + 2] + d.segU32[i * 9 + 3]) * d.segW[i]) / 2, 1.5) + 3 / z
      const bx = this.segBox
      if (wx < bx[i * 4] - half || wx > bx[i * 4 + 2] + half || wy < bx[i * 4 + 1] - half || wy > bx[i * 4 + 3] + half) continue
      let dm = Infinity
      let px = 0
      let py = 0
      for (let k = d.segPtsOff[i]; k < d.segPtsOff[i + 1] - 1; k++) {
        const ax = d.segPts[k * 2]
        const ay = d.segPts[k * 2 + 1]
        const vx = d.segPts[k * 2 + 2] - ax
        const vy = d.segPts[k * 2 + 3] - ay
        const l2 = vx * vx + vy * vy || 1
        const t = Math.max(0, Math.min(1, ((wx - ax) * vx + (wy - ay) * vy) / l2))
        const qx = ax + vx * t
        const qy = ay + vy * t
        const dd = Math.hypot(qx - wx, qy - wy)
        if (dd < dm) {
          dm = dd
          px = qx
          py = qy
        }
      }
      if (dm <= half && dm / half < bd) {
        bd = dm / half
        best = { seg: i, x: px, y: py }
      }
    }
    return best
  }

  /** Alvo de encaixe sob o cursor: nó (14 px) > corpo de via > ponto livre. */
  private snapAt(sx: number, sy: number): Snap {
    const d = this.data!
    const { x, y, z } = this.cam
    const wx = (sx - x) / z
    const wy = (sy - y) / z
    const r2 = (14 / z) ** 2
    let bi = -1
    let bd = r2
    for (let i = 0; i < d.nodeXY.length / 2; i++) {
      const dd = (d.nodeXY[i * 2] - wx) ** 2 + (d.nodeXY[i * 2 + 1] - wy) ** 2
      if (dd < bd) {
        bd = dd
        bi = i
      }
    }
    if (bi >= 0) return { x: d.nodeXY[bi * 2], y: d.nodeXY[bi * 2 + 1], kind: 'node', node: d.nodeOsm[bi] }
    const hit = this.nearestSeg(sx, sy)
    if (hit) return { x: hit.x, y: hit.y, kind: 'way', way: d.segKeys[hit.seg * 3] }
    return { x: wx, y: wy, kind: 'free' }
  }

  /** Polilinha do trecho em desenho até o cursor, ou null se ainda não há início. */
  private currentPiece(end: Snap): { pts: V[]; ctrl: V | null; ok: boolean } | null {
    const st = this.draw.start
    if (!st) return null
    const a: V = [st.x, st.y]
    const b: V = [end.x, end.y]
    if (!this.buildCurve) return { pts: [a, b], ctrl: null, ok: true }
    if (this.draw.ctrl) return { pts: quad(a, this.draw.ctrl, b), ctrl: this.draw.ctrl, ok: curveOk(a, this.draw.ctrl, b) }
    if (this.draw.lastDir) {
      const k = Math.hypot(b[0] - a[0], b[1] - a[1]) / 2
      const c: V = [a[0] + this.draw.lastDir[0] * k, a[1] + this.draw.lastDir[1] * k]
      return { pts: quad(a, c, b), ctrl: c, ok: curveOk(a, c, b) }
    }
    return { pts: [a, b], ctrl: null, ok: true }
  }

  private toNewPts(pts: V[], start: Snap, end: Snap): NewPt[] {
    return pts.map((p, i) => {
      const s = i === 0 ? start : i === pts.length - 1 ? end : null
      const out: NewPt = { x: Math.round(p[0] * 1000) / 1000, y: Math.round(p[1] * 1000) / 1000 }
      if (s?.kind === 'node') out.node = s.node
      if (s?.kind === 'way') out.way = s.way
      return out
    })
  }

  private buildClick() {
    const snap = this.cursorSnap
    if (!snap || !this.data) return
    const dr = this.draw
    if (!dr.start) {
      this.draw = { start: snap, ctrl: null, lastDir: null, newChain: true }
      return this.drawBuild()
    }
    if (this.buildCurve && !dr.ctrl && !dr.lastDir) {
      dr.ctrl = [snap.x, snap.y] // 2º clique define a curvatura
      return this.drawBuild()
    }
    const piece = this.currentPiece(snap)
    if (!piece || !piece.ok || polyLen(piece.pts) < MIN_PIECE) return
    this.onBuildCommit(this.toNewPts(piece.pts, dr.start, snap), dr.newChain)
    const n = piece.pts.length
    const t: V = [piece.pts[n - 1][0] - piece.pts[n - 2][0], piece.pts[n - 1][1] - piece.pts[n - 2][1]]
    const tl = Math.hypot(t[0], t[1]) || 1
    this.draw = { start: snap, ctrl: null, lastDir: [t[0] / tl, t[1] / tl], newChain: false }
    this.drawBuild()
  }

  /** Marcador do ponto onde o semáforo será colocado. */
  private drawSignalCursor() {
    const g = this.buildG
    g.clear()
    const sn = this.cursorSnap
    if (this.mode === 'signal' && sn && sn.kind !== 'free') {
      const px = 1 / this.cam.z
      g.circle(sn.x, sn.y, 9 * px).fill({ color: 0x0b1220, alpha: 0.6 }).stroke({ width: 2 * px, color: sn.kind === 'node' ? 0xfbbf24 : 0x34d399 })
      g.circle(sn.x, sn.y, 3.5 * px).fill(sn.kind === 'node' ? 0xfbbf24 : 0x34d399)
    }
    this.requestRender()
  }

  /** Prévia da via (largura real), marcadores de encaixe e rótulo de comprimento. */
  private drawBuild() {
    const g = this.buildG
    g.clear()
    const snap = this.cursorSnap
    if (this.mode !== 'build' || !snap || !this.data) {
      this.badge.hidden = true
      return this.requestRender()
    }
    const px = 1 / this.cam.z
    const st = this.draw.start
    const piece = this.currentPiece(snap)
    let len = 0
    let valid = false
    if (st && piece) {
      len = polyLen(piece.pts)
      valid = len >= MIN_PIECE && piece.ok
      const color = valid ? 0x4cc9f0 : 0xef4444
      g.moveTo(piece.pts[0][0], piece.pts[0][1])
      for (let i = 1; i < piece.pts.length; i++) g.lineTo(piece.pts[i][0], piece.pts[i][1])
      g.stroke({ width: presetWidth(this.buildPreset), color, alpha: 0.38, join: 'round', cap: 'butt' })
      g.moveTo(piece.pts[0][0], piece.pts[0][1])
      for (let i = 1; i < piece.pts.length; i++) g.lineTo(piece.pts[i][0], piece.pts[i][1])
      g.stroke({ width: 1.8 * px, color: 0xffffff, alpha: 0.9, join: 'round' })
      if (piece.ctrl && this.draw.ctrl) {
        g.circle(piece.ctrl[0], piece.ctrl[1], 5 * px).stroke({ width: 1.6 * px, color: 0xffffff, alpha: 0.9 })
      }
      g.circle(st.x, st.y, 6 * px).fill({ color: 0xffffff, alpha: 0.95 }).stroke({ width: 2 * px, color: 0x0b1110 })
    }
    // alvo de encaixe sob o cursor
    if (snap.kind === 'node') g.circle(snap.x, snap.y, 11 * px).stroke({ width: 2.4 * px, color: 0x4ade80 })
    else if (snap.kind === 'way') g.circle(snap.x, snap.y, 8 * px).stroke({ width: 2.2 * px, color: 0xfacc15 })
    g.circle(snap.x, snap.y, 3.5 * px).fill({ color: snap.kind === 'free' ? 0xffffff : snap.kind === 'node' ? 0x4ade80 : 0xfacc15 })

    // rótulo
    const hint = snap.kind === 'node' ? 'conectar' : snap.kind === 'way' ? 'dividir a via' : ''
    const stage = this.buildCurve && st && !this.draw.ctrl && !this.draw.lastDir ? 'clique para definir a curva' : ''
    const text = [st ? `${len.toFixed(0)} m${valid ? '' : piece?.ok === false ? ' · curva muito fechada' : ' · curto'}` : 'iniciar aqui', hint, stage].filter(Boolean).join(' · ')
    this.badge.textContent = text
    this.badge.hidden = false
    this.badge.style.transform = `translate(${snap.x * this.cam.z + this.cam.x + 16}px, ${snap.y * this.cam.z + this.cam.y + 16}px)`
    this.badge.classList.toggle('bad', !!st && !valid)
    this.requestRender()
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
      g.stroke({ width: total + 1.4, color: this.mode === 'erase' ? 0xef4444 : C.nodeMinor, alpha: this.mode === 'erase' ? 0.5 : selected ? 0.3 : 0.16, cap: 'butt', join: 'round' })
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

  /** Mapa de calor por trecho direcionado (níveis 0..4, -1 sem dado); null esconde. */
  setHeat(levels: Int8Array | null, palette: number[] = HEAT_PALETTE) {
    this.heat.set(this.data, levels, palette)
    this.requestRender()
  }

  /** Relógio da simulação para os semáforos (null = sem simulação: semáforos parados no instante 0). */
  setSimClock(t: number | null) {
    this.simClock = t
    if (t == null) {
      this.simClockDrawn = -1
      this.lastSignalDraw = 0
    } else if (this.signals.g.visible && t !== this.simClockDrawn) {
      this.simClockDrawn = t
      this.signals.draw(t, this.cam.z, this.selected)
      this.requestRender()
    }
  }

  /** Quadro da simulação (null limpa). */
  setVehicles(buf: Float32Array | null) {
    this.vehicles.set(buf, this.cam.z)
    this.followTrack()
    this.requestRender()
  }

  /** Câmera e realce do veículo acompanhado, na posição interpolada do instante atual. */
  private followTrack() {
    if (this.trackId == null) return
    if (this.follow) {
      const me = this.vehicles.find(this.trackId)
      if (me) {
        this.cam.x = this.app.screen.width / 2 - me.x * this.cam.z
        this.cam.y = this.app.screen.height / 2 - me.y * this.cam.z
        this.camChanged()
      }
    }
    this.drawTrack()
  }

  /** A câmera acompanha o veículo selecionado. */
  setFollow(on: boolean) {
    this.follow = on
  }

  /** Rota restante do veículo selecionado (pontos xy planos); null limpa. O último ponto é o destino. */
  setTrack(id: number | null, pts: ArrayLike<number> | null, blocker: number | null = null) {
    this.trackId = id
    this.trackBlocker = blocker
    this.trackPts = pts && pts.length >= 4 ? Float32Array.from(pts) : null
    this.drawTrack()
    this.requestRender()
  }

  private drawTrack() {
    const g = this.trackG
    g.clear()
    if (this.trackId == null) return
    const px = 1 / this.cam.z
    const me = this.vehicles.find(this.trackId)
    const p = this.trackPts
    if (p) {
      const poly = () => {
        // a rota nasce na posição atual do veículo
        g.moveTo(me ? me.x : p[0], me ? me.y : p[1])
        for (let i = 2; i < p.length; i += 2) g.lineTo(p[i], p[i + 1])
      }
      poly()
      g.stroke({ width: 7 * px, color: 0x0b1220, alpha: 0.55, cap: 'round', join: 'round' })
      poly()
      g.stroke({ width: 3.5 * px, color: 0x22d3ee, alpha: 0.95, cap: 'round', join: 'round' })
      const dx = p[p.length - 2]
      const dy = p[p.length - 1]
      g.circle(dx, dy, 11 * px).fill({ color: 0x0b1220, alpha: 0.7 }).stroke({ width: 2.5 * px, color: 0xffffff })
      g.circle(dx, dy, 5 * px).fill(0xf43f5e)
    }
    if (me) {
      const r = Math.max(me.len * 0.9, 9 * px)
      g.circle(me.x, me.y, r).stroke({ width: 2.5 * px, color: 0xffffff })
      // quem está impedindo este veículo de seguir
      const bl = this.trackBlocker != null ? this.vehicles.find(this.trackBlocker) : null
      if (bl) {
        g.moveTo(me.x, me.y).lineTo(bl.x, bl.y).stroke({ width: 3 * px, color: 0xfb923c, alpha: 0.95 })
        g.circle(bl.x, bl.y, Math.max(bl.len * 0.9, 9 * px)).stroke({ width: 2.5 * px, color: 0xfb923c })
      }
    }
  }

  setSignals(defs: SignalDef[]) {
    this.signals.setSignals(defs)
    this.lastSignalDraw = 0
    this.requestRender()
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
    this.nodeConns = null
    this.lanes = null
    this.signs.clear()
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
    /** arrasto de vértice de zona */
    let zv: { id: number; idx: number; moved: boolean } | null = null
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
    cv.addEventListener('contextmenu', (e) => {
      e.preventDefault()
      if (this.mode === 'zones') {
        const r = cv.getBoundingClientRect()
        const v = this.zoneVertexAt(e.clientX - r.left, e.clientY - r.top)
        if (v) {
          const z = this.zones.find((q) => q.id === v.id)
          if (z && z.poly.length > 3) this.onZoneEdit(z.id, z.poly.filter((_, i) => i !== v.idx))
        } else {
          this.zoneDraw = []
          this.zoneCursor = null
          this.drawZones()
        }
        return
      }
      if (this.mode === 'build') {
        this.draw = { start: null, ctrl: null, lastDir: null, newChain: true }
        this.drawBuild()
      }
    })
    cv.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      cv.setPointerCapture(e.pointerId)
      if (this.mode === 'zones') {
        const r = cv.getBoundingClientRect()
        const v = this.zoneVertexAt(e.clientX - r.left, e.clientY - r.top)
        if (v && !this.zoneDraw.length) {
          zv = { ...v, moved: false }
          return
        }
      }
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
      if (zv) {
        zv.moved = true
        const [wx, wy] = world(e)
        this.zones = this.zones.map((z) => (z.id === zv!.id ? { ...z, poly: z.poly.map((p, i): [number, number] => (i === zv!.idx ? [wx, wy] : p)) } : z))
        this.drawZones()
        return
      }
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
      if (this.mode === 'build' && this.data) {
        this.cursorSnap = this.snapAt(e.clientX - r.left, e.clientY - r.top)
        this.drawBuild()
        return
      }
      if (this.mode === 'signal' && this.data) {
        this.cursorSnap = this.snapAt(e.clientX - r.left, e.clientY - r.top)
        this.drawSignalCursor()
        return
      }
      if (this.mode === 'zones') {
        if (this.zoneDraw.length) {
          this.zoneCursor = [(e.clientX - r.left - this.cam.x) / this.cam.z, (e.clientY - r.top - this.cam.y) / this.cam.z]
          this.drawZones()
        } else {
          cv.style.cursor = this.zoneVertexAt(e.clientX - r.left, e.clientY - r.top) ? 'move' : 'crosshair'
        }
        return
      }
      if (this.mode === 'lanes' || this.mode === 'erase') {
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
      if (zv) {
        const v = zv
        zv = null
        if (v.moved) {
          const z = this.zones.find((q) => q.id === v.id)
          if (z) this.onZoneEdit(z.id, z.poly.map((p): [number, number] => [Math.round(p[0] * 10) / 10, Math.round(p[1] * 10) / 10]))
        }
        return
      }
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
      if (wasClick && this.mode === 'zones') {
        const r = cv.getBoundingClientRect()
        this.zoneClick(e.clientX - r.left, e.clientY - r.top)
        return
      }
      if (wasClick && this.mode === 'signal' && this.data) {
        const r = cv.getBoundingClientRect()
        const sn = this.snapAt(e.clientX - r.left, e.clientY - r.top)
        if (sn.kind === 'node') this.onSignalPlace({ kind: 'node', x: sn.x, y: sn.y, node: sn.node })
        else if (sn.kind === 'way') this.onSignalPlace({ kind: 'way', x: sn.x, y: sn.y, way: sn.way })
        return
      }
      if (wasClick && this.mode === 'build') {
        const r = cv.getBoundingClientRect()
        this.cursorSnap = this.snapAt(e.clientX - r.left, e.clientY - r.top)
        this.buildClick()
        return
      }
      if (wasClick && this.mode === 'erase') {
        const r = cv.getBoundingClientRect()
        const sg = this.pickSeg(e.clientX - r.left, e.clientY - r.top)
        if (sg != null) {
          this.hoverSeg = null
          this.drawSeg()
          this.onErase(sg)
        }
        return
      }
      if (wasClick && this.mode === 'lanes') {
        const r = cv.getBoundingClientRect()
        const sg = this.pickSeg(e.clientX - r.left, e.clientY - r.top)
        this.selectedSeg = sg
        this.drawSeg()
        this.onSegSelect(sg)
        return
      }
      if (wasClick && this.user.vehicles) {
        const [wx, wy] = world(e)
        const vid = this.vehicles.pick(wx, wy, Math.max(4, 10 / this.cam.z))
        if (vid != null) {
          this.onVehiclePick(vid)
          return
        }
        if (this.trackId != null) this.onVehiclePick(null)
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
    if (this.mode === 'zones' && e.key === 'Enter' && this.zoneDraw.length >= 3) return this.finishZone()
    if (e.key !== 'Escape') return
    if (this.mode === 'zones') {
      if (this.zoneDraw.length) {
        this.zoneDraw = []
        this.zoneCursor = null
      } else if (this.selZone != null) {
        this.selZone = null
        this.onZoneSelect(null)
      }
      return this.drawZones()
    }
    if (this.mode === 'build') {
      this.draw = { start: null, ctrl: null, lastDir: null, newChain: true }
      this.drawBuild()
      return
    }
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
    if (!this.nodeConns) return null
    return pickConn(this.nodeConns, (sx - x) / z, (sy - y) / z, 7 / z)
  }

  /** Conexões do nó selecionado, entregues pelo núcleo (ignora respostas de um nó que já não está selecionado). */
  setNodeConns(nc: NodeConns | null, node: number | null = nc?.node ?? null) {
    if (node !== this.selected) return
    this.nodeConns = nc
    this.lanes = nc ? nodeLanes(nc) : null
    this.redrawOverlay(true)
  }

  /** Placas de preferência do nó selecionado (lista vazia remove). Ignora respostas de um nó que já não está selecionado. */
  setApproaches(list: Approach[], node: number | null) {
    if (node !== this.selected) return
    this.signs.setApproaches(list, this.cam)
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
    if (this.mode === 'build') this.drawBuild()
    if (this.zones.length || this.zoneDraw.length) this.drawZones()
    this.vehicles.rezoom(z)
    if (this.trackId != null) this.drawTrack()
    this.labels?.reposition(this.cam)
    this.signs?.reposition(this.cam)
    this.scheduleLabels()
    if (this.selected != null && Math.abs(z / (this.overlayZoom || z) - 1) > 0.02) this.redrawOverlay(false)
    if (this.hover != null && this.data) drawHover(this.hoverG, this.data, this.hover === this.selected ? null : this.hover, z)
    this.onZoom(z)
    this.requestRender()
  }

  private applyLayers() {
    this.cullTiles()
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
    this.zonesG.visible = this.user.zones
    this.vehicles.container.visible = this.user.vehicles
    if (this.zoneEl) this.zoneEl.style.display = this.user.zones ? '' : 'none'
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
    else drawTurns(this.turnsG, this.data, this.nodeConns, this.selected, this.cam.z, { highlight: this.hoverConn, activeSrc: this.activeSrc, drag: this.connDrag })
    drawHover(this.hoverG, this.data, this.hover === this.selected ? null : this.hover, this.cam.z)
    this.requestRender()
  }

  // ------------------------------------------------------------ laço de render sob demanda

  requestRender() {
    this.dirty = true
  }

  private loop = () => {
    if (this.destroyed) return
    const t0 = performance.now()
    try {
      this.frame()
    } catch (e) {
      console.error('Erro no quadro de render', e)
      this.dirty = true
    }
    perf.uiMs += performance.now() - t0
    this.raf = requestAnimationFrame(this.loop)
  }

  private frame() {
    // veículos: a interpolação entre quadros da simulação roda na GPU; aqui só avança o instante
    if (this.vehicles.tick(performance.now())) {
      this.followTrack()
      this.dirty = true
    }
    // os semáforos só mudam com o relógio da simulação (ou ao pedir redesenho: zoom, seleção, edição)
    if (this.signals.g.visible && this.lastSignalDraw === 0) {
      this.lastSignalDraw = performance.now()
      this.signals.draw(this.simClock ?? 0, this.cam.z, this.selected)
      this.dirty = true
    }
    if (this.dirty) {
      this.dirty = false
      this.gpu?.begin()
      this.app.render()
      this.gpu?.end()
      perf.renders++
    }
    this.gpu?.poll()
  }
}
