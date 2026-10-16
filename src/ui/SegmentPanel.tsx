import { useMemo } from 'react'
import { MAX_LANES, MAX_W, MIN_W, segInfo, segsOfWay } from '../model/segments'
import { getSegRecord, useStore } from '../model/store'

function Stepper({ value, min, max, onChange, label }: { value: number; min: number; max: number; onChange: (v: number) => void; label: string }) {
  return (
    <div className="stepper" role="group" aria-label={label}>
      <button className="btn icon sm" disabled={value <= min} onClick={() => onChange(value - 1)} aria-label={`Menos ${label}`}>
        −
      </button>
      <output>{value}</output>
      <button className="btn icon sm" disabled={value >= max} onClick={() => onChange(value + 1)} aria-label={`Mais ${label}`}>
        +
      </button>
    </div>
  )
}

export function SegmentPanel() {
  const { data, selectedSeg, editRev, scope, setScope, editSegment, resetSegment, rebuilding, selectSeg } = useStore()
  const info = useMemo(
    () => (data && selectedSeg != null && selectedSeg < data.segKeys.length / 3 ? segInfo(data, selectedSeg, getSegRecord()) : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data, selectedSeg, editRev],
  )
  const wayCount = useMemo(() => (data && selectedSeg != null ? segsOfWay(data, selectedSeg).length : 0), [data, selectedSeg])

  if (!info) {
    return (
      <section className="panel">
        <h2>Faixas</h2>
        <p className="hint">Clique numa via do mapa para editar o número de faixas, o sentido, a largura e as faixas de ônibus.</p>
      </section>
    )
  }
  const e = info.edit
  const total = (e.lf + e.lb) * e.w

  return (
    <section className="panel">
      <h2>
        Faixas <small>trecho {info.index}</small>
        {rebuilding && <span className="busy">recalculando…</span>}
        <button className="x" onClick={() => selectSeg(null)} aria-label="Fechar">
          ×
        </button>
      </h2>
      <div className="seg-title">
        <b>{info.name}</b>
        <span>{info.roundabout ? 'rotatória' : info.highway}</span>
      </div>
      <p className="hint">
        {info.lengthM.toFixed(0)} m · via OSM {info.way}
        {info.maxspeed ? ` · ${info.maxspeed} km/h` : ''}
        {info.edited ? ' · editado' : ''}
      </p>

      <div className="seg-scope" role="radiogroup" aria-label="Escopo da edição">
        <button role="radio" aria-checked={scope === 'seg'} className={scope === 'seg' ? 'on' : ''} onClick={() => setScope('seg')}>
          Só este trecho
        </button>
        <button role="radio" aria-checked={scope === 'way'} className={scope === 'way' ? 'on' : ''} onClick={() => setScope('way')} disabled={wayCount < 2}>
          Toda a via ({wayCount})
        </button>
      </div>

      <label className="check">
        <input type="checkbox" checked={e.oneway} onChange={(ev) => editSegment({ kind: 'oneway', value: ev.target.checked })} />
        <span>Mão única</span>
      </label>

      <div className="field">
        <span>{e.oneway ? 'Faixas' : 'Faixas no sentido do desenho (→)'}</span>
        <Stepper value={e.lf} min={1} max={MAX_LANES} label="faixas no sentido →" onChange={(v) => editSegment({ kind: 'lanes', lf: v })} />
      </div>
      {!e.oneway && (
        <div className="field">
          <span>Faixas no sentido oposto (←)</span>
          <Stepper value={e.lb} min={1} max={MAX_LANES} label="faixas no sentido ←" onChange={(v) => editSegment({ kind: 'lanes', lb: v })} />
        </div>
      )}

      <div className="field col">
        <span>
          Largura da faixa <b>{e.w.toFixed(1)} m</b> <em>· pista {total.toFixed(1)} m</em>
        </span>
        <input type="range" min={MIN_W} max={MAX_W} step={0.1} value={e.w} onChange={(ev) => editSegment({ kind: 'width', value: Number(ev.target.value) })} aria-label="Largura da faixa" />
      </div>

      <div className="lane-list">
        <div className="lane-group">
          <span className="lane-dir">→ {e.oneway ? 'Sentido único' : 'Sentido do desenho'}</span>
          {e.busF.map((bus, i) => (
            <label className="check lane-row" key={`f${i}`}>
              <input type="checkbox" checked={bus} onChange={(ev) => editSegment({ kind: 'bus', dir: 0, idx: i, value: ev.target.checked })} />
              <span>
                Faixa {i + 1} <i className="hintx">({i === 0 ? 'esquerda' : i === e.lf - 1 ? 'direita' : 'meio'})</i>
              </span>
              <em className={bus ? 'bus on' : 'bus'}>ônibus</em>
            </label>
          ))}
        </div>
        {!e.oneway && (
          <div className="lane-group">
            <span className="lane-dir">← Sentido oposto</span>
            {e.busB.map((bus, i) => (
              <label className="check lane-row" key={`b${i}`}>
                <input type="checkbox" checked={bus} onChange={(ev) => editSegment({ kind: 'bus', dir: 1, idx: i, value: ev.target.checked })} />
                <span>
                  Faixa {i + 1} <i className="hintx">({i === 0 ? 'esquerda' : i === e.lb - 1 ? 'direita' : 'meio'})</i>
                </span>
                <em className={bus ? 'bus on' : 'bus'}>ônibus</em>
              </label>
            ))}
          </div>
        )}
      </div>

      <div className="row tight">
        <button className="btn sm" onClick={() => editSegment({ kind: 'reverse' })} title="Inverte o sentido de desenho do trecho (útil para mão única)">
          Inverter sentido
        </button>
        <button className="btn sm" onClick={resetSegment} disabled={!info.edited}>
          Restaurar {scope === 'way' ? 'via' : 'trecho'}
        </button>
      </div>
      <p className="hint">Ao mudar as faixas, as conversões dos cruzamentos vizinhos voltam ao padrão.</p>
    </section>
  )
}
