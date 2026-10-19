import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { PHASE_COLORS } from '../engine/SignalLayer'
import { SEG_STRIDE, type NetworkData, type SignalDef } from '../model/types'
import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'
import { IconSearch } from './Icons'

const hex = (c: number) => `#${c.toString(16).padStart(6, '0')}`
const phaseColor = (i: number) => hex(PHASE_COLORS[i % PHASE_COLORS.length])

function streetOf(d: NetworkData, end: number): string {
  const n = d.segU32[(end >> 1) * SEG_STRIDE + 5]
  return n === 0xffffffff ? 'Sem nome' : d.meta.names[n]
}

/** Nome curto do cruzamento: até duas ruas que chegam nele. */
function signalLabel(d: NetworkData, def: SignalDef): string {
  const names = [...new Set(def.ends.map((e) => streetOf(d, e.end)))]
  return names.length === 0 ? `Nó ${def.node}` : names.slice(0, 2).join(' × ')
}

const cttu = (ext: number) => `CTTU ${String(ext).padStart(3, '0')}`

/** Posição do nó no seu controlador CTTU (os nós de mesmo código compartilham ciclo e defasagem). */
type Ctrl = { ext: number; pos: number; count: number; nodes: number[] }

/** Ordem da lista com os nós de um mesmo controlador CTTU lado a lado, e o controlador de cada índice. */
function controllers(signals: SignalDef[]): { order: number[]; ctrl: Map<number, Ctrl> } {
  const byExt = new Map<number, number[]>()
  signals.forEach((s, i) => {
    if (s.ext != null) byExt.set(s.ext, [...(byExt.get(s.ext) ?? []), i])
  })
  const order: number[] = []
  const ctrl = new Map<number, Ctrl>()
  signals.forEach((s, i) => {
    if (s.ext == null) return order.push(i)
    const ms = byExt.get(s.ext)!
    if (ms[0] !== i) return
    ms.forEach((m, pos) => {
      order.push(m)
      ctrl.set(m, { ext: s.ext!, pos, count: ms.length, nodes: ms.map((x) => signals[x].node) })
    })
  })
  return { order, ctrl }
}

/** Rótulo de direção: de onde o veículo vem (a = ângulo de deslocamento, Y para baixo). */
function fromWhere(a: number): string {
  const deg = ((a * 180) / Math.PI + 360) % 360 // 0 = leste, 90 = sul
  const dirs = ['oeste', 'noroeste', 'norte', 'nordeste', 'leste', 'sudeste', 'sul', 'sudoeste']
  // viajando para o leste (0°) significa vindo do oeste
  return dirs[Math.round(deg / 45) % 8]
}

const YELLOW = '#facc15'
const ALL_RED = '#7a3040'

/** Ciclo inteiro numa barra: verde de cada fase na cor dela, amarelo e vermelho geral entre elas. */
function CycleBar({ def }: { def: SignalDef }) {
  const cycle = def.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0) || 1
  const pct = (v: number) => `${(v / cycle) * 100}%`
  return (
    <div className="cycle">
      <div className="cycle-bar" role="img" aria-label={def.phases.map((p, j) => `fase ${j + 1}: ${p.green} s de verde`).join(', ')}>
        {def.phases.map((p, j) => (
          <span key={j} style={{ display: 'contents' }}>
            <i style={{ width: pct(p.green), background: phaseColor(j) }} title={`Fase ${j + 1}: verde ${p.green} s`}>
              {p.green / cycle > 0.12 && `F${j + 1}`}
            </i>
            {p.yellow > 0 && <i style={{ width: pct(p.yellow), background: YELLOW }} title={`Fase ${j + 1}: amarelo ${p.yellow} s`} />}
            {p.allRed > 0 && <i style={{ width: pct(p.allRed), background: ALL_RED }} title={`Fase ${j + 1}: vermelho geral ${p.allRed} s`} />}
          </span>
        ))}
      </div>
      <div className="cycle-axis">
        <span>0 s</span>
        <span>{cycle.toFixed(0)} s</span>
      </div>
    </div>
  )
}

/** Quando a aproximação tem verde (e amarelo) ao longo do ciclo; o resto é vermelho. */
function ApproachTimeline({ def, end }: { def: SignalDef; end: number }) {
  const cycle = def.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0) || 1
  const green = def.phases.reduce((a, p) => a + (p.ends.includes(end) ? p.green : 0), 0)
  return (
    <span className="appr-time" title={`${green} s de verde por ciclo (${Math.round((green / cycle) * 100)} %)`}>
      {def.phases.map((p, j) => {
        const on = p.ends.includes(end)
        return (
          <span key={j} style={{ display: 'contents' }}>
            <i style={{ width: `${(p.green / cycle) * 100}%`, background: on ? phaseColor(j) : undefined }} />
            <i style={{ width: `${(p.yellow / cycle) * 100}%`, background: on ? YELLOW : undefined }} />
            <i style={{ width: `${(p.allRed / cycle) * 100}%` }} />
          </span>
        )
      })}
    </span>
  )
}

