import { Buffer, BufferUsage, Container, Geometry, GlProgram, Mesh, Shader } from 'pixi.js'
import { lineColor, lineOn, type TransitData, type TransitRecord } from '../model/transit'
import { LineMesh, type Poly } from './lineMesh'

/** Zoom mínimo (px/m) para as paradas aparecerem. */
const STOPS_Z = 0.5
/** largura dos itinerários: max(1,6 m, 2,2 px) */
const ROUTE_MIN_W = 1.6
const ROUTE_PX = 2.2

/** floats por vértice das paradas: centro (2), canto −1..1 (2), escala (1), anel (1), cor do disco (3), da borda (3) */
const STOP_STRIDE = 12

const STOP_VERT = `#version 300 es
precision highp float;
in vec2 aPosition;
in vec2 aCorner;
in float aScale;
in float aRing;
in vec3 aFill;
in vec3 aEdge;
uniform mat3 uProjectionMatrix;
uniform mat3 uWorldTransformMatrix;
uniform mat3 uTransformMatrix;
uniform float uZoom;
out vec2 vLocal;
out float vR;
out float vPx;
out float vRing;
out vec3 vFill;
out vec3 vEdge;
void main() {
  float px = 1.0 / uZoom;
  float r = max(1.5, 4.5 * px) * aScale;
  // o quad cobre o disco, a borda e o anel da parada selecionada
  float ext = r + (aRing > 0.5 ? 4.5 : 1.5) * px;
  vLocal = aCorner * ext;
  vR = r;
  vPx = px;
  vRing = aRing;
  vFill = aFill;
  vEdge = aEdge;
  vec3 p = uProjectionMatrix * uWorldTransformMatrix * uTransformMatrix * vec3(aPosition + vLocal, 1.0);
  gl_Position = vec4(p.xy, 0.0, 1.0);
}`

const STOP_FRAG = `#version 300 es
precision mediump float;
in vec2 vLocal;
in float vR;
in float vPx;
in float vRing;
in vec3 vFill;
in vec3 vEdge;
out vec4 finalColor;
void main() {
  float d = length(vLocal);
  // disco com borda de 1,2 px centrada no raio; anel verde de 2 px a 3 px do disco na parada selecionada
  float bw = 0.6 * vPx;
  vec3 c;
  float outer;
  if (vRing > 0.5 && d > vR + bw) {
    float rr = vR + 3.0 * vPx;
    if (abs(d - rr) > vPx * 1.5) discard;
    c = vec3(0.267, 0.831, 0.573);
    outer = rr + vPx;
    float a = (1.0 - smoothstep(outer - 0.5 * vPx, outer + 0.5 * vPx, d)) * smoothstep(rr - vPx - 0.5 * vPx, rr - vPx + 0.5 * vPx, d);
    finalColor = vec4(c * a, a);
    return;
  }
  c = d < vR - bw ? vFill : vEdge;
  outer = vR + bw;
  float a = 1.0 - smoothstep(outer - 0.5 * vPx, outer + 0.5 * vPx, d);
  if (a <= 0.0) discard;
  finalColor = vec4(c * a, a);
}`

const rgb = (c: number): [number, number, number] => [((c >> 16) & 255) / 255, ((c >> 8) & 255) / 255, (c & 255) / 255]

/**
 * Camada Ônibus: itinerários das linhas ligadas (uma cor por linha, na faixa da direita do caminho encaixado) e as
 * paradas. Tudo em malhas estáticas com a largura em pixels de tela calculada no shader: o zoom só muda uniforms. Os
 * itinerários são refeitos quando linhas ligadas ou cores mudam, o realce quando a linha selecionada muda, e as
 * paradas (baratas) a cada `set`, quando visíveis.
 */
export class BusLayer {
  readonly container = new Container()
  private routes = new LineMesh({ widthPx: ROUTE_PX, minW: ROUTE_MIN_W, alpha: 0.7 })
  private selOutline = new LineMesh({ widthPx: ROUTE_PX, minW: ROUTE_MIN_W, extraPx: 4, alpha: 0.9 })
  private selLineMesh = new LineMesh({ widthPx: ROUTE_PX, minW: ROUTE_MIN_W, extraPx: 1.5 })
  private stops: Mesh<Geometry, Shader> | null = null
  private stopsShader: Shader
  private stopsDirty = true
  private data: TransitData | null = null
  private rec: TransitRecord | null = null
  private selLine: string | null = null
  private selStop: string | null = null
  private routesKey = ''
  private selKey = ''
  private zoom = 1

  constructor() {
    this.stopsShader = new Shader({
      glProgram: GlProgram.from({ vertex: STOP_VERT, fragment: STOP_FRAG, name: 'bus-stops' }),
      resources: { stopUniforms: { uZoom: { value: 1, type: 'f32' } } },
    })
    this.container.addChild(this.routes.container, this.selOutline.container, this.selLineMesh.container)
  }

  set(data: TransitData | null, rec: TransitRecord, selLine: string | null, selStop: string | null, zoom: number) {
    const newData = data !== this.data
    this.data = data
    this.rec = rec
    this.selLine = selLine
    this.selStop = selStop
    this.stopsDirty = true
    this.buildRoutes(newData)
    this.rezoom(zoom)
  }

