import { create } from 'zustand'
import type { LayerKey, LayerState } from '../engine/MapView'
import { viewRef } from '../engine/instance'
import { editNetwork, importMap } from './importer'
import {
  applyConnEdits,
  clearAll,
  configFromDef,
  connKey,
  exportScenario,
  loadConnEdits,
  loadSigConfigs,
  loadMap,
  loadSegEdits,
  saveConnEdits,
  saveSigConfigs,
  saveMap,
  saveSegEdits,
  segEditKey,
  type ConnEdits,
  type ScenarioFile,
  type SigConfigs,
  type SegEdit,
  type SegEdits,
} from './persistence'
import { applyPatch, segInfo, segsOfWay, type SegPatch } from './segments'
import { CONN_ABSENT, CONN_ALLOWED, type NetworkData, type SignalDef } from './types'

export type EditMode = 'nodes' | 'lanes'
export type EditScope = 'seg' | 'way'

interface State {
  data: NetworkData | null
  fileName: string
  loading: boolean
  /** reconstruindo a rede após edição de faixas */
  rebuilding: boolean
  error: string | null
  importMs: number
  layers: LayerState
  mode: EditMode
  selected: number | null
  selectedSeg: number | null
  scope: EditScope
  signals: SignalDef[]
  playing: boolean
  speed: number
  zoom: number
  toast: string | null
  /** Incrementa a cada edição (força os painéis a reler os dados). */
  editRev: number
  connEdits: number
  segEdits: number
  sigEdits: number

  setLayer: (k: LayerKey, on: boolean) => void
  setMode: (m: EditMode) => void
  setScope: (s: EditScope) => void
  select: (n: number | null, focus?: boolean) => void
  selectSeg: (s: number | null) => void
  setZoom: (z: number) => void
  openFile: (file: File) => Promise<void>
  loadUrl: (url: string, name: string) => Promise<void>
  restore: () => Promise<void>
  toggleConns: (conns: number[]) => void
  /** Arrasto origem -> destino no nó selecionado: liga a conversão ou, se já ligada, remove. */
  connectLanes: (fromLane: number, toLane: number) => void
  resetConns: (node?: number) => void
  /** Edita a configuração de faixas do trecho selecionado (ou de toda a via, conforme o escopo). */
  editSegment: (patch: SegPatch) => void
  resetSegment: () => void
  setPhase: (signalIdx: number, phase: number, key: 'green' | 'yellow' | 'allRed', value: number) => void
  setOffset: (signalIdx: number, value: number) => void
  /** Move uma aproximação (id de ponta) para outra fase do semáforo. */
  moveApproach: (signalIdx: number, end: number, toPhase: number) => void
  addPhase: (signalIdx: number) => void
  removePhase: (signalIdx: number, phase: number) => void
  /** Liga um semáforo num cruzamento (índice do nó). */
  addSignal: (node: number) => void
  removeSignal: (node: number) => void
  /** Volta o semáforo ao padrão do OSM (ou o remove, se foi adicionado pelo usuário). */
  resetSignal: (node: number) => void
  resetSignals: () => void
  setPlaying: (p: boolean) => void
  setSpeed: (s: number) => void
  exportScenario: () => void
  notify: (msg: string) => void
}

// ------------------------------------------------------------------ edições (fora do React)

let saveTimer = 0
let connSaveTimer = 0
let rebuildTimer = 0
let rebuildBusy = false
let rebuildPending = false

/** Permissões originais (do núcleo) para saber o que mudou. */
let connOrig = new Uint8Array(0)
/** Conversões editadas (autoridade): sobrevivem à reconstrução da rede. */
let connRecord: ConnEdits = {}
/** Configurações de trecho editadas (enviadas ao núcleo). */
let segRecord: SegEdits = {}
/** Semáforos adicionados/removidos/reconfigurados. */
let sigRecord: SigConfigs = {}
const editPayload = () => JSON.stringify({ segments: Object.values(segRecord), signals: Object.values(sigRecord) })
export const getSegRecord = () => segRecord

const snapshotConns = (d: NetworkData) => {
  connOrig = new Uint8Array(d.connU32.length / 5)
  for (let c = 0; c < connOrig.length; c++) connOrig[c] = d.connU32[c * 5 + 4]
}

const noteConn = (d: NetworkData, c: number) => {
  const k = connKey(d, c)
  if (d.connU32[c * 5 + 4] !== connOrig[c]) connRecord[k] = d.connU32[c * 5 + 4]
  else delete connRecord[k]
}

const afterConnEdit = () => {
  useStore.setState((s) => ({ editRev: s.editRev + 1, connEdits: Object.keys(connRecord).length }))
  viewRef.current?.refreshOverlay()
  clearTimeout(connSaveTimer)
  const snapshot = { ...connRecord }
  connSaveTimer = window.setTimeout(() => saveConnEdits(snapshot), 300)
}