function NumberField({ value, onChange, label, max = 600 }: { value: number; onChange: (v: number) => void; label: string; max?: number }) {
  return <input type="number" min={0} max={max} step={1} value={value} aria-label={label} onChange={(e) => onChange(Number(e.target.value))} />
}

function Editor({ index, def, ctrl }: { index: number; def: SignalDef; ctrl?: Ctrl }) {
  const { data, select, setPhase, setOffset, moveApproach, addPhase, removePhase, removeSignal, resetSignal } = useStore(useShallow((s) => ({ data: s.data, select: s.select, setPhase: s.setPhase, setOffset: s.setOffset, moveApproach: s.moveApproach, addPhase: s.addPhase, removePhase: s.removePhase, removeSignal: s.removeSignal, resetSignal: s.resetSignal })))
  if (!data) return null
  const cycle = def.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
  const owner = new Map<number, number>()
  def.phases.forEach((p, i) => p.ends.forEach((e) => owner.set(e, i)))

  return (
    <div className="sig-editor">
      {ctrl && (
        <p className="hint" style={{ margin: 0 }}>
          {ctrl.count > 1 ? (
            <>
              Controlador <b>{cttu(ctrl.ext)}</b>: {ctrl.count} nós com o mesmo ciclo e defasagem, fases alinhadas por eixo.{' '}
              {ctrl.nodes.map((n, k) =>
                k === ctrl.pos ? (
                  <b key={n}>nó {k + 1} </b>
                ) : (
                  <button key={n} className="btn sm" onClick={() => select(n, true)} title="Selecionar este nó do controlador">
                    nó {k + 1}
                  </button>
                ),
              )}
            </>
          ) : (
            <>
              Controlador <b>{cttu(ctrl.ext)}</b> (base oficial; tempos sintéticos).
            </>
          )}
        </p>
      )}
      <dl className="sig-facts">
        <div>
          <dt>Ciclo</dt>
          <dd>{cycle.toFixed(0)} s</dd>
        </div>
        <div>
          <dt>Fases</dt>
          <dd>{def.phases.length}</dd>
        </div>
        <div>
          <dt>Aproximações</dt>
          <dd>{def.ends.length}</dd>
        </div>
        <div>
          <dt>Nós</dt>
          <dd>{ctrl?.count ?? 1}</dd>
        </div>
      </dl>
      <CycleBar def={def} />
      <div className="row tight">
        <label className="inline">
          Defasagem
          <NumberField value={def.offset} onChange={(v) => setOffset(index, v)} label="Defasagem do ciclo (s)" />
          <span className="unit">s</span>
        </label>
      </div>

      <div className="ph-row ph-title">
        <span />
        <span>Verde</span>
        <span>Amarelo</span>
        <span>Verm. geral</span>
      </div>
      {def.phases.map((p, j) => (
        <div className="ph-row" key={j}>
          <span className="ph-name">
            <i className="dot" style={{ background: phaseColor(j) }} />
            Fase {j + 1}
          </span>
          {(['green', 'yellow', 'allRed'] as const).map((k) => (
            <NumberField key={k} value={p[k]} label={`${k} fase ${j + 1}`} onChange={(v) => setPhase(index, j, k, v)} />
          ))}
          {def.phases.length > 1 && (
            <button className="x-sm" onClick={() => removePhase(index, j)} aria-label={`Remover fase ${j + 1}`} title="Remover fase (as aproximações vão para a fase vizinha)">
              ×
            </button>
          )}
          {p.ends.length === 0 && <span className="warn">fase vazia</span>}
        </div>
      ))}
      <button className="btn sm" onClick={() => addPhase(index)} disabled={def.phases.length >= PHASE_COLORS.length}>
        + Fase
      </button>

      <h3>Quem recebe verde em cada fase</h3>
      <ul className="appr">
        {def.ends.map((e) => (
          <li key={e.end}>
            <span className="appr-dir" style={{ transform: `rotate(${e.a}rad)` }} aria-hidden="true">
              →
            </span>
            <span className="appr-name">
              {streetOf(data, e.end)}
              <i>vindo do {fromWhere(e.a)}</i>
              <ApproachTimeline def={def} end={e.end} />
            </span>
            <span className="appr-ph" role="radiogroup" aria-label={`Fase de ${streetOf(data, e.end)}`}>
              {def.phases.map((_, j) => (
                <button
                  key={j}
                  role="radio"
                  aria-checked={owner.get(e.end) === j}
                  className={owner.get(e.end) === j ? 'on' : ''}
                  style={owner.get(e.end) === j ? { background: phaseColor(j), borderColor: phaseColor(j) } : { borderColor: phaseColor(j), color: phaseColor(j) }}
                  onClick={() => moveApproach(index, e.end, j)}
                  title={`Fase ${j + 1}`}
                >
                  {j + 1}
                </button>
              ))}
            </span>
          </li>
        ))}
      </ul>

      <div className="row tight">
        <button className="btn sm" onClick={() => resetSignal(def.node)}>
          Restaurar padrão
        </button>
        <button className="btn sm danger" onClick={() => removeSignal(def.node)}>
          Remover semáforo
        </button>
      </div>
    </div>
  )
}