  /** Zoom: só uniforms e a visibilidade das paradas (construídas na primeira vez que aparecem). */
  rezoom(zoom: number) {
    this.zoom = zoom
    for (const m of [this.routes, this.selOutline, this.selLineMesh]) m.rezoom(zoom)
    const show = !!this.data && zoom >= STOPS_Z
    if (show && this.stopsDirty) this.buildStops()
    ;(this.stopsShader.resources.stopUniforms.uniforms as { uZoom: number }).uZoom = zoom
    if (this.stops) this.stops.visible = show
  }

  /** Parada mais próxima de (x, y) a até `r` m (só as visíveis). */
  pickStop(x: number, y: number, r: number): string | null {
    if (!this.data || this.zoom < STOPS_Z) return null
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

  /** Itinerários e realce da linha selecionada, só se o que eles mostram mudou. */
  private buildRoutes(newData: boolean) {
    const { data, rec } = this
    const poly = (k: number, color: number): Poly => ({ pts: data!.pts, a: data!.off[k], b: data!.off[k + 1], color })
    // trecho -> cor (cada trecho uma vez; a última linha que passa nele dá a cor)
    const colorOf = new Map<number, number>()
    let key = ''
    let selPolys: number[] = []
    let selColor = 0xffffff
    if (data && rec) {
      for (const l of data.lines) {
        const c = lineColor(l.numero, rec.lines[l.id])
        const on = lineOn(rec, l.id)
        key += on ? `${c},` : '-,'
        if (l.id === this.selLine) selColor = c
        const isSel = l.id === this.selLine
        if (!on && !isSel) continue
        for (const p of l.padroes)
          for (const t of p.trechos) {
            if (t.pts < 0) continue
            if (on) colorOf.set(t.pts, c)
            if (isSel) selPolys.push(t.pts)
          }
      }
      selPolys = [...new Set(selPolys)]
    }
    if (newData || key !== this.routesKey) {
      this.routesKey = key
      this.routes.set(data ? [...colorOf].map(([k, c]) => poly(k, c)) : [])
    }
    const selKey = `${this.selLine}:${selColor}`
    if (newData || selKey !== this.selKey) {
      this.selKey = selKey
      this.selOutline.set(selPolys.map((k) => poly(k, 0x0b1220)))
      this.selLineMesh.set(selPolys.map((k) => poly(k, selColor)))
    }
    this.routes.alpha = this.selLine ? 0.25 : 0.7
  }

  /** Paradas: disco claro com borda escura; baia com borda laranja; editadas/criadas em laranja; as da linha
   *  selecionada maiores e a selecionada com anel verde. */
  private buildStops() {
    this.stopsDirty = false
    const { data } = this
    const n = data?.stops.length ?? 0
    if (this.stops) {
      this.stops.geometry.destroy(true)
      this.container.removeChild(this.stops)
      this.stops.destroy()
      this.stops = null
    }
    if (!data || n === 0) return
    const selNum = this.selLine ? data.lines.find((x) => x.id === this.selLine)?.numero : undefined
    const v = new Float32Array(n * 4 * STOP_STRIDE)
    const idx = new Uint32Array(n * 6)
    const corners = [-1, -1, 1, -1, 1, 1, -1, 1]
    const light = rgb(0xf8fafc)
    const orange = rgb(0xff9a3d)
    const dark = rgb(0x0b1220)
    // a parada selecionada vai por último (por cima das vizinhas)
    const order = data.stops.map((_, i) => i)
    const si = data.stops.findIndex((s) => s.id === this.selStop)
    if (si >= 0) order.push(order.splice(si, 1)[0])
    let o = 0
    order.forEach((i, q) => {
      const s = data.stops[i]
      const isSel = s.id === this.selStop
      const scale = isSel ? 1.8 : selNum != null && s.linhas.includes(selNum) ? 1.3 : 1
      const fill = s.editada || s.nova ? orange : light
      const edge = s.baia ? orange : dark
      for (let c = 0; c < 4; c++) {
        v.set([s.x, s.y, corners[c * 2], corners[c * 2 + 1], scale, isSel ? 1 : 0, ...fill, ...edge], o)
        o += STOP_STRIDE
      }
      const b = q * 4
      idx.set([b, b + 1, b + 2, b, b + 2, b + 3], q * 6)
    })
    const buffer = new Buffer({ data: v, usage: BufferUsage.VERTEX })
    const bytes = STOP_STRIDE * 4
    const at = (offset: number, format: 'float32' | 'float32x2' | 'float32x3') => ({ buffer, format, stride: bytes, offset })
    const geometry = new Geometry({
      attributes: {
        aPosition: at(0, 'float32x2'),
        aCorner: at(8, 'float32x2'),
        aScale: at(16, 'float32'),
        aRing: at(20, 'float32'),
        aFill: at(24, 'float32x3'),
        aEdge: at(36, 'float32x3'),
      },
      indexBuffer: idx,
    })
    this.stops = new Mesh<Geometry, Shader>({ geometry, shader: this.stopsShader })
    this.container.addChild(this.stops)
  }
}
