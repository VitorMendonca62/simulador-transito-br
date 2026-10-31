/// <reference lib="webworker" />
import init, { import_osm, import_osm_signals, type Network } from '../../core/pkg/core.js'
import type { NetworkData, NetworkMeta, RouterIn, RouterOut, SimStats, TileData, TileUpdate, WorkerIn, WorkerOut } from '../model/types'

let ready: Promise<unknown> | null = null
// A rede fica viva no worker (fases futuras: edição e simulação rodam aqui).
let current: Network | null = null
/** bytes do mapa importado, para criar redes extras (execução headless de cenários) */
let sourceBytes: Uint8Array | null = null

/**
 * Semáforos oficiais da CTTU (gerados por scripts/semaforos_recife/gerar_semaforos.py). Quando as edições pedem
 * `signalSource: "cttu"`, substituem os `highway=traffic_signals` do OSM; fora do Recife nenhum ponto cai no mapa
 * e ficam os do OSM.
 */
const CTTU_URL = `${import.meta.env.BASE_URL}data/semaforos_cttu.json`
let cttuSignals: Promise<string> | null = null
const loadCttu = () =>
  (cttuSignals ??= fetch(new URL(CTTU_URL, self.location.origin)).then((r) => {
    if (!r.ok) throw new Error(`não foi possível ler ${CTTU_URL} (${r.status})`)
    return r.text()
  })).catch((e) => {
    cttuSignals = null
    throw e
  })

/** Origem dos semáforos pedida nas edições (JSON do núcleo). */
const sourceOf = (edits?: string): 'osm' | 'cttu' => (edits && /"signalSource"\s*:\s*"cttu"/.test(edits) ? 'cttu' : 'osm')
/** origem com que `current` foi montada */
let currentSource: 'osm' | 'cttu' = 'osm'

async function importNet(bytes: Uint8Array, source: 'osm' | 'cttu'): Promise<Network> {
  return source === 'cttu' ? import_osm_signals(bytes, await loadCttu()) : import_osm(bytes)
}

/** Ladrilhos sujos: buffers concatenados fatiados por visões (sem cópia), um conjunto de malhas por ladrilho. */
function extractTiles(net: Network): TileUpdate {
  const index = net.tiles_index()
  const pos = net.tiles_pos()
  const idx = net.tiles_idx()
  const dirty: TileData[] = []
  let po = 0
  let io = 0
  const takeF = (n: number) => {
    const v = pos.subarray(po, po + n)
    po += n
    return v
  }
  const takeU = (n: number) => {
    const v = idx.subarray(io, io + n)
    io += n
    return v
  }
  for (let i = 0; i < index.length; i += 20) {
    const L = index.subarray(i + 1, i + 20)
    const asphaltPos = takeF(L[0])
    const busPos = takeF(L[1])
    const whitePos = takeF(L[2])
    const yellowPos = takeF(L[3])
    const shape = takeF(L[4])
    const minor = takeF(L[5])
    const major = takeF(L[6])
    const base = { asphalt: takeU(L[7]), bus: takeU(L[8]), white: takeU(L[9]), yellow: takeU(L[10]) }
    const hiPos = [takeF(L[11]), takeF(L[12]), takeF(L[13]), takeF(L[14])]
    const hiIdx = [takeU(L[15]), takeU(L[16]), takeU(L[17]), takeU(L[18])]
    dirty.push({
      id: index[i],
      asphalt: { pos: asphaltPos, idx: base.asphalt },
      bus: { pos: busPos, idx: base.bus },
      white: { pos: whitePos, idx: base.white },
      yellow: { pos: yellowPos, idx: base.yellow },
      shape,
      minor,
      major,
      hi: {
        asphalt: { pos: hiPos[0], idx: hiIdx[0] },
        bus: { pos: hiPos[1], idx: hiIdx[1] },
        white: { pos: hiPos[2], idx: hiIdx[2] },
        yellow: { pos: hiPos[3], idx: hiIdx[3] },
      },
    })
  }
  return { dirty, removed: Array.from(net.tiles_removed()) }
}

function extract(net: Network): NetworkData {
  return {
    meta: JSON.parse(net.meta_json()) as NetworkMeta,
    nodeXY: net.node_xy(),
    nodeInfo: net.node_info(),
    nodeOsm: net.node_osm_ids(),
    nodeEnds: net.node_ends(),
    nodeEndOff: net.node_end_off(),
    segU32: net.seg_u32(),
    laneU32: net.lane_u32(),
    laneF32: net.lane_f32(),
    tiles: extractTiles(net),
    labels: net.labels_f32(),
    segKeys: net.seg_keys(),
    segW: net.seg_w(),
    segCuts: net.seg_cuts(),
    segPts: net.seg_pts(),
    segPtsOff: net.seg_pts_off(),
  }
}

