import type { NetworkData, WorkerIn, WorkerOut } from './types'

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
