import type { SegEdit } from './persistence'
import { segEditKey } from './persistence'
import type { NetworkData } from './types'
import { LANE_STRIDE, SEG_STRIDE } from './types'
import { HIGHWAY_NAMES } from './inspect'

export const MAX_LANES = 8
export const MIN_W = 2.4
export const MAX_W = 4.5

export type SegPatch =
  | { kind: 'lanes'; lf?: number; lb?: number }
  | { kind: 'oneway'; value: boolean }
  | { kind: 'width'; value: number }
  | { kind: 'bus'; dir: 0 | 1; idx: number; value: boolean }
  | { kind: 'reverse' }

export interface SegInfo {
  index: number
  name: string
  highway: string
  roundabout: boolean
  maxspeed: number
  way: number
  edited: boolean
  edit: SegEdit
  lengthM: number
}

const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v))

/** Configuração atual do trecho (lida dos dados já reconstruídos). */
export function currentEdit(d: NetworkData, seg: number): SegEdit {
  const o = seg * SEG_STRIDE
  const lf = d.segU32[o + 2]
  const lb = d.segU32[o + 3]
  const base = d.segU32[o + 8]
  const bus = (lane: number) => d.laneU32[lane * LANE_STRIDE + 5] === 1
  return {
    way: d.segKeys[seg * 3],
    from: d.segKeys[seg * 3 + 1],
    to: d.segKeys[seg * 3 + 2],
    oneway: (d.segU32[o + 6] & 1) === 1,
    lf,
    lb,
    w: Math.round(d.segW[seg] * 100) / 100,
    busF: Array.from({ length: lf }, (_, i) => bus(base + i)),
    busB: Array.from({ length: lb }, (_, i) => bus(base + lf + i)),
    reverse: false,
  }
}

export function segInfo(d: NetworkData, seg: number, record: Record<string, SegEdit>): SegInfo {
  const o = seg * SEG_STRIDE
  const cur = currentEdit(d, seg)
  const saved = record[segEditKey(cur)]
  const nameIdx = d.segU32[o + 5]
  const a = d.segPtsOff[seg]
  const b = d.segPtsOff[seg + 1]
  let len = 0
  for (let i = a + 1; i < b; i++) len += Math.hypot(d.segPts[i * 2] - d.segPts[i * 2 - 2], d.segPts[i * 2 + 1] - d.segPts[i * 2 - 1])
  return {
    index: seg,
    name: nameIdx === 0xffffffff ? 'Sem nome' : d.meta.names[nameIdx],
    highway: HIGHWAY_NAMES[d.segU32[o + 4]] ?? '?',
    roundabout: (d.segU32[o + 6] & 2) !== 0,
    maxspeed: d.segU32[o + 7],
    way: cur.way,
    edited: saved != null,
    edit: saved ?? cur,
    lengthM: len,
  }
}

export function applyPatch(cur: SegEdit, p: SegPatch): SegEdit {
  const e: SegEdit = { ...cur, busF: [...cur.busF], busB: [...cur.busB] }
  const fit = () => {
    e.lf = clamp(Math.round(e.lf), 1, MAX_LANES)
    e.lb = e.oneway ? 0 : clamp(Math.round(e.lb), 1, MAX_LANES)
    e.busF = Array.from({ length: e.lf }, (_, i) => e.busF[i] ?? false)
    e.busB = Array.from({ length: e.lb }, (_, i) => e.busB[i] ?? false)
  }
  switch (p.kind) {
    case 'lanes':
      if (p.lf != null) e.lf = p.lf
      if (p.lb != null) e.lb = p.lb
      break
    case 'oneway':
      e.oneway = p.value
      if (!p.value && e.lb < 1) e.lb = 1
      break
    case 'width':
      e.w = clamp(Math.round(p.value * 10) / 10, MIN_W, MAX_W)
      break
    case 'bus':
      ;(p.dir === 0 ? e.busF : e.busB)[p.idx] = p.value
      break
    case 'reverse':
      e.reverse = !e.reverse
      if (!e.oneway) {
        ;[e.lf, e.lb] = [e.lb, e.lf]
        ;[e.busF, e.busB] = [e.busB, e.busF]
      }
      break
  }
  fit()
  return e
}

/** Todos os trechos que pertencem à mesma via (way OSM) do trecho dado. */
export function segsOfWay(d: NetworkData, seg: number): number[] {
  const way = d.segKeys[seg * 3]
  const out: number[] = []
  for (let i = 0; i < d.segKeys.length / 3; i++) if (d.segKeys[i * 3] === way) out.push(i)
  return out
}