function transferables(d: NetworkData): ArrayBuffer[] {
  const seen = new Set<ArrayBuffer>()
  const add = (b: ArrayBufferLike) => seen.add(b as ArrayBuffer)
  for (const [k, v] of Object.entries(d)) {
    if (k === 'tiles' || k === 'meta') continue
    if (ArrayBuffer.isView(v)) add(v.buffer)
  }
  for (const t of d.tiles.dirty) {
    for (const m of [t.asphalt, t.bus, t.white, t.yellow, t.hi.asphalt, t.hi.bus, t.hi.white, t.hi.yellow]) {
      add(m.pos.buffer)
      add(m.idx.buffer)
    }
    add(t.shape.buffer)
    add(t.minor.buffer)
    add(t.major.buffer)
  }
  return [...seen]
}

// ------------------------------------------------------------------ simulação
let simRunning = false
let simSpeed = 1
let simLast = 0
let simTimer = 0
let simPost = 0
/** instante do último quadro enviado e ainda não confirmado pela UI (0 = nenhum pendente) */
let simInFlight = 0
/** sem confirmação por tanto tempo (aba em segundo plano, UI travada) o worker volta a enviar */
const SIM_ACK_TIMEOUT = 1000
let simCarry = 0
let simStatsAt = 0
let simHeatKind = 0 // -1 desligado
let simTrackId: number | null = null
let simTrackAt = 0
const SIM_DT = 0.2
/** intervalo entre quadros enviados à UI (15 Hz); a UI interpola entre eles */
const SIM_FRAME_MS = 66

const postSim = (msg: WorkerOut, t: Transferable[] = []) => (self as unknown as Worker).postMessage(msg, t)

// ocupação do worker (tempo calculando a simulação) e memória do WASM, informadas à UI uma vez por segundo
let busyMs = 0
let wasmMem: WebAssembly.Memory | null = null
self.setInterval(() => postSim({ type: 'perf', busyMs, wasmBytes: wasmMem?.buffer.byteLength ?? 0 }), 1000)

// ------------------------------------------------------------------ rotas em outros workers
/**
 * Workers auxiliares (`router.worker.ts`) com uma cópia do roteador por faixa: calculam as árvores iniciais das zonas
 * em paralelo e os lotes das rotas em tempo real, fora do passo da simulação. O lote pedido num passo é instalado
 * um número fixo de passos depois; se ainda não chegou, a simulação espera (o resultado não depende da máquina).
 * `null` = indisponíveis: a simulação calcula tudo no próprio passo.
 */
let routers: Worker[] | null = null
let routersFailed = false
/** simulação atual (respostas de uma anterior são descartadas) */
let routerGen = 0
let routerNext = 0
/** árvores iniciais ainda por chegar (0 = pronta) */
let initPending = 0

function getRouters(): Worker[] | null {
  if (routers || routersFailed) return routers
  try {
    const n = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 2))
    routers = Array.from({ length: n }, () => {
      const w = new Worker(new URL('./router.worker.ts', import.meta.url), { type: 'module' })
      w.onmessage = onRouter
      w.onerror = (e) => dropRouters(e.message || 'falha ao carregar o worker de rotas')
      return w
    })
  } catch (e) {
    dropRouters(e instanceof Error ? e.message : String(e))
  }
  return routers
}

/** Sem workers de rotas: o que faltar é calculado no próprio passo da simulação. */
function dropRouters(why: string) {
  console.warn('rotas no próprio worker da simulação:', why)
  routers?.forEach((w) => w.terminate())
  routers = null
  routersFailed = true
  if (initPending > 0) {
    initPending = 0
    postSim({ type: 'simReady' })
  }
}

const postRouter = (w: Worker, m: RouterIn, t: Transferable[] = []) => w.postMessage(m, t)

function onRouter(ev: MessageEvent<RouterOut>) {
  const m = ev.data
  if (m.gen !== routerGen || !current?.sim_active()) return
  if (m.type === 'error') return dropRouters(m.message)
  if (m.init) {
    current.sim_init_put(m.zone, m.dist)
    if (initPending > 0 && --initPending === 0) postSim({ type: 'simReady' })
  } else current.sim_route_put(m.zone, m.dist)
}

/** Entrega aos workers de rotas o lote que a simulação acabou de pedir (zonas divididas entre eles). */
function sendRouteBatch(net: Network) {
  const zones = net.sim_route_take()
  if (!zones.length || !routers) return
  const pen = net.sim_route_pen()
  const cm = net.sim_route_cm()
  const parts = new Map<Worker, { zone: number; targets: Uint32Array }[]>()
  for (const z of zones) {
    const w = routers[routerNext++ % routers.length]
    parts.set(w, [...(parts.get(w) ?? []), { zone: z, targets: net.sim_zone_targets(z) }])
  }
  for (const [w, list] of parts) {
    const p = pen.slice()
    const c = cm.slice()
    postRouter(w, { type: 'trees', gen: routerGen, init: false, pen: p, cm: c, zones: list }, [p.buffer, c.buffer])
  }
}

