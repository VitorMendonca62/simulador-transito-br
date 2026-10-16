import { useMemo, useState } from 'react'
import { useStore } from '../model/store'

export function SignalPanel() {
  const { data, signals, selected, playing, speed, setPhase, resetSignals, setPlaying, setSpeed, select } = useStore()
  const [open, setOpen] = useState<number | null>(null)

  const current = useMemo(() => {
    const i = signals.findIndex((s) => s.node === selected)
    return i >= 0 ? i : open
  }, [signals, selected, open])

  if (!data) return null
  return (
    <section className="panel">
      <h2>
        Semáforos <small>{signals.length}</small>
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
        <button className="btn sm" onClick={resetSignals}>
          Restaurar padrão
        </button>
      </div>
      {signals.length === 0 && <p className="hint">Nenhum <code>highway=traffic_signals</code> neste mapa.</p>}
      <ul className="sigs">
        {signals.map((s, i) => {
          const total = s.phases.reduce((a, p) => a + p.green + p.yellow + p.allRed, 0)
          const isOpen = current === i
          return (
            <li key={s.node} className={isOpen ? 'open' : ''}>
              <button
                className="sig-head"
                onClick={() => {
                  setOpen(isOpen ? null : i)
                  if (!isOpen) select(s.node, true)
                }}
              >
                <span>Nó {s.node}</span>
                <span>
                  {s.phases.length} fases · ciclo {total.toFixed(0)} s
                </span>
              </button>
              {isOpen && (
                <div className="phases">
                  <div className="ph-row ph-title">
                    <span />
                    <span>Verde</span>
                    <span>Amarelo</span>
                    <span>Vermelho geral</span>
                  </div>
                  {s.phases.map((p, j) => (
                    <div className="ph-row" key={j}>
                      <span>Fase {j + 1}</span>
                      {(['green', 'yellow', 'allRed'] as const).map((k) => (
                        <input
                          key={k}
                          type="number"
                          min={0}
                          max={240}
                          step={1}
                          value={p[k]}
                          aria-label={`${k} fase ${j + 1}`}
                          onChange={(e) => setPhase(i, j, k, Number(e.target.value))}
                        />
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