type SigRowProps = {
  index: number
  def: SignalDef
  ctrl?: Ctrl
  data: NetworkData
  isOpen: boolean
  mode: string
  setOpen: (i: number | null) => void
  setMode: (m: 'nodes') => void
  select: (n: number | null, focus?: boolean) => void
}

/** Uma linha da lista; memorizada para que a lista de ~1.000 semáforos não seja refeita a cada atualização do store. */
const SigRow = memo(function SigRow({ index: i, def: s, ctrl, data, isOpen, mode, setOpen, setMode, select }: SigRowProps) {
  const label = useMemo(() => signalLabel(data, s), [data, s])
  const total = s.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
  const li = useRef<HTMLLIElement>(null)
  // aberto pela lista ou por um clique no mapa: rola a lista até ele
  useEffect(() => {
    if (isOpen) li.current?.scrollIntoView({ block: 'start' })
  }, [isOpen])
  return (
    <li ref={li} className={[isOpen ? 'open' : '', ctrl && ctrl.pos > 0 ? 'sig-sub' : ''].join(' ').trim() || undefined}>
      <button
        className="sig-head"
        aria-expanded={isOpen}
        onClick={() => {
          setOpen(isOpen ? null : i)
          if (!isOpen) {
            if (mode !== 'nodes') setMode('nodes')
            select(s.node, true)
          }
        }}
      >
        <span className="sig-name">{label}</span>
        {ctrl && (
          <span className="sig-ctrl" title={ctrl.count > 1 ? `Controlador ${cttu(ctrl.ext)}: ${ctrl.count} nós com ciclo e defasagem comuns` : `Controlador ${cttu(ctrl.ext)}`}>
            {cttu(ctrl.ext)}
            {ctrl.count > 1 && ` · ${ctrl.pos + 1}/${ctrl.count}`}
          </span>
        )}
        <span>
          {s.phases.length} fases · {total.toFixed(0)} s
        </span>
      </button>
      {isOpen && <Editor index={i} def={s} ctrl={ctrl} />}
    </li>
  )
})

export function SignalPanel() {
  const { data, signals, selected, select, setMode, mode, resetSignals, sigEdits } = useStore(useShallow((s) => ({ data: s.data, signals: s.signals, selected: s.selected, select: s.select, setMode: s.setMode, mode: s.mode, resetSignals: s.resetSignals, sigEdits: s.sigEdits })))
  const [open, setOpen] = useState<number | null>(null)
  const [query, setQuery] = useState('')
  const { order, ctrl } = useMemo(() => controllers(signals), [signals])
  // busca por rua ou código CTTU; o controlador entra inteiro quando um nó dele casa
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '')
    if (!q || !data) return order
    const hit = new Set<number>()
    order.forEach((i) => {
      const s = signals[i]
      const text = `${signalLabel(data, s)} ${s.ext != null ? cttu(s.ext) + ' ' + s.ext : ''}`.toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '')
      if (text.includes(q)) hit.add(s.ext != null ? -1 - s.ext : i)
    })
    return order.filter((i) => hit.has(i) || (signals[i].ext != null && hit.has(-1 - signals[i].ext!)))
  }, [order, signals, query, data])

  const current = useMemo(() => {
    const i = signals.findIndex((s) => s.node === selected)
    return i >= 0 ? i : open != null && open < signals.length ? open : null
  }, [signals, selected, open])

  if (!data) return null
  return (
    <section className="panel">
      <h2>
        Semáforos <small>{signals.length}</small>
        {sigEdits > 0 && <small className="edited">{sigEdits} editado{sigEdits > 1 ? 's' : ''}</small>}
      </h2>
      {sigEdits > 0 && (
        <div className="row tight">
          <button className="btn sm" onClick={resetSignals} title="Descarta todas as edições de semáforo">
            Restaurar todos
          </button>
        </div>
      )}
      {signals.length === 0 && (
        <p className="hint">
          Nenhum semáforo neste mapa. Selecione um cruzamento (modo <b>Conversões</b>) e use <b>Adicionar semáforo</b>.
        </p>
      )}
      {signals.length > 0 && (
        <label className="search">
          <IconSearch size={15} />
          <input type="search" placeholder="Buscar rua ou código CTTU" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Buscar semáforo" />
        </label>
      )}
      {query && <p className="hint">{shown.length} de {signals.length} nós</p>}
      <ul className="sigs">
        {shown.map((i) => (
          <SigRow key={signals[i].node} index={i} def={signals[i]} ctrl={ctrl.get(i)} data={data} isOpen={current === i} mode={mode} setOpen={setOpen} setMode={setMode} select={select} />
        ))}
      </ul>
    </section>
  )
}
