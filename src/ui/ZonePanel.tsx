import { useState } from 'react'
import { hexColor, PROFILES, profileById, ZONE_COLORS, type Zone } from '../model/demand'
import { useStore } from '../model/store'

const hhmm = (h: number) => `${String(Math.floor(h)).padStart(2, '0')}:${String(Math.round((h % 1) * 60)).padStart(2, '0')}`

function ProfileChart({ values, hour, color }: { values: number[]; hour: number; color: string }) {
  const max = Math.max(...values, 0.01)
  return (
    <svg className="profile" viewBox="0 0 96 28" role="img" aria-label="Perfil horário de geração">
      {values.map((v, h) => (
        <rect key={h} x={h * 4 + 0.5} y={26 - (v / max) * 24} width={3} height={(v / max) * 24} rx={1} fill={Math.floor(hour) === h ? '#fff' : color} opacity={Math.floor(hour) === h ? 1 : 0.55} />
      ))}
    </svg>
  )
}

function ZoneEditor({ z }: { z: Zone }) {
  const { updateZone, removeZone, demandParams, demand } = useStore()
  const info = demand?.zones.find((q) => q.id === z.id)
  const prof = profileById(z.profile)
  return (
    <div className="zone-editor">
      <label className="field">
        <span>Nome</span>
        <input className="txt" value={z.name} onChange={(e) => updateZone(z.id, { name: e.target.value })} aria-label="Nome da zona" />
      </label>
      <label className="field">
        <span>Geração no pico (veíc./h)</span>
        <input className="num" type="number" min={0} step={50} value={z.gen} onChange={(e) => updateZone(z.id, { gen: Math.max(0, Number(e.target.value)) })} />
      </label>
      <label className="field">
        <span>Atração (peso)</span>
        <input className="num" type="number" min={0} step={0.5} value={z.attr} onChange={(e) => updateZone(z.id, { attr: Math.max(0, Number(e.target.value)) })} />
      </label>
      <label className="field col">
        <span>Perfil horário</span>
        <select className="sel wide" value={z.profile} onChange={(e) => updateZone(z.id, { profile: e.target.value })}>
          {PROFILES.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
      </label>
      <ProfileChart values={prof.values} hour={demandParams.hour} color={hexColor(z.color)} />
      <div className="swatches" role="radiogroup" aria-label="Cor da zona">
        {ZONE_COLORS.map((c) => (
          <button key={c} role="radio" aria-checked={z.color === c} className={z.color === c ? 'on' : ''} style={{ background: hexColor(c) }} onClick={() => updateZone(z.id, { color: c })} aria-label={`Cor ${hexColor(c)}`} />
        ))}
      </div>
      {info && (
        <p className="hint">
          {info.nodes} nós · {(info.roadM / 1000).toFixed(2)} km de via · gera <b>{Math.round(info.genNow)}</b> veíc./h às {hhmm(demandParams.hour)}
        </p>
      )}
      {info?.warning && <p className="warn-line">⚠ {info.warning} — a zona não gera nem atrai viagens.</p>}
      <div className="row tight">
        <button className="btn sm danger" onClick={() => removeZone(z.id)}>
          Excluir zona
        </button>
      </div>
    </div>
  )
}

function OdMatrix() {
  const { zones, demand, odManual, setOdCell } = useStore()
  const [edit, setEdit] = useState<string | null>(null)
  if (!demand || zones.length < 2) return null
  const rowSum = demand.matrix.map((r) => r.reduce((a, b) => a + b, 0))
  const colSum = zones.map((_, j) => demand.matrix.reduce((a, r) => a + r[j], 0))
  return (
    <div className="od">
      <h3>Matriz OD (veíc./h)</h3>
      <div className="od-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">de \ para</th>
              {zones.map((z) => (
                <th key={z.id} scope="col" title={z.name}>
                  <i className="dot" style={{ background: hexColor(z.color) }} />
                  {z.id}
                </th>
              ))}
              <th scope="col">Σ</th>
            </tr>
          </thead>
          <tbody>
            {zones.map((zi, i) => (
              <tr key={zi.id}>
                <th scope="row" title={zi.name}>
                  <i className="dot" style={{ background: hexColor(zi.color) }} />
                  {zi.id}
                </th>
                {zones.map((zj, j) => {
                  const key = `${zi.id}>${zj.id}`
                  const manual = odManual[key] != null
                  return (
                    <td key={zj.id} className={manual ? 'manual' : ''}>
                      {edit === key ? (
                        <input
                          autoFocus
                          type="number"
                          min={0}
                          defaultValue={Math.round(demand.matrix[i][j])}
                          aria-label={`Viagens ${zi.name} para ${zj.name}`}
                          onBlur={(e) => {
                            setEdit(null)
                            if (e.target.value !== '') setOdCell(zi.id, zj.id, Number(e.target.value))
                          }}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                            if (e.key === 'Escape') setEdit(null)
                          }}
                        />
                      ) : (
                        <button onClick={() => setEdit(key)} title={manual ? 'Valor fixado (clique para editar)' : 'Clique para fixar um valor'}>
                          {Math.round(demand.matrix[i][j])}
                        </button>
                      )}
                    </td>
                  )
                })}
                <td className="sum">{Math.round(rowSum[i])}</td>
              </tr>
            ))}
            <tr className="sum">
              <th scope="row">Σ</th>
              {colSum.map((v, j) => (
                <td key={j}>{Math.round(v)}</td>
              ))}
              <td>{Math.round(rowSum.reduce((a, b) => a + b, 0))}</td>
            </tr>
          </tbody>
        </table>
      </div>
      {Object.keys(odManual).length > 0 && (
        <button className="link" onClick={() => Object.keys(odManual).forEach((k) => { const [a, b] = k.split('>').map(Number); setOdCell(a, b, null) })}>
          Limpar valores fixados ({Object.keys(odManual).length})
        </button>
      )}
    </div>
  )
}

