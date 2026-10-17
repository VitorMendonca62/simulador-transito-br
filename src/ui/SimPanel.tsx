import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'
import { col, LineChart } from './Charts'
import { StepControls, StuckList, VehicleCard } from './DebugPanel'

const SPEEDS = [1, 2, 5, 10, 20, 30]
const hhmm = (sec: number) => {
  const s = Math.floor(sec) % 86400
  return `${String(Math.floor(s / 3600)).padStart(2, '0')}:${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
}

export function SimPanel() {
  const { sim, simToggle, simSpeed, simReset, zones, demandParams, setHeat, setReplay } = useStore(useShallow((s) => ({ sim: s.sim, simToggle: s.simToggle, simSpeed: s.simSpeed, simReset: s.simReset, zones: s.zones, demandParams: s.demandParams, setHeat: s.setHeat, setReplay: s.setReplay })))
  const st = sim.stats
  const running = sim.phase === 'running'
  return (
    <section className="panel">
      <h2>
        Simulação
        {sim.phase !== 'idle' && <small>{running ? 'rodando' : 'pausada'}</small>}
      </h2>
      <div className="row tight">
        <button className="btn primary sm" onClick={simToggle} disabled={sim.starting}>
          {sim.starting ? 'Preparando…' : sim.phase === 'idle' ? 'Iniciar' : running ? 'Pausar' : 'Continuar'}
        </button>
        <button className="btn sm" onClick={simReset} disabled={sim.phase === 'idle'}>
          Parar
        </button>
        <select className="sel" value={sim.speed} onChange={(e) => simSpeed(Number(e.target.value))} aria-label="Velocidade da simulação">
          {SPEEDS.map((v) => (
            <option key={v} value={v}>
              {v}×
            </option>
          ))}
        </select>
      </div>
      {sim.phase === 'idle' && (
        <p className="hint">
          {zones.length < 2 ? 'Desenhe zonas no modo Zonas para gerar viagens.' : `Inicia às ${String(Math.floor(demandParams.hour)).padStart(2, '0')}:${String(Math.round((demandParams.hour % 1) * 60)).padStart(2, '0')} com ${zones.length} zonas.`}
        </p>
      )}
      {st && (
        <dl className="stats">
          <div>
            <dt>Relógio</dt>
            <dd>{hhmm(st.time)}</dd>
          </div>
          <div>
            <dt>Na rede</dt>
            <dd>{st.alive.toLocaleString('pt-BR')}</dd>
          </div>
          <div>
            <dt>Gerados</dt>
            <dd>{st.spawned.toLocaleString('pt-BR')}</dd>
          </div>
          <div>
            <dt>Concluídos</dt>
            <dd>{st.finished.toLocaleString('pt-BR')}</dd>
          </div>
          <div>
            <dt>Na fila de entrada</dt>
            <dd>{st.queued.toLocaleString('pt-BR')}</dd>
          </div>
          <div title="Veículos que fizeram uma manobra proibida (faixa errada) para seguir a rota">
            <dt>Manobras irregulares</dt>
            <dd>{(st.illegalMoves ?? 0).toLocaleString('pt-BR')}</dd>
          </div>
          <div>
            <dt>Viagem média</dt>
            <dd>{st.meanTravel ? `${(st.meanTravel / 60).toFixed(1)} min` : '—'}</dd>
          </div>
        </dl>
      )}
      <StepControls />
      <VehicleCard />
      <StuckList />
      {sim.phase !== 'idle' && (
        <div className="seg-scope three" role="radiogroup" aria-label="Mapa de calor">
          {(
            [
              ['off', 'Sem calor'],
              ['speed', 'Velocidade'],
              ['queue', 'Filas'],
            ] as const
          ).map(([k, label]) => (
            <button key={k} role="radio" aria-checked={sim.heat === k} className={sim.heat === k ? 'on' : ''} onClick={() => setHeat(k)}>
              {label}
            </button>
          ))}
        </div>
      )}
      {sim.series && sim.series.length >= 12 && (
        <div className="charts">
          <LineChart title="Veículos na rede" lines={[{ values: col(sim.series, 1), color: '#60a5fa' }]} cursor={sim.replay} onPick={(i) => setReplay(i)} />
          <LineChart title="Velocidade média" unit=" km/h" lines={[{ values: col(sim.series, 3), color: '#34d399' }]} cursor={sim.replay} onPick={(i) => setReplay(i)} />
          <LineChart title="Concluídos por minuto" lines={[{ values: col(sim.series, 2), color: '#fbbf24' }]} cursor={sim.replay} onPick={(i) => setReplay(i)} />
          <LineChart title="Fila de entrada" lines={[{ values: col(sim.series, 4), color: '#f472b6' }]} cursor={sim.replay} onPick={(i) => setReplay(i)} />
          {sim.histLen > 1 && sim.heat !== 'off' && (
            <label className="field col">
              <span>
                Reprodução do mapa de calor{' '}
                <b>{sim.replay == null ? 'ao vivo' : hhmm(col(sim.series, 0)[sim.replay] ?? 0).slice(0, 5)}</b>
                {sim.replay != null && (
                  <button className="link" onClick={() => setReplay(null)}>
                    {' '}
                    voltar ao vivo
                  </button>
                )}
              </span>
              <input type="range" min={0} max={sim.histLen - 1} step={1} value={sim.replay ?? sim.histLen - 1} onChange={(e) => setReplay(Number(e.target.value))} aria-label="Minuto reproduzido" />
            </label>
          )}
        </div>
      )}
      {sim.phase !== 'idle' && (
        <ul className="legend" aria-label="Legenda de velocidade">
          <li><i style={{ background: '#ef4444' }} />parado</li>
          <li><i style={{ background: '#fb923c' }} />&lt; 20</li>
          <li><i style={{ background: '#facc15' }} />&lt; 40</li>
          <li><i style={{ background: '#34d399' }} />livre</li>
          <li><i style={{ background: '#60a5fa' }} />ônibus</li>
        </ul>
      )}
    </section>
  )
}
