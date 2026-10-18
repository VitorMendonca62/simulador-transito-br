import { useEffect, useRef, useState } from 'react'
import { MapView } from './engine/MapView'
import { viewRef } from './engine/instance'
import { getSegRecord, useStore } from './model/store'
import { useShallow } from 'zustand/react/shallow'
import { Inspector } from './ui/Inspector'
import { BuildPanel, ErasePanel } from './ui/BuildPanel'
import { SegmentPanel } from './ui/SegmentPanel'
import { ZonePanel } from './ui/ZonePanel'
import { LayerPanel } from './ui/LayerPanel'
import { CttuSignalBox } from './ui/CttuSignalBox'
import { SignalPanel } from './ui/SignalPanel'
import { ScenarioPanel } from './ui/ScenarioPanel'
import { SimPanel } from './ui/SimPanel'
import { PerfStats } from './ui/PerfStats'
import { Toolbar } from './ui/Toolbar'

/** Só este componente assina o zoom: o pan (zoom igual) não re-renderiza o resto da tela. */
function ZoomLabel() {
  const zoom = useStore((s) => Math.round(s.zoom * 100) / 100)
  return <span>{zoom.toFixed(2)} px/m</span>
}

export function App() {
  const host = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState(false)
  const { data, loading, error, toast, fileName, importMs, mode } = useStore(useShallow((s) => ({ data: s.data, loading: s.loading, error: s.error, toast: s.toast, fileName: s.fileName, importMs: s.importMs, mode: s.mode })))

  useEffect(() => {
    const view = new MapView(host.current!)
    view.onSelect = (n) => useStore.getState().onViewSelect(n)
    view.onPriority = (key, level) => useStore.getState().setPriority(key, level)
    view.onSegSelect = (sg) => useStore.setState({ selectedSeg: sg })
    view.onSignalPlace = (p) => useStore.getState().placeSignal(p)
    view.onBuildCommit = (pts, newChain) => useStore.getState().addRoad(pts, newChain)
    view.onErase = (seg) => useStore.getState().eraseSegment(seg)
    view.onZoneCreate = (poly) => useStore.getState().addZone(poly)
    view.onZoneSelect = (id) => useStore.getState().selectZone(id)
    view.onZoneEdit = (id, poly) => useStore.getState().updateZone(id, { poly })
    view.onZoom = (z) => useStore.getState().setZoom(z)
    view.onVehiclePick = (id) => useStore.getState().pickVehicle(id)
    view.onConnClick = (c) => useStore.getState().toggleConns([c])
    view.onConnEdit = (from, to) => useStore.getState().connectLanes(from, to)
    let alive = true
    view.init().then(() => {
      if (!alive) return
      viewRef.current = view
      if (import.meta.env.DEV) Object.assign(window, { __view: view, __store: useStore, __seg: getSegRecord })
      void useStore.getState().restore()
    })
    return () => {
      alive = false
      viewRef.current = null
      view.destroy()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !e.shiftKey) {
        const t = e.target as HTMLElement | null
        if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return
        e.preventDefault()
        useStore.getState().undo()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDrag(false)
    const f = e.dataTransfer.files[0]
    if (f) void useStore.getState().openFile(f)
  }

  return (
    <div
      className="app"
      onDragOver={(e) => {
        e.preventDefault()
        setDrag(true)
      }}
      onDragLeave={(e) => e.currentTarget === e.target && setDrag(false)}
      onDrop={onDrop}
    >
      <div className="map" ref={host} />
      <Toolbar />

      {data && (
        <aside className="side">
          <SimPanel />
          <ScenarioPanel />
          <LayerPanel />
          {mode === 'nodes' && <Inspector />}
          {mode === 'lanes' && <SegmentPanel />}
          {mode === 'build' && <BuildPanel />}
          {mode === 'erase' && <ErasePanel />}
          {mode === 'zones' && <ZonePanel />}
          {mode === 'signal' && (
            <section className="panel">
              <h2>Semáforo</h2>
              <p className="hint">Clique em qualquer ponto de uma via para colocar um semáforo (a via é dividida ali, com uma fase extra para pedestres). Clique num nó para ligar o semáforo dele. Depois ajuste as fases no modo Conversões; para remover, use <b>Remover semáforo</b>.</p>
              <CttuSignalBox />
            </section>
          )}
          <SignalPanel />
        </aside>
      )}

      {!data && !loading && (
        <div className="empty">
          <h1>Simulador de Trânsito BR</h1>
          <p>
            Importe um mapa do OpenStreetMap (<code>.osm</code> ou <code>.pbf</code>) ou arraste o arquivo para cá.
          </p>
          <div className="row">
            <button className="btn primary" onClick={() => document.getElementById('file-input')?.click()}>
              Importar mapa
            </button>
            <button className="btn" onClick={() => void useStore.getState().loadUrl('/sample.osm', 'sample.osm')}>
              Abrir exemplo
            </button>
            <button className="btn" onClick={() => void useStore.getState().loadUrl('/sample.pbf', 'sample.pbf')}>
              Exemplo .pbf
            </button>
          </div>
        </div>
      )}

      {data && (
        <div className="status">
          <span>{fileName}</span>
          <span>{data.meta.counts.nodes.toLocaleString('pt-BR')} nós</span>
          <span>{data.meta.counts.segments.toLocaleString('pt-BR')} trechos</span>
          <span>{data.meta.counts.lanes.toLocaleString('pt-BR')} faixas</span>
          <span title={data.meta.signalSource === 'ext' ? 'Semáforos da base oficial da CTTU (dados.recife.pe.gov.br)' : 'Semáforos das tags do OpenStreetMap'}>
            {data.meta.counts.signals} semáforos{data.meta.signalSource === 'ext' ? ' (CTTU)' : ''}
          </span>
          <span>importado em {importMs.toFixed(0)} ms</span>
          <ZoomLabel />
          <PerfStats />
        </div>
      )}

      {loading && <div className="loading">Processando mapa…</div>}
      {error && (
        <div className="error" role="alert" onClick={() => useStore.setState({ error: null })}>
          {error}
        </div>
      )}
      {toast && <div className="toast">{toast}</div>}
      {drag && <div className="drop">Solte o arquivo .osm / .pbf para importar</div>}
    </div>
  )
}
