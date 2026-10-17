import { Buffer, BufferUsage, Container, Geometry, GlProgram, Mesh, Shader } from 'pixi.js'

/** floats por veículo no quadro do núcleo: x, y, dx, dy (frente e direção), velocidade, comprimento (negativo = ônibus). */
const STRIDE = 6
const BYTES = STRIDE * 4
/** acima desta distância (m) entre dois quadros o slot foi reaproveitado por outro veículo: sem interpolar */
const SNAP_DIST = 100

const VERT = `#version 300 es
precision highp float;
in vec2 aPosition;
in vec4 aP0;
in vec2 aM0;
in vec4 aP1;
in vec2 aM1;
uniform mat3 uProjectionMatrix;
uniform mat3 uWorldTransformMatrix;
uniform mat3 uTransformMatrix;
uniform float uT;
uniform float uFlip;
uniform float uZoom;
out vec3 vColor;
void main() {
  // quadro anterior (a) e atual (b): quem é quem depende de qual buffer recebeu o quadro mais novo
  vec4 pa = uFlip < 0.5 ? aP0 : aP1;
  vec2 ma = uFlip < 0.5 ? aM0 : aM1;
  vec4 pb = uFlip < 0.5 ? aP1 : aP0;
  vec2 mb = uFlip < 0.5 ? aM1 : aM0;
  float len = abs(mb.y);
  if (len < 0.01) { gl_Position = vec4(2.0, 2.0, 0.0, 1.0); vColor = vec3(0.0); return; }
  float bus = mb.y < 0.0 ? 1.0 : 0.0;
  float w0 = bus > 0.5 ? 2.5 : 1.9;
  // em zoom baixo o retângulo é ampliado para continuar visível
  float f = clamp(3.2 / (uZoom * w0), 1.0, 6.0);
  float L = len * f;
  float W = w0 * f;
  bool prevOk = abs(ma.y) > 0.01 && distance(pa.xy, pb.xy) < ${SNAP_DIST.toFixed(1)};
  float t = prevOk ? uT : 1.0;
  vec2 dm = mix(prevOk ? pa.zw : pb.zw, pb.zw, t);
  float dl = length(dm);
  vec2 d = dl > 1e-4 ? dm / dl : pb.zw;
  // (x, y) é a frente do veículo; o centro fica meio comprimento atrás
  vec2 c = mix(pa.xy - (prevOk ? pa.zw : pb.zw) * L * 0.5, pb.xy - pb.zw * L * 0.5, t);
  vec2 world = c + d * (aPosition.x * L) + vec2(-d.y, d.x) * (aPosition.y * W);
  float kmh = mb.x * 3.6;
  vColor = bus > 0.5 ? vec3(0.376, 0.647, 0.980)
         : kmh < 5.0 ? vec3(0.937, 0.267, 0.267)
         : kmh < 20.0 ? vec3(0.984, 0.573, 0.235)
         : kmh < 40.0 ? vec3(0.980, 0.800, 0.082)
         : vec3(0.204, 0.827, 0.600);
  vec3 p = uProjectionMatrix * uWorldTransformMatrix * uTransformMatrix * vec3(world, 1.0);
  gl_Position = vec4(p.xy, 0.0, 1.0);
}`

const FRAG = `#version 300 es
precision mediump float;
in vec3 vColor;
out vec4 finalColor;
void main() { finalColor = vec4(vColor, 1.0); }`

/**
 * Veículos por instancing: um retângulo orientado por slot (o id do veículo é a posição no quadro).
 * Os dois últimos quadros do núcleo ficam em dois buffers da GPU e o vertex shader interpola entre eles
 * (posição e direção), então o custo por quadro desenhado na CPU é só atualizar um uniform; o zoom
 * (tamanho mínimo na tela) também é um uniform. Cor por faixa de velocidade e ônibus calculada no shader.
 */
