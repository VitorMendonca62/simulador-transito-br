import { Buffer, BufferUsage, Container, Geometry, GlProgram, Mesh, Shader } from 'pixi.js'

/** floats por vértice: ponto do eixo (2), deslocamento em meias-larguras (2), cor rgb (3) */
const STRIDE = 7

const VERT = `#version 300 es
precision highp float;
in vec2 aPosition;
in vec2 aOff;
in vec3 aColor;
uniform mat3 uProjectionMatrix;
uniform mat3 uWorldTransformMatrix;
uniform mat3 uTransformMatrix;
uniform float uZoom;
uniform float uWidthPx;
uniform float uMinW;
uniform float uExtraPx;
out vec3 vColor;
void main() {
  // largura em pixels de tela, com mínimo em metros (em zoom alto o traço acompanha a via)
  float hw = (max(uMinW, uWidthPx / uZoom) + uExtraPx / uZoom) * 0.5;
  vec2 world = aPosition + aOff * hw;
  vColor = aColor;
  vec3 p = uProjectionMatrix * uWorldTransformMatrix * uTransformMatrix * vec3(world, 1.0);
  gl_Position = vec4(p.xy, 0.0, 1.0);
}`

const FRAG = `#version 300 es
precision mediump float;
in vec3 vColor;
uniform float uAlpha;
out vec4 finalColor;
void main() { finalColor = vec4(vColor * uAlpha, uAlpha); }`

let program: GlProgram | null = null

/** Polilinha a desenhar: pontos `a..b` (índices de ponto, b exclusivo) de `pts` (xy planos). */
export interface Poly {
  pts: ArrayLike<number>
  a: number
  b: number
  color: number
}

/** Largura do traço: `max(minW m, widthPx px) + extraPx px`. */
export interface LineStyle {
  widthPx: number
  minW?: number
  extraPx?: number
  alpha?: number
}

/**
 * Traços de largura constante na tela numa malha estática: cada ponto vira dois vértices deslocados pelo miter e a
 * largura sai do zoom no vertex shader. Mudar o zoom só atualiza um uniform (sem triangular de novo, ao contrário do
 * `Graphics.stroke`); a geometria só é refeita quando as polilinhas mudam. Pontas quadradas, juntas em miter.
 */
export class LineMesh {
  readonly container = new Container()
  private shader: Shader
  private mesh: Mesh<Geometry, Shader> | null = null

  constructor(style: LineStyle) {
    program ??= GlProgram.from({ vertex: VERT, fragment: FRAG, name: 'line-mesh' })
    this.shader = new Shader({
      glProgram: program,
      resources: {
        lineUniforms: {
          uZoom: { value: 1, type: 'f32' },
          uWidthPx: { value: style.widthPx, type: 'f32' },
          uMinW: { value: style.minW ?? 0, type: 'f32' },
          uExtraPx: { value: style.extraPx ?? 0, type: 'f32' },
          uAlpha: { value: style.alpha ?? 1, type: 'f32' },
        },
      },
    })
  }

  private uniforms() {
    return this.shader.resources.lineUniforms.uniforms as { uZoom: number; uWidthPx: number; uMinW: number; uExtraPx: number; uAlpha: number }
  }

  rezoom(zoom: number) {
    this.uniforms().uZoom = zoom
  }

  set alpha(a: number) {
    this.uniforms().uAlpha = a
  }

  /** Troca as polilinhas (lista vazia limpa). */
  set(polys: Iterable<Poly>) {
    const g = buildRibbons(polys)
    if (this.mesh) {
      this.mesh.geometry.destroy(true)
      if (g) this.mesh.geometry = g
    } else if (g) {
      this.mesh = new Mesh<Geometry, Shader>({ geometry: g, shader: this.shader })
      this.container.addChild(this.mesh)
    }
    if (this.mesh) this.mesh.visible = g != null
  }
}

/** Geometria das fitas (null se não há segmento a desenhar). */
function buildRibbons(polys: Iterable<Poly>): Geometry | null {
  const v: number[] = []
  const idx: number[] = []
  const xs: number[] = []
  const ys: number[] = []
  for (const p of polys) {
    // pontos sem repetidos consecutivos (segmento de comprimento zero não tem direção)
    xs.length = ys.length = 0
    for (let i = p.a; i < p.b; i++) {
      const x = p.pts[i * 2]
      const y = p.pts[i * 2 + 1]
      const n = xs.length
      if (n && Math.abs(x - xs[n - 1]) < 1e-4 && Math.abs(y - ys[n - 1]) < 1e-4) continue
      xs.push(x)
      ys.push(y)
    }
    const n = xs.length
    if (n < 2) continue
    const r = ((p.color >> 16) & 255) / 255
    const gr = ((p.color >> 8) & 255) / 255
    const b = (p.color & 255) / 255
    const base = v.length / STRIDE
    for (let i = 0; i < n; i++) {
      // direções do segmento anterior e do seguinte (nas pontas, só a que existe)
      let dx0 = 0, dy0 = 0, dx1 = 0, dy1 = 0
      if (i > 0) {
        const l = Math.hypot(xs[i] - xs[i - 1], ys[i] - ys[i - 1])
        dx0 = (xs[i] - xs[i - 1]) / l
        dy0 = (ys[i] - ys[i - 1]) / l
      }
      if (i < n - 1) {
        const l = Math.hypot(xs[i + 1] - xs[i], ys[i + 1] - ys[i])
        dx1 = (xs[i + 1] - xs[i]) / l
        dy1 = (ys[i + 1] - ys[i]) / l
      }
      if (i === 0) (dx0 = dx1), (dy0 = dy1)
      if (i === n - 1) (dx1 = dx0), (dy1 = dy0)
      // miter: normal média, alongada por 1/cos(meio ângulo), limitada em curvas fechadas
      let mx = -(dy0 + dy1)
      let my = dx0 + dx1
      const ml = Math.hypot(mx, my)
      if (ml < 1e-6) (mx = -dy0), (my = dx0)
      else (mx /= ml), (my /= ml)
      const cos = mx * -dy0 + my * dx0
      const k = 1 / Math.max(cos, 0.25)
      mx *= k
      my *= k
      // pontas quadradas: as extremidades avançam meia largura além do ponto
      const cap = i === 0 ? -1 : i === n - 1 ? 1 : 0
      const tx = cap * dx0
      const ty = cap * dy0
      v.push(xs[i], ys[i], mx + tx, my + ty, r, gr, b)
      v.push(xs[i], ys[i], -mx + tx, -my + ty, r, gr, b)
      if (i > 0) {
        const q = base + (i - 1) * 2
        idx.push(q, q + 1, q + 2, q + 1, q + 3, q + 2)
      }
    }
  }
  if (!idx.length) return null
  const buffer = new Buffer({ data: new Float32Array(v), usage: BufferUsage.VERTEX })
  const bytes = STRIDE * 4
  return new Geometry({
    attributes: {
      aPosition: { buffer, format: 'float32x2', stride: bytes, offset: 0 },
      aOff: { buffer, format: 'float32x2', stride: bytes, offset: 8 },
      aColor: { buffer, format: 'float32x3', stride: bytes, offset: 16 },
    },
    indexBuffer: new Uint32Array(idx),
  })
}