export function ZonePanel() {
  const { zones, selectedZone, selectZone, demandParams, setDemandParams, demand } = useStore()
  const sel = zones.find((z) => z.id === selectedZone) ?? null
  const total = demand ? demand.zones.reduce((a, z) => a + z.genNow, 0) : 0
  return (
    <section className="panel">
      <h2>
        Zonas de demanda <small>{zones.length}</small>
      </h2>
      {zones.length === 0 && (
        <p className="hint">
          Clique no mapa para marcar os vértices; <kbd>Enter</kbd>, duplo clique ou clique no primeiro ponto fecha a zona. <kbd>Esc</kbd> cancela. Botão direito num vértice o remove.
        </p>
      )}
      <ul className="zone-list">
        {zones.map((z) => {
          const info = demand?.zones.find((q) => q.id === z.id)
          return (
            <li key={z.id}>
              <button className={z.id === selectedZone ? 'on' : ''} onClick={() => selectZone(z.id === selectedZone ? null : z.id)}>
                <i className="dot" style={{ background: hexColor(z.color) }} />
                <span className="zn">{z.name}</span>
                <span className="zg">{info ? `${Math.round(info.genNow)} /h` : ''}</span>
                {info?.warning && <span className="warn-ico" title={info.warning}>⚠</span>}
              </button>
            </li>
          )
        })}
      </ul>
      {sel && <ZoneEditor z={sel} />}

      <div className="params">
        <h3>Parâmetros de demanda</h3>
        <label className="field col">
          <span>
            Hora do dia <b>{hhmm(demandParams.hour)}</b>
            {zones.length > 0 && <em> · {Math.round(total)} veíc./h gerados</em>}
          </span>
          <input type="range" min={0} max={23.75} step={0.25} value={demandParams.hour} onChange={(e) => setDemandParams({ hour: Number(e.target.value) })} aria-label="Hora do dia" />
        </label>
        <label className="field col">
          <span>
            Sensibilidade ao tempo de viagem <b>{demandParams.beta.toFixed(2)}</b>
          </span>
          <input type="range" min={0} max={0.3} step={0.01} value={demandParams.beta} onChange={(e) => setDemandParams({ beta: Number(e.target.value) })} aria-label="Sensibilidade ao tempo de viagem" />
          <span className="hint">0 = destinos só por atração · alto = prefere zonas próximas</span>
        </label>
        <label className="check">
          <input type="checkbox" checked={demandParams.intrazone} onChange={(e) => setDemandParams({ intrazone: e.target.checked })} />
          <span>Permitir viagens dentro da mesma zona</span>
        </label>
        <label className="field">
          <span>Ônibus no fluxo (%)</span>
          <input className="num" type="number" min={0} max={50} step={1} value={demandParams.busShare} onChange={(e) => setDemandParams({ busShare: Math.min(50, Math.max(0, Number(e.target.value))) })} />
        </label>
        <label className="field">
          <span>Semente aleatória</span>
          <input className="num" type="number" min={0} step={1} value={demandParams.seed} onChange={(e) => setDemandParams({ seed: Math.max(0, Math.round(Number(e.target.value))) })} />
        </label>
      </div>
      <OdMatrix />
    </section>
  )
}
