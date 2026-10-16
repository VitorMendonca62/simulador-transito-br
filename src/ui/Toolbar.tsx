import { useRef } from 'react'
import { viewRef } from '../engine/instance'
import { useStore } from '../model/store'

export function Toolbar() {
  const input = useRef<HTMLInputElement>(null)
  const { data, loading, mode, setMode } = useStore()
  const st = useStore.getState
  return (
    <header className="toolbar">
      <div className="brand">
        <span className="logo" />
        Simulador de Trânsito <b>BR</b>
      </div>
      <input
        id="file-input"
        ref={input}
        type="file"
        accept=".osm,.xml,.pbf,.json"
        hidden
        onChange={(e) => {
          const f = e.target.files?.[0]
          if (f) void st().openFile(f)
          e.target.value = ''
        }}
      />
      <div className="row">
        <button className="btn primary" disabled={loading} onClick={() => input.current?.click()} title="Importar .osm, .pbf (mapa) ou .json (cenário)">
          Importar mapa
        </button>
        <button className="btn" disabled={loading} onClick={() => void st().loadUrl('/sample.osm', 'sample.osm')}>
          Exemplo
        </button>
        {data && (
          <>
            <button className="btn" onClick={() => st().exportScenario()} title="Exporta os tempos de semáforo editados">
              Exportar cenário
            </button>
            <span className="sep" />
            <div className="mode-switch" role="group" aria-label="Ferramenta">
              {(
                [
                  ['nodes', 'Conversões', 'Selecionar cruzamentos, editar conversões e semáforos'],
                  ['lanes', 'Faixas', 'Selecionar vias e editar faixas'],
                  ['build', 'Construir', 'Desenhar vias novas'],
                  ['erase', 'Demolir', 'Remover trechos de via'],
                  ['signal', 'Semáforo', 'Colocar semáforo em qualquer ponto de uma via'],
                  ['zones', 'Zonas', 'Desenhar zonas de geração/atração de viagens'],
                ] as const
              ).map(([m, label, tip]) => (
                <button key={m} aria-pressed={mode === m} onClick={() => setMode(m)} title={tip}>
                  {label}
                </button>
              ))}
            </div>
            <span className="sep" />
            <button className="btn icon" onClick={() => viewRef.current?.zoomBy(1.4)} aria-label="Aproximar">
              +
            </button>
            <button className="btn icon" onClick={() => viewRef.current?.zoomBy(1 / 1.4)} aria-label="Afastar">
              −
            </button>
            <button className="btn" onClick={() => viewRef.current?.fit()}>
              Ajustar
            </button>
          </>
        )}
      </div>
    </header>
  )
}
