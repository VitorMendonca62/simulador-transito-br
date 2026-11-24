import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useStore } from '../model/store'
import './tour.css'

/** Tour guiado do primeiro acesso: destaca as partes da tela, uma por vez, depois que o primeiro mapa abre. */

const DONE_KEY = 'tour-done-v1'
const START_EVENT = 'tour:start'

type Step = {
  /** seletor do elemento destacado; sem alvo, o cartão fica no centro */
  target?: string
  title: string
  body: ReactNode
}

const STEPS: Step[] = [
  {
    title: 'Bem-vindo ao Simulador de Trânsito BR',
    body: (
      <>
        Aqui você edita a malha viária de uma cidade e simula o trânsito nela. Em poucos passos mostramos onde fica cada coisa. Use <kbd>←</kbd> <kbd>→</kbd> para navegar
        e <kbd>Esc</kbd> para sair.
      </>
    ),
  },
  {
    target: '.toolbar .menu-wrap',
    title: 'Importar mapa ou cenário',
    body: (
      <>
        Abra um mapa do OpenStreetMap (<kbd>.osm</kbd> / <kbd>.pbf</kbd>) ou aplique um cenário <kbd>.json</kbd> salvo antes. Também dá para arrastar o arquivo para a tela.
      </>
    ),
  },
  {
    target: '.toolbar button[title^="Abre o mapa do Recife"]',
    title: 'Mapa do Recife',
    body: <>O Recife já vem pronto, com as zonas da Pesquisa OD 2018, os semáforos da CTTU e as linhas de ônibus.</>,
  },
  {
    target: '.rail',
    title: 'Ferramentas de edição',
    body: (
      <>
        Cada ícone abre um painel: <b>conversões</b>, <b>faixas</b>, <b>construção e demolição</b>, <b>semáforos</b>, <b>zonas de demanda</b>, <b>linhas de ônibus</b> e{' '}
        <b>camadas</b>. O topo de cada painel explica como usar a ferramenta.
      </>
    ),
  },
  {
    target: '.rail-btn[aria-label="Zonas de demanda"]',
    title: 'Zonas de demanda',
    body: <>A simulação precisa de viagens. Desenhe zonas de origem e destino no mapa ou importe as da Pesquisa OD 2018.</>,
  },
  {
    target: '.sim-dock',
    title: 'Simulação',
    body: (
      <>
        Inicie, pause e acelere a simulação por aqui e acompanhe os indicadores. Em <b>Parâmetros de demanda</b> você ajusta a hora do dia e o comportamento dos motoristas.
        Clique num veículo no mapa para segui-lo.
      </>
    ),
  },
  {
    target: '.toolbar button[title^="Baixa um .json"]',
    title: 'Salvar o seu cenário',
    body: <>As edições ficam guardadas neste navegador. Para levar para outro lugar ou compartilhar, exporte o cenário em <kbd>.json</kbd>.</>,
  },
  {
    target: '.status',
    title: 'Barra de status',
    body: <>Tamanho do mapa, escala e desempenho da simulação.</>,
  },
  {
    title: 'Pronto para começar',
    body: (
      <>
        Arraste o mapa para mover e use a roda do mouse (ou os botões de zoom) para aproximar. <kbd>Ctrl</kbd>+<kbd>Z</kbd> desfaz a última edição. Para rever este tour,
        clique no <b>?</b> na barra de cima.
      </>
    ),
  },
]

/** Abre o tour de novo (botão de ajuda). */
export function startTour() {
  window.dispatchEvent(new Event(START_EVENT))
}

function isDone() {
  try {
    return localStorage.getItem(DONE_KEY) === '1'
  } catch {
    return true // sem armazenamento, não insiste a cada visita
  }
}

function markDone() {
  try {
    localStorage.setItem(DONE_KEY, '1')
  } catch {
    /* armazenamento indisponível: só não lembra */
  }
}

/** Primeiro passo a partir de `from`, na direção `dir`, cujo alvo existe e aparece na tela. */
function findStep(from: number, dir: 1 | -1): number {
  for (let k = from; k >= 0 && k < STEPS.length; k += dir) {
    const t = STEPS[k].target
    if (!t) return k
    const el = document.querySelector(t)
    if (el && el.getClientRects().length > 0) return k
  }
  return -1
}

type Box = { x: number; y: number; w: number; h: number }
const PAD = 6
const GAP = 12
const MARGIN = 12

