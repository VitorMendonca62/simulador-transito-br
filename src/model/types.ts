/** Dados da rede devolvidos pelo núcleo WASM (todos em metros locais, Y para baixo). */

export interface MeshData {
  pos: Float32Array
  idx: Uint32Array
}

export interface SignalPhase {
  green: number
  yellow: number
  allRed: number
  /** ids de ponta (seg*2 + 0 início, seg*2 + 1 fim) que recebem verde nesta fase */
  ends: number[]
}

export interface SignalEnd {
  end: number
  x: number
  y: number
  a: number
}

export interface SignalDef {
  node: number
  phases: SignalPhase[]
  ends: SignalEnd[]
}

export interface NetworkMeta {
  bounds: [number, number, number, number]
  counts: { nodes: number; segments: number; lanes: number; connections: number; signals: number }
  names: string[]
  signals: SignalDef[]
}

export interface NetworkData {
  meta: NetworkMeta
  nodeXY: Float32Array
  nodeInfo: Uint32Array // [grau, flags(bit0 semáforo, bit1 travessia)]
  nodeOsm: Float64Array
  nodeEnds: Uint32Array
  nodeEndOff: Uint32Array
  shapeXY: Float32Array
  segU32: Uint32Array // [from,to,lf,lb,hw,name,flags,maxspeed,laneBase] x9
  laneU32: Uint32Array // [seg,dir,idx,from,to,bus] x6
  laneF32: Float32Array // [sx,sy,sdx,sdy,ex,ey,edx,edy] x8
  connU32: Uint32Array // [node,fromLane,toLane,kind,allowed] x5
  connF32: Float32Array // bézier x8
  connOff: Uint32Array
  asphalt: MeshData
  bus: MeshData
  white: MeshData
  yellow: MeshData
  labels: Float32Array // [x,y,ang,len,rank,name,width] x7
}

export const SEG_STRIDE = 9
export const LANE_STRIDE = 6
export const CONN_STRIDE = 5

export const KIND_LEFT = 0
export const KIND_STRAIGHT = 1
export const KIND_RIGHT = 2
export const KIND_UTURN = 3

export type WorkerIn = { type: 'import'; buffer: ArrayBuffer; name: string }
export type WorkerOut =
  | { type: 'done'; data: NetworkData; ms: number }
  | { type: 'error'; message: string }
