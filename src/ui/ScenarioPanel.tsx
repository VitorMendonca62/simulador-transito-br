import { useMemo, useState } from 'react'
import { SEG_STRIDE } from '../model/types'
import type { BenchResult, SimReport } from '../model/types'
import { useStore } from '../model/store'
import { col, LineChart } from './Charts'

type Metric = { label: string; get: (r: SimReport) => number; fmt: (v: number) => string; better: 'up' | 'down' | 'none' }
const n0 = (v: number) => v.toLocaleString('pt-BR', { maximumFractionDigits: 0 })
const n1 = (v: number) => v.toLocaleString('pt-BR', { maximumFractionDigits: 1 })
const n2 = (v: number) => v.toLocaleString('pt-BR', { maximumFractionDigits: 2 })

const METRICS: Metric[] = [
  { label: 'Viagens concluídas', get: (r) => r.trips, fmt: n0, better: 'up' },
  { label: 'Vazão (veíc./h)', get: (r) => r.throughputPerH, fmt: n0, better: 'up' },
  { label: 'Velocidade média (km/h)', get: (r) => r.meanSpeedKmh, fmt: n1, better: 'up' },
  { label: 'Atraso total, incl. retidos (veíc·h)', get: (r) => r.totalDelayAllVehH, fmt: n2, better: 'down' },
  { label: 'Atraso médio por veículo (s)', get: (r) => r.meanDelayAll, fmt: n0, better: 'down' },
  { label: 'Tempo médio das concluídas (s)', get: (r) => r.meanTravel, fmt: n0, better: 'none' },
  { label: 'Fila máxima num cruzamento (veíc.)', get: (r) => r.maxQueue, fmt: n0, better: 'down' },
  { label: 'Fila de entrada ao fim (veíc.)', get: (r) => r.queued, fmt: n0, better: 'down' },
  { label: 'Veículos na rede ao fim', get: (r) => r.alive, fmt: n0, better: 'none' },
]

function Delta({ a, b, better, fmt }: { a: number; b: number; better: Metric['better']; fmt: (v: number) => string }) {
  const d = b - a
  if (Math.abs(d) < 1e-9) return <td className="delta">—</td>
  const pct = a !== 0 ? ` (${d > 0 ? '+' : ''}${n1((d / Math.abs(a)) * 100)}%)` : ''
  const good = better === 'none' ? null : (better === 'up') === d > 0
  return (
    <td className={`delta ${good == null ? '' : good ? 'good' : 'bad'}`}>
      {d > 0 ? '+' : ''}
      {fmt(d)}
      {pct}
    </td>
  )
}

function Compare({ A, B, nameA, nameB }: { A: BenchResult; B: BenchResult; nameA: string; nameB: string }) {
  const { data, compare, setCompare } = useStore()
  const nodeLabel = useMemo(() => {
    const byOsm = new Map<number, number>()
    if (data) for (let n = 0; n < data.nodeOsm.length; n++) byOsm.set(data.nodeOsm[n], n)
    return (osm: number) => {
      const n = byOsm.get(osm)
      if (n == null || !data) return `nó OSM ${osm}`
      const names = new Set<string>()
      for (let i = data.nodeEndOff[n]; i < data.nodeEndOff[n + 1]; i++) {
        const nm = data.segU32[(data.nodeEnds[i] >> 1) * SEG_STRIDE + 5]
        if (nm !== 0xffffffff) names.add(data.meta.names[nm])
      }
      return [...names].slice(0, 2).join(' × ') || `nó OSM ${osm}`
    }
  }, [data])

  const jmap = (r: SimReport) => new Map(r.junctions.map((j) => [j.osm, j]))
  const ja = jmap(A.report)
  const jb = jmap(B.report)
  const osms = [...new Set([...ja.keys(), ...jb.keys()])]
  const rows = osms
    .map((o) => ({ osm: o, a: ja.get(o)?.delayVehH ?? 0, b: jb.get(o)?.delayVehH ?? 0 }))
    .sort((x, y) => Math.abs(y.b - y.a) - Math.abs(x.b - x.a))
    .slice(0, 8)

  return (
    <div className="compare">
      <table>
        <thead>
          <tr>
            <th scope="col">Métrica</th>
            <th scope="col">A · {nameA}</th>
            <th scope="col">B · {nameB}</th>
            <th scope="col">B − A</th>
          </tr>
        </thead>
        <tbody>
          {METRICS.map((m) => (
            <tr key={m.label}>
              <th scope="row">{m.label}</th>
              <td>{m.fmt(m.get(A.report))}</td>
              <td>{m.fmt(m.get(B.report))}</td>
              <Delta a={m.get(A.report)} b={m.get(B.report)} better={m.better} fmt={m.fmt} />
            </tr>
          ))}
        </tbody>
      </table>
      <p className="hint">Mesma semente e mesma demanda. "Atraso total" soma também quem ficou preso na rede e na fila de entrada; o tempo médio só das concluídas esconde gargalos.</p>

      <LineChart title="Velocidade média (A azul · B laranja)" unit=" km/h" lines={[{ values: col(A.series as Float32Array, 3), color: '#60a5fa', label: 'A' }, { values: col(B.series as Float32Array, 3), color: '#fb923c', label: 'B' }]} />
      <LineChart title="Veículos na rede" lines={[{ values: col(A.series as Float32Array, 1), color: '#60a5fa', label: 'A' }, { values: col(B.series as Float32Array, 1), color: '#fb923c', label: 'B' }]} />

      {rows.length > 0 && (
        <>
          <h3>Cruzamentos com maior variação de atraso (veíc·h)</h3>
          <table>
            <thead>
              <tr>
                <th scope="col">Cruzamento</th>
                <th scope="col">A</th>
                <th scope="col">B</th>
                <th scope="col">B − A</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.osm}>
                  <th scope="row">{nodeLabel(r.osm)}</th>
                  <td>{n2(r.a)}</td>
                  <td>{n2(r.b)}</td>
                  <Delta a={r.a} b={r.b} better="down" fmt={n2} />
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}

      <label className="check">
        <input type="checkbox" checked={compare.diff} onChange={(e) => setCompare({ diff: e.target.checked })} />
        <span>Mapa de diferença (vermelho = B pior, azul = B melhor)</span>
      </label>
    </div>
  )
}

