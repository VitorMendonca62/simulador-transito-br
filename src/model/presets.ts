/** Tipos de via do construtor (estilo Cities: Skylines): cada um vira uma classe OSM + nº de faixas. */
export interface RoadPreset {
  id: string
  label: string
  hint: string
  /** classe OSM usada pelo núcleo (define largura da faixa e marcações) */
  hw: string
  oneway: boolean
  lf: number
  lb: number
  /** largura da faixa (m) da classe — deve espelhar `way_attrs` no núcleo */
  laneW: number
}

export const ROAD_PRESETS: RoadPreset[] = [
  { id: 'local', label: 'Rua local', hint: '1 faixa por sentido', hw: 'residential', oneway: false, lf: 1, lb: 1, laneW: 3.1 },
  { id: 'collector', label: 'Rua coletora', hint: '1+1, via de bairro', hw: 'tertiary', oneway: false, lf: 1, lb: 1, laneW: 3.1 },
  { id: 'avenue2', label: 'Avenida 2+2', hint: 'duas faixas por sentido', hw: 'secondary', oneway: false, lf: 2, lb: 2, laneW: 3.4 },
  { id: 'avenue3', label: 'Avenida 3+3', hint: 'três faixas por sentido', hw: 'primary', oneway: false, lf: 3, lb: 3, laneW: 3.4 },
  { id: 'oneway1', label: 'Mão única', hint: '1 faixa', hw: 'residential', oneway: true, lf: 1, lb: 0, laneW: 3.1 },
  { id: 'oneway2', label: 'Mão única larga', hint: '2 faixas', hw: 'secondary', oneway: true, lf: 2, lb: 0, laneW: 3.4 },
  { id: 'expressway', label: 'Via expressa', hint: '3+3, sem acesso direto', hw: 'trunk', oneway: false, lf: 3, lb: 3, laneW: 3.6 },
]

export const presetWidth = (p: RoadPreset) => (p.lf + (p.oneway ? 0 : p.lb)) * p.laneW
