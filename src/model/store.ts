import { create } from 'zustand'
import type { LayerKey, LayerState } from '../engine/MapView'
import { viewRef } from '../engine/instance'
import { DEFAULT_PARAMS, demandPayload, PROFILES, ZONE_COLORS, type DemandParams, type Zone } from './demand'
import { applyOdImport, buildOdImport, OD2018_URL, type OdImport, type OdSource } from './odImport'

import { DIFF_PALETTE, diffLevels, HEAT_PALETTE, queueLevels, speedLevels } from '../engine/HeatLayer'
import type { VehicleInfo } from './types'
import { simTrack, editNetwork, fetchHist, fetchApproaches, fetchNodeConns, importMap, onSim, resolveDemand, runBench, sendConnEdits, sendPrioEdits, simHeatKind, simRun, simStart, simStop, type DemandResult } from './importer'
import {
  clearAll,
  configFromDef,
  connKey,
  exportScenario,
  loadConnEdits,
  loadPrioEdits,
  loadDemand,
  loadRemoved,
  loadRoads,
  loadScenarios,
  loadSigConfigs,
  loadSignalSource,
  loadMap,
  loadSegEdits,
  saveConnEdits,
  savePrioEdits,
  saveDemand,
  saveRoads,
  saveScenarios,
  saveSigConfigs,
  saveSignalSource,
  saveMap,
  saveSegEdits,
  segEditKey,
  type ConnEdits,
  type PrioEdits,
  type NewPt,
  type NewRoad,
  type ScenarioFile,
  type ScenarioRec,
  type SegKeyTuple,
  type SigConfigs,
  type SignalSource,
  type SegEdit,
  type SegEdits,
} from './persistence'
import { ROAD_PRESETS } from './presets'
import { applyPatch, segInfo, segsOfWay, type SegPatch } from './segments'
import { CONN_ABSENT, CONN_ALLOWED, type NetworkData, type NodeConns, type SignalDef, type SimStats } from './types'

/** recorte do Recife do OpenStreetMap (outubro de 2026), comprimido */
const RECIFE_URL = `${import.meta.env.BASE_URL}data/mapa-recife.osm.gz`

export type EditMode = 'nodes' | 'lanes' | 'build' | 'erase' | 'zones' | 'signal'
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
  /** conexões do nó selecionado (sob demanda) */
  nodeConns: NodeConns | null
  selectedSeg: number | null
  scope: EditScope
  signals: SignalDef[]
  zoom: number
  toast: string | null
  /** Incrementa a cada edição (força os painéis a reler os dados). */
  editRev: number
  connEdits: number
  segEdits: number
  sigEdits: number
  /** tipo de via do construtor e modo reta/curva */
  buildPreset: string
  buildCurve: boolean
  /** ações que podem ser desfeitas (construir/demolir) */
  historyLen: number
  /** zonas de geração/atração de viagens e a matriz OD resolvida pelo núcleo */
  zones: Zone[]
  demandParams: DemandParams
  odManual: Record<string, number>
  /** matriz da Pesquisa OD importada (resumo para a interface) */
  odImport: { name: string; scale: number; merged: number; gateways: number; dropped: number } | null
  odImporting: boolean
  /** origem dos semáforos: base oficial da CTTU (Recife) ou tags do OSM */
  signalSource: SignalSource
  signalsImporting: boolean
  selectedZone: number | null
  demand: DemandResult | null
  /** simulação de tráfego: ocioso, pausada ou rodando */
  sim: {
    phase: 'idle' | 'paused' | 'running'
    speed: number
    stats: SimStats | null
    starting: boolean
    heat: 'off' | 'speed' | 'queue'
    /** por minuto simulado: t, vivos, concluídos, vel. média (km/h), fila de entrada, 0 */
    series: Float32Array | null
    histLen: number
    /** balde (minuto) em reprodução; null = ao vivo */
    replay: number | null
    /** veículo selecionado: rota restante até o destino */
    track: { id: number; alive: boolean; zone: number; kind: number; v: number; wait: number; info: VehicleInfo | null } | null
    follow: boolean
  }
  setFollow: (on: boolean) => void
  pickVehicle: (id: number | null) => void
  /** cenários salvos para comparar (A/B) e a execução em andamento */
  scenarios: ScenarioRec[]
  bench: { id: number | null; progress: number; error: string | null }
  compare: { a: number | null; b: number | null; diff: boolean }

  setHeat: (h: 'off' | 'speed' | 'queue') => void
  setReplay: (bucket: number | null) => void
  saveScenario: (name: string, minutes: number) => void
  deleteScenario: (id: number) => void
  runScenario: (id: number) => Promise<void>
  cancelBench: () => void
  setCompare: (patch: Partial<{ a: number | null; b: number | null; diff: boolean }>) => void

  simStart: () => Promise<void>
  simToggle: () => void
  simSpeed: (s: number) => void
  simReset: () => void
  addZone: (poly: [number, number][]) => void
  updateZone: (id: number, patch: Partial<Zone>) => void
  removeZone: (id: number) => void
  selectZone: (id: number | null) => void
  setDemandParams: (patch: Partial<DemandParams>) => void
  /** Fixa (ou libera, com null) uma célula da matriz OD em veículos/h. */
  setOdCell: (fromId: number, toId: number, value: number | null) => void
  /**
   * substitui as zonas pelas da Pesquisa OD 2018 que cobrem o mapa e fixa a matriz de carro; com `gateways`, o tráfego das
   * cidades vizinhas entra e sai por portais nas vias principais da divisa (senão, pela zona de borda mais próxima)
   */
  importOd2018: (opts?: { gateways?: boolean }) => Promise<void>
  setOdScale: (scale: number) => void
  /** desliga a matriz importada (mantém as zonas, volta ao modelo gravitacional) */
  clearOdImport: () => void
  /** troca os semáforos da rede: `cttu` = base oficial da CTTU no lugar dos do OSM; `osm` = volta aos do OSM */
  setSignalSource: (src: SignalSource) => Promise<void>
  setLayer: (k: LayerKey, on: boolean) => void
  setMode: (m: EditMode) => void
  setScope: (s: EditScope) => void
  setBuildPreset: (id: string) => void
  setBuildCurve: (on: boolean) => void
  /** Confirma um trecho desenhado; `newChain` inicia uma via nova (nome novo). */
  addRoad: (pts: NewPt[], newChain: boolean) => void
  eraseSegment: (seg: number) => void
  undo: () => void
  /** Número de ramos com placa de preferência escolhida. */
  prioCount: number
  /** Define a placa de um ramo do cruzamento selecionado (0 = automático). */
  setPriority: (key: string, level: number) => void
  resetPriorities: (node?: number) => void
  select: (n: number | null, focus?: boolean) => void
  /** Seleção feita no próprio mapa (a vista já está atualizada). */
  onViewSelect: (n: number | null) => void
  selectSeg: (s: number | null) => void
  setZoom: (z: number) => void
  openFile: (file: File) => Promise<void>
  loadUrl: (url: string, name: string) => Promise<void>
  /** mapa do Recife embutido no app; opcionalmente já com as zonas da Pesquisa OD 2018 e os semáforos da CTTU */
  loadRecife: (opts: { zones: boolean; signals: boolean }) => Promise<void>
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
  /** semáforo fora de cruzamento: nó existente (id OSM) ou ponto no corpo da via `way` */
  placeSignal: (p: { kind: 'node' | 'way'; x: number; y: number; node?: number; way?: number }) => void
  removeSignal: (node: number) => void
  /** Volta o semáforo ao padrão do OSM (ou o remove, se foi adicionado pelo usuário). */
  resetSignal: (node: number) => void
  resetSignals: () => void
  exportScenario: () => void
  notify: (msg: string) => void
}

