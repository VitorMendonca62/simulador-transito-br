import { useEffect, useRef, useState } from 'react'
import { viewRef } from '../engine/instance'
import { useStore } from '../model/store'
import { useShallow } from 'zustand/react/shallow'
import { IconChevron, IconDownload, IconFile, IconFit, IconMap, IconMinus, IconPin, IconPlus, IconUpload } from './Icons'
import { RecifeDialog } from './RecifeDialog'

/** Abre o seletor de arquivo do tipo pedido: mapa (.osm/.pbf) ou cenário (.json). */
export function pickFile(kind: 'map' | 'scenario') {
  const el = document.getElementById(kind === 'map' ? 'file-map' : 'file-scenario') as HTMLInputElement | null
  el?.click()
}

function FileInputs() {
  const open = (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    if (f) void useStore.getState().openFile(f)
    e.target.value = ''
  }
  return (
    <>
      <input id="file-map" type="file" accept=".osm,.xml,.pbf" hidden onChange={open} />
      <input id="file-scenario" type="file" accept=".json" hidden onChange={open} />
    </>
  )
}

/** Botão "Importar" com as duas opções explícitas: um mapa novo ou um cenário sobre o mapa aberto. */
function ImportMenu() {
  const { data, loading } = useStore(useShallow((s) => ({ data: s.data, loading: s.loading })))
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === 'Escape' : !box.current?.contains(e.target as Node)) setOpen(false)
    }
    window.addEventListener('pointerdown', close)
    window.addEventListener('keydown', close)
    return () => {
      window.removeEventListener('pointerdown', close)
      window.removeEventListener('keydown', close)
    }
  }, [open])
  const choose = (kind: 'map' | 'scenario') => {
    setOpen(false)
    pickFile(kind)
  }
  return (
    <div className="menu-wrap" ref={box}>
      <button className="btn primary with-ico" disabled={loading} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <IconUpload size={16} />
        Importar
        <IconChevron size={14} />
      </button>
      {open && (
        <div className="menu" role="menu">
          <button role="menuitem" onClick={() => choose('map')}>
            <IconMap size={18} />
            <span>
              <b>
                Mapa <kbd>.osm</kbd> <kbd>.pbf</kbd>
              </b>
              <i>Abre um mapa novo do OpenStreetMap e descarta as edições atuais</i>
            </span>
          </button>
          <button role="menuitem" disabled={!data} onClick={() => choose('scenario')}>
            <IconFile size={18} />
            <span>
              <b>
                Cenário <kbd>.json</kbd>
              </b>
              <i>{data ? 'Aplica vias, faixas, semáforos, conversões e demanda salvos sobre o mapa aberto' : 'Abra um mapa antes de aplicar um cenário'}</i>
            </span>
          </button>
        </div>
      )}
    </div>
  )
}

export function Toolbar() {
  const { data, loading } = useStore(useShallow((s) => ({ data: s.data, loading: s.loading })))
  const [recife, setRecife] = useState(false)
  const st = useStore.getState
  return (
    <header className="toolbar">
      <div className="brand">
        <span className="logo" />
        Simulador de Trânsito <b>BR</b>
      </div>
      <FileInputs />
      <div className="row">
        <ImportMenu />
        <button className="btn with-ico" disabled={loading} onClick={() => setRecife(true)} title="Abre o mapa do Recife que vem com o simulador">
          <IconPin size={16} />
          Recife
        </button>
        {data && (
          <button className="btn with-ico" onClick={() => st().exportScenario()} title="Baixa um .json com todas as edições (vias, faixas, semáforos, conversões e demanda)">
            <IconDownload size={16} />
            Exportar cenário
          </button>
        )}
      </div>
      {data ? (
        <div className="row zoom" role="group" aria-label="Zoom">
          <button className="btn icon" onClick={() => viewRef.current?.zoomBy(1.4)} aria-label="Aproximar" title="Aproximar">
            <IconPlus size={16} />
          </button>
          <button className="btn icon" onClick={() => viewRef.current?.zoomBy(1 / 1.4)} aria-label="Afastar" title="Afastar">
            <IconMinus size={16} />
          </button>
          <button className="btn with-ico" onClick={() => viewRef.current?.fit()} title="Enquadrar o mapa inteiro">
            <IconFit size={16} />
            Ajustar
          </button>
        </div>
      ) : (
        <span />
      )}
      {recife && <RecifeDialog onClose={() => setRecife(false)} />}
    </header>
  )
}
