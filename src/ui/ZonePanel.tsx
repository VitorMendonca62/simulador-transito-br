import { useState } from 'react'
import { hexColor, PROFILES, profileById, ZONE_COLORS, type Zone } from '../model/demand'
import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'

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
  const { updateZone, removeZone, demandParams, demand } = useStore(useShallow((s) => ({ updateZone: s.updateZone, removeZone: s.removeZone, demandParams: s.demandParams, demand: s.demand })))
  const info = demand?.zones.find((q) => q.id === z.id)
  const prof = z.profileValues ? { values: z.profileValues } : profileById(z.profile)
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
        <select className="sel wide" value={z.profileValues ? 'od2018' : z.profile} onChange={(e) => updateZone(z.id, { profile: e.target.value, profileValues: undefined })}>
          {z.profileValues && <option value="od2018">Pesquisa OD 2018 (desta zona)</option>}
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

const MAX_MATRIX = 20

/** Importação das zonas e da matriz de carro da Pesquisa OD 2018 (Recife). */
function OdImportBox() {
  const { odImport, odImporting, importOd2018, setOdScale, clearOdImport, zones, data } = useStore(
    useShallow((s) => ({ odImport: s.odImport, odImporting: s.odImporting, importOd2018: s.importOd2018, setOdScale: s.setOdScale, clearOdImport: s.clearOdImport, zones: s.zones, data: s.data })),
  )
  const [gateways, setGateways] = useState(true)
  const start = () => {
    if (zones.length > 0 && !odImport && !window.confirm(`Substituir as ${zones.length} zonas atuais pelas da Pesquisa OD 2018?`)) return
    void importOd2018({ gateways })
  }
  const gatewayCheck = (
    <label className="check">
      <input type="checkbox" checked={gateways} onChange={(e) => setGateways(e.target.checked)} />
      <span>Entradas e saídas para as cidades vizinhas nas vias principais da divisa (BR-101, BR-232, Agamenon…)</span>
    </label>
  )
  if (!odImport) {
    return (
      <div className="od-import">
        <button className="btn sm" disabled={!data || odImporting} onClick={start}>
          {odImporting ? 'Importando…' : 'Importar zonas da Pesquisa OD 2018'}
        </button>
        {gatewayCheck}
        <p className="hint">Zonas de tráfego do Recife/RMR com a matriz de carro dirigindo (trabalho + educação) por hora.</p>
      </div>
    )
  }
  return (
    <div className="od-import">
      <p className="hint">
        <b>{odImport.name}</b>:{' '}
        {odImport.gateways > 0 ? (
          <>
            {zones.length - odImport.gateways} zonas no mapa; as {odImport.merged} zonas de fora entram e saem por {odImport.gateways} portais na divisa.
            {odImport.dropped > 0 && <> {Math.round(odImport.dropped).toLocaleString('pt-BR')} viagens/dia entre cidades vizinhas não passam pelo mapa e ficam de fora.</>}
          </>
        ) : (
          <>
            {zones.length} zonas no mapa, {odImport.merged} de fora entram pela borda mais próxima.
          </>
        )}{' '}
        A matriz da hora escolhida fica fixada e acompanha o relógio da simulação.
      </p>
      <label className="field col">
        <span>
          Escala da demanda <b>{Math.round(odImport.scale * 100)}%</b>
        </span>
        <input type="range" min={0} max={1.5} step={0.05} value={odImport.scale} onChange={(e) => setOdScale(Number(e.target.value))} aria-label="Escala da demanda importada" />
      </label>
      {odImport.gateways > 0 && <GatewayTable />}
      {gatewayCheck}
      <div className="row tight">
        <button className="btn sm" disabled={odImporting} onClick={start}>
          Reimportar
        </button>
        <button className="btn sm danger" onClick={clearOdImport} title="Mantém as zonas e volta ao modelo gravitacional">
          Desligar matriz
        </button>
      </div>
    </div>
  )
}

