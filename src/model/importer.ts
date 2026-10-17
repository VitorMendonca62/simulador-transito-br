import { perf } from '../engine/perf'
import type { Approach } from '../engine/SignLayer'
import type { BenchResult, NetworkData, NodeConns, SimStats, WorkerIn, WorkerOut } from './types'

let worker: Worker | null = null

function getWorker(): Worker {
  worker ??= new Worker(new URL('../workers/core.worker.ts', import.meta.url), { type: 'module' })
  return worker
}

/** Importa um .osm/.pbf no núcleo WASM (Web Worker). O buffer é copiado: o original continua utilizável. */
export function importMap(buffer: ArrayBuffer, name: string, edits?: string): Promise<{ data: NetworkData; ms: number }> {
  const copy = buffer.slice(0)
  return request({ type: 'import', buffer: copy, name, edits }, [copy])
}

/** Reconstrói a rede já carregada no worker com as edições de trecho (JSON). */
export function editNetwork(edits: string): Promise<{ data: NetworkData; ms: number }> {
  return request({ type: 'edit', edits }, [])
}

function request(msg: WorkerIn, transfer: Transferable[]): Promise<{ data: NetworkData; ms: number }> {
  const w = getWorker()
  return new Promise((resolve, reject) => {
    const onMsg = (ev: MessageEvent<WorkerOut>) => {
      if (ev.data.type !== 'done' && ev.data.type !== 'error') return // respostas avulsas, simulação etc.
      w.removeEventListener('message', onMsg)
      w.removeEventListener('error', onErr)
      if (ev.data.type === 'done') resolve({ data: ev.data.data, ms: ev.data.ms })
      else reject(new Error(ev.data.message))
    }
    const onErr = (ev: ErrorEvent) => {
      w.removeEventListener('message', onMsg)
      w.removeEventListener('error', onErr)
      reject(new Error(ev.message || 'Falha no worker'))
    }
    w.addEventListener('message', onMsg)
    w.addEventListener('error', onErr)
    w.postMessage(msg, transfer)
  })
}

let perfAttached = false
/** Passa a registrar a ocupação do worker e a memória do WASM em `perf`. */
export function watchWorkerPerf() {
  if (perfAttached) return
  perfAttached = true
  getWorker().addEventListener('message', (ev: MessageEvent<WorkerOut>) => {
    if (ev.data.type !== 'perf') return
    perf.simBusyMs = ev.data.busyMs
    perf.wasmBytes = ev.data.wasmBytes
  })
}

let callSeq = 0
type CallMsg = { type: 'demand'; json: string } | { type: 'nodeConns'; node: number } | { type: 'approaches'; node: number } | { type: 'simHist'; bucket: number } | { type: 'simStuck'; n: number }
type CallResult = Extract<WorkerOut, { type: 'result' }>

/** Chamada avulsa ao núcleo (demanda, conexões do nó…); várias podem estar em voo. */
function call(msg: CallMsg): Promise<CallResult> {
  const w = getWorker()
  const id = ++callSeq
  return new Promise((resolve, reject) => {
    const onMsg = (ev: MessageEvent<WorkerOut>) => {
      if (ev.data.type !== 'result' || ev.data.id !== id) return
      w.removeEventListener('message', onMsg)
      if (ev.data.error) reject(new Error(ev.data.error))
      else resolve(ev.data)
    }
    w.addEventListener('message', onMsg)
    w.postMessage({ ...msg, id } as WorkerIn)
  })
}

/** Resolve zonas sobre a rede e devolve a matriz OD (veículos/h na hora pedida). */
export async function resolveDemand(payload: unknown): Promise<DemandResult> {
  return JSON.parse((await call({ type: 'demand', json: JSON.stringify(payload) })).json ?? '{}') as DemandResult
}

/** Conexões do nó (padrão, candidatas, proibidas e edições do usuário), geradas sob demanda no núcleo. */
export async function fetchNodeConns(node: number): Promise<NodeConns> {
  const r = await call({ type: 'nodeConns', node })
  const a = r.u32 ?? new Uint32Array(0)
  const n = a.length / 5
  const u32 = new Uint32Array(n * 5)
  const base = new Uint8Array(n)
  for (let c = 0; c < n; c++) {
    u32[c * 5] = node
    u32[c * 5 + 1] = a[c * 5]
    u32[c * 5 + 2] = a[c * 5 + 1]
    u32[c * 5 + 3] = a[c * 5 + 2]
    u32[c * 5 + 4] = a[c * 5 + 3]
    base[c] = a[c * 5 + 4]
  }
  return { node, n, u32, f32: r.f32 ?? new Float32Array(0), base }
}

/** Ramos do nó com faixa de entrada, para as placas de preferência. */
export async function fetchApproaches(node: number): Promise<Approach[]> {
  return JSON.parse((await call({ type: 'approaches', node })).json ?? '[]') as Approach[]
}

/** Atualiza no núcleo as placas de preferência (valem a partir da próxima simulação). */
export function sendPrioEdits(record: Record<string, number>) {
  getWorker().postMessage({ type: 'prioEdits', json: JSON.stringify(record) } satisfies WorkerIn)
}

