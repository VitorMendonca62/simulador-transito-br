/**
 * Nomes de rua em DOM (sobre o canvas). O texto não passa pelo WebGL: evita texturas de texto
 * geradas sob demanda durante o zoom (que podiam sair corrompidas) e fica nítido em qualquer escala.
 *
 * Candidatos vêm do núcleo (um por trecho). `place` escolhe quais mostrar (sem colisão em tela,
 * cabendo no trecho) e `reposition` só atualiza transforms, barato o bastante para rodar a cada movimento.
 */
export class LabelLayer {
  readonly el: HTMLDivElement
  private labels: Float32Array = new Float32Array(0)
  private names: string[] = []
  private order: number[] = []
  private pool = new Map<number, HTMLSpanElement>()
  private shown = new Set<number>()
  private visible = true

  constructor(host: HTMLElement) {
    this.el = document.createElement('div')
    this.el.className = 'street-labels'
    this.el.setAttribute('aria-hidden', 'true')
    host.appendChild(this.el)
  }

  destroy() {
    this.el.remove()
  }

  setVisible(v: boolean) {
    this.visible = v
    this.el.style.display = v ? '' : 'none'
  }

  setData(labels: Float32Array, names: string[]) {
    this.clear()
    this.labels = labels
    this.names = names
    const n = labels.length / 7
    // importância: vias maiores primeiro, depois trechos mais longos
    this.order = Array.from({ length: n }, (_, i) => i).sort(
      (a, b) => labels[a * 7 + 4] - labels[b * 7 + 4] || labels[b * 7 + 3] - labels[a * 7 + 3],
    )
  }

  clear() {
    this.el.replaceChildren()
    this.pool.clear()
    this.shown.clear()
  }

  /** Tamanho da fonte em px de tela para um trecho de largura `w` (m) no zoom `z`. */
  private fontPx(w: number, z: number) {
    return Math.min(22, Math.max(11, w * z * 0.5))
  }

  /** Escolhe os rótulos visíveis para a câmera atual (x,y = deslocamento, z = px/m). */
  place(cam: { x: number; y: number; z: number }, vw: number, vh: number) {
    if (!this.visible) return
    const { x: cx, y: cy, z } = cam
    const L = this.labels
    const next = new Set<number>()
    const cell = 80
    const grid = new Map<number, [number, number, number, number][]>()
    const collides = (x0: number, y0: number, x1: number, y1: number) => {
      for (let gx = Math.floor(x0 / cell); gx <= Math.floor(x1 / cell); gx++) {
        for (let gy = Math.floor(y0 / cell); gy <= Math.floor(y1 / cell); gy++) {
          const b = grid.get(gx * 100003 + gy)
          if (b && b.some((r) => x0 < r[2] && x1 > r[0] && y0 < r[3] && y1 > r[1])) return true
        }
      }
      return false
    }
    const insert = (x0: number, y0: number, x1: number, y1: number) => {
      for (let gx = Math.floor(x0 / cell); gx <= Math.floor(x1 / cell); gx++) {
        for (let gy = Math.floor(y0 / cell); gy <= Math.floor(y1 / cell); gy++) {
          const k = gx * 100003 + gy
          const b = grid.get(k)
          if (b) b.push([x0, y0, x1, y1])
          else grid.set(k, [[x0, y0, x1, y1]])
        }
      }
    }

    for (const i of this.order) {
      const o = i * 7
      const sx = L[o] * z + cx
      const sy = L[o + 1] * z + cy
      if (sx < -100 || sy < -60 || sx > vw + 100 || sy > vh + 60) continue
      const px = this.fontPx(L[o + 6], z)
      const name = this.names[L[o + 5]]
      const textW = name.length * px * 0.62
      if (textW * 1.15 > L[o + 3] * z) continue
      const a = L[o + 2]
      const cos = Math.abs(Math.cos(a))
      const sin = Math.abs(Math.sin(a))
      const hw = (textW * cos + px * sin) / 2 + 3
      const hh = (textW * sin + px * cos) / 2 + 2
      if (collides(sx - hw, sy - hh, sx + hw, sy + hh)) continue
      insert(sx - hw, sy - hh, sx + hw, sy + hh)
      next.add(i)
    }

    for (const i of this.shown) {
      if (!next.has(i)) this.pool.get(i)!.style.display = 'none'
    }
    for (const i of next) {
      let s = this.pool.get(i)
      if (!s) {
        s = document.createElement('span')
        s.textContent = this.names[L[i * 7 + 5]]
        this.pool.set(i, s)
        this.el.appendChild(s)
      }
      s.style.display = ''
    }
    this.shown = next
    this.reposition(cam)
  }

  /** Atualiza só posição/rotação/tamanho dos rótulos já escolhidos. */
  reposition(cam: { x: number; y: number; z: number }) {
    if (!this.visible) return
    const L = this.labels
    for (const i of this.shown) {
      const o = i * 7
      const s = this.pool.get(i)!
      const px = this.fontPx(L[o + 6], cam.z)
      s.style.transform = `translate(${(L[o] * cam.z + cam.x).toFixed(1)}px,${(L[o + 1] * cam.z + cam.y).toFixed(1)}px) rotate(${L[o + 2].toFixed(4)}rad) scale(${(px / 22).toFixed(3)}) translate(-50%,-50%)`
    }
  }
}