// ------------------------------------------------------------------ edições (fora do React)

let saveTimer = 0
let connSaveTimer = 0
let rebuildTimer = 0
let rebuildBusy = false
let rebuildPending = false

/** Conversões editadas (autoridade): sobrevivem à reconstrução da rede. */
let connRecord: ConnEdits = {}
/** Placas de preferência escolhidas (chave `nóOsm|ramo` -> nível 1..3). */
let prioRecord: PrioEdits = {}
let prioTimer = 0
/** Configurações de trecho editadas (enviadas ao núcleo). */
let segRecord: SegEdits = {}
/** Semáforos adicionados/removidos/reconfigurados. */
let sigRecord: SigConfigs = {}
/** Vias criadas e trechos demolidos (ordem de criação importa: encaixes dependem das anteriores). */
let roadRecord: NewRoad[] = []
let removedRecord: SegKeyTuple[] = []
type Action = { t: 'road'; id: number } | { t: 'removed'; key: SegKeyTuple }
let history: Action[] = []
let chainName = ''
/** Demanda (fonte da verdade fora do React, como os demais registros). */
let zoneRecord: Zone[] = []
let paramsRecord: DemandParams = { ...DEFAULT_PARAMS }
let odRecord: Record<string, number> = {}
let odImportRecord: OdImport | null = null
/** origem dos semáforos da rede (vai junto das edições para o núcleo) */
let sigSourceRecord: SignalSource = 'osm'
/** matriz fixada por hora enviada à simulação (só com importação ativa) */
let odHourly: Record<string, number>[] | undefined
let demandTimer = 0
let demandSave = 0
let cancelBenchFn: (() => void) | null = null
const nextRoadId = () => roadRecord.reduce((m, r) => Math.max(m, r.id), 900_000_000_000_000) + 1
const editPayload = () => JSON.stringify({ segments: Object.values(segRecord), signals: Object.values(sigRecord), roads: roadRecord, removed: removedRecord, connections: connRecord, priorities: prioRecord, signalSource: sigSourceRecord })
export const getSegRecord = () => segRecord

/** Registra (ou apaga, se voltou ao padrão) a edição da conversão `c` do nó carregado. */
const noteConn = (d: NetworkData, nc: NodeConns, c: number) => {
  const k = connKey(d, nc.node, nc.u32[c * 5 + 1], nc.u32[c * 5 + 2])
  if (nc.u32[c * 5 + 4] !== nc.base[c]) connRecord[k] = nc.u32[c * 5 + 4]
  else delete connRecord[k]
}

