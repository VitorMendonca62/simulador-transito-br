import type { Renderer } from 'pixi.js'

/** Contadores acumulados desde o início; o `PerfStats` lê e converte em taxas a cada segundo. */
export const perf = {
  /** quadros realmente desenhados pelo mapa (o mapa só redesenha quando algo muda) */
  renders: 0,
  /** tempo (ms) da thread principal no laço de desenho e no tratamento de quadros da simulação */
  uiMs: 0,
  /** tempo (ms) de GPU nos desenhos do mapa; só se o navegador oferecer o timer de GPU */
  gpuMs: 0,
  gpuSupported: false,
  /** tempo (ms) que o worker passou calculando a simulação */
  simBusyMs: 0,
  /** memória linear do WASM no worker (bytes) */
  wasmBytes: 0,
}

/** Mede o tempo de GPU dos desenhos com EXT_disjoint_timer_query_webgl2 (ausente em alguns navegadores). */
export class GpuTimer {
  private gl: WebGL2RenderingContext | null = null
  private ext: { TIME_ELAPSED_EXT: number; GPU_DISJOINT_EXT: number } | null = null
  private active: WebGLQuery | null = null
  private pending: WebGLQuery[] = []

  constructor(renderer: Renderer) {
    const gl = (renderer as unknown as { gl?: WebGL2RenderingContext }).gl
    if (!gl || typeof gl.createQuery !== 'function') return
    this.ext = gl.getExtension('EXT_disjoint_timer_query_webgl2')
    if (this.ext) {
      this.gl = gl
      perf.gpuSupported = true
    }
  }

  begin() {
    const { gl, ext } = this
    if (!gl || !ext || this.active || this.pending.length > 8) return
    const q = gl.createQuery()
    gl.beginQuery(ext.TIME_ELAPSED_EXT, q)
    this.active = q
  }

  end() {
    const { gl, ext } = this
    if (!gl || !ext || !this.active) return
    gl.endQuery(ext.TIME_ELAPSED_EXT)
    this.pending.push(this.active)
    this.active = null
  }

  /** Recolhe as medições já prontas (o resultado da GPU chega alguns quadros depois). */
  poll() {
    const { gl, ext } = this
    if (!gl || !ext) return
    while (this.pending.length) {
      const q = this.pending[0]
      if (!gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE)) break
      if (!gl.getParameter(ext.GPU_DISJOINT_EXT)) perf.gpuMs += (gl.getQueryParameter(q, gl.QUERY_RESULT) as number) / 1e6
      gl.deleteQuery(q)
      this.pending.shift()
    }
  }
}
