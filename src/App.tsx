import { useEffect, useRef, useState } from 'react'
import { MapView } from './engine/MapView'
import { viewRef } from './engine/instance'
import { getSegRecord, useStore } from './model/store'
import { useShallow } from 'zustand/react/shallow'
import { Dock } from './ui/Dock'
import { PerfStats } from './ui/PerfStats'
import { pickFile, Toolbar } from './ui/Toolbar'
import { RecifeDialog } from './ui/RecifeDialog'
import { IconMap, IconPin } from './ui/Icons'

/** Comprimento redondo (1, 2 ou 5 × 10ⁿ m) que ocupa até ~110 px na tela. */
function niceLength(pxPerM: number): number {
  const max = 110 / pxPerM
  const p = 10 ** Math.floor(Math.log10(max))
  return [5, 2, 1].map((k) => k * p).find((v) => v <= max) ?? p
}

const fmtDist = (m: number) => (m >= 1000 ? `${(m / 1000).toLocaleString('pt-BR')} km` : `${m.toLocaleString('pt-BR')} m`)

/** Escala do mapa: barra com distância redonda e razão 1:N (tela a 96 px por polegada, 1 px = 0,2646 mm).
 * Só este componente assina o zoom: o pan (zoom igual) não re-renderiza o resto da tela. */
function ScaleBar() {
  // 3 algarismos significativos: muda pouco no zoom contínuo, mas não perde precisão longe
  const zoom = useStore((s) => (s.zoom > 0 ? Number(s.zoom.toPrecision(3)) : 0))
  if (!(zoom > 0)) return null
  const len = niceLength(zoom)
  const denom = 1 / zoom / 0.000264583
  const round = 10 ** Math.max(0, Math.floor(Math.log10(denom)) - 1)
  const ratio = Math.round(denom / round) * round
  return (
    <span className="scale" title={`${zoom.toLocaleString('pt-BR')} px por metro na tela`}>
      <i style={{ width: `${len * zoom}px` }} />
      {fmtDist(len)}
      <em>1:{ratio.toLocaleString('pt-BR')}</em>
    </span>
  )
}

export function App() {
  const host = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState(false)
  const { data, loading, error, toast, fileName, importMs } = useStore(useShallow((s) => ({ data: s.data, loading: s.loading, error: s.error, toast: s.toast, fileName: s.fileName, importMs: s.importMs })))
  const [recife, setRecife] = useState(false)

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

      {data && <Dock />}

      {!data && !loading && (
        <div className="empty">
          <h1>Simulador de Trânsito BR</h1>
          <p>
            Importe um mapa do OpenStreetMap (<code>.osm</code> ou <code>.pbf</code>) ou arraste o arquivo para cá.
          </p>
          <div className="row">
            <button className="btn primary with-ico" onClick={() => setRecife(true)}>
              <IconPin size={16} />
              Abrir o Recife
            </button>
            <button className="btn with-ico" onClick={() => pickFile('map')}>
              <IconMap size={16} />
              Importar mapa (.osm / .pbf)
            </button>
            <button className="btn" onClick={() => void useStore.getState().loadUrl('/sample.osm', 'sample.osm')}>
              Exemplo sintético
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
          <ScaleBar />
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
      {recife && <RecifeDialog onClose={() => setRecife(false)} />}
      <footer className="credits">
        Feito por{' '}
        <a href="https://github.com/VitorMendonca62" target="_blank" rel="noreferrer">
          vitormendonca62
        </a>
        <span aria-hidden="true">·</span>
        <a href="https://www.linkedin.com/in/vitormendonca62/" target="_blank" rel="noreferrer">
          LinkedIn
        </a>
        <span aria-hidden="true">·</span>
        <a href="https://github.com/VitorMendonca62/simulador-transito-br" target="_blank" rel="noreferrer" title="Código-fonte sob a licença GNU AGPL v3.0">
          Código-fonte (AGPL)
        </a>
      </footer>
      {drag && <div className="drop">Solte o arquivo .osm / .pbf para importar</div>}
    </div>
  )
}
