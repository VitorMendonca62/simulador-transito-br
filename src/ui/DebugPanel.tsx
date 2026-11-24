import { useEffect, useState } from 'react'
import { fetchStuck, getLastSimInit, simStepBy, type StuckItem } from '../model/importer'
import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'

/** parcelas de comportamento da moto (versão 2, fase 9) */
const MOTO_PARCEL_LABEL: Record<string, string> = {
  vermelho: 'avança vermelho',
  faixaOnibus: 'faixa de ônibus',
  contramao: 'contramão',
  corredorRapido: 'corredor rápido',
}

const f = (n: number | null | undefined, d = 1) => (n == null || !Number.isFinite(n) ? '—' : n.toFixed(d))

/** Estado completo do veículo selecionado, para depurar o comportamento. */
export function VehicleCard() {
  const { sim, zones, pickVehicle, setFollow } = useStore(useShallow((s) => ({ sim: s.sim, zones: s.zones, pickVehicle: s.pickVehicle, setFollow: s.setFollow })))
  const t = sim.track
  if (!t) return null
  const info = t.info
  const zoneName = (id: number) => zones.find((z) => z.id === id)?.name ?? `zona ${id}`
  const choice = info ? (typeof info.choice === 'string' ? info.choice : `${info.choice.kind} → faixa ${info.choice.toIdx + 1}`) : ''
  return (
    <div className="veh-card">
      <div>
        <strong>
          {t.kind === 1 ? 'Ônibus' : t.kind === 2 ? 'Moto' : 'Carro'} #{t.id}
        </strong>
        <span className="veh-actions">
          <label className="check inline">
            <input type="checkbox" checked={sim.follow} onChange={(e) => setFollow(e.target.checked)} />
            <span>seguir</span>
          </label>
          <button className="link" onClick={() => info && void navigator.clipboard?.writeText(JSON.stringify(info, null, 2))} disabled={!info}>
            copiar JSON
          </button>
          <button className="link" onClick={() => pickVehicle(null)}>
            fechar
          </button>
        </span>
      </div>
      {!t.alive && <div>Chegou ao destino (ou saiu da rede).</div>}
      {t.alive && info && (
        <dl className="veh-grid">
          {info.bus ? (
            <>
              <dt>Linha</dt>
              <dd>
                <b>{info.bus.linha}</b> · trecho {info.bus.passo + 1} de {info.bus.passos}
              </dd>
              <dt>Próxima parada</dt>
              <dd>
                {info.bus.proximaParada < info.bus.paradas ? `${info.bus.proximaParadaNome} (${info.bus.proximaParada + 1} de ${info.bus.paradas})` : 'nenhuma (fim do itinerário)'}
                {info.bus.embarcando > 0 && ` · embarcando, mais ${f(info.bus.embarcando, 0)} s`}
                {info.bus.naBaia && ' · na baia'}
              </dd>
              <dt>Atraso</dt>
              <dd>
                {f(info.delay / 60, 1)} min sobre o tempo livre, dos quais {f(info.bus.tempoEmbarcando / 60, 1)} min nas paradas
              </dd>
            </>
          ) : (
            <>
              <dt>Viagem</dt>
              <dd>
                {zoneName(info.origin ?? -1)} → {zoneName(info.dest ?? -1)} (nó {info.targetNode})
              </dd>
            </>
          )}
          <dt>Onde</dt>
          <dd>
            {info.street || `via ${info.way}`} · {info.where}
            {info.laneIdx >= 0 ? ` · faixa ${info.laneIdx + 1}` : ''} · {f(info.s, 0)}/{f(info.len, 0)} m
          </dd>
          <dt>Velocidade</dt>
          <dd>
            {f(info.v * 3.6, 0)} km/h (desejada {f(info.vDesired * 3.6, 0)}) · acel. {f(info.acc, 2)} m/s²
          </dd>
          <dt>Parado</dt>
          <dd>{info.wait > 0.3 ? `há ${f(info.wait, 0)} s` : 'não'}</dd>
          <dt>Líder</dt>
          <dd>
            {info.leader ? (
              <>
                <button className="link" onClick={() => useStore.getState().pickVehicle(info.leader!.id)}>
                  #{info.leader.id}
                </button>{' '}
                a {f(info.leader.gap, 1)} m, {f(info.leader.v * 3.6, 0)} km/h
              </>
            ) : info.front ? (
              'primeiro da faixa'
            ) : (
              '—'
            )}
          </dd>
          <dt>Próxima manobra</dt>
          <dd>
            {choice} · {f(info.rest, 0)} m até o fim da faixa
          </dd>
          <dt>Sinal</dt>
          <dd>
            {info.signal}
            {info.signal === 'vermelho' ? ` (há ${f(info.redAge, 0)} s)` : ''}
          </dd>
          <dt>Pode entrar?</dt>
          <dd className={info.enter ? (info.enter.ok && !info.enter.held ? 'ok' : 'bad') : ''}>
            {info.enter ? (
              info.enter.ok ? (
                info.enter.held ? (
                  <>
                    sim, mas retido — {info.enter.held.reason}
                    {info.enter.held.blocker >= 0 && (
                      <>
                        {' '}
                        (por{' '}
                        <button className="link" onClick={() => useStore.getState().pickVehicle(info.enter!.held!.blocker)}>
                          #{info.enter.held.blocker}
                        </button>
                        )
                      </>
                    )}
                  </>
                ) : (
                  'sim'
                )
              ) : (
                <>
                  não — {info.enter.reason}
                  {info.enter.blocker >= 0 && (
                    <>
                      {' '}
                      (por{' '}
                      <button className="link" onClick={() => useStore.getState().pickVehicle(info.enter!.blocker)}>
                        #{info.enter.blocker}
                      </button>
                      )
                    </>
                  )}
                </>
              )
            ) : (
              'ainda não está na frente da faixa'
            )}
          </dd>
          <dt>Faixas (custo até o destino)</dt>
          <dd>
            {info.lanes.map((l) => (
              <span key={l.idx} className={`lane-chip${l.here ? ' here' : ''}`} title={l.conn ? `conversão: ${l.conn}` : 'sem conversão'}>
                {l.idx + 1}: {l.cost == null ? '∞' : `${f(l.cost, 0)} s`}
              </span>
            ))}
          </dd>
          <dt>Tempo</dt>
          <dd>
            {f(info.elapsed, 0)} s de viagem · livre {f(info.freeTime, 0)} s · atraso {f(info.delay, 0)} s
          </dd>
          <dt>Outros</dt>
          <dd>
            {info.ruleBreaker ? 'infrator · ' : ''}
            {info.boxBlocker ? 'fecha cruzamento · ' : ''}
            {info.motoParcels?.length ? `${info.motoParcels.map((p) => MOTO_PARCEL_LABEL[p] ?? p).join(', ')} · ` : ''}
            {info.inContra ? 'na contramão · ' : ''}
            {info.heldByBox ? 'esperando cruzamento ocupado · ' : ''}troca de faixa em {f(info.laneChangeCooldown, 1)} s ({info.laneChangeBlock}) · via de hierarquia {info.rank}
          </dd>
        </dl>
      )}
    </div>
  )
}