export function ScenarioPanel() {
  const { scenarios, bench, compare, saveScenario, deleteScenario, runScenario, cancelBench, setCompare, zones } = useStore()
  const [name, setName] = useState('')
  const [minutes, setMinutes] = useState(30)
  const a = scenarios.find((s) => s.id === compare.a)
  const b = scenarios.find((s) => s.id === compare.b)
  const running = bench.id != null
  return (
    <section className="panel">
      <h2>
        Cenários (A/B) <small>{scenarios.length}</small>
      </h2>
      <p className="hint">Salve o estado atual (vias, semáforos, conversões e demanda) como cenário, altere o mapa, salve de novo e compare. Cada cenário roda sem desenho, com a mesma semente.</p>
      <div className="row tight">
        <input className="txt" style={{ width: '46%' }} placeholder="Nome (ex.: base)" value={name} onChange={(e) => setName(e.target.value)} aria-label="Nome do cenário" />
        <label className="inline">
          <input className="num" type="number" min={5} max={240} step={5} value={minutes} onChange={(e) => setMinutes(Math.max(5, Math.min(240, Number(e.target.value))))} aria-label="Duração em minutos" />
          <span className="unit">min</span>
        </label>
        <button
          className="btn sm"
          disabled={zones.length < 2}
          onClick={() => {
            saveScenario(name.trim(), minutes)
            setName('')
          }}
          title={zones.length < 2 ? 'Desenhe ao menos 2 zonas' : ''}
        >
          Salvar atual
        </button>
      </div>
      {running && (
        <div className="bench" role="status">
          <div className="bar">
            <i style={{ width: `${Math.round(bench.progress * 100)}%` }} />
          </div>
          <span>Executando… {Math.round(bench.progress * 100)}%</span>
          <button className="link" onClick={cancelBench}>
            cancelar
          </button>
        </div>
      )}
      {bench.error && <p className="warn-line">⚠ {bench.error}</p>}
      <ul className="scen">
        {scenarios.map((s) => (
          <li key={s.id}>
            <span className="sn">{s.name}</span>
            <span className="sm">{s.minutes} min {s.result ? '· resultado salvo' : '· sem resultado'}</span>
            <span className="ab" role="group" aria-label="Escolher como A ou B">
              {(['a', 'b'] as const).map((k) => (
                <button key={k} className={compare[k] === s.id ? 'on' : ''} aria-pressed={compare[k] === s.id} disabled={!s.result} onClick={() => setCompare({ [k]: compare[k] === s.id ? null : s.id })} title={s.result ? '' : 'Rode o cenário primeiro'}>
                  {k.toUpperCase()}
                </button>
              ))}
            </span>
            <button className="btn sm" disabled={running} onClick={() => void runScenario(s.id)}>
              {s.result ? 'Rodar de novo' : 'Rodar'}
            </button>
            <button className="x-sm" onClick={() => deleteScenario(s.id)} aria-label={`Excluir ${s.name}`}>
              ×
            </button>
          </li>
        ))}
      </ul>
      {a?.result && b?.result && <Compare A={a.result} B={b.result} nameA={a.name} nameB={b.name} />}
    </section>
  )
}