/** Atualiza no núcleo as conversões editadas (sem reconstruir a rede). */
export function sendConnEdits(record: Record<string, number>) {
  getWorker().postMessage({ type: 'connEdits', json: JSON.stringify(record) } satisfies WorkerIn)
}

export interface DemandResult {
  zones: { id: number; nodes: number; roadM: number; centroid: [number, number]; genNow: number; warning: string | null }[]
  matrix: number[][]
}

// ------------------------------------------------------------------ simulação

export interface SimFrame {
  time: number
  stats: SimStats
  /** por veículo: x, y, rumo, velocidade, tipo (0 carro / 1 ônibus), comprimento */
  buf: Float32Array
}

let simListener: ((m: Extract<WorkerOut, { type: 'sim' | 'simEnded' }>) => void) | null = null
let simAttached = false

function attachSim() {
  if (simAttached) return
  simAttached = true
  getWorker().addEventListener('message', (ev: MessageEvent<WorkerOut>) => {
    if (ev.data.type !== 'sim' && ev.data.type !== 'simEnded') return
    const t0 = performance.now()
    simListener?.(ev.data)
    perf.uiMs += performance.now() - t0
    // confirma depois do próximo quadro pintado: o worker só manda outro quando a UI deu conta do anterior
    if (ev.data.type === 'sim') requestAnimationFrame(() => getWorker().postMessage({ type: 'simAck' } satisfies WorkerIn))
  })
}

/** Registra o receptor dos quadros da simulação (um só). */
export function onSim(fn: (m: Extract<WorkerOut, { type: 'sim' | 'simEnded' }>) => void) {
  attachSim()
  simListener = fn
}

/** Inicia (pausada) uma simulação nova no núcleo. */
export function simStart(payload: unknown): Promise<void> {
  const w = getWorker()
  attachSim()
  return new Promise((resolve, reject) => {
    const onMsg = (ev: MessageEvent<WorkerOut>) => {
      if (ev.data.type !== 'simReady') return
      w.removeEventListener('message', onMsg)
      if (ev.data.error) reject(new Error(ev.data.error))
      else resolve()
    }
    w.addEventListener('message', onMsg)
    w.postMessage({ type: 'simInit', json: JSON.stringify(payload) } satisfies WorkerIn)
  })
}

export const simRun = (running: boolean, speed: number) => getWorker().postMessage({ type: 'simRun', running, speed } satisfies WorkerIn)
export const simTrack = (veh: number | null) => getWorker().postMessage({ type: 'simTrack', veh } satisfies WorkerIn)
/** Com a simulação pausada, avança `seconds` s simulados (depuração passo a passo). */
export const simStepBy = (seconds: number) => getWorker().postMessage({ type: 'simStepBy', seconds } satisfies WorkerIn)

export interface StuckItem {
  id: number
  wait: number
  ok: boolean
  reason: string
  code: number
  blocker: number
  street: string
  signal: number
}

/** Veículos na frente de faixa parados há mais tempo, com o motivo. */
export async function fetchStuck(n = 12): Promise<StuckItem[]> {
  const r = await call({ type: 'simStuck', n })
  return r.json ? (JSON.parse(r.json) as StuckItem[]) : []
}

export const simStop = () => getWorker().postMessage({ type: 'simStop' } satisfies WorkerIn)

/** Tipo de mapa de calor pedido ao núcleo: 0 velocidade, 1 fila, -1 desligado. */
export const simHeatKind = (kind: number) => getWorker().postMessage({ type: 'simHeat', kind } satisfies WorkerIn)

/** Razão de velocidade por trecho direcionado no balde `bucket` (replay). */
export async function fetchHist(bucket: number): Promise<Uint8Array> {
  return (await call({ type: 'simHist', bucket })).u8 ?? new Uint8Array(0)
}

let benchSeq = 0

/** Executa um cenário sem desenhar; devolve progresso (0..1) e o resultado. */
export function runBench(edits: string, sim: string, minutes: number, onProgress: (p: number) => void): { promise: Promise<BenchResult>; cancel: () => void } {
  const w = getWorker()
  const id = ++benchSeq
  let off: () => void = () => {}
  const promise = new Promise<BenchResult>((resolve, reject) => {
    const onMsg = (ev: MessageEvent<WorkerOut>) => {
      const m = ev.data
      if (m.type === 'benchProgress' && m.id === id) onProgress(m.done)
      else if (m.type === 'benchDone' && m.id === id) {
        off()
        resolve(m.result)
      } else if (m.type === 'benchError' && m.id === id) {
        off()
        reject(new Error(m.message))
      }
    }
    off = () => w.removeEventListener('message', onMsg)
    w.addEventListener('message', onMsg)
    w.postMessage({ type: 'bench', id, edits, sim, minutes } satisfies WorkerIn)
  })
  return { promise, cancel: () => w.postMessage({ type: 'benchCancel' } satisfies WorkerIn) }
}
