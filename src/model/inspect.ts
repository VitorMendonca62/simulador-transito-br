import type { NetworkData, NodeConns } from './types'
import { CONN_ABSENT, CONN_ALLOWED, CONN_STRIDE, LANE_STRIDE, SEG_STRIDE } from './types'

export const HIGHWAY_NAMES = ['Autoestrada', 'Via expressa', 'Primária', 'Secundária', 'Terciária', 'Local', 'Residencial', 'Zona calma', 'Serviço', 'Corredor de ônibus']
export const KIND_LABEL = ['Esquerda', 'Em frente', 'Direita', 'Retorno']
export const KIND_ARROW = ['↰', '↑', '↱', '↶']

export interface LaneInfo {
  lane: number
  index: number
  bus: boolean
  kinds: { kind: number; allowed: boolean; conns: number[] }[]
}
export interface ApproachInfo {
  end: number
  street: string
  highway: string
  roundabout: boolean
  lanes: LaneInfo[]
  outLanes: number
}
export interface NodeInfo {
  id: number
  osmId: number
  degree: number
  signal: boolean
  crossing: boolean
  allowed: number
  blocked: number
  approaches: ApproachInfo[]
}

export function describeNode(d: NetworkData, node: number, nc: NodeConns | null): NodeInfo {
  const flags = d.nodeInfo[node * 2 + 1]
  const n = nc?.n ?? 0
  const byLane = new Map<number, { kind: number; allowed: boolean; conns: number[] }[]>()
  let allowed = 0
  let blocked = 0
  for (let c = 0; c < n; c++) {
    const o = c * CONN_STRIDE
    const lane = nc!.u32[o + 1]
    const st = nc!.u32[o + 4]
    if (st === CONN_ABSENT) continue
    const ok = st === CONN_ALLOWED
    allowed += ok ? 1 : 0
    blocked += ok ? 0 : 1
    const arr = byLane.get(lane) ?? []
    const kind = nc!.u32[o + 3]
    const ex = arr.find((k) => k.kind === kind)
    if (ex) {
      ex.conns.push(c)
      ex.allowed = ex.allowed || ok
    } else arr.push({ kind, allowed: ok, conns: [c] })
    byLane.set(lane, arr)
  }
  const approaches: ApproachInfo[] = []
  for (let i = d.nodeEndOff[node]; i < d.nodeEndOff[node + 1]; i++) {
    const end = d.nodeEnds[i]
    const so = (end >> 1) * SEG_STRIDE
    const lf = d.segU32[so + 2]
    const lb = d.segU32[so + 3]
    const base = d.segU32[so + 8]
    const atTo = (end & 1) === 1
    const inStart = atTo ? base : base + lf
    const inCount = atTo ? lf : lb
    const outCount = atTo ? lb : lf
    const nameIdx = d.segU32[so + 5]
    const flagsSeg = d.segU32[so + 6]
    const lanes: LaneInfo[] = []
    for (let k = 0; k < inCount; k++) {
      const lane = inStart + k
      lanes.push({ lane, index: k, bus: d.laneU32[lane * LANE_STRIDE + 5] === 1, kinds: byLane.get(lane) ?? [] })
    }
    approaches.push({
      end,
      street: nameIdx === 0xffffffff ? 'Sem nome' : d.meta.names[nameIdx],
      highway: HIGHWAY_NAMES[d.segU32[so + 4]] ?? '?',
      roundabout: (flagsSeg & 2) !== 0,
      lanes,
      outLanes: outCount,
    })
  }
  return {
    id: node,
    osmId: d.nodeOsm[node],
    degree: d.nodeInfo[node * 2],
    signal: (flags & 1) === 1,
    crossing: (flags & 2) === 2,
    allowed,
    blocked,
    approaches,
  }
}