export class VehicleLayer {
  readonly container = new Container()
  private arrays: [Float32Array, Float32Array] = [new Float32Array(0), new Float32Array(0)]
  private used: [number, number] = [0, 0]
  private buffers: [Buffer, Buffer]
  private geometry: Geometry
  private shader: Shader
  private mesh: Mesh<Geometry, Shader>
  /** buffer que guarda o quadro mais novo */
  private cur: 0 | 1 = 0
  private frameAt = 0
  private interval = 66
  private alpha = 1
  private settled = true

  constructor() {
    const mk = () => new Buffer({ data: new Float32Array(STRIDE), usage: BufferUsage.VERTEX | BufferUsage.COPY_DST, shrinkToFit: false })
    this.buffers = [mk(), mk()]
    const corners = new Buffer({ data: new Float32Array([-0.5, -0.5, 0.5, -0.5, 0.5, 0.5, -0.5, 0.5]), usage: BufferUsage.VERTEX | BufferUsage.COPY_DST })
    const inst = (buffer: Buffer, offset: number, format: 'float32x4' | 'float32x2') => ({ buffer, format, stride: BYTES, offset, instance: true })
    this.geometry = new Geometry({
      attributes: {
        aPosition: { buffer: corners, format: 'float32x2', stride: 8, offset: 0 },
        aP0: inst(this.buffers[0], 0, 'float32x4'),
        aM0: inst(this.buffers[0], 16, 'float32x2'),
        aP1: inst(this.buffers[1], 0, 'float32x4'),
        aM1: inst(this.buffers[1], 16, 'float32x2'),
      },
      indexBuffer: new Uint32Array([0, 1, 2, 0, 2, 3]),
      instanceCount: 0,
    })
    this.shader = new Shader({
      glProgram: GlProgram.from({ vertex: VERT, fragment: FRAG, name: 'vehicles' }),
      resources: { vehicleUniforms: { uT: { value: 1, type: 'f32' }, uFlip: { value: 0, type: 'f32' }, uZoom: { value: 1, type: 'f32' } } },
    })
    this.mesh = new Mesh<Geometry, Shader>({ geometry: this.geometry, shader: this.shader })
    this.mesh.visible = false
    this.container.addChild(this.mesh)
  }

  private uniforms() {
    return this.shader.resources.vehicleUniforms.uniforms as { uT: number; uFlip: number; uZoom: number }
  }

  /** Nº de veículos no quadro atual. */
  get count() {
    const a = this.arrays[this.cur]
    let n = 0
    for (let i = 5; i < this.used[this.cur] * STRIDE; i += STRIDE) if (a[i] !== 0) n++
    return n
  }

  private grow(slots: number) {
    const cap = Math.max(slots, Math.ceil(this.arrays[0].length / STRIDE * 1.5), 1024)
    for (const k of [0, 1] as const) {
      const na = new Float32Array(cap * STRIDE)
      na.set(this.arrays[k].subarray(0, this.used[k] * STRIDE))
      this.arrays[k] = na
      this.buffers[k].data = na
    }
  }

  /** Novo quadro do núcleo (null limpa): vira o "atual"; o anterior fica como ponto de partida da interpolação. */
  set(buf: Float32Array | null, zoom: number) {
    const n = buf ? Math.floor(buf.length / STRIDE) : 0
    if (!buf || n === 0) {
      this.used = [0, 0]
      this.arrays[0].fill(0)
      this.arrays[1].fill(0)
      this.mesh.visible = false
      this.geometry.instanceCount = 0
      this.alpha = 1
      this.settled = true
      return
    }
    const first = this.used[0] === 0 && this.used[1] === 0
    if (n * STRIDE > this.arrays[0].length) this.grow(n)
    const now = performance.now()
    if (!first) {
      const dt = now - this.frameAt
      this.interval = Math.min(250, Math.max(30, this.interval * 0.7 + dt * 0.3))
    }
    const target: 0 | 1 = first ? 0 : this.cur === 0 ? 1 : 0
    this.write(target, buf, n)
    if (first) this.write(1, buf, n) // sem quadro anterior: começa parado no lugar
    this.cur = target
    this.frameAt = now
    this.alpha = first ? 1 : 0
    this.settled = first
    this.uniforms().uFlip = this.cur === 1 ? 0 : 1
    this.uniforms().uZoom = zoom
    this.uniforms().uT = this.alpha
    this.geometry.instanceCount = Math.max(this.used[0], this.used[1])
    this.mesh.visible = true
  }

