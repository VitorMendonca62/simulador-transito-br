import { useMemo, useState } from 'react'
import { PHASE_COLORS } from '../engine/SignalLayer'
import { SEG_STRIDE, type NetworkData, type SignalDef } from '../model/types'
import { useStore } from '../model/store'

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

/** Rótulo de direção: de onde o veículo vem (a = ângulo de deslocamento, Y para baixo). */
function fromWhere(a: number): string {
  const deg = ((a * 180) / Math.PI + 360) % 360 // 0 = leste, 90 = sul
  const dirs = ['oeste', 'noroeste', 'norte', 'nordeste', 'leste', 'sudeste', 'sul', 'sudoeste']
  // viajando para o leste (0°) significa vindo do oeste
  return dirs[Math.round(deg / 45) % 8]
}

function NumberField({ value, onChange, label, max = 600 }: { value: number; onChange: (v: number) => void; label: string; max?: number }) {
  return <input type="number" min={0} max={max} step={1} value={value} aria-label={label} onChange={(e) => onChange(Number(e.target.value))} />
}

function Editor({ index, def }: { index: number; def: SignalDef }) {
  const { data, setPhase, setOffset, moveApproach, addPhase, removePhase, removeSignal, resetSignal } = useStore()
  if (!data) return null
  const cycle = def.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
  const owner = new Map<number, number>()
  def.phases.forEach((p, i) => p.ends.forEach((e) => owner.set(e, i)))

  return (
    <div className="sig-editor">
      <div className="row tight">
        <span className="hint" style={{ margin: 0 }}>
          Ciclo <b>{cycle.toFixed(0)} s</b>
        </span>
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

export function SignalPanel() {
  const { data, signals, selected, playing, speed, setPlaying, setSpeed, select, setMode, mode, resetSignals, sigEdits } = useStore()
  const [open, setOpen] = useState<number | null>(null)

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
      <div className="row tight">
        <button className="btn sm" onClick={() => setPlaying(!playing)}>
          {playing ? 'Pausar' : 'Rodar'}
        </button>
        <select className="sel" value={speed} onChange={(e) => setSpeed(Number(e.target.value))} aria-label="Velocidade do relógio">
          {[1, 2, 5, 10].map((v) => (
            <option key={v} value={v}>
              {v}×
            </option>
          ))}
        </select>
        {sigEdits > 0 && (
          <button className="btn sm" onClick={resetSignals} title="Descarta todas as edições de semáforo">
            Restaurar todos
          </button>
        )}
      </div>
      {signals.length === 0 && (
        <p className="hint">
          Nenhum semáforo neste mapa. Selecione um cruzamento (modo <b>Conversões</b>) e use <b>Adicionar semáforo</b>.
        </p>
      )}
      <ul className="sigs">
        {signals.map((s, i) => {
          const total = s.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
          const isOpen = current === i
          return (
            <li key={s.node} className={isOpen ? 'open' : ''}>
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
                <span className="sig-name">{signalLabel(data, s)}</span>
                <span>
                  {s.phases.length} fases · {total.toFixed(0)} s
                </span>
              </button>
              {isOpen && <Editor index={i} def={s} />}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
