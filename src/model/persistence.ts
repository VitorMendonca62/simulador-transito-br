import { del, get, set } from 'idb-keyval'
import type { NetworkData, SignalDef } from './types'

export interface PhaseEdit {
  green: number
  yellow: number
  allRed: number
}
/** Edições de semáforo indexadas pelo id OSM do nó (estável entre importações). */
export type SignalEdits = Record<string, PhaseEdit[]>

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
export const segEditKey = (e: { way: number; from: number; to: number }) => `${e.way}:${e.from}:${e.to}`

export interface ScenarioFile {
  version: 1
  app: 'simulador-transito'
  map: { name: string }
  signals: SignalEdits
  /** formato 2: chaves estáveis `nóOsm|way:de:para.dir.idx>...` */
  connections?: ConnEdits
  connFormat?: 2
  segments?: SegEdit[]
}

const K_MAP = 'last-map'
const K_EDITS = 'signal-edits'
const K_CONNS = 'conn-edits-v2'
const K_SEGS = 'seg-edits'

const safe = async <T>(f: () => Promise<T>, fallback: T): Promise<T> => {
  try {
    return await f()
  } catch {
    return fallback
  }
}

export const saveMap = (name: string, buffer: ArrayBuffer) => safe(() => set(K_MAP, { name, buffer }), undefined)
export const loadMap = () => safe(() => get<{ name: string; buffer: ArrayBuffer }>(K_MAP), undefined)
export const saveEdits = (e: SignalEdits) => safe(() => set(K_EDITS, e), undefined)
export const loadEdits = () => safe(async () => (await get<SignalEdits>(K_EDITS)) ?? {}, {} as SignalEdits)
export const saveConnEdits = (e: ConnEdits) => safe(() => set(K_CONNS, e), undefined)
export const loadConnEdits = () => safe(async () => (await get<ConnEdits>(K_CONNS)) ?? {}, {} as ConnEdits)
export const saveSegEdits = (e: SegEdits) => safe(() => set(K_SEGS, e), undefined)
export const loadSegEdits = () => safe(async () => (await get<SegEdits>(K_SEGS)) ?? {}, {} as SegEdits)
export const clearAll = () =>
  safe(async () => {
    for (const k of [K_MAP, K_EDITS, K_CONNS, K_SEGS, 'conn-edits']) await del(k)
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

export function collectEdits(data: NetworkData, signals: SignalDef[]): SignalEdits {
  const out: SignalEdits = {}
  for (const s of signals) {
    out[String(data.nodeOsm[s.node])] = s.phases.map((p) => ({ green: p.green, yellow: p.yellow, allRed: p.allRed }))
  }
  return out
}

/** Aplica tempos salvos aos semáforos padrão (só quando o nº de fases coincide). */
export function applyEdits(data: NetworkData, edits: SignalEdits): SignalDef[] {
  return data.meta.signals.map((s) => {
    const e = edits[String(data.nodeOsm[s.node])]
    if (!e || e.length !== s.phases.length) return s
    return { ...s, phases: s.phases.map((p, i) => ({ ...p, green: e[i].green, yellow: e[i].yellow, allRed: e[i].allRed })) }
  })
}

export function exportScenario(name: string, data: NetworkData, signals: SignalDef[], connections: ConnEdits, segments: SegEdits) {
  const file: ScenarioFile = {
    version: 1,
    app: 'simulador-transito',
    map: { name },
    signals: collectEdits(data, signals),
    connections,
    connFormat: 2,
    segments: Object.values(segments),
  }
  const blob = new Blob([JSON.stringify(file, null, 2)], { type: 'application/json' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `cenario-${name.replace(/\.[^.]+$/, '')}.json`
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}