/** Inicia a simulação com as árvores iniciais calculadas em paralelo pelos workers de rotas. */
function simInitExt(net: Network, json: string): boolean {
  const pool = getRouters()
  if (!pool) return false
  net.sim_init_ext(json)
  routerGen++
  const bytes = net.sim_router_bytes()
  for (const w of pool) {
    const b = bytes.slice()
    postRouter(w, { type: 'graph', gen: routerGen, bytes: b }, [b.buffer])
  }
  const nz = net.sim_zone_count()
  const parts: { zone: number; targets: Uint32Array }[][] = pool.map(() => [])
  for (let z = 0; z < nz; z++) parts[z % pool.length].push({ zone: z, targets: net.sim_zone_targets(z) })
  initPending = net.sim_init_missing()
  pool.forEach((w, k) => parts[k].length && postRouter(w, { type: 'trees', gen: routerGen, init: true, zones: parts[k] }))
  if (initPending === 0) postSim({ type: 'simReady' })
  return true
}

/** Avança a simulação por fatias de tempo real: passos fixos de 0,2 s, no máximo ~10 ms por fatia. */
function simLoop() {
  const t0 = performance.now()
  simSlice()
  busyMs += performance.now() - t0
}

function simSlice() {
  simTimer = 0
  if (!current || !simRunning) return
  if (!current.sim_active()) {
    simRunning = false
    postSim({ type: 'simEnded', reason: 'a rede foi editada' })
    return
  }
  const now = performance.now()
  simCarry += ((now - simLast) / 1000) * simSpeed
  simLast = now
  const budget = 10
  const t0 = performance.now()
  let ticks = 0
  while (simCarry >= SIM_DT && performance.now() - t0 < budget) {
    // lote de rotas ainda nos workers: espera (sem eles, o próprio passo calcula)
    if (routers && current.sim_route_wait()) break
    current.sim_step(1)
    sendRouteBatch(current)
    simCarry -= SIM_DT
    ticks++
  }
  if (simCarry > 2) simCarry = 2 // não acumula atraso indefinido
  // controle de vazão: a UI define o ritmo dos quadros; o worker continua simulando enquanto espera
  if (ticks > 0 && now - simPost > SIM_FRAME_MS && (simInFlight === 0 || now - simInFlight > SIM_ACK_TIMEOUT)) {
    simPost = now
    simInFlight = now
    const buf = current.sim_snapshot()
    let stats = lastStats
    let heat: Uint8Array | undefined
    let series: Float32Array | undefined
    let histLen: number | undefined
    if (now - simStatsAt > 1000) {
      simStatsAt = now
      stats = lastStats = JSON.parse(current.sim_stats())
      if (simHeatKind >= 0) heat = current.sim_heat(simHeatKind)
      series = current.sim_series()
      histLen = current.sim_hist_len()
    }
    let track: string | undefined
    if (simTrackId != null && now - simTrackAt > 250) {
      simTrackAt = now
      track = current.sim_track(simTrackId)
    }
    const tr: Transferable[] = [buf.buffer as ArrayBuffer]
    if (heat) tr.push(heat.buffer as ArrayBuffer)
    if (series) tr.push(series.buffer as ArrayBuffer)
    postSim({ type: 'sim', time: current.sim_time(), stats, buf, heat, series, histLen, track }, tr)
  }
  simTimer = self.setTimeout(simLoop, 4)
}
let lastStats: SimStats = { time: 0, alive: 0, spawned: 0, finished: 0, lost: 0, queued: 0, meanTravel: 0, redRuns: 0, laneChanges: 0 }