/** Lugar do cartão: abaixo, acima, à esquerda ou à direita do alvo, no primeiro em que cabe; sempre dentro da tela. */
function placeCard(t: Box | null, cw: number, ch: number): { left: number; top: number } {
  const W = window.innerWidth
  const H = window.innerHeight
  const clampX = (x: number) => Math.min(Math.max(x, MARGIN), W - cw - MARGIN)
  const clampY = (y: number) => Math.min(Math.max(y, MARGIN), H - ch - MARGIN)
  if (!t) return { left: clampX((W - cw) / 2), top: clampY((H - ch) / 2) }
  const cx = t.x + t.w / 2 - cw / 2
  const cy = t.y + t.h / 2 - ch / 2
  if (t.y + t.h + GAP + ch <= H - MARGIN) return { left: clampX(cx), top: t.y + t.h + GAP }
  if (t.y - GAP - ch >= MARGIN) return { left: clampX(cx), top: t.y - GAP - ch }
  if (t.x - GAP - cw >= MARGIN) return { left: t.x - GAP - cw, top: clampY(cy) }
  if (t.x + t.w + GAP + cw <= W - MARGIN) return { left: t.x + t.w + GAP, top: clampY(cy) }
  return { left: clampX(cx), top: clampY(H - ch - MARGIN) }
}

export function Tour() {
  const { hasMap, loading } = useStore(useShallow((s) => ({ hasMap: s.data != null, loading: s.loading })))
  const [step, setStep] = useState(-1)
  const [box, setBox] = useState<Box | null>(null)
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null)
  const card = useRef<HTMLDivElement>(null)
  const nextBtn = useRef<HTMLButtonElement>(null)
  const open = step >= 0

  // primeiro mapa aberto neste navegador: começa sozinho, depois que a tela assenta
  useEffect(() => {
    if (!hasMap || loading || open || isDone()) return
    const id = window.setTimeout(() => setStep(findStep(0, 1)), 700)
    return () => window.clearTimeout(id)
  }, [hasMap, loading, open])

  useEffect(() => {
    const start = () => setStep(findStep(0, 1))
    window.addEventListener(START_EVENT, start)
    return () => window.removeEventListener(START_EVENT, start)
  }, [])

  const close = () => {
    markDone()
    setStep(-1)
  }
  const go = (dir: 1 | -1) => {
    const k = findStep(step + dir, dir)
    if (k < 0) {
      if (dir === 1) close()
      return
    }
    setStep(k)
  }

  // área do alvo: medida ao trocar de passo, ao redimensionar e quando o próprio alvo muda de tamanho
  useLayoutEffect(() => {
    if (!open) return
    const sel = STEPS[step].target
    const el = sel ? document.querySelector(sel) : null
    const measure = () => {
      if (!el) return setBox(null)
      const r = el.getBoundingClientRect()
      setBox({ x: r.left - PAD, y: r.top - PAD, w: r.width + PAD * 2, h: r.height + PAD * 2 })
    }
    measure()
    window.addEventListener('resize', measure)
    const ro = el ? new ResizeObserver(measure) : null
    if (el) ro!.observe(el)
    return () => {
      window.removeEventListener('resize', measure)
      ro?.disconnect()
    }
  }, [step, open])

  // cartão posicionado depois de medido (a altura depende do texto)
  useLayoutEffect(() => {
    if (!open || !card.current) return
    const r = card.current.getBoundingClientRect()
    setPos(placeCard(box, r.width, r.height))
  }, [box, step, open])

  useEffect(() => {
    if (open) nextBtn.current?.focus()
  }, [step, open])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
      else if (e.key === 'ArrowRight') go(1)
      else if (e.key === 'ArrowLeft') go(-1)
      else return
      e.preventDefault()
      e.stopPropagation()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  })

  if (!open) return null
  const s = STEPS[step]
  const first = findStep(step - 1, -1) < 0
  const last = findStep(step + 1, 1) < 0
  // posição entre os passos visíveis nesta tela (os de alvo ausente na tela não contam)
  const visible = STEPS.map((_, k) => k).filter((k) => findStep(k, 1) === k)
  const n = visible.indexOf(step) + 1

  return (
    <div className="tour" role="dialog" aria-modal="true" aria-labelledby="tour-title" aria-describedby="tour-body">
      <div className="tour-shield" onClick={close} />
      {box ? <div className="tour-spot" style={{ left: box.x, top: box.y, width: box.w, height: box.h }} /> : <div className="tour-dim" />}
      <div ref={card} className="tour-card" style={pos ? { left: pos.left, top: pos.top } : { visibility: 'hidden' }}>
        <span className="tour-count">
          {n} de {visible.length}
        </span>
        <h2 id="tour-title">{s.title}</h2>
        <p id="tour-body">{s.body}</p>
        <div className="tour-dots" aria-hidden="true">
          {visible.map((k) => (
            <i key={k} className={k === step ? 'on' : undefined} />
          ))}
        </div>
        <div className="row end">
          {!last && (
            <button className="btn link-btn" onClick={close}>
              Pular tour
            </button>
          )}
          {!first && (
            <button className="btn" onClick={() => go(-1)}>
              Voltar
            </button>
          )}
          <button ref={nextBtn} className="btn primary" onClick={() => go(1)}>
            {last ? 'Começar' : 'Próximo'}
          </button>
        </div>
      </div>
    </div>
  )
}
