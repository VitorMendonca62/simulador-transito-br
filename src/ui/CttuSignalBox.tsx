import { useShallow } from 'zustand/react/shallow'
import { useStore } from '../model/store'

/** Troca os semáforos do mapa pelos da base oficial da CTTU (Recife), como a importação das zonas OD. */
export function CttuSignalBox() {
  const { data, signals, signalSource, signalsImporting, setSignalSource } = useStore(
    useShallow((s) => ({ data: s.data, signals: s.signals, signalSource: s.signalSource, signalsImporting: s.signalsImporting, setSignalSource: s.setSignalSource })),
  )
  if (signalSource !== 'cttu') {
    return (
      <div className="od-import">
        <button className="btn sm" disabled={!data || signalsImporting} onClick={() => void setSignalSource('cttu')}>
          {signalsImporting ? 'Importando…' : 'Importar semáforos da CTTU'}
        </button>
        <p className="hint">
          Substitui os semáforos do OpenStreetMap pelos 712 da base oficial da CTTU (Portal de Dados Abertos do Recife), encaixados nos cruzamentos e
          travessias do mapa. Os tempos continuam os padrão do simulador: a CTTU não publica ciclos nem fases.
        </p>
      </div>
    )
  }
  const ctrl = new Set(signals.map((s) => s.ext).filter((x) => x != null)).size
  return (
    <div className="od-import">
      <p className="hint">
        <b>Base oficial da CTTU</b>: {ctrl} semáforos encaixados em {signals.length} nós do mapa. Os do OSM foram removidos. Tempos e fases são os padrão do
        simulador (a CTTU não publica os planos).
      </p>
      <div className="row tight">
        <button className="btn sm" disabled={signalsImporting} onClick={() => void setSignalSource('osm')} title="Descarta os semáforos da CTTU e volta às tags do OpenStreetMap">
          {signalsImporting ? 'Restaurando…' : 'Voltar aos semáforos do OSM'}
        </button>
      </div>
    </div>
  )
}
