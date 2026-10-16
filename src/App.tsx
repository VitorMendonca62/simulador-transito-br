import { useEffect, useRef, useState } from 'react'
import { MapView } from './engine/MapView'
import { viewRef } from './engine/instance'
import { useStore } from './model/store'
import { Inspector } from './ui/Inspector'
import { LayerPanel } from './ui/LayerPanel'
import { SignalPanel } from './ui/SignalPanel'
import { Toolbar } from './ui/Toolbar'

export function App() {
  const host = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState(false)
  const { data, loading, error, toast, fileName, importMs, zoom } = useStore()

  useEffect(() => {
    const view = new MapView(host.current!)
    view.onSelect = (n) => useStore.setState({ selected: n })
    view.onZoom = (z) => useStore.getState().setZoom(z)
    view.onConnClick = (c) => useStore.getState().toggleConns([c])
    let alive = true
    view.init().then(() => {
      if (!alive) return
      viewRef.current = view
      if (import.meta.env.DEV) Object.assign(window, { __view: view, __store: useStore })
      void useStore.getState().restore()
    })
    return () => {
      alive = false
      viewRef.current = null
      view.destroy()
    }
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
          <LayerPanel />
          <Inspector />
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
          <span>{data.meta.counts.connections.toLocaleString('pt-BR')} conexões</span>
          <span>{data.meta.counts.signals} semáforos</span>
          <span>importado em {importMs.toFixed(0)} ms</span>
          <span>{zoom.toFixed(2)} px/m</span>
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
