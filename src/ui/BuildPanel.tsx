import { ROAD_PRESETS, type RoadPreset } from '../model/presets'
import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'

/** Corte transversal esquemático da via (faixas, eixo amarelo), como o seletor de vias do Cities: Skylines. */
function CrossSection({ p }: { p: RoadPreset }) {
  const lanes = p.lf + (p.oneway ? 0 : p.lb)
  return (
    <span className="xs" aria-hidden="true" style={{ width: Math.min(96, 14 + lanes * 13) }}>
      {Array.from({ length: lanes }, (_, i) => (
        <i key={i} className={!p.oneway && i === p.lf ? 'mid' : ''} />
      ))}
    </span>
  )
}

export function BuildPanel() {
  const { buildPreset, buildCurve, setBuildPreset, setBuildCurve, historyLen, undo, rebuilding } = useStore(useShallow((s) => ({ buildPreset: s.buildPreset, buildCurve: s.buildCurve, setBuildPreset: s.setBuildPreset, setBuildCurve: s.setBuildCurve, historyLen: s.historyLen, undo: s.undo, rebuilding: s.rebuilding })))
  return (
    <section className="panel">
      <h2>
        Construir via
        {rebuilding && <span className="busy">recalculando…</span>}
      </h2>
      <div className="seg-scope" role="radiogroup" aria-label="Forma do traçado">
        <button role="radio" aria-checked={!buildCurve} className={!buildCurve ? 'on' : ''} onClick={() => setBuildCurve(false)}>
          Reta
        </button>
        <button role="radio" aria-checked={buildCurve} className={buildCurve ? 'on' : ''} onClick={() => setBuildCurve(true)}>
          Curva
        </button>
      </div>
      <ul className="presets" role="radiogroup" aria-label="Tipo de via">
        {ROAD_PRESETS.map((p) => (
          <li key={p.id}>
            <button role="radio" aria-checked={buildPreset === p.id} className={buildPreset === p.id ? 'on' : ''} onClick={() => setBuildPreset(p.id)}>
              <CrossSection p={p} />
              <span className="pr-name">
                {p.label}
                <i>{p.hint}</i>
              </span>
            </button>
          </li>
        ))}
      </ul>
      <ul className="tips">
        <li>
          <kbd>Clique</kbd> inicia; cada novo clique continua a via.
        </li>
        {buildCurve && (
          <li>
            Na curva: 1º clique início, 2º define a curvatura, 3º fim. Depois ela continua suave.
          </li>
        )}
        <li>Encaixa em nós (anel verde) e divide vias (anel amarelo). Cruzamentos nascem sozinhos onde a via atravessa outra.</li>
        <li>
          <kbd>Esc</kbd> ou botão direito encerra o traçado. <kbd>Ctrl</kbd>+<kbd>Z</kbd> desfaz.
        </li>
      </ul>
      <div className="row tight">
        <button className="btn sm" onClick={undo} disabled={historyLen === 0}>
          Desfazer ({historyLen})
        </button>
      </div>
    </section>
  )
}

export function ErasePanel() {
  const { historyLen, undo, rebuilding } = useStore(useShallow((s) => ({ historyLen: s.historyLen, undo: s.undo, rebuilding: s.rebuilding })))
  return (
    <section className="panel">
      <h2>
        Demolir
        {rebuilding && <span className="busy">recalculando…</span>}
      </h2>
      <p className="hint">Passe o mouse sobre um trecho de via (fica vermelho) e clique para removê-lo. Vale para vias do mapa e para as que você criou.</p>
      <p className="hint">
        <kbd>Ctrl</kbd>+<kbd>Z</kbd> desfaz a última demolição ou construção.
      </p>
      <div className="row tight">
        <button className="btn sm" onClick={undo} disabled={historyLen === 0}>
          Desfazer ({historyLen})
        </button>
      </div>
    </section>
  )
}
