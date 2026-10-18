/** Dados da rede devolvidos pelo núcleo WASM (todos em metros locais, Y para baixo). */

export interface MeshData {
  pos: Float32Array
  idx: Uint32Array
}

/** Conteúdo desenhável de um ladrilho (malhas locais + pontos de nós). */
export interface TileData {
  id: number
  asphalt: MeshData
  bus: MeshData
  white: MeshData
  yellow: MeshData
  /** vértices intermediários das vias, nós menores e cruzamentos (xy) */
  shape: Float32Array
  minor: Float32Array
  major: Float32Array
}
export interface TileUpdate {
  dirty: TileData[]
  removed: number[]
}

/** Conexões de um único nó, geradas sob demanda pelo núcleo (layout dos antigos arrays globais). */
export interface NodeConns {
  node: number
  n: number
  /** [nó, faixaOrigem, faixaDestino, tipo, estado] x n */
  u32: Uint32Array
  /** Bézier x8 por conexão */
  f32: Float32Array
  /** estado padrão (antes das edições) por conexão */
  base: Uint8Array
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
  /** chave estável da aproximação (way:de:para:lado) */
  key: string
  x: number
  y: number
  a: number
}

export interface SignalDef {
  node: number
  /** id OSM do nó (estável entre reconstruções) */
  osm: number
  /** defasagem do ciclo (s) */
  offset: number
  phases: SignalPhase[]
  ends: SignalEnd[]
}

export interface NetworkMeta {
  bounds: [number, number, number, number]
  /** origem da projeção: [lat_centro, lon_centro, metros por grau de longitude] */
  geo: [number, number, number]
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
  segU32: Uint32Array // [from,to,lf,lb,hw,name,flags,maxspeed,laneBase] x9
  laneU32: Uint32Array // [seg,dir,idx,from,to,bus] x6
  laneF32: Float32Array // [sx,sy,sdx,sdy,ex,ey,edx,edy] x8
  /** ladrilhos regenerados nesta (re)construção e ladrilhos que ficaram vazios */
  tiles: TileUpdate
  labels: Float32Array // [x,y,ang,len,rank,name,width] x7
  segKeys: Float64Array // [way, nóOsmInicial, nóOsmFinal] x3, orientação original
  segW: Float32Array // largura da faixa (m) por trecho
  segCuts: Float32Array // recuos [início, fim] x2 por trecho (m)
  segPts: Float32Array // eixo dos trechos (xy), offsets em segPtsOff
  segPtsOff: Uint32Array
}

export const SEG_STRIDE = 9
export const LANE_STRIDE = 6
export const CONN_STRIDE = 5

export const KIND_LEFT = 0
export const KIND_STRAIGHT = 1
export const KIND_RIGHT = 2
export const KIND_UTURN = 3

export type WorkerIn =
  | { type: 'import'; buffer: ArrayBuffer; name: string; edits?: string }
  | { type: 'edit'; edits: string }
  | { type: 'demand'; id: number; json: string }
  | { type: 'nodeConns'; id: number; node: number }
  | { type: 'connEdits'; json: string }
  | { type: 'prioEdits'; json: string }
  | { type: 'approaches'; id: number; node: number }
  | { type: 'simInit'; json: string }
  | { type: 'simRun'; running: boolean; speed: number }
  | { type: 'simStop' }
  /** a UI aplicou e pintou o último quadro da simulação: o worker pode enviar o próximo */
  | { type: 'simAck' }
  | { type: 'simHeat'; kind: number }
  | { type: 'simTrack'; veh: number | null }
  | { type: 'simStepBy'; seconds: number }
  | { type: 'simStuck'; id: number; n: number }
  | { type: 'simHist'; id: number; bucket: number }
  | { type: 'bench'; id: number; edits: string; sim: string; minutes: number }
  | { type: 'benchCancel' }
export type WorkerOut =
  | { type: 'done'; data: NetworkData; ms: number }
  | { type: 'error'; message: string }
  | { type: 'sim'; time: number; stats: SimStats; buf: Float32Array; heat?: Uint8Array; series?: Float32Array; histLen?: number; track?: string }
  | { type: 'benchProgress'; id: number; done: number }
  | { type: 'benchDone'; id: number; result: BenchResult }
  | { type: 'benchError'; id: number; message: string }
  | { type: 'simEnded'; reason: string }
  | { type: 'simReady'; error?: string }
  /** uma vez por segundo: tempo acumulado calculando a simulação e memória do WASM */
  | { type: 'perf'; busyMs: number; wasmBytes: number }
  | { type: 'result'; id: number; json?: string; u32?: Uint32Array; f32?: Float32Array; u8?: Uint8Array; error?: string }

/** Estado de uma conexão de faixa (connU32[c*5+4]). */
export const CONN_BLOCKED = 0 // proibida por restrição OSM (vermelho)
export const CONN_ALLOWED = 1 // ligada
export const CONN_ABSENT = 2 // candidata não ligada (só aparece ao arrastar)

/** Estatísticas da simulação (1 Hz). */
export interface SimStats {
  time: number
  alive: number
  spawned: number
  finished: number
  lost: number
  queued: number
  meanTravel: number
  redRuns: number
  laneChanges: number
  illegalMoves?: number
  /** trocas de faixa na linha contínua antes do nó (preso na fila da faixa errada) */
  solidCrossings?: number
  /** freadas acima de 5 m/s² */
  harshBrakes?: number
  /** viagens da matriz que não nasceram (sem caminho da origem ao destino) */
  skipped?: number
}

/** Resultado de uma execução headless de um cenário (comparação A/B). */
export interface BenchResult {
  report: SimReport
  /** por minuto: t, vivos, concluídos no minuto, velocidade média (km/h), fila de entrada, 0 */
  series: Float32Array
  /** razão média de velocidade por trecho direcionado (0..254, 255 sem dados) */
  ratio: Uint8Array
  /** [way, nóInicial, nóFinal] por trecho (para alinhar com outro cenário) */
  segKeys: Float64Array
  minutes: number
}

export interface SimReport {
  elapsed: number
  spawned: number
  trips: number
  alive: number
  queued: number
  meanTravel: number
  meanDelay: number
  totalDelayVehH: number
  aliveDelayVehH: number
  queueDelayVehH: number
  totalDelayAllVehH: number
  meanDelayAll: number
  meanSpeedKmh: number
  throughputPerH: number
  maxQueue: number
  od: { from: number; to: number; n: number; meanTravel: number; meanDelay: number }[]
  junctions: { node: number; osm: number; delayVehH: number; queueMax: number }[]
}

/** Estado detalhado de um veículo (depuração), vindo de `Sim::explain`. */
export interface VehicleInfo {
  id: number
  kind: string
  origin: number
  dest: number
  targetNode: number
  where: string
  way: number
  street: string
  laneIdx: number
  s: number
  len: number
  rest: number
  front: boolean
  v: number
  vDesired: number
  acc: number
  wait: number
  leader: { id: number; gap: number; v: number; wait: number } | null
  choice: string | { conn: number; kind: string; len: number; pen: number; toLane: number; toIdx: number }
  enter: { ok: boolean; code: number; reason: string; blocker: number } | null
  signal: string
  redAge: number
  rank: number
  toNode: number
  laneChangeCooldown: number
  laneChangeBlock: string
  costOwn: number | null
  lanes: { idx: number; here: boolean; cost: number | null; conn: string | null }[]
  ruleBreaker: boolean
  elapsed: number
  freeTime: number
  remainingFree: number | null
  delay: number
}