const afterConnEdit = () => {
  useStore.setState((s) => ({ editRev: s.editRev + 1, connEdits: Object.keys(connRecord).length }))
  viewRef.current?.refreshOverlay()
  clearTimeout(connSaveTimer)
  const snapshot = { ...connRecord }
  connSaveTimer = window.setTimeout(() => {
    saveConnEdits(snapshot)
    sendConnEdits(snapshot)
  }, 300)
}

const afterPrioEdit = (node: number | null) => {
  useStore.setState({ prioCount: Object.keys(prioRecord).length })
  const snapshot = { ...prioRecord }
  sendPrioEdits(snapshot)
  clearTimeout(prioTimer)
  prioTimer = window.setTimeout(() => savePrioEdits(snapshot), 300)
  if (node != null) void refreshApproaches(node)
}

/** Reconsulta as placas do nó e redesenha. */
const refreshApproaches = async (node: number) => {
  const list = await fetchApproaches(node)
  if (useStore.getState().selected === node) viewRef.current?.setApproaches(list, node)
}

export const useStore = create<State>((set, get) => {
  const persistDemand = () => {
    clearTimeout(demandSave)
    demandSave = window.setTimeout(() => saveDemand({ zones: zoneRecord, params: paramsRecord, odManual: odRecord, odImport: odImportRecord }), 300)
  }
  /** Reaplica a matriz importada para a hora, a escala e a opção intrazona atuais. */
  const syncOdImport = () => {
    odHourly = undefined
    if (!odImportRecord) return
    const r = applyOdImport(odImportRecord, zoneRecord, paramsRecord.hour, paramsRecord.intrazone)
    zoneRecord = r.zones
    odRecord = r.odManual
    odHourly = r.odHourly
  }
  /** Recalcula a matriz OD no núcleo (debounce) e sincroniza zonas com o mapa. */
  const refreshDemand = () => {
    const imp = odImportRecord
      ? { name: odImportRecord.name, scale: odImportRecord.scale, merged: odImportRecord.merged, gateways: odImportRecord.gateways?.length ?? 0, dropped: odImportRecord.dropped ?? 0 }
      : null
    set({ zones: [...zoneRecord], demandParams: { ...paramsRecord }, odManual: { ...odRecord }, odImport: imp })
    viewRef.current?.setZones(zoneRecord, get().selectedZone)
    persistDemand()
    clearTimeout(demandTimer)
    demandTimer = window.setTimeout(async () => {
      if (!get().data || zoneRecord.length === 0) return set({ demand: null })
      try {
        set({ demand: await resolveDemand(demandPayload(zoneRecord, paramsRecord, odRecord)) })
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) })
      }
    }, 150)
  }

  /** Instala dados novos (importação ou reconstrução). */
  const install = (data: NetworkData, opts: { fit: boolean }) => {
    const view = viewRef.current
    if (opts.fit) view?.setNetwork(data)
    else view?.updateNetwork(data)
    const signals = data.meta.signals
    view?.setSignals(signals)
    view?.setZones(zoneRecord, get().selectedZone)
    set((s) => ({
      data,
      signals,
      sigEdits: Object.keys(sigRecord).length,
      historyLen: history.length,
      editRev: s.editRev + 1,
      connEdits: Object.keys(connRecord).length,
      prioCount: Object.keys(prioRecord).length,
      segEdits: Object.keys(segRecord).length,
    }))
    refreshDemand()
    const sel = get().selected
    if (sel != null) void loadNodeConns(sel)
  }

  /** Mapa de calor da diferença B − A (razão de velocidade média) sobre a rede atual. */
  const applyDiff = () => {
    const { compare, scenarios, data } = get()
    const view = viewRef.current
    if (!compare.diff || !data || !view) return void (compare.diff ? null : get().sim.heat === 'off' && view?.setHeat(null))
    const A = scenarios.find((x) => x.id === compare.a)?.result
    const B = scenarios.find((x) => x.id === compare.b)?.result
    if (!A || !B) return view.setHeat(null)
    const index = (r: NonNullable<typeof A>) => {
      const m = new Map<string, number>()
      for (let i = 0; i < r.segKeys.length / 3; i++) for (const dir of [0, 1]) m.set(`${r.segKeys[i * 3]}:${r.segKeys[i * 3 + 1]}:${r.segKeys[i * 3 + 2]}:${dir}`, r.ratio[i * 2 + dir])
      return m
    }
    const ia = index(A)
    const ib = index(B)
    const nd = (data.segKeys.length / 3) * 2
    const diff = new Float32Array(nd).fill(NaN)
    for (let i = 0; i < nd / 2; i++) {
      for (const dir of [0, 1]) {
        const k = `${data.segKeys[i * 3]}:${data.segKeys[i * 3 + 1]}:${data.segKeys[i * 3 + 2]}:${dir}`
        const a = ia.get(k)
        const b = ib.get(k)
        if (a != null && b != null && a !== 255 && b !== 255) diff[i * 2 + dir] = (b - a) / 254
      }
    }
    view.setHeat(diffLevels(diff), DIFF_PALETTE)
  }

  /** Pede ao núcleo as conexões do nó (geradas só para ele) e entrega ao desenho e ao painel. */
  const loadNodeConns = async (node: number) => {
    try {
      const nc = await fetchNodeConns(node)
      if (get().selected !== node) return
      set({ nodeConns: nc })
      viewRef.current?.setNodeConns(nc, node)
      const { data } = get()
      // sem placas onde já há semáforo (o sinal manda) ou em nós de passagem
      if (data && !(data.nodeInfo[node * 2 + 1] & 1) && data.nodeInfo[node * 2] >= 3) {
        const list = await fetchApproaches(node)
        if (get().selected === node) viewRef.current?.setApproaches(list, node)
      }
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
    }
  }

  const run = async (buffer: ArrayBuffer, name: string, persist: boolean) => {
    set({ loading: true, error: null })
    try {
      if (persist) {
        await clearAll()
        set({ scenarios: [] })
        connRecord = {}
        prioRecord = {}
        segRecord = {}
        sigRecord = {}
        roadRecord = []
        removedRecord = []
        history = []
        zoneRecord = []
        paramsRecord = { ...DEFAULT_PARAMS }
        odRecord = {}
        odImportRecord = null
        odHourly = undefined
        sigSourceRecord = 'osm'
        set({ selectedZone: null, demand: null })
      } else {
        connRecord = await loadConnEdits()
        prioRecord = await loadPrioEdits()
        segRecord = await loadSegEdits()
        sigRecord = await loadSigConfigs()
        sigSourceRecord = await loadSignalSource()
        set({ scenarios: await loadScenarios() })
        roadRecord = await loadRoads()
        removedRecord = await loadRemoved()
        const dm = await loadDemand()
        zoneRecord = dm?.zones ?? []
        paramsRecord = { ...DEFAULT_PARAMS, ...(dm?.params ?? {}) }
        odRecord = dm?.odManual ?? {}
        odImportRecord = dm?.odImport ?? null
        syncOdImport()
        history = [...roadRecord.map((r): Action => ({ t: 'road', id: r.id })), ...removedRecord.map((key): Action => ({ t: 'removed', key }))]
      }
      const { data, ms } = await importMap(buffer, name, editPayload())
      if (persist) await saveMap(name, buffer)
      set({ signalSource: sigSourceRecord })
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
    rebuildTimer = window.setTimeout(() => void rebuild(), 40)
    saveSegEdits({ ...segRecord })
    saveSigConfigs({ ...sigRecord })
    saveRoads([...roadRecord], [...removedRecord])
    set((s) => ({ historyLen: history.length, segEdits: Object.keys(segRecord).length, sigEdits: Object.keys(sigRecord).length, editRev: s.editRev }))
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
    layers: { roads: true, markings: true, labels: true, nodes: true, shapes: false, signals: true, turns: true, grid: true, zones: true, vehicles: true },
    mode: 'nodes',
    selected: null,
    nodeConns: null,
    prioCount: 0,
    selectedSeg: null,
    scope: 'seg',
    signals: [],
    zoom: 1,
    toast: null,
    editRev: 0,
    connEdits: 0,
    segEdits: 0,
    sigEdits: 0,
    buildPreset: 'local',
    buildCurve: false,
    historyLen: 0,
    zones: [],
    demandParams: { ...DEFAULT_PARAMS },
    odManual: {},
    odImport: null,
    odImporting: false,
    signalSource: 'osm',
    signalsImporting: false,
    selectedZone: null,
    demand: null,
    sim: { phase: 'idle', speed: 5, stats: null, starting: false, heat: 'off', series: null, histLen: 0, replay: null, track: null, follow: false },
    scenarios: [],
    bench: { id: null, progress: 0, error: null },
    compare: { a: null, b: null, diff: false },

    setHeat: (h) => {
      set((s) => ({ sim: { ...s.sim, heat: h, replay: null } }))
      simHeatKind(h === 'off' ? -1 : h === 'speed' ? 0 : 1)
      if (h === 'off') viewRef.current?.setHeat(null)
    },
    setFollow: (on) => {
      viewRef.current?.setFollow(on)
      set((s) => ({ sim: { ...s.sim, follow: on } }))
    },
    pickVehicle: (id) => {
      simTrack(id)
      if (id == null) viewRef.current?.setTrack(null, null)
      set((s) => ({ sim: { ...s.sim, track: id == null ? null : { id, alive: true, zone: -1, kind: 0, v: 0, wait: 0, info: null } } }))
    },
    setReplay: async (bucket) => {
      set((s) => ({ sim: { ...s.sim, replay: bucket } }))
      if (bucket == null) return
      const v = await fetchHist(bucket)
      if (get().sim.replay !== bucket) return
      viewRef.current?.setHeat(v.length ? speedLevels(v) : null)
    },
    saveScenario: (name, minutes) => {
      const { data } = get()
      if (!data) return
      const dp = demandPayload(zoneRecord, paramsRecord, odRecord)
      const sim = JSON.stringify({ zones: dp.zones, params: { seed: paramsRecord.seed, hour: paramsRecord.hour, busShare: paramsRecord.busShare, illegalPct: paramsRecord.illegalPct ?? 0, boxBlockPct: paramsRecord.boxBlockPct ?? 0, reroute: paramsRecord.reroute ?? true, appPct: paramsRecord.appPct ?? 60, perception: paramsRecord.perception ?? 0.3, beta: paramsRecord.beta, intrazone: paramsRecord.intrazone, odManual: odRecord, odHourly } })
      const id = get().scenarios.reduce((m, x) => Math.max(m, x.id), 0) + 1
      const next = [...get().scenarios, { id, name: name || `Cenário ${id}`, edits: editPayload(), sim, minutes }]
      set({ scenarios: next })
      saveScenarios(next)
    },
    deleteScenario: (id) => {
      const next = get().scenarios.filter((x) => x.id !== id)
      const c = get().compare
      set({ scenarios: next, compare: { ...c, a: c.a === id ? null : c.a, b: c.b === id ? null : c.b } })
      saveScenarios(next)
    },
    runScenario: async (id) => {
      const rec = get().scenarios.find((x) => x.id === id)
      if (!rec || get().bench.id != null) return
      if (!JSON.parse(rec.sim).zones?.length) return get().notify('O cenário não tem zonas de demanda.')
      set({ bench: { id, progress: 0, error: null } })
      try {
        const { promise, cancel } = runBench(rec.edits, rec.sim, rec.minutes, (p) => set((s) => ({ bench: { ...s.bench, progress: p } })))
        cancelBenchFn = cancel
        const result = await promise
        const next = get().scenarios.map((x) => (x.id === id ? { ...x, result } : x))
        set({ scenarios: next, bench: { id: null, progress: 1, error: null } })
        saveScenarios(next)
      } catch (e) {
        set({ bench: { id: null, progress: 0, error: e instanceof Error && e.message === 'cancelado' ? null : e instanceof Error ? e.message : String(e) } })
      }
    },
    cancelBench: () => cancelBenchFn?.(),
    setCompare: (patch) => {
      set((s) => ({ compare: { ...s.compare, ...patch } }))
      applyDiff()
    },

    simStart: async () => {
      const { data } = get()
      if (!data) return
      if (zoneRecord.length < 1 || (zoneRecord.length < 2 && !paramsRecord.intrazone)) {
        return get().notify('Desenhe ao menos 2 zonas (modo Zonas) para gerar viagens.')
      }
      set((s) => ({ sim: { ...s.sim, starting: true } }))
      try {
        const dp = demandPayload(zoneRecord, paramsRecord, odRecord)
        await simStart({
          zones: dp.zones,
          params: { seed: paramsRecord.seed, hour: paramsRecord.hour, busShare: paramsRecord.busShare, illegalPct: paramsRecord.illegalPct ?? 0, boxBlockPct: paramsRecord.boxBlockPct ?? 0, reroute: paramsRecord.reroute ?? true, appPct: paramsRecord.appPct ?? 60, perception: paramsRecord.perception ?? 0.3, beta: paramsRecord.beta, intrazone: paramsRecord.intrazone, odManual: odRecord, odHourly },
        })
        simHeatKind(get().sim.heat === 'off' ? -1 : get().sim.heat === 'speed' ? 0 : 1)
        simRun(true, get().sim.speed)
        set((s) => ({ sim: { ...s.sim, phase: 'running', starting: false, stats: null, replay: null, series: null, histLen: 0 } }))
      } catch (e) {
        set((s) => ({ sim: { ...s.sim, starting: false }, error: e instanceof Error ? e.message : String(e) }))
      }
    },
    simToggle: () => {
      const { sim } = get()
      if (sim.phase === 'idle') return void get().simStart()
      const run = sim.phase !== 'running'
      simRun(run, sim.speed)
      set({ sim: { ...sim, phase: run ? 'running' : 'paused' } })
    },
    simSpeed: (s) => {
      const { sim } = get()
      if (sim.phase !== 'idle') simRun(sim.phase === 'running', s)
      set({ sim: { ...sim, speed: s } })
    },
    simReset: () => {
      simStop()
      viewRef.current?.setVehicles(null)
      viewRef.current?.setSimClock(null)
      viewRef.current?.setHeat(null)
      set((s) => ({ sim: { ...s.sim, phase: 'idle', stats: null, replay: null, series: null, histLen: 0 } }))
    },

    addZone: (poly) => {
      const id = zoneRecord.reduce((m, z) => Math.max(m, z.id), 0) + 1
      zoneRecord.push({ id, name: `Zona ${id}`, poly, gen: 600, attr: 1, profile: PROFILES[3].id, color: ZONE_COLORS[(id - 1) % ZONE_COLORS.length] })
      set({ selectedZone: id })
      refreshDemand()
    },
    updateZone: (id, patch) => {
      zoneRecord = zoneRecord.map((z) => (z.id === id ? { ...z, ...patch } : z))
      refreshDemand()
    },
    removeZone: (id) => {
      zoneRecord = zoneRecord.filter((z) => z.id !== id)
      odRecord = Object.fromEntries(Object.entries(odRecord).filter(([k]) => !k.split('>').includes(String(id))))
      set({ selectedZone: get().selectedZone === id ? null : get().selectedZone })
      refreshDemand()
    },
    selectZone: (id) => {
      set({ selectedZone: id })
      viewRef.current?.setZones(zoneRecord, id)
    },
    setDemandParams: (patch) => {
      paramsRecord = { ...paramsRecord, ...patch }
      if (patch.hour != null || patch.intrazone != null) syncOdImport()
      refreshDemand()
    },
    setOdCell: (fromId, toId, value) => {
      const k = `${fromId}>${toId}`
      if (value == null || !Number.isFinite(value)) delete odRecord[k]
      else odRecord[k] = Math.max(0, value)
      refreshDemand()
    },

    importOd2018: async (opts = {}) => {
      const { data } = get()
      if (!data || get().odImporting) return
      set({ odImporting: true })
      try {
        const res = await fetch(OD2018_URL)
        if (!res.ok) throw new Error(`não foi possível ler ${OD2018_URL} (${res.status})`)
        const { zones, imp } = buildOdImport((await res.json()) as OdSource, data, opts)
        zoneRecord = zones
        odImportRecord = imp
        syncOdImport()
        set({ selectedZone: null })
        refreshDemand()
        const ng = imp.gateways?.length ?? 0
        get().notify(
          ng > 0
            ? `${zones.length - ng} zonas no mapa e ${ng} entradas/saídas na divisa para ${imp.merged} zonas de fora.`
            : `${zones.length} zonas importadas; ${imp.merged} zonas fora do mapa incorporadas à borda.`,
        )
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) })
      } finally {
        set({ odImporting: false })
      }
    },
    setOdScale: (scale) => {
      if (!odImportRecord) return
      odImportRecord = { ...odImportRecord, scale: Math.max(0, scale) }
      syncOdImport()
      refreshDemand()
    },
    setSignalSource: async (src) => {
      if (!get().data || get().signalsImporting) return
      const prev = sigSourceRecord
      sigSourceRecord = src
      set({ signalsImporting: true })
      try {
        const { data } = await editNetwork(editPayload())
        // a CTTU só cobre o Recife: fora dele o núcleo mantém os semáforos do OSM
        const got: SignalSource = data.meta.signalSource === 'ext' ? 'cttu' : 'osm'
        sigSourceRecord = got
        saveSignalSource(got)
        install(data, { fit: false })
        set({ signalSource: got })
        if (src === 'cttu' && got !== 'cttu') get().notify('Nenhum semáforo da CTTU cai neste mapa (a base cobre só o Recife). Ficam os do OSM.')
        else if (got === 'cttu') {
          const ctrl = new Set(data.meta.signals.map((s) => s.ext).filter((x) => x != null)).size
          get().notify(`${ctrl} semáforos da CTTU importados (${data.meta.signals.length} nós). Os do OSM foram removidos.`)
        } else get().notify(`Semáforos do OSM restaurados (${data.meta.signals.length}).`)
      } catch (e) {
        sigSourceRecord = prev
        set({ error: e instanceof Error ? e.message : String(e) })
      } finally {
        set({ signalsImporting: false })
      }
    },
    clearOdImport: () => {
      odImportRecord = null
      odRecord = {}
      syncOdImport()
      refreshDemand()
    },

    setLayer: (k, on) => {
      viewRef.current?.setLayer(k, on)
      set((s) => ({ layers: { ...s.layers, [k]: on } }))
    },
    setMode: (m) => {
      viewRef.current?.setMode(m)
      if (m === 'build') viewRef.current?.setBuild(ROAD_PRESETS.find((p) => p.id === get().buildPreset) ?? ROAD_PRESETS[0], get().buildCurve)
      set({ mode: m, selected: null, nodeConns: null, selectedSeg: null, selectedZone: m === 'zones' ? get().selectedZone : null })
    },
    setScope: (scope) => set({ scope }),
    setBuildPreset: (id) => {
      set({ buildPreset: id })
      viewRef.current?.setBuild(ROAD_PRESETS.find((p) => p.id === id) ?? ROAD_PRESETS[0], get().buildCurve)
    },
    setBuildCurve: (on) => {
      set({ buildCurve: on })
      viewRef.current?.setBuild(ROAD_PRESETS.find((p) => p.id === get().buildPreset) ?? ROAD_PRESETS[0], on)
    },
    addRoad: (pts, newChain) => {
      const preset = ROAD_PRESETS.find((p) => p.id === get().buildPreset) ?? ROAD_PRESETS[0]
      if (newChain || !chainName) {
        const n = roadRecord.reduce((m, r) => Math.max(m, Number(/^Via nova (\d+)$/.exec(r.name)?.[1] ?? 0)), 0) + 1
        chainName = `Via nova ${n}`
      }
      const road: NewRoad = { id: nextRoadId(), hw: preset.hw, name: chainName, oneway: preset.oneway, lf: preset.lf, lb: preset.oneway ? 0 : preset.lb, pts }
      roadRecord.push(road)
      history.push({ t: 'road', id: road.id })
      scheduleRebuild()
    },
    eraseSegment: (seg) => {
      const { data } = get()
      if (!data) return
      const key: SegKeyTuple = [data.segKeys[seg * 3], data.segKeys[seg * 3 + 1], data.segKeys[seg * 3 + 2]]
      if (removedRecord.some((k) => k[0] === key[0] && k[1] === key[1] && k[2] === key[2])) return
      removedRecord.push(key)
      history.push({ t: 'removed', key })
      set({ selectedSeg: null })
      scheduleRebuild()
    },
    undo: () => {
      const a = history.pop()
      if (!a) return
      if (a.t === 'road') roadRecord = roadRecord.filter((r) => r.id !== a.id)
      else removedRecord = removedRecord.filter((k) => !(k[0] === a.key[0] && k[1] === a.key[1] && k[2] === a.key[2]))
      set({ selectedSeg: null })
      scheduleRebuild()
    },
    onViewSelect: (n) => {
      set({ selected: n, nodeConns: null })
      if (n != null) void loadNodeConns(n)
    },
    select: (n, focus = false) => {
      // a vista chama onViewSelect (atualiza estado e pede as conexões ao núcleo)
      viewRef.current?.select(n, focus)
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
          prioRecord = { ...(sc.priorities ?? {}) }
          savePrioEdits({ ...prioRecord })
          sigRecord = Object.fromEntries((sc.signalConfigs ?? []).map((c) => [String(c.node), c]))
          sigSourceRecord = sc.signalSource === 'cttu' ? 'cttu' : 'osm'
          saveSignalSource(sigSourceRecord)
          set({ signalSource: sigSourceRecord })
          roadRecord = sc.roads ?? []
          removedRecord = sc.removed ?? []
          history = [...roadRecord.map((r): Action => ({ t: 'road', id: r.id })), ...removedRecord.map((key): Action => ({ t: 'removed', key }))]
          saveRoads([...roadRecord], [...removedRecord])
          zoneRecord = sc.demand?.zones ?? []
          paramsRecord = { ...DEFAULT_PARAMS, ...(sc.demand?.params ?? {}) }
          odRecord = sc.demand?.odManual ?? {}
          odImportRecord = sc.demand?.odImport ?? null
          syncOdImport()
          persistDemand()
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

    loadRecife: async ({ zones, signals }) => {
      const name = 'mapa-recife.osm'
      set({ loading: true, error: null })
      try {
        const res = await fetch(RECIFE_URL)
        if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`)
        // o servidor pode já ter descomprimido (Content-Encoding); senão, descomprime aqui
        const raw = new Uint8Array(await res.arrayBuffer())
        const gz = raw[0] === 0x1f && raw[1] === 0x8b
        const buf = gz ? await new Response(new Blob([raw]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer() : raw.buffer
        await run(buf, name, true)
      } catch (e) {
        set({ loading: false, error: `Não foi possível carregar ${name}: ${e instanceof Error ? e.message : e}` })
        return
      }
      if (!get().data) return
      if (signals) await get().setSignalSource('cttu')
      if (zones) await get().importOd2018({ gateways: true })
    },

    restore: async () => {
      const saved = await loadMap()
      if (saved) await run(saved.buffer, saved.name, false)
    },

    toggleConns: (conns) => {
      const { data, nodeConns: nc } = get()
      if (!data || !nc || conns.length === 0) return
      // alguma ligada -> remove todas; senão liga todas
      const any = conns.some((c) => nc.u32[c * 5 + 4] === CONN_ALLOWED)
      for (const c of conns) {
        nc.u32[c * 5 + 4] = any ? CONN_ABSENT : CONN_ALLOWED
        noteConn(data, nc, c)
      }
      afterConnEdit()
    },

    connectLanes: (fromLane, toLane) => {
      const { data, nodeConns: nc } = get()
      if (!data || !nc) return
      for (let c = 0; c < nc.n; c++) {
        if (nc.u32[c * 5 + 1] === fromLane && nc.u32[c * 5 + 2] === toLane) {
          nc.u32[c * 5 + 4] = nc.u32[c * 5 + 4] === CONN_ALLOWED ? CONN_ABSENT : CONN_ALLOWED
          noteConn(data, nc, c)
          afterConnEdit()
          return
        }
      }
    },

    setPriority: (key, level) => {
      const { data, selected } = get()
      if (!data || selected == null) return
      const k = `${data.nodeOsm[selected]}|${key}`
      if (level > 0) prioRecord[k] = level
      else delete prioRecord[k]
      afterPrioEdit(selected)
    },

    resetPriorities: (node) => {
      const { data, selected } = get()
      if (!data) return
      if (node == null) prioRecord = {}
      else for (const k of Object.keys(prioRecord)) if (k.startsWith(`${data.nodeOsm[node]}|`)) delete prioRecord[k]
      afterPrioEdit(selected)
    },

    resetConns: (node) => {
      const { data, nodeConns: nc, selected } = get()
      if (!data) return
      if (node == null) {
        connRecord = {}
        afterConnEdit()
        if (selected != null) void loadNodeConns(selected)
        return
      }
      if (!nc || nc.node !== node) return
      for (let c = 0; c < nc.n; c++) {
        nc.u32[c * 5 + 4] = nc.base[c]
        delete connRecord[connKey(data, nc.node, nc.u32[c * 5 + 1], nc.u32[c * 5 + 2])]
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
    setOffset: (si, value) => {
      const offset = Math.min(600, Math.max(0, Number.isFinite(value) ? value : 0))
      // nós de um mesmo controlador CTTU têm uma defasagem só
      const ext = get().signals[si]?.ext
      get().signals.forEach((d, i) => {
        if (i === si || (ext != null && d.ext === ext)) editSignalLocal(i, (def) => ({ ...def, offset }))
      })
    },
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
    placeSignal: (p) => {
      const { data } = get()
      if (!data) return
      if (p.kind === 'node') {
        const idx = Array.from(data.nodeOsm).indexOf(p.node ?? NaN)
        if (idx >= 0) get().addSignal(idx)
      } else {
        const road: NewRoad = { id: nextRoadId(), hw: 'mark', name: 'Semáforo em trecho', oneway: false, lf: 0, lb: 0, pts: [{ x: Math.round(p.x * 1000) / 1000, y: Math.round(p.y * 1000) / 1000, way: p.way }] }
        roadRecord.push(road)
        history.push({ t: 'road', id: road.id })
        scheduleRebuild()
      }
      // quando a rede nova chegar, seleciona o nó criado para configurar as fases
      const unsub = useStore.subscribe((st, prev) => {
        if (st.data === prev.data || !st.data) return
        unsub()
        const d = st.data
        for (let i = 0; i < d.nodeXY.length / 2; i++) {
          if (Math.hypot(d.nodeXY[i * 2] - p.x, d.nodeXY[i * 2 + 1] - p.y) < 2) {
            get().setMode('nodes')
            get().select(i, false)
            return
          }
        }
      })
    },
    removeSignal: (node) => {
      const { data } = get()
      if (!data) return
      const osm = data.nodeOsm[node]
      // semáforos criados em trecho: some também o ponto que divide a via
      const nx = data.nodeXY[node * 2]
      const ny = data.nodeXY[node * 2 + 1]
      const before = roadRecord.length
      roadRecord = roadRecord.filter((r) => !(r.hw === 'mark' && r.pts[0] && Math.hypot(r.pts[0].x - nx, r.pts[0].y - ny) < 1.5))
      if (roadRecord.length !== before) {
        delete sigRecord[String(osm)]
        scheduleRebuild()
        return
      }
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

    exportScenario: () => {
      const { data, fileName } = get()
      if (data) exportScenario(fileName, { ...sigRecord }, { ...connRecord }, { ...prioRecord }, { ...segRecord }, [...roadRecord], [...removedRecord], { zones: zoneRecord, params: paramsRecord, odManual: odRecord, odImport: odImportRecord }, sigSourceRecord)
    },
    notify: (msg) => {
      set({ toast: msg })
      setTimeout(() => set((s) => (s.toast === msg ? { toast: null } : s)), 3500)
    },
  }
})

export type { SegEdit }

// quadros da simulação vindos do worker
onSim((m) => {
  const st = useStore
  if (m.type === 'sim') {
    viewRef.current?.setVehicles(m.buf)
    viewRef.current?.setSimClock(m.time)
    const cur = st.getState().sim
    // estatísticas e séries só mudam a ~1 Hz (o worker as manda junto com `series`): não re-renderiza a UI a cada quadro
    if (m.series || !cur.stats) st.setState((s) => ({ sim: { ...s.sim, stats: m.stats, series: m.series ?? s.sim.series, histLen: m.histLen ?? s.sim.histLen } }))
    if (m.track != null && cur.track) {
      const t = JSON.parse(m.track) as { id?: number; alive: boolean; zone: number; kind: number; v: number; wait: number; pts: number[]; info?: VehicleInfo }
      if (t.id == null || t.id === cur.track.id) {
        viewRef.current?.setTrack(t.alive ? cur.track.id : null, t.alive ? t.pts : null, t.info?.enter && t.info.enter.blocker >= 0 ? t.info.enter.blocker : null)
        st.setState((s) => (s.sim.track ? { sim: { ...s.sim, track: { ...s.sim.track, alive: t.alive, zone: t.zone ?? s.sim.track.zone, kind: t.kind ?? 0, v: t.v ?? 0, wait: t.wait ?? 0, info: t.info ?? null } } } : {}))
      }
    }
    if (m.heat && cur.heat !== 'off' && cur.replay == null) {
      viewRef.current?.setHeat(cur.heat === 'speed' ? speedLevels(m.heat) : queueLevels(m.heat), HEAT_PALETTE)
    }
  } else {
    viewRef.current?.setVehicles(null)
    viewRef.current?.setSimClock(null)
    viewRef.current?.setTrack(null, null)
    st.setState((s) => ({ sim: { ...s.sim, phase: 'idle', stats: null, replay: null, track: null } }))
    if (st.getState().sim.heat === 'off') viewRef.current?.setHeat(null)
    st.getState().notify(`Simulação encerrada: ${m.reason}.`)
  }
})
