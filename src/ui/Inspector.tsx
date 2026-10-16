import { useMemo } from 'react'
import { describeNode, KIND_ARROW, KIND_LABEL } from '../model/inspect'
import { useStore } from '../model/store'

export function Inspector() {
  const { data, selected, select, editRev, toggleConns, resetConns, addSignal, removeSignal, rebuilding } = useStore()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const info = useMemo(() => (data && selected != null ? describeNode(data, selected) : null), [data, selected, editRev])

  if (!info) {
    return (
      <section className="panel">
        <h2>Cruzamento</h2>
        <p className="hint">Clique num nó laranja do mapa para ver as conversões possíveis de cada faixa.</p>
      </section>
    )
  }
  return (
    <section className="panel">
      <h2>
        Cruzamento <small>nó {info.id}</small>
        <button className="x" onClick={() => select(null)} aria-label="Fechar">
          ×
        </button>
      </h2>
      <div className="chips">
        <span className="chip">{info.degree} ramos</span>
        {info.signal && <span className="chip sig">semáforo</span>}
        {info.crossing && <span className="chip">travessia</span>}
        <span className="chip ok">{info.allowed} permitidas</span>
        {info.blocked > 0 && <span className="chip bad">{info.blocked} proibidas</span>}
      </div>
      <div className="row tight">
        {info.signal ? (
          <button className="btn sm danger" disabled={rebuilding} onClick={() => removeSignal(info.id)}>
            Remover semáforo
          </button>
        ) : (
          <button className="btn sm" disabled={rebuilding || info.degree < 3} onClick={() => addSignal(info.id)} title={info.degree < 3 ? 'Só em cruzamentos (3 ou mais ramos)' : ''}>
            Adicionar semáforo
          </button>
        )}
      </div>
      <p className="hint">
        OSM node {info.osmId} · clique nas setas ou nas curvas do mapa para permitir/proibir.
        {' '}
        <button className="link" onClick={() => resetConns(info.id)}>
          Restaurar nó
        </button>
      </p>
      {info.approaches.map((a) => (
        <div className="approach" key={a.end}>
          <div className="a-head">
            <b>{a.street}</b>
            <span>{a.roundabout ? 'rotatória' : a.highway}</span>
          </div>
          {a.lanes.length === 0 ? (
            <div className="hint">Somente saída (mão única)</div>
          ) : (
            a.lanes.map((l) => (
              <div className="lane" key={l.lane}>
                <span className="lane-n">F{l.index + 1}{l.bus ? ' 🚌' : ''}</span>
                <span className="arrows">
                  {l.kinds.length === 0 && <i className="none">sem saída</i>}
                  {[...new Map(l.kinds.map((k) => [k.kind, k])).values()].map((k) => (
                    <button
                      key={k.kind}
                      className={k.allowed ? 'on' : 'off'}
                      onClick={() => toggleConns(k.conns)}
                      title={`${KIND_LABEL[k.kind]}: ${k.allowed ? 'permitido (clique para proibir)' : 'proibido (clique para permitir)'}`}
                    >
                      {KIND_ARROW[k.kind]}
                    </button>
                  ))}
                </span>
              </div>
            ))
          )}
        </div>
      ))}
    </section>
  )
}
