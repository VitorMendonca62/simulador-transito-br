import { useEffect, useRef, useState } from 'react'
import { useStore } from '../model/store'
import { IconPin, IconSignal, IconZones } from './Icons'

/** Pergunta, antes de abrir o mapa do Recife, se já entram as zonas da Pesquisa OD 2018 e os semáforos da CTTU. */
export function RecifeDialog({ onClose }: { onClose: () => void }) {
  const [zones, setZones] = useState(true)
  const [signals, setSignals] = useState(true)
  const dlg = useRef<HTMLDialogElement>(null)
  const hasMap = useStore((s) => s.data != null)
  useEffect(() => {
    dlg.current?.showModal()
  }, [])
  const go = () => {
    onClose()
    void useStore.getState().loadRecife({ zones, signals })
  }
  return (
    <dialog ref={dlg} className="dialog" onClose={onClose} onClick={(e) => e.target === dlg.current && dlg.current?.close()}>
      <form method="dialog" onSubmit={go}>
        <h2>
          <IconPin size={18} /> Importar o mapa do Recife
        </h2>
        <p className="hint">
          Recorte do OpenStreetMap do Recife (outubro de 2026, ~25 mil nós).{hasMap && ' O mapa aberto e as edições dele serão substituídos.'} Junto com o mapa, deseja importar:
        </p>
        <label className="opt">
          <input type="checkbox" checked={zones} onChange={(e) => setZones(e.target.checked)} />
          <IconZones size={18} />
          <span>
            <b>Zonas de demanda</b>
            <i>Zonas e matriz de viagens de carro da Pesquisa OD 2018, com entradas e saídas na divisa</i>
          </span>
        </label>
        <label className="opt">
          <input type="checkbox" checked={signals} onChange={(e) => setSignals(e.target.checked)} />
          <IconSignal size={18} />
          <span>
            <b>Semáforos da CTTU</b>
            <i>Base oficial da prefeitura no lugar dos semáforos do OSM, com os cruzamentos conferidos à mão</i>
          </span>
        </label>
        <div className="row end">
          <button type="button" className="btn" onClick={() => dlg.current?.close()}>
            Cancelar
          </button>
          <button type="submit" className="btn primary" autoFocus>
            {zones || signals ? 'Importar mapa e selecionados' : 'Importar só o mapa'}
          </button>
        </div>
      </form>
    </dialog>
  )
}
