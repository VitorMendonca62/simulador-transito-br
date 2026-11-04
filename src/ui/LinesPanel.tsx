import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { hexColor } from '../model/demand'
import { useStore } from '../model/store'
import { fold, LINE_COLORS, lineColor, lineOn, type BusLineReport, type LineEditRec, type TransitLine } from '../model/transit'
import { IconSearch } from './Icons'

const mmss = (s: number) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, '0')}`

/** Parada selecionada no mapa: tipo, linhas que passam e, com a simulação, os próximos ônibus e a espera média. */
function StopCard() {
  const { transit, transitEdits, selectedStop, stopInfo, selectStop, selectLine, setStopBaia, removeStop, restoreStop, simPhase } = useStore(
    useShallow((s) => ({
      transit: s.transit,
      transitEdits: s.transitEdits,
      selectedStop: s.selectedStop,
      stopInfo: s.stopInfo,
      selectStop: s.selectStop,
      selectLine: s.selectLine,
      setStopBaia: s.setStopBaia,
      removeStop: s.removeStop,
      restoreStop: s.restoreStop,
      simPhase: s.sim.phase,
    })),
  )
  const st = transit?.stops.find((x) => x.id === selectedStop)
  if (!st) return null
  const gtfsEdited = transitEdits.stops[st.id] != null && !st.nova
  return (
    <section className="panel stop-card">
      <h2>
        {st.nome} <small>{st.nova ? 'parada nova' : st.id}</small>
        <button className="x-btn x-sm" onClick={() => selectStop(null)} aria-label="Fechar parada">
          ×
        </button>
      </h2>
      <div className="seg-scope" role="group" aria-label="Tipo da parada">
        <button className={!st.baia ? 'on' : ''} aria-pressed={!st.baia} onClick={() => setStopBaia(st.id, false)} title="O ônibus para na faixa; quem vem atrás ultrapassa ou espera">
          Na faixa
        </button>
        <button className={st.baia ? 'on' : ''} aria-pressed={st.baia} onClick={() => setStopBaia(st.id, true)} title="O ônibus sai da faixa para embarcar e volta com brecha">
          Baia
        </button>
      </div>
      {st.linhas.length > 0 ? (
        <p className="chips">
          {st.linhas.map((n) => {
            const l = transit?.lines.find((x) => x.numero === n)
            return (
              <button key={n} className="chip line-chip" style={{ ['--lc' as string]: hexColor(lineColor(n, l ? transitEdits.lines[l.id] : undefined)) }} onClick={() => l && selectLine(l.id)}>
                {n}
              </button>
            )
          })}
        </p>
      ) : (
        <p className="warn-line">Nenhuma linha encaixada passa a até 15 m desta parada.</p>
      )}
      {simPhase !== 'idle' && stopInfo && (
        <>
          <h3>Próximos ônibus</h3>
          {stopInfo.proximos.length === 0 ? (
            <p className="hint">Nenhum ônibus a caminho agora.</p>
          ) : (
            <ul className="next-buses">
              {stopInfo.proximos.map((b) => (
                <li key={b.veh}>
                  <b>{b.linha}</b>
                  <span>{b.embarcando ? 'embarcando' : `${mmss(b.eta)} · ${(b.dist / 1000).toFixed(1)} km`}</span>
                </li>
              ))}
            </ul>
          )}
          {stopInfo.espera.some((w) => w.esperaMedia != null) && (
            <>
              <h3>Espera média (intervalo medido)</h3>
              <ul className="next-buses">
                {stopInfo.espera
                  .filter((w) => w.esperaMedia != null)
                  .map((w) => (
                    <li key={w.linha}>
                      <b>{w.linha}</b>
                      <span>
                        {mmss(w.esperaMedia!)} · intervalo {mmss(w.intervaloMedio ?? 0)}
                      </span>
                    </li>
                  ))}
              </ul>
            </>
          )}
          {stopInfo.naBaia > 0 && <p className="hint">{stopInfo.naBaia} ônibus na baia agora.</p>}
        </>
      )}
      <div className="row tight">
        {gtfsEdited && (
          <button className="btn sm" onClick={() => restoreStop(st.id)} title="Volta à posição e ao tipo do GTFS">
            Restaurar
          </button>
        )}
        <button className="btn sm danger" onClick={() => removeStop(st.id)}>
          Remover parada
        </button>
      </div>
      <p className="hint">Arraste a parada no modo Editar paradas para movê-la ao longo da via. Ctrl+Z desfaz.</p>
    </section>
  )
}

/** Fator de frequência por hora: 24 campos em grade e um para todas as horas. */
function FactorGrid({ id, rec }: { id: string; rec?: LineEditRec }) {
  const setLineFactor = useStore((s) => s.setLineFactor)
  const hour = useStore((s) => Math.floor(s.demandParams.hour))
  const f = rec?.factor ?? Array.from({ length: 24 }, () => 1)
  return (
    <>
      <div className="factor-all">
        <span>Frequência (× GTFS)</span>
        <label>
          todas as horas
          <input className="num" type="number" min={0} max={10} step={0.1} placeholder="1" onChange={(e) => e.target.value !== '' && setLineFactor(id, null, Number(e.target.value))} />
        </label>
      </div>
      <div className="factor-grid" role="group" aria-label="Fator de frequência por hora">
        {f.map((v, h) => (
          <label key={h} className={[h === hour ? 'now' : '', v !== 1 ? 'edited' : ''].join(' ').trim() || undefined} title={`${h}h: ${v}× as partidas do GTFS`}>
            <i>{h}h</i>
            <input type="number" min={0} max={10} step={0.1} value={v} onChange={(e) => setLineFactor(id, h, Number(e.target.value))} />
          </label>
        ))}
      </div>
    </>
  )
}

interface RowProps {
  line: TransitLine
  rec?: LineEditRec
  open: boolean
  rep?: BusLineReport
  onOpen: (id: string | null) => void
}

const LineRow = memo(function LineRow({ line: l, rec, open, rep, onOpen }: RowProps) {
  const { setLineOn, setLineColor, resetLine } = useStore(useShallow((s) => ({ setLineOn: s.setLineOn, setLineColor: s.setLineColor, resetLine: s.resetLine })))
  const li = useRef<HTMLLIElement>(null)
  useEffect(() => {
    if (open) li.current?.scrollIntoView({ block: 'nearest' })
  }, [open])
  const on = !rec?.off
  const color = lineColor(l.numero, rec)
  const tr = l.padroes.flatMap((p) => p.trechos)
  const ok = tr.filter((t) => t.ok).length
  const edited = rec != null && (rec.off || rec.color != null || rec.factor?.some((f) => f !== 1))
  return (
    <li ref={li} className={open ? 'open' : undefined}>
      <div className="line-head">
        <input type="checkbox" checked={on} onChange={(e) => setLineOn(l.id, e.target.checked)} aria-label={`Linha ${l.numero} ligada`} />
        <button className="line-name" aria-expanded={open} onClick={() => onOpen(open ? null : l.id)}>
          <i className="dot" style={{ background: hexColor(color) }} />
          <b>{l.numero}</b>
          <span>{l.nome}</span>
        </button>
        <small className={ok < tr.length ? 'warn' : undefined} title={`${ok} de ${tr.length} trechos encaixados na rede`}>
          {rep ? `${rep.velocidadeKmh.toFixed(0)} km/h` : `${ok}/${tr.length}`}
        </small>
      </div>
      {open && (
        <div className="line-body">
          <div className="swatches" role="radiogroup" aria-label="Cor da linha">
            {LINE_COLORS.map((c) => (
              <button key={c} role="radio" aria-checked={color === c} className={color === c ? 'on' : ''} style={{ background: hexColor(c) }} onClick={() => setLineColor(l.id, c)} aria-label={`Cor ${hexColor(c)}`} />
            ))}
          </div>
          <FactorGrid id={l.id} rec={rec} />
          {rep && (
            <dl className="stats">
              <dt>Viagens concluídas</dt>
              <dd>{rep.viagens}</dd>
              <dt>Velocidade comercial</dt>
              <dd>{rep.velocidadeKmh.toFixed(1)} km/h</dd>
              <dt>Tempo de viagem</dt>
              <dd>{mmss(rep.tempoMedio)}</dd>
              <dt>Parado nas paradas / fora</dt>
              <dd>
                {mmss(rep.paradoNasParadas)} / {mmss(rep.paradoForaDasParadas)}
              </dd>
              <dt>Intervalo nas paradas</dt>
              <dd>{rep.intervaloMedio > 0 ? `${mmss(rep.intervaloMedio)} (CV ${rep.intervaloCV.toFixed(2)})` : '—'}</dd>
              <dt>Comboios / paradas perdidas</dt>
              <dd>
                {rep.comboios} / {rep.paradasPerdidas}
              </dd>
            </dl>
          )}
          <ul className="patterns">
            {l.padroes.map((p) => (
              <li key={p.id}>
                <b>{p.destino || p.id}</b>
                {p.trechos.map((t, k) => (
                  <span key={k} className={t.ok ? 'ok' : 'bad'} title={t.aviso ?? `${t.paradas} paradas`}>
                    {t.ok ? `trecho ${k + 1}: ${t.paradas} paradas` : `trecho ${k + 1} desligado: ${t.aviso}`}
                  </span>
                ))}
              </li>
            ))}
          </ul>
          {edited && (
            <button className="btn sm" onClick={() => resetLine(l.id)}>
              Restaurar linha
            </button>
          )}
        </div>
      )}
    </li>
  )
})

export function LinesPanel() {
  const { data, transit, transitEdits, transitSource, transitImporting, setTransitSource, selectedLine, selectLine, mode, setMode, setAllLines, busReport } = useStore(
    useShallow((s) => ({
      data: s.data,
      transit: s.transit,
      transitEdits: s.transitEdits,
      transitSource: s.transitSource,
      transitImporting: s.transitImporting,
      setTransitSource: s.setTransitSource,
      selectedLine: s.selectedLine,
      selectLine: s.selectLine,
      mode: s.mode,
      setMode: s.setMode,
      setAllLines: s.setAllLines,
      busReport: s.busReport,
    })),
  )
  const [query, setQuery] = useState('')
  const shown = useMemo(() => {
    const q = fold(query.trim())
    const all = transit?.lines ?? []
    return q ? all.filter((l) => fold(`${l.numero} ${l.nome}`).includes(q)) : all
  }, [transit, query])
  const reports = useMemo(() => new Map((busReport?.linhas ?? []).map((r) => [r.linha, r])), [busReport])

  if (!data) return null
  if (transitSource === 'none') {
    return (
      <section className="panel">
        <h2>Linhas de ônibus</h2>
        <p className="hint">Sem linhas: na simulação, os ônibus são uma parcela do fluxo entre zonas. As linhas do GTFS do Grande Recife só cobrem o Recife.</p>
        <button className="btn primary" disabled={transitImporting} onClick={() => void setTransitSource('recife')}>
          {transitImporting ? 'Carregando…' : 'Carregar linhas do Recife'}
        </button>
      </section>
    )
  }
  const sum = transit?.summary
  const onCount = (transit?.lines ?? []).filter((l) => lineOn(transitEdits, l.id)).length
  return (
    <>
      <section className="panel">
        <h2>
          Linhas de ônibus <small>{transit ? `${onCount} de ${transit.lines.length} ligadas` : '…'}</small>
        </h2>
        {sum && (
          <p className="hint">
            {sum.ok} de {sum.trechos} trechos encaixados na rede · {sum.paradas.toLocaleString('pt-BR')} paradas nos itinerários
            {sum.paradas_fora > 0 && ` (${sum.paradas_fora} fora do caminho)`}
          </p>
        )}
        {sum && sum.falhas.length > 0 && (
          <details className="fails">
            <summary>{sum.trechos - sum.ok} trechos desligados</summary>
            <ul>
              {sum.falhas.slice(0, 40).map((f, k) => (
                <li key={k}>
                  <b>{f.linha}</b> {f.padrao}: {f.aviso}
                </li>
              ))}
            </ul>
          </details>
        )}
        {busReport && (
          <p className="hint">
            Simulação: {busReport.ativos} ônibus na rede · {busReport.concluidas} viagens concluídas · {busReport.velocidadeKmh.toFixed(1)} km/h de velocidade comercial ·{' '}
            {busReport.paradasPerdidas} paradas perdidas
          </p>
        )}
        <div className="mode-switch wide" role="group" aria-label="Ferramenta">
          <button aria-pressed={mode !== 'stops'} onClick={() => setMode('nodes')}>
            Selecionar
          </button>
          <button aria-pressed={mode === 'stops'} onClick={() => setMode('stops')}>
            Editar paradas
          </button>
        </div>
        <div className="row tight">
          <button className="btn sm" onClick={() => setAllLines(true)}>
            Ligar todas
          </button>
          <button className="btn sm" onClick={() => setAllLines(false)}>
            Desligar todas
          </button>
          <button className="btn sm" disabled={transitImporting} onClick={() => void setTransitSource('none')} title="Sem linhas, os ônibus voltam a ser uma parcela do fluxo">
            Remover linhas
          </button>
        </div>
      </section>
      <StopCard />
      <section className="panel">
        <label className="search">
          <IconSearch size={15} />
          <input type="search" placeholder="Buscar número ou nome da linha" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Buscar linha" />
        </label>
        {query && (
          <p className="hint">
            {shown.length} de {transit?.lines.length ?? 0} linhas
          </p>
        )}
        <ul className="lines">
          {shown.map((l) => (
            <LineRow key={l.id} line={l} rec={transitEdits.lines[l.id]} open={selectedLine === l.id} rep={reports.get(l.numero)} onOpen={selectLine} />
          ))}
        </ul>
      </section>
    </>
  )
}
