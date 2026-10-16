import { del, get, set } from 'idb-keyval'
import type { NetworkData, SignalDef } from './types'

export interface PhaseEdit {
  green: number
  yellow: number
  allRed: number
}
/** Edições de semáforo indexadas pelo id OSM do nó (estável entre importações). */
export type SignalEdits = Record<string, PhaseEdit[]>

/** Conversões alteradas: "osmDoNó:faixaOrigem:faixaDestino" -> 1 permitida / 0 proibida. */
export type ConnEdits = Record<string, 0 | 1>

export interface ScenarioFile {
  version: 1
  app: 'simulador-transito'
  map: { name: string }
  signals: SignalEdits
  connections?: ConnEdits
}

const K_MAP = 'last-map'
const K_EDITS = 'signal-edits'
const K_CONNS = 'conn-edits'

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
export const clearAll = () => safe(async () => (await del(K_MAP), await del(K_EDITS), await del(K_CONNS)), undefined)

export const connKey = (data: NetworkData, c: number) =>
  `${data.nodeOsm[data.connU32[c * 5]]}:${data.connU32[c * 5 + 1]}:${data.connU32[c * 5 + 2]}`

/** Compara as permissões atuais com as originais e devolve só o que mudou. */
export function collectConnEdits(data: NetworkData, original: Uint8Array): ConnEdits {
  const out: ConnEdits = {}
  for (let c = 0; c < original.length; c++) {
    const now = data.connU32[c * 5 + 4] as 0 | 1
    if (now !== original[c]) out[connKey(data, c)] = now
  }
  return out
}

/** Aplica edições salvas (ignora conversões que não existem mais). Devolve quantas foram aplicadas. */
export function applyConnEdits(data: NetworkData, edits: ConnEdits): number {
  const keys = Object.keys(edits)
  if (keys.length === 0) return 0
  const byOsm = new Map<number, number>()
  for (let n = 0; n < data.nodeOsm.length; n++) byOsm.set(data.nodeOsm[n], n)
  let applied = 0
  for (const k of keys) {
    const [osm, from, to] = k.split(':').map(Number)
    const n = byOsm.get(osm)
    if (n == null) continue
    for (let c = data.connOff[n]; c < data.connOff[n + 1]; c++) {
      if (data.connU32[c * 5 + 1] === from && data.connU32[c * 5 + 2] === to) {
        data.connU32[c * 5 + 4] = edits[k]
        applied++
        break
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

export function exportScenario(name: string, data: NetworkData, signals: SignalDef[], connections: ConnEdits) {
  const file: ScenarioFile = { version: 1, app: 'simulador-transito', map: { name }, signals: collectEdits(data, signals), connections }
  const blob = new Blob([JSON.stringify(file, null, 2)], { type: 'application/json' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `cenario-${name.replace(/\.[^.]+$/, '')}.json`
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}