/** Veículos/h que entram e saem por cada portal da divisa na hora atual (células fixadas da matriz). */
function GatewayTable() {
  const { zones, odManual, demandParams, selectedZone, selectZone } = useStore(
    useShallow((s) => ({ zones: s.zones, odManual: s.odManual, demandParams: s.demandParams, selectedZone: s.selectedZone, selectZone: s.selectZone })),
  )
  const flow = new Map<number, [number, number]>()
  for (const [k, v] of Object.entries(odManual)) {
    const [o, d] = k.split('>').map(Number)
    const fo = flow.get(o) ?? [0, 0]
    fo[0] += v
    flow.set(o, fo)
    const fd = flow.get(d) ?? [0, 0]
    fd[1] += v
    flow.set(d, fd)
  }
  const gates = zones.filter((z) => z.gateway).map((z) => ({ z, f: flow.get(z.id) ?? [0, 0] })).sort((a, b) => b.f[0] + b.f[1] - (a.f[0] + a.f[1]))
  const tot = gates.reduce((s, g) => [s[0] + g.f[0], s[1] + g.f[1]], [0, 0])
  return (
    <div className="od gw">
      <h3>
        Divisa às {hhmm(demandParams.hour)} <small>entram {Math.round(tot[0])} · saem {Math.round(tot[1])} veíc./h</small>
      </h3>
      <div className="od-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">portal</th>
              <th scope="col">entram</th>
              <th scope="col">saem</th>
            </tr>
          </thead>
          <tbody>
            {gates.map(({ z, f }) => (
              <tr key={z.id} className={z.id === selectedZone ? 'on' : ''} onClick={() => selectZone(z.id === selectedZone ? null : z.id)} style={{ cursor: 'pointer' }}>
                <th scope="row" title={z.name}>
                  {z.name.replace(/^Divisa · /, '')}
                </th>
                <td>{Math.round(f[0])}</td>
                <td>{Math.round(f[1])}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function OdMatrix() {
  const { zones, demand, odManual, setOdCell } = useStore(useShallow((s) => ({ zones: s.zones, demand: s.demand, odManual: s.odManual, setOdCell: s.setOdCell })))
  const [edit, setEdit] = useState<string | null>(null)
  if (!demand || zones.length < 2) return null
  if (zones.length > MAX_MATRIX) {
    const n = Object.keys(odManual).length
    return (
      <div className="od">
        <h3>Matriz OD (veíc./h)</h3>
        <p className="hint">
          {zones.length} zonas: a tabela só aparece até {MAX_MATRIX}. {n > 0 && <>{n} pares fixados nesta hora.</>}
        </p>
      </div>
    )
  }
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
  const { zones, selectedZone, selectZone, demandParams, setDemandParams, demand } = useStore(useShallow((s) => ({ zones: s.zones, selectedZone: s.selectedZone, selectZone: s.selectZone, demandParams: s.demandParams, setDemandParams: s.setDemandParams, demand: s.demand })))
  const sel = zones.find((z) => z.id === selectedZone) ?? null
  const total = demand ? demand.zones.reduce((a, z) => a + z.genNow, 0) : 0
  return (
    <section className="panel">
      <h2>
        Zonas de demanda <small>{zones.length}</small>
      </h2>
      <OdImportBox />
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
        <label className="check" title="Durante a simulação, as rotas são recalculadas com a velocidade medida em cada trecho; quem usa app desvia de congestionamentos no caminho.">
          <input type="checkbox" checked={demandParams.reroute ?? true} onChange={(e) => setDemandParams({ reroute: e.target.checked })} />
          <span>Rotas em tempo real (estilo Waze)</span>
        </label>
        <label className="field" title="Motoristas que seguem o tempo real. Os demais escolhem a rota pelo tempo de fluxo livre.">
          <span>Usam app de navegação (%)</span>
          <input className="num" type="number" min={0} max={100} step={5} disabled={!(demandParams.reroute ?? true)} value={demandParams.appPct ?? 60} onChange={(e) => setDemandParams({ appPct: Math.min(100, Math.max(0, Number(e.target.value))) })} />
        </label>
        <label className="field" title="Quanto cada motorista erra ao avaliar a próxima via. 0 = todos escolhem igual (tudo ou nada); 0,3 = espalha entre caminhos com poucos segundos de diferença.">
          <span>Erro de percepção</span>
          <input className="num" type="number" min={0} max={1} step={0.05} value={demandParams.perception ?? 0.3} onChange={(e) => setDemandParams({ perception: Math.min(1, Math.max(0, Number(e.target.value))) })} />
        </label>
        <label className="field" title="Motoristas na faixa errada que, em vez de dar a volta, usam a conversão proibida de outra faixa. 0 = ninguém.">
          <span>Ignoram a sinalização (%)</span>
          <input className="num" type="number" min={0} max={100} step={5} value={demandParams.illegalPct ?? 0} onChange={(e) => setDemandParams({ illegalPct: Math.min(100, Math.max(0, Number(e.target.value))) })} />
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
