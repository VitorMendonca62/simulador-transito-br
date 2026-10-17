/**
 * Placas de preferência do cruzamento selecionado (estilo TM:PE), em DOM sobre o canvas.
 * Cada ramo com faixa de entrada ganha uma placa redonda ao lado da via; clicar abre as opções
 * (automático, preferencial, dê a preferência, pare). Posições seguem a câmera via `reposition`.
 */
export interface Approach {
  /** chave estável do ramo (`way:de:para:lado`) */
  key: string
  x: number
  y: number
  /** direção de chegada ao nó (sentido da viagem) */
  dx: number
  dy: number
  name: string
  /** escolha do usuário: 0 automático, 1 preferencial, 2 dê a preferência, 3 pare */
  level: number
  /** o que o mapa implica sozinho: 1 via com preferência, 2 deve ceder */
  auto: number
}

export const PRIO_LABEL = ['Automático', 'Via preferencial', 'Dê a preferência', 'Pare']

const SIGN: Record<string, string> = {
  // 0 = automático (cor conforme `auto`): círculo tracejado com "A"
  auto: `<circle cx="20" cy="20" r="14" fill="none" stroke="currentColor" stroke-width="2" stroke-dasharray="4 3"/><text x="20" y="25.5" text-anchor="middle" font-size="16" font-weight="700" fill="currentColor">A</text>`,
  // preferencial (CTB A-... losango amarelo com borda)
  prio: `<path d="M20 4 36 20 20 36 4 20Z" fill="#f7c600" stroke="#fff" stroke-width="2.5"/><path d="M20 10 30 20 20 30 10 20Z" fill="none" stroke="#1a1a1a" stroke-width="1.8"/>`,
  // R-2 dê a preferência (triângulo invertido)
  yield: `<path d="M4 8H36L20 35Z" fill="#fff" stroke="#d62828" stroke-width="5" stroke-linejoin="round"/>`,
  // R-1 pare (octógono)
  stop: `<path d="M13 4H27L36 13V27L27 36H13L4 27V13Z" fill="#d62828" stroke="#fff" stroke-width="2"/><text x="20" y="24.5" text-anchor="middle" font-size="11" font-weight="800" fill="#fff" font-family="system-ui,sans-serif">PARE</text>`,
}
const KINDS = ['auto', 'prio', 'yield', 'stop']

function svg(level: number, auto = 0, size = 38): string {
  const k = KINDS[level]
  const color = auto === 1 ? '#facc15' : auto === 2 ? '#c6d0cc' : '#c6d0cc'
  return `<svg viewBox="0 0 40 40" width="${size}" height="${size}" style="color:${color}" aria-hidden="true">${SIGN[k]}</svg>`
}

export class SignLayer {
  readonly el: HTMLDivElement
  private items: { a: Approach; btn: HTMLButtonElement }[] = []
  private menu: HTMLDivElement | null = null
  private open: string | null = null
  onPick: (key: string, level: number) => void = () => {}

  constructor(host: HTMLElement) {
    this.el = document.createElement('div')
    this.el.className = 'sign-layer'
    host.appendChild(this.el)
  }

  destroy() {
    this.el.remove()
  }

  clear() {
    this.el.replaceChildren()
    this.items = []
    this.menu = null
    this.open = null
  }

  /** Substitui as placas (lista vazia limpa). Mantém o menu aberto se o ramo ainda existe. */
  setApproaches(list: Approach[], cam: { x: number; y: number; z: number }) {
    const keep = this.open
    this.clear()
    for (const a of list) {
      const btn = document.createElement('button')
      btn.className = `sign lv${a.level}`
      btn.type = 'button'
      btn.innerHTML = svg(a.level, a.auto)
      btn.title = `${a.name || 'Via sem nome'} — ${PRIO_LABEL[a.level]}${a.level === 0 ? (a.auto === 1 ? ' (pelo mapa: preferencial)' : ' (pelo mapa: cede)') : ''}`
      btn.addEventListener('click', (e) => {
        e.stopPropagation()
        this.toggle(a.key)
      })
      this.el.appendChild(btn)
      this.items.push({ a, btn })
    }
    if (keep && this.items.some((i) => i.a.key === keep)) this.toggle(keep)
    this.reposition(cam)
  }

  private toggle(key: string) {
    this.menu?.remove()
    this.menu = null
    if (this.open === key) {
      this.open = null
      return
    }
    const it = this.items.find((i) => i.a.key === key)
    if (!it) return
    this.open = key
    const m = document.createElement('div')
    m.className = 'sign-menu'
    KINDS.forEach((_, lv) => {
      const b = document.createElement('button')
      b.type = 'button'
      b.className = lv === it.a.level ? 'on' : ''
      b.innerHTML = `${svg(lv, it.a.auto, 30)}<span>${PRIO_LABEL[lv]}</span>`
      b.addEventListener('click', (e) => {
        e.stopPropagation()
        this.open = null
        this.onPick(key, lv)
      })
      m.appendChild(b)
    })
    this.el.appendChild(m)
    this.menu = m
    this.placeMenu()
  }

  private cam = { x: 0, y: 0, z: 1 }

  private anchor(a: Approach, z: number): [number, number] {
    // recuo ao longo da via e deslocamento para a direita de quem chega (y para baixo: direita = (-dy, dx))
    const back = 46
    const side = 30
    const sx = a.x * z + this.cam.x - a.dx * back - a.dy * side
    const sy = a.y * z + this.cam.y - a.dy * back + a.dx * side
    return [sx, sy]
  }

  private placeMenu() {
    if (!this.menu || !this.open) return
    const it = this.items.find((i) => i.a.key === this.open)
    if (!it) return
    const [sx, sy] = this.anchor(it.a, this.cam.z)
    this.menu.style.transform = `translate(${Math.round(sx + 24)}px, ${Math.round(sy - 20)}px)`
  }

  reposition(cam: { x: number; y: number; z: number }) {
    this.cam = cam
    for (const { a, btn } of this.items) {
      const [sx, sy] = this.anchor(a, cam.z)
      btn.style.transform = `translate(${Math.round(sx - 19)}px, ${Math.round(sy - 19)}px)`
    }
    this.placeMenu()
  }
}