self.onmessage = async (ev: MessageEvent<WorkerIn>) => {
  const msg = ev.data
  const post = (m: WorkerOut, t: Transferable[] = []) => (self as unknown as Worker).postMessage(m, t)
  try {
    ready ??= init()
    wasmMem ??= ((await ready) as { memory: WebAssembly.Memory }).memory
    if (msg.type === 'demand') {
      try {
        if (!current) throw new Error('Nenhum mapa carregado')
        post({ type: 'result', id: msg.id, json: current.demand_json(msg.json) })
      } catch (e) {
        post({ type: 'result', id: msg.id, error: e instanceof Error ? e.message : String(e) })
      }
      return
    }
    if (msg.type === 'nodeConns') {
      try {
        if (!current) throw new Error('Nenhum mapa carregado')
        const u32 = current.node_conns_u32(msg.node)
        const f32 = current.node_conns_f32(msg.node)
        post({ type: 'result', id: msg.id, u32, f32 }, [u32.buffer as ArrayBuffer, f32.buffer as ArrayBuffer])
      } catch (e) {
        post({ type: 'result', id: msg.id, error: e instanceof Error ? e.message : String(e) })
      }
      return
    }
    if (msg.type === 'approaches') {
      try {
        if (!current) throw new Error('Nenhum mapa carregado')
        post({ type: 'result', id: msg.id, json: current.node_approaches_json(msg.node) })
      } catch (e) {
        post({ type: 'result', id: msg.id, error: e instanceof Error ? e.message : String(e) })
      }
      return
    }
    if (msg.type === 'simInit') {
      try {
        if (!current) throw new Error('Nenhum mapa carregado')
        simRunning = false
        routerGen++
        initPending = 0
        if (!simInitExt(current, msg.json)) {
          current.sim_init(msg.json)
          postSim({ type: 'simReady' })
        }
      } catch (e) {
        postSim({ type: 'simReady', error: e instanceof Error ? e.message : String(e) })
      }
      return
    }
    if (msg.type === 'simRun') {
      simSpeed = msg.speed
      if (msg.running && !simRunning && current?.sim_active()) {
        simRunning = true
        simInFlight = 0
        simLast = performance.now()
        simCarry = 0
        if (!simTimer) simTimer = self.setTimeout(simLoop, 0)
      } else if (!msg.running) simRunning = false
      return
    }
    if (msg.type === 'simStepBy') {
      if (current && current.sim_active() && !simRunning) {
        const ticks = Math.max(1, Math.round(msg.seconds / SIM_DT))
        current.sim_step(ticks)
        simTrackAt = 0
        const buf = current.sim_snapshot()
        const track = simTrackId != null ? current.sim_track(simTrackId) : undefined
        lastStats = JSON.parse(current.sim_stats())
        const series = current.sim_series()
        postSim({ type: 'sim', time: current.sim_time(), stats: lastStats, buf, series, histLen: current.sim_hist_len(), track }, [buf.buffer as ArrayBuffer, series.buffer as ArrayBuffer])
      }
      return
    }
    if (msg.type === 'simStuck') {
      post({ type: 'result', id: msg.id, json: current && current.sim_active() ? current.sim_stuck(msg.n) : '[]' })
      return
    }
    if (msg.type === 'simTrack') {
      simTrackId = msg.veh
      simTrackAt = 0
      if (current && msg.veh != null && !simRunning) postSim({ type: 'sim', time: current.sim_time(), stats: lastStats, buf: current.sim_snapshot(), track: current.sim_track(msg.veh) })
      return
    }
    if (msg.type === 'simHeat') {
      simHeatKind = msg.kind
      return
    }
    if (msg.type === 'simHist') {
      try {
        const u8 = current ? current.sim_hist(msg.bucket) : new Uint8Array(0)
        post({ type: 'result', id: msg.id, u8 }, [u8.buffer as ArrayBuffer])
      } catch (e) {
        post({ type: 'result', id: msg.id, error: e instanceof Error ? e.message : String(e) })
      }
      return
    }
    if (msg.type === 'simAck') {
      simInFlight = 0
      return
    }
    if (msg.type === 'simStop') {
      simRunning = false
      simInFlight = 0
      routerGen++
      initPending = 0
      current?.sim_stop()
      return
    }
    if (msg.type === 'connEdits') {
      current?.set_conn_edits(msg.json)
      return
    }
    if (msg.type === 'prioEdits') {
      current?.set_prio_edits(msg.json)
      return
    }
    const t0 = performance.now()
    if (msg.type === 'import') {
      current?.free()
      sourceBytes = new Uint8Array(msg.buffer)
      currentSource = sourceOf(msg.edits)
      current = await importNet(sourceBytes, currentSource)
      if (msg.edits && msg.edits !== '[]') current.apply_edits(msg.edits)
    } else if (msg.type === 'edit') {
      if (!current || !sourceBytes) throw new Error('Nenhum mapa carregado')
      const src = sourceOf(msg.edits)
      if (src !== currentSource) {
        // os semáforos externos mudam os nós da rede bruta: remonta a partir dos bytes do mapa
        const next = await importNet(sourceBytes, src)
        current.free()
        current = next
        currentSource = src
      }
      current.apply_edits(msg.edits)
    } else return
    const data = extract(current)
    current.clear_delta()
    post({ type: 'done', data, ms: performance.now() - t0 }, transferables(data))
  } catch (e) {
    post({ type: 'error', message: e instanceof Error ? e.message : String(e) })
  }
}
