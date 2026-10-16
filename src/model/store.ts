import { create } from 'zustand'
import type { LayerKey, LayerState } from '../engine/MapView'
import { viewRef } from '../engine/instance'
import { importMap } from './importer'
import {
  applyConnEdits,
  applyEdits,
  clearAll,
  collectConnEdits,
  collectEdits,
  loadConnEdits,
  saveConnEdits,
  exportScenario,
  loadEdits,
  loadMap,
  saveEdits,
  saveMap,
  type ScenarioFile,
} from './persistence'
import type { NetworkData, SignalDef } from './types'

interface State {
  data: NetworkData | null
  fileName: string
  loading: boolean
  error: string | null
  importMs: number
  layers: LayerState
  selected: number | null
  signals: SignalDef[]
  playing: boolean
  speed: number
  zoom: number
  toast: string | null
  /** Incrementa a cada edição de conversão (força o painel a reler os dados). */
  editRev: number
  connEdits: number

  setLayer: (k: LayerKey, on: boolean) => void
  select: (n: number | null, focus?: boolean) => void
  setZoom: (z: number) => void
  openFile: (file: File) => Promise<void>
  loadUrl: (url: string, name: string) => Promise<void>
  restore: () => Promise<void>
  toggleConns: (conns: number[]) => void
  resetConns: (node?: number) => void
  setPhase: (signalIdx: number, phase: number, key: 'green' | 'yellow' | 'allRed', value: number) => void
  resetSignals: () => void
  setPlaying: (p: boolean) => void
  setSpeed: (s: number) => void
  exportScenario: () => void
  notify: (msg: string) => void
}

let saveTimer = 0
let connSaveTimer = 0
/** Permissões originais (do núcleo) para calcular o que foi editado. */
let connOrig = new Uint8Array(0)

const snapshotConns = (d: NetworkData) => {
  connOrig = new Uint8Array(d.connU32.length / 5)
  for (let c = 0; c < connOrig.length; c++) connOrig[c] = d.connU32[c * 5 + 4]
}

export const useStore = create<State>((set, get) => {
  const run = async (buffer: ArrayBuffer, name: string, persist: boolean) => {
    set({ loading: true, error: null })
    try {
      const { data, ms } = await importMap(buffer, name)
      const edits = persist ? {} : await loadEdits()
      const signals = applyEdits(data, edits)
      snapshotConns(data)
      if (!persist) applyConnEdits(data, await loadConnEdits())
      if (persist) {
        await clearAll()
        await saveMap(name, buffer)
      }
      viewRef.current?.setNetwork(data)
      viewRef.current?.setSignals(signals)
      set({ data, signals, fileName: name, importMs: ms, loading: false, selected: null, editRev: 0, connEdits: Object.keys(collectConnEdits(data, connOrig)).length })
    } catch (e) {
      set({ loading: false, error: e instanceof Error ? e.message : String(e) })
    }
  }

  return {
    data: null,
    fileName: '',
    loading: false,
    error: null,
    importMs: 0,
    layers: { roads: true, markings: true, labels: true, nodes: true, shapes: false, signals: true, turns: true, grid: true },
    selected: null,
    signals: [],
    playing: true,
    speed: 1,
    zoom: 1,
    toast: null,
    editRev: 0,
    connEdits: 0,

    setLayer: (k, on) => {
      viewRef.current?.setLayer(k, on)
      set((s) => ({ layers: { ...s.layers, [k]: on } }))
    },
    select: (n, focus = false) => {
      viewRef.current?.select(n, focus)
      set({ selected: n })
    },
    setZoom: (z) => set({ zoom: z }),

    openFile: async (file) => {
      if (/\.json$/i.test(file.name)) {
        const { data } = get()
        if (!data) return get().notify('Importe um mapa antes de aplicar um cenário.')
        try {
          const sc = JSON.parse(await file.text()) as ScenarioFile
          if (sc.app !== 'simulador-transito') throw new Error('arquivo não é um cenário do simulador')
          const signals = applyEdits(data, sc.signals ?? {})
          viewRef.current?.setSignals(signals)
          for (let c = 0; c < connOrig.length; c++) data.connU32[c * 5 + 4] = connOrig[c]
          applyConnEdits(data, sc.connections ?? {})
          const ce = collectConnEdits(data, connOrig)
          set({ signals, editRev: get().editRev + 1, connEdits: Object.keys(ce).length })
          viewRef.current?.refreshOverlay()
          saveEdits(collectEdits(data, signals))
          saveConnEdits(ce)
          get().notify('Cenário aplicado.')
        } catch (e) {
          set({ error: `Cenário inválido: ${e instanceof Error ? e.message : e}` })
        }
        return
      }
      await run(await file.arrayBuffer(), file.name, true)
    },

    loadUrl: async (url, name) => {
      set({ loading: true, error: null })
      try {
        const res = await fetch(url)
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        await run(await res.arrayBuffer(), name, true)
      } catch (e) {
        set({ loading: false, error: `Não foi possível carregar ${name}: ${e instanceof Error ? e.message : e}` })
      }
    },

    restore: async () => {
      const saved = await loadMap()
      if (saved) await run(saved.buffer, saved.name, false)
    },

    toggleConns: (conns) => {
      const { data } = get()
      if (!data || conns.length === 0) return
      // alguma permitida -> proíbe todas; senão libera todas
      const any = conns.some((c) => data.connU32[c * 5 + 4] === 1)
      for (const c of conns) data.connU32[c * 5 + 4] = any ? 0 : 1
      const edits = collectConnEdits(data, connOrig)
      set((s) => ({ editRev: s.editRev + 1, connEdits: Object.keys(edits).length }))
      viewRef.current?.refreshOverlay()
      clearTimeout(connSaveTimer)
      connSaveTimer = window.setTimeout(() => saveConnEdits(edits), 300)
    },

    resetConns: (node) => {
      const { data } = get()
      if (!data) return
      const [a, b] = node == null ? [0, connOrig.length] : [data.connOff[node], data.connOff[node + 1]]
      for (let c = a; c < b; c++) data.connU32[c * 5 + 4] = connOrig[c]
      const edits = collectConnEdits(data, connOrig)
      set((s) => ({ editRev: s.editRev + 1, connEdits: Object.keys(edits).length }))
      viewRef.current?.refreshOverlay()
      saveConnEdits(edits)
    },

    setPhase: (si, pi, key, value) => {
      const { signals, data } = get()
      const next = signals.map((s, i) =>
        i !== si ? s : { ...s, phases: s.phases.map((p, j) => (j !== pi ? p : { ...p, [key]: Math.max(0, value) })) },
      )
      viewRef.current?.setSignals(next)
      set({ signals: next })
      clearTimeout(saveTimer)
      saveTimer = window.setTimeout(() => data && saveEdits(collectEdits(data, next)), 400)
    },

    resetSignals: () => {
      const { data } = get()
      if (!data) return
      const signals = data.meta.signals
      viewRef.current?.setSignals(signals)
      set({ signals })
      saveEdits({})
    },

    setPlaying: (p) => {
      viewRef.current?.setPlaying(p)
      set({ playing: p })
    },
    setSpeed: (s) => {
      viewRef.current?.setSpeed(s)
      set({ speed: s })
    },
    exportScenario: () => {
      const { data, fileName, signals } = get()
      if (data) exportScenario(fileName, data, signals, collectConnEdits(data, connOrig))
    },
    notify: (msg) => {
      set({ toast: msg })
      setTimeout(() => set((s) => (s.toast === msg ? { toast: null } : s)), 3500)
    },
  }
})
