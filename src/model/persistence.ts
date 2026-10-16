import { del, get, set } from 'idb-keyval'
import type { NetworkData, SignalDef } from './types'

/** Configuração de um semáforo (enviada ao núcleo e salva): liga/desliga e fases. */
export interface SigConfig {
  /** id OSM do nó */
  node: number
  enabled: boolean
  offset: number
  /** null = fases padrão geradas pelo núcleo */
  phases: { green: number; yellow: number; allRed: number; approaches: string[] }[] | null
}
export type SigConfigs = Record<string, SigConfig>

/** Conversões alteradas: chave estável -> estado (1 ligada, 0 proibida, 2 ausente). */
export type ConnEdits = Record<string, number>

/** Configuração de faixas de um trecho (valores finais; enviada ao núcleo). */
export interface SegEdit {
  way: number
  from: number
  to: number
  oneway: boolean
  lf: number
  lb: number
  /** largura da faixa em metros */
  w: number
  busF: boolean[]
  busB: boolean[]
  reverse: boolean
}
export type SegEdits = Record<string, SegEdit>

/** Ponto de uma via nova (metros locais). `node` encaixa num nó; `way` no corpo de uma via. */
export interface NewPt {
  x: number
  y: number
  node?: number
  way?: number
}
export interface NewRoad {
  id: number
  hw: string
  name: string
  oneway: boolean
  lf: number
  lb: number
  pts: NewPt[]
}
/** Trecho demolido: [way, nóOsmInicial, nóOsmFinal]. */
export type SegKeyTuple = [number, number, number]
export const segEditKey = (e: { way: number; from: number; to: number }) => `${e.way}:${e.from}:${e.to}`

export interface ScenarioFile {
  version: 1
  app: 'simulador-transito'
  map: { name: string }
  /** semáforos adicionados, removidos ou reconfigurados */
  signalConfigs?: SigConfig[]
  /** formato 2: chaves estáveis `nóOsm|way:de:para.dir.idx>...` */
  connections?: ConnEdits
  connFormat?: 2
  segments?: SegEdit[]
  roads?: NewRoad[]
  removed?: SegKeyTuple[]
}

const K_MAP = 'last-map'
const K_SIGS = 'sig-config'
const K_CONNS = 'conn-edits-v2'
const K_SEGS = 'seg-edits'
const K_ROADS = 'new-roads'
const K_REMOVED = 'removed-segs'

const safe = async <T>(f: () => Promise<T>, fallback: T): Promise<T> => {
  try {
    return await f()
  } catch {
    return fallback
  }
}

export const saveMap = (name: string, buffer: ArrayBuffer) => safe(() => set(K_MAP, { name, buffer }), undefined)
export const loadMap = () => safe(() => get<{ name: string; buffer: ArrayBuffer }>(K_MAP), undefined)
export const saveSigConfigs = (e: SigConfigs) => safe(() => set(K_SIGS, e), undefined)
export const loadSigConfigs = () => safe(async () => (await get<SigConfigs>(K_SIGS)) ?? {}, {} as SigConfigs)
export const saveConnEdits = (e: ConnEdits) => safe(() => set(K_CONNS, e), undefined)
export const loadConnEdits = () => safe(async () => (await get<ConnEdits>(K_CONNS)) ?? {}, {} as ConnEdits)
export const saveSegEdits = (e: SegEdits) => safe(() => set(K_SEGS, e), undefined)
export const loadSegEdits = () => safe(async () => (await get<SegEdits>(K_SEGS)) ?? {}, {} as SegEdits)
export const saveRoads = (r: NewRoad[], removed: SegKeyTuple[]) =>
  safe(async () => (await set(K_ROADS, r), await set(K_REMOVED, removed)), undefined)
export const loadRoads = () => safe(async () => (await get<NewRoad[]>(K_ROADS)) ?? [], [] as NewRoad[])
export const loadRemoved = () => safe(async () => (await get<SegKeyTuple[]>(K_REMOVED)) ?? [], [] as SegKeyTuple[])
export const clearAll = () =>
  safe(async () => {
    for (const k of [K_MAP, K_SIGS, K_CONNS, K_SEGS, K_ROADS, K_REMOVED, 'conn-edits', 'signal-edits']) await del(k)
  }, undefined)

/** Identidade estável de uma faixa: trecho (way:de:para) + sentido + posição. */
export function laneKey(d: NetworkData, lane: number): string {
  const seg = d.laneU32[lane * 6]
  const k = seg * 3
  return `${d.segKeys[k]}:${d.segKeys[k + 1]}:${d.segKeys[k + 2]}.${d.laneU32[lane * 6 + 1]}.${d.laneU32[lane * 6 + 2]}`
}

export const connKey = (d: NetworkData, c: number) =>
  `${d.nodeOsm[d.connU32[c * 5]]}|${laneKey(d, d.connU32[c * 5 + 1])}>${laneKey(d, d.connU32[c * 5 + 2])}`

/** Aplica edições de conversão (ignora as que não existem mais). Devolve quantas foram aplicadas. */
export function applyConnEdits(d: NetworkData, edits: ConnEdits): number {
  const byNode = new Map<number, Map<string, number>>()
  const byOsm = new Map<number, number>()
  for (const [k, v] of Object.entries(edits)) {
    const osm = Number(k.slice(0, k.indexOf('|')))
    if (!byOsm.size) for (let n = 0; n < d.nodeOsm.length; n++) byOsm.set(d.nodeOsm[n], n)
    const n = byOsm.get(osm)
    if (n == null) continue
    const m = byNode.get(n) ?? new Map<string, number>()
    m.set(k, v)
    byNode.set(n, m)
  }
  let applied = 0
  for (const [n, m] of byNode) {
    for (let c = d.connOff[n]; c < d.connOff[n + 1]; c++) {
      const v = m.get(connKey(d, c))
      if (v != null) {
        d.connU32[c * 5 + 4] = v
        applied++
      }
    }
  }
  return applied
}

/** Configuração completa de um semáforo a partir do estado atual (para salvar/reaplicar). */
export function configFromDef(def: SignalDef): SigConfig {
  const keyOf = new Map(def.ends.map((e) => [e.end, e.key]))
  return {
    node: def.osm,
    enabled: true,
    offset: def.offset,
    phases: def.phases.map((p) => ({
      green: p.green,
      yellow: p.yellow,
      allRed: p.allRed,
      approaches: p.ends.map((e) => keyOf.get(e)).filter((k): k is string => k != null),
    })),
  }
}

export function exportScenario(name: string, sigs: SigConfigs, connections: ConnEdits, segments: SegEdits, roads: NewRoad[], removed: SegKeyTuple[]) {
  const file: ScenarioFile = {
    version: 1,
    app: 'simulador-transito',
    map: { name },
    signalConfigs: Object.values(sigs),
    connections,
    connFormat: 2,
    segments: Object.values(segments),
    roads,
    removed,
  }
  const blob = new Blob([JSON.stringify(file, null, 2)], { type: 'application/json' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `cenario-${name.replace(/\.[^.]+$/, '')}.json`
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}
