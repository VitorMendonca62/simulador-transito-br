/// <reference lib="webworker" />
import init, { import_osm, type Network } from '../../core/pkg/core.js'
import type { NetworkData, NetworkMeta, WorkerIn, WorkerOut } from '../model/types'

let ready: Promise<unknown> | null = null
// A rede fica viva no worker (fases futuras: edição e simulação rodam aqui).
let current: Network | null = null

function extract(net: Network): NetworkData {
  const mesh = (pos: Float32Array, idx: Uint32Array) => ({ pos, idx })
  return {
    meta: JSON.parse(net.meta_json()) as NetworkMeta,
    nodeXY: net.node_xy(),
    nodeInfo: net.node_info(),
    nodeOsm: net.node_osm_ids(),
    nodeEnds: net.node_ends(),
    nodeEndOff: net.node_end_off(),
    shapeXY: net.shape_xy(),
    segU32: net.seg_u32(),
    laneU32: net.lane_u32(),
    laneF32: net.lane_f32(),
    connU32: net.conn_u32(),
    connF32: net.conn_f32(),
    connOff: net.conn_off(),
    asphalt: mesh(net.asphalt_pos(), net.asphalt_idx()),
    bus: mesh(net.bus_pos(), net.bus_idx()),
    white: mesh(net.white_pos(), net.white_idx()),
    yellow: mesh(net.yellow_pos(), net.yellow_idx()),
    labels: net.labels_f32(),
  }
}

function transferables(d: NetworkData): ArrayBuffer[] {
  const out: ArrayBuffer[] = []
  for (const v of Object.values(d)) {
    if (ArrayBuffer.isView(v)) out.push(v.buffer as ArrayBuffer)
    else if (v && typeof v === 'object' && 'pos' in v) {
      out.push((v as { pos: Float32Array }).pos.buffer as ArrayBuffer, (v as { idx: Uint32Array }).idx.buffer as ArrayBuffer)
    }
  }
  return out
}

self.onmessage = async (ev: MessageEvent<WorkerIn>) => {
  const msg = ev.data
  if (msg.type !== 'import') return
  const post = (m: WorkerOut, t: Transferable[] = []) => (self as unknown as Worker).postMessage(m, t)
  try {
    ready ??= init()
    await ready
    const t0 = performance.now()
    current?.free()
    current = import_osm(new Uint8Array(msg.buffer))
    const data = extract(current)
    post({ type: 'done', data, ms: performance.now() - t0 }, transferables(data))
  } catch (e) {
    post({ type: 'error', message: e instanceof Error ? e.message : String(e) })
  }
}