/** Avanço passo a passo (com a simulação pausada). */
export function StepControls() {
  const phase = useStore((s) => s.sim.phase)
  if (phase !== 'paused') return null
  return (
    <div className="row tight step-row">
      <span className="muted">Avançar:</span>
      {[0.2, 1, 5, 30].map((s) => (
        <button key={s} className="btn sm" onClick={() => simStepBy(s)}>
          +{String(s).replace('.', ',')} s
        </button>
      ))}
    </div>
  )
}

/** Veículos parados há mais tempo na frente das faixas e o motivo: aponta os gargalos. */
export function StuckList() {
  const phase = useStore((s) => s.sim.phase)
  const time = useStore((s) => s.sim.stats?.time ?? 0)
  const [open, setOpen] = useState(false)
  const [items, setItems] = useState<StuckItem[]>([])
  useEffect(() => {
    if (!open || phase === 'idle') return
    let alive = true
    const run = () => fetchStuck(12).then((r) => alive && setItems(r)).catch(() => {})
    run()
    const id = window.setInterval(run, phase === 'running' ? 1500 : 60_000)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [open, phase, phase === 'paused' ? time : 0])
  if (phase === 'idle') return null
  return (
    <div className="stuck">
      <button className="link" onClick={() => setOpen(!open)}>
        {open ? '▾' : '▸'} Parados há mais tempo (gargalos)
      </button>
      {open && (
        <ul>
          {items.length === 0 && <li className="muted">Nenhum veículo parado há mais de 5 s na frente de faixa.</li>}
          {items.map((it) => (
            <li key={it.id}>
              <button className="link" onClick={() => useStore.getState().pickVehicle(it.id)}>
                #{it.id}
              </button>{' '}
              {Math.round(it.wait)} s · {it.street || 'via sem nome'}
              <div className="muted">
                {it.ok
                  ? it.held
                    ? `liberado, mas retido — ${it.held.reason}${it.held.blocker >= 0 ? ` (por #${it.held.blocker})` : ''}`
                    : 'pode entrar agora'
                  : `${it.reason || 'sem motivo'}${it.blocker >= 0 ? ` (por #${it.blocker})` : ''}`}
              </div>
            </li>
          ))}
        </ul>
      )}
      <button className="link" onClick={downloadSimInit} title="Zonas, matriz OD e parâmetros desta simulação, para medir o desempenho do núcleo fora do navegador (perf_sim com PERF_SIM_JSON).">
        Baixar JSON da simulação
      </button>
    </div>
  )
}

function downloadSimInit() {
  const json = getLastSimInit()
  if (!json) return
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([json], { type: 'application/json' }))
  a.download = 'simulacao.json'
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}
