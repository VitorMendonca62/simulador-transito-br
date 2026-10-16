import type { LayerKey } from '../engine/MapView'
import { useStore } from '../model/store'

const ITEMS: [LayerKey, string, string][] = [
  ['roads', 'Vias', 'Asfalto e faixas de ônibus'],
  ['vehicles', 'Veículos', 'Carros e ônibus da simulação'],
  ['markings', 'Sinalização de solo', 'Faixas brancas/amarelas, zebra, retenção'],
  ['labels', 'Nomes das ruas', ''],
  ['nodes', 'Nós de cruzamento', 'Laranja = cruzamento, verde = ponta/trecho'],
  ['shapes', 'Pontos de forma', 'Vértices intermediários (zoom alto)'],
  ['signals', 'Semáforos', ''],
  ['turns', 'Conversões do nó', 'Visualização estilo TM:PE'],
  ['zones', 'Zonas de demanda', 'Áreas de geração/atração de viagens'],
  ['grid', 'Grade', ''],
]

export function LayerPanel() {
  const { layers, setLayer } = useStore()
  return (
    <section className="panel">
      <h2>Camadas</h2>
      <ul className="layers">
        {ITEMS.map(([k, label, hint]) => (
          <li key={k}>
            <label title={hint}>
              <input type="checkbox" checked={layers[k]} onChange={(e) => setLayer(k, e.target.checked)} />
              <span>{label}</span>
            </label>
          </li>
        ))}
      </ul>
    </section>
  )
}
