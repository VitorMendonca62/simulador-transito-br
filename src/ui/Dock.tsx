import { useState, type ReactNode } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useStore, type EditMode } from '../model/store'
import { BuildPanel, ErasePanel } from './BuildPanel'
import { CttuSignalBox } from './CttuSignalBox'
import { IconClose, IconCompare, IconLayers, IconLanes, IconSignal, IconSim, IconTurns, IconWorks, IconZones } from './Icons'
import { Inspector } from './Inspector'
import { LayerPanel } from './LayerPanel'
import { ScenarioPanel } from './ScenarioPanel'
import { SegmentPanel } from './SegmentPanel'
import { SignalPanel } from './SignalPanel'
import { SimPanel } from './SimPanel'
import { DemandParamsPanel, ZonePanel } from './ZonePanel'

export type DockId = 'turns' | 'lanes' | 'works' | 'signals' | 'zones' | 'sim' | 'scenarios' | 'layers'

type Item = {
  id: DockId
  label: string
  icon: (p: { size?: number }) => ReactNode
  /** ferramenta do mapa que o painel liga (sem = mantém a atual) */
  modes?: EditMode[]
  /** como usar, no topo do painel */
  guide: ReactNode
}

/** Ordem: editar a malha (conversões, faixas, obras, semáforos), demanda e simulação, depois a vista. */
const ITEMS: Item[] = [
  {
    id: 'turns',
    label: 'Conversões',
    icon: IconTurns,
    modes: ['nodes'],
    guide: (
      <>
        Clique num <b>cruzamento</b> (nó laranja) para ver as conversões. Arraste de uma origem (ponto grande) até um destino para ligar; clique numa curva para remover.
        Nos cruzamentos sem semáforo, as placas definem a preferência.
      </>
    ),
  },
  {
    id: 'lanes',
    label: 'Faixas',
    icon: IconLanes,
    modes: ['lanes'],
    guide: <>Clique numa <b>via</b> para selecionar o trecho e ajustar nº de faixas, mão, largura e faixas de ônibus.</>,
  },
  {
    id: 'works',
    label: 'Construção e demolição',
    icon: IconWorks,
    modes: ['build', 'erase'],
    guide: (
      <>
        <b>Construir</b>: clique para começar a via e para cada ponto. <b>Demolir</b>: clique num trecho para removê-lo. <kbd>Ctrl</kbd>+<kbd>Z</kbd> desfaz.
      </>
    ),
  },
  {
    id: 'signals',
    label: 'Semáforos',
    icon: IconSignal,
    modes: ['nodes', 'signal'],
    guide: (
      <>
        Clique num semáforo da lista ou do mapa para ver <b>fases, ciclo e quem recebe verde</b>. Em <b>Colocar</b>, clique numa via para criar um semáforo ali ou num nó para
        ligar o dele.
      </>
    ),
  },
  {
    id: 'zones',
    label: 'Zonas de demanda',
    icon: IconZones,
    modes: ['zones'],
    guide: <>Desenhe zonas de geração e atração de viagens no mapa (clique nos vértices) ou importe as da Pesquisa OD 2018.</>,
  },
  {
    id: 'sim',
    label: 'Simulação',
    icon: IconSim,
    guide: <>Inicie a simulação depois de ter zonas de demanda. Em <b>Parâmetros de demanda</b>, ajuste a hora e o comportamento dos motoristas. Clique num veículo no mapa para acompanhá-lo.</>,
  },
  {
    id: 'scenarios',
    label: 'Cenários A/B',
    icon: IconCompare,
    guide: <>Salve o estado atual como cenário, altere o mapa, salve de novo e compare os dois com a mesma demanda e semente.</>,
  },
  {
    id: 'layers',
    label: 'Camadas',
    icon: IconLayers,
    guide: <>Escolha o que aparece no mapa.</>,
  },
]

function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: [T, string][]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="mode-switch wide" role="group" aria-label={label}>
      {options.map(([v, l]) => (
        <button key={v} aria-pressed={value === v} onClick={() => onChange(v)}>
          {l}
        </button>
      ))}
    </div>
  )
}

function Body({ id }: { id: DockId }) {
  const { mode, setMode } = useStore(useShallow((s) => ({ mode: s.mode, setMode: s.setMode })))
  switch (id) {
    case 'turns':
      return <Inspector />
    case 'lanes':
      return <SegmentPanel />
    case 'works':
      return (
        <>
          <Segmented label="Ferramenta" value={mode === 'erase' ? 'erase' : 'build'} options={[['build', 'Construir'], ['erase', 'Demolir']]} onChange={setMode} />
          {mode === 'erase' ? <ErasePanel /> : <BuildPanel />}
        </>
      )
    case 'signals':
      return (
        <>
          <Segmented label="Ferramenta" value={mode === 'signal' ? 'signal' : 'nodes'} options={[['nodes', 'Selecionar'], ['signal', 'Colocar semáforo']]} onChange={setMode} />
          <CttuSignalBox />
          <SignalPanel />
        </>
      )
    case 'zones':
      return <ZonePanel />
    case 'sim':
      return (
        <>
          <SimPanel />
          <DemandParamsPanel />
        </>
      )
    case 'scenarios':
      return <ScenarioPanel />
    case 'layers':
      return <LayerPanel />
  }
}

/** Trilho de ícones à direita; cada um abre o painel com as informações e as indicações da ferramenta. */
export function Dock() {
  const { mode, setMode } = useStore(useShallow((s) => ({ mode: s.mode, setMode: s.setMode })))
  const [open, setOpen] = useState<DockId | null>('turns')
  const item = ITEMS.find((i) => i.id === open)
  // ícone da ferramenta ativa no mapa: o painel aberto, se é dele; senão o primeiro que a usa
  const active = item?.modes?.includes(mode) ? item.id : ITEMS.find((i) => i.modes?.includes(mode))?.id

  const toggle = (it: Item) => {
    if (open === it.id) return setOpen(null)
    setOpen(it.id)
    // liga a ferramenta do painel; se a atual já serve (ex.: Selecionar e Conversões), mantém a seleção
    if (it.modes && !it.modes.includes(mode)) setMode(it.modes[0])
  }

  return (
    <>
      <nav className="rail" aria-label="Ferramentas">
        {ITEMS.map((it, k) => (
          <span key={it.id} className="rail-slot">
            {(k === 4 || k === 7) && <span className="rail-sep" aria-hidden="true" />}
            <button className="rail-btn" aria-pressed={open === it.id} aria-label={it.label} data-tip={it.label} onClick={() => toggle(it)}>
              <it.icon size={20} />
              {active === it.id && open !== it.id && <i className="rail-on" title="Ferramenta ativa no mapa" aria-hidden="true" />}
            </button>
          </span>
        ))}
      </nav>
      {item && (
        <aside key={item.id} className="drawer" aria-label={item.label}>
          <header className="drawer-head">
            <span className="drawer-ico">
              <item.icon size={18} />
            </span>
            <h1>{item.label}</h1>
            <button className="x-btn" onClick={() => setOpen(null)} aria-label="Fechar painel">
              <IconClose size={16} />
            </button>
          </header>
          <p className="guide">{item.guide}</p>
          <div className="drawer-body">
            <Body id={item.id} />
          </div>
        </aside>
      )}
    </>
  )
}