export const useStore = create<State>((set, get) => {
  /** Instala dados novos (importação ou reconstrução). */
  const install = (data: NetworkData, opts: { fit: boolean }) => {
    snapshotConns(data)
    applyConnEdits(data, connRecord)
    const view = viewRef.current
    if (opts.fit) view?.setNetwork(data)
    else view?.updateNetwork(data)
    const signals = data.meta.signals
    view?.setSignals(signals)
    set((s) => ({
      data,
      signals,
      sigEdits: Object.keys(sigRecord).length,
      editRev: s.editRev + 1,
      connEdits: Object.keys(connRecord).length,
      segEdits: Object.keys(segRecord).length,
    }))
  }

  const run = async (buffer: ArrayBuffer, name: string, persist: boolean) => {
    set({ loading: true, error: null })
    try {
      if (persist) {
        await clearAll()
        connRecord = {}
        segRecord = {}
        sigRecord = {}
      } else {
        connRecord = await loadConnEdits()
        segRecord = await loadSegEdits()
        sigRecord = await loadSigConfigs()
      }
      const { data, ms } = await importMap(buffer, name, editPayload())
      if (persist) await saveMap(name, buffer)
      set({ fileName: name, importMs: ms, loading: false, selected: null, selectedSeg: null })
      install(data, { fit: true })
    } catch (e) {
      set({ loading: false, error: e instanceof Error ? e.message : String(e) })
    }
  }

  /** Reconstrói a rede no núcleo com as edições de trecho, mantendo câmera e seleção. */
  const rebuild = async () => {
    if (rebuildBusy) {
      rebuildPending = true
      return
    }
    rebuildBusy = true
    set({ rebuilding: true })
    try {
      const old = get()
      const { data } = await editNetwork(editPayload())
      const sameShape = old.data && old.data.segKeys.length === data.segKeys.length
      if (!sameShape) set({ selectedSeg: null })
      install(data, { fit: false })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
    } finally {
      rebuildBusy = false
      if (rebuildPending) {
        rebuildPending = false
        void rebuild()
      } else set({ rebuilding: false })
    }
  }
  const scheduleRebuild = () => {
    clearTimeout(rebuildTimer)
    rebuildTimer = window.setTimeout(() => void rebuild(), 90)
    saveSegEdits({ ...segRecord })
    saveSigConfigs({ ...sigRecord })
    set((s) => ({ segEdits: Object.keys(segRecord).length, sigEdits: Object.keys(sigRecord).length, editRev: s.editRev }))
  }

  /** Edita um semáforo sem reconstruir a rede (só dados de fase): atualiza o desenho e o registro salvo. */
  const editSignalLocal = (si: number, fn: (d: SignalDef) => SignalDef) => {
    const { signals } = get()
    const def = signals[si]
    if (!def) return
    const next = fn(def)
    const list = signals.map((x, i) => (i === si ? next : x))
    sigRecord[String(next.osm)] = configFromDef(next)
    viewRef.current?.setSignals(list)
    set({ signals: list, sigEdits: Object.keys(sigRecord).length })
    clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => saveSigConfigs({ ...sigRecord }), 300)
  }

  return {
    data: null,
    fileName: '',
    loading: false,
    rebuilding: false,
    error: null,
    importMs: 0,
    layers: { roads: true, markings: true, labels: true, nodes: true, shapes: false, signals: true, turns: true, grid: true },
    mode: 'nodes',
    selected: null,
    selectedSeg: null,
    scope: 'seg',
    signals: [],
    playing: true,
    speed: 1,
    zoom: 1,
    toast: null,
    editRev: 0,
    connEdits: 0,
    segEdits: 0,
    sigEdits: 0,

    setLayer: (k, on) => {
      viewRef.current?.setLayer(k, on)
      set((s) => ({ layers: { ...s.layers, [k]: on } }))
    },
    setMode: (m) => {
      viewRef.current?.setMode(m)
      set({ mode: m, selected: null, selectedSeg: null })
    },
    setScope: (scope) => set({ scope }),
    select: (n, focus = false) => {
      viewRef.current?.select(n, focus)
      set({ selected: n })
    },
    selectSeg: (s) => {
      viewRef.current?.selectSeg(s)
      set({ selectedSeg: s })
    },
    setZoom: (z) => set({ zoom: z }),

    openFile: async (file) => {
      if (/\.json$/i.test(file.name)) {
        const { data } = get()
        if (!data) return get().notify('Importe um mapa antes de aplicar um cenário.')
        try {
          const sc = JSON.parse(await file.text()) as ScenarioFile
          if (sc.app !== 'simulador-transito') throw new Error('arquivo não é um cenário do simulador')
          segRecord = Object.fromEntries((sc.segments ?? []).map((e) => [segEditKey(e), e]))
          connRecord = sc.connFormat === 2 ? { ...(sc.connections ?? {}) } : {}
          sigRecord = Object.fromEntries((sc.signalConfigs ?? []).map((c) => [String(c.node), c]))
          saveSegEdits({ ...segRecord })
          saveConnEdits({ ...connRecord })
          saveSigConfigs({ ...sigRecord })
          // reconstrói a partir do cenário (faixas, semáforos e conversões)
          const { data: nd } = await editNetwork(editPayload())
          install(nd, { fit: false })
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
      // alguma ligada -> remove todas; senão liga todas
      const any = conns.some((c) => data.connU32[c * 5 + 4] === CONN_ALLOWED)
      for (const c of conns) {
        data.connU32[c * 5 + 4] = any ? CONN_ABSENT : CONN_ALLOWED
        noteConn(data, c)
      }
      afterConnEdit()
    },

    connectLanes: (fromLane, toLane) => {
      const { data, selected } = get()
      if (!data || selected == null) return
      for (let c = data.connOff[selected]; c < data.connOff[selected + 1]; c++) {
        if (data.connU32[c * 5 + 1] === fromLane && data.connU32[c * 5 + 2] === toLane) {
          data.connU32[c * 5 + 4] = data.connU32[c * 5 + 4] === CONN_ALLOWED ? CONN_ABSENT : CONN_ALLOWED
          noteConn(data, c)
          afterConnEdit()
          return
        }
      }
    },

    resetConns: (node) => {
      const { data } = get()
      if (!data) return
      const [a, b] = node == null ? [0, connOrig.length] : [data.connOff[node], data.connOff[node + 1]]
      for (let c = a; c < b; c++) {
        data.connU32[c * 5 + 4] = connOrig[c]
        delete connRecord[connKey(data, c)]
      }
      afterConnEdit()
    },

    editSegment: (patch) => {
      const { data, selectedSeg, scope } = get()
      if (!data || selectedSeg == null) return
      const targets = scope === 'way' ? segsOfWay(data, selectedSeg) : [selectedSeg]
      for (const t of targets) {
        const cur = segInfo(data, t, segRecord).edit
        const next = applyPatch(cur, patch)
        segRecord[segEditKey(next)] = next
      }
      scheduleRebuild()
    },

    resetSegment: () => {
      const { data, selectedSeg, scope } = get()
      if (!data || selectedSeg == null) return
      const targets = scope === 'way' ? segsOfWay(data, selectedSeg) : [selectedSeg]
      for (const t of targets) {
        const k = segEditKey({ way: data.segKeys[t * 3], from: data.segKeys[t * 3 + 1], to: data.segKeys[t * 3 + 2] })
        delete segRecord[k]
      }
      scheduleRebuild()
    },

    setPhase: (si, pi, key, value) => {
      editSignalLocal(si, (def) => ({
        ...def,
        phases: def.phases.map((p, j) => (j !== pi ? p : { ...p, [key]: Math.min(600, Math.max(0, Number.isFinite(value) ? value : 0)) })),
      }))
    },
    setOffset: (si, value) => editSignalLocal(si, (def) => ({ ...def, offset: Math.min(600, Math.max(0, Number.isFinite(value) ? value : 0)) })),
    moveApproach: (si, end, toPhase) =>
      editSignalLocal(si, (def) => ({
        ...def,
        phases: def.phases.map((p, j) => ({ ...p, ends: j === toPhase ? [...p.ends.filter((e) => e !== end), end] : p.ends.filter((e) => e !== end) })),
      })),
    addPhase: (si) => editSignalLocal(si, (def) => ({ ...def, phases: [...def.phases, { green: 20, yellow: 3, allRed: 2, ends: [] }] })),
    removePhase: (si, pi) =>
      editSignalLocal(si, (def) => {
        if (def.phases.length < 2) return def
        const gone = def.phases[pi]
        const target = pi > 0 ? pi - 1 : 1
        const phases = def.phases
          .map((p, j) => (j === target ? { ...p, ends: [...p.ends, ...gone.ends] } : p))
          .filter((_, j) => j !== pi)
        return { ...def, phases }
      }),

    addSignal: (node) => {
      const { data } = get()
      if (!data) return
      const osm = data.nodeOsm[node]
      sigRecord[String(osm)] = { node: osm, enabled: true, offset: 0, phases: null }
      scheduleRebuild()
    },
    removeSignal: (node) => {
      const { data } = get()
      if (!data) return
      const osm = data.nodeOsm[node]
      sigRecord[String(osm)] = { node: osm, enabled: false, offset: 0, phases: null }
      scheduleRebuild()
    },
    resetSignal: (node) => {
      const { data } = get()
      if (!data) return
      delete sigRecord[String(data.nodeOsm[node])]
      scheduleRebuild()
    },
    resetSignals: () => {
      sigRecord = {}
      scheduleRebuild()
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
      const { data, fileName } = get()
      if (data) exportScenario(fileName, { ...sigRecord }, { ...connRecord }, { ...segRecord })
    },
    notify: (msg) => {
      set({ toast: msg })
      setTimeout(() => set((s) => (s.toast === msg ? { toast: null } : s)), 3500)
    },
  }
})

export type { SegEdit }