  private write(k: 0 | 1, buf: Float32Array, n: number) {
    const arr = this.arrays[k]
    const prevUsed = this.used[k]
    arr.set(buf.length === n * STRIDE ? buf : buf.subarray(0, n * STRIDE))
    if (prevUsed > n) arr.fill(0, n * STRIDE, prevUsed * STRIDE)
    this.used[k] = n
    this.buffers[k].update(Math.max(n, prevUsed) * BYTES)
  }

  /** Reaplica o tamanho mínimo na tela (zoom): só um uniform, sem reconstruir nada. */
  rezoom(zoom: number) {
    this.uniforms().uZoom = zoom
  }

  /** Avança a interpolação para o instante `now`; true enquanto ainda há movimento a desenhar. */
  tick(now: number): boolean {
    if (this.settled) return false
    this.alpha = Math.min(1, (now - this.frameAt) / this.interval)
    this.uniforms().uT = this.alpha
    if (this.alpha >= 1) this.settled = true
    return true
  }

  private slot(id: number): { a: Float32Array; o: number } | null {
    const a = this.arrays[this.cur]
    const o = id * STRIDE
    if (id < 0 || id >= this.used[this.cur] || a[o + 5] === 0) return null
    return { a, o }
  }

  /** Centro, direção e comprimento do veículo `id`, já interpolado para o instante atual. */
  find(id: number): { x: number; y: number; a: number; len: number } | null {
    const s = this.slot(id)
    if (!s) return null
    const { a, o } = s
    const L = Math.abs(a[o + 5])
    const prev = this.arrays[1 - this.cur]
    const po = o
    const prevOk = id < this.used[1 - this.cur] && prev[po + 5] !== 0 && Math.hypot(prev[po] - a[o], prev[po + 1] - a[o + 1]) < SNAP_DIST
    const t = prevOk ? this.alpha : 1
    const fx = prevOk ? prev[po] + (a[o] - prev[po]) * t : a[o]
    const fy = prevOk ? prev[po + 1] + (a[o + 1] - prev[po + 1]) * t : a[o + 1]
    let dx = prevOk ? prev[po + 2] + (a[o + 2] - prev[po + 2]) * t : a[o + 2]
    let dy = prevOk ? prev[po + 3] + (a[o + 3] - prev[po + 3]) * t : a[o + 3]
    const dl = Math.hypot(dx, dy)
    if (dl > 1e-4) {
      dx /= dl
      dy /= dl
    } else {
      dx = a[o + 2]
      dy = a[o + 3]
    }
    return { x: fx - dx * L * 0.5, y: fy - dy * L * 0.5, a: Math.atan2(dy, dx), len: L }
  }

  /** Id do veículo mais próximo de (wx, wy) dentro de `r` metros do centro desenhado (interpolado), ou null. */
  pick(wx: number, wy: number, r: number): number | null {
    const a = this.arrays[this.cur]
    const prev = this.arrays[1 - this.cur]
    const np = this.used[1 - this.cur]
    let best: number | null = null
    let bd = r * r
    for (let id = 0; id < this.used[this.cur]; id++) {
      const o = id * STRIDE
      const L = Math.abs(a[o + 5])
      if (L === 0) continue
      // a frente desenhada fica entre a do quadro anterior e a do atual: descarta rápido o que está longe dos dois
      const px = id < np ? prev[o] : a[o]
      const py = id < np ? prev[o + 1] : a[o + 1]
      const m = r + L
      if (Math.max(px, a[o]) < wx - m || Math.min(px, a[o]) > wx + m || Math.max(py, a[o + 1]) < wy - m || Math.min(py, a[o + 1]) > wy + m) continue
      const c = this.find(id)
      if (!c) continue
      const d = (c.x - wx) ** 2 + (c.y - wy) ** 2
      if (d < bd) {
        bd = d
        best = id
      }
    }
    return best
  }
}
