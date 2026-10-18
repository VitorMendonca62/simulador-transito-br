/** Importação das zonas de tráfego e da matriz de carro da Pesquisa OD Metropolitana 2018 (Recife). */
import { ZONE_COLORS, type Zone } from './demand'
import { findGateways, gatewaySplitter, gatewayZones, inside, internalZones, type GatewayInfo } from './gatewayImport'
import type { NetworkData } from './types'

/** Arquivo gerado por scripts/od_recife/gerar_od.py (public/data/od2018_zonas.json). */
export interface OdSource {
  fonte: string
  ano: number
  unidade: string
  zonas: { id: number; municipio: string; bairro: string | null; ring: [number, number][] }[]
  /** 24 listas planas [origem, destino, veíc./h, ...] por hora de partida */
  horas: number[][]
}

/** Matriz importada já remapeada para as zonas do simulador. Persistida junto da demanda. */
export interface OdImport {
  name: string
  /** multiplicador aplicado à geração e às células fixadas */
  scale: number
  /** 24 listas planas [idOrigem, idDestino, veíc./h] (escala 1) */
  hourly: number[][]
  /** zonas da pesquisa fora do mapa (incorporadas à zona de borda mais próxima, ou servidas pelos portais) */
  merged: number
  /** portais de entrada/saída na divisa do mapa (`gatewayImport.ts`); ausente = zonas de fora incorporadas */
  gateways?: GatewayInfo[]
  /** veículos por dia útil descartados: viagens entre cidades vizinhas que não atravessam o mapa */
  dropped?: number
}

export const OD2018_URL = `${import.meta.env.BASE_URL}data/od2018_zonas.json`

const M_PER_DEG = 111_319.490_793_273_57
/** Mínimo de nós da rede dentro do polígono para a zona ficar no mapa. */
const MIN_NODES = 3

type Pt = [number, number]

/** Sutherland–Hodgman contra o retângulo [x0, y0, x1, y1]. */
function clipRect(poly: Pt[], [x0, y0, x1, y1]: number[]): Pt[] {
  const edges: [(p: Pt) => boolean, (a: Pt, b: Pt) => Pt][] = [
    [(p) => p[0] >= x0, (a, b) => [x0, a[1] + ((b[1] - a[1]) * (x0 - a[0])) / (b[0] - a[0])]],
    [(p) => p[0] <= x1, (a, b) => [x1, a[1] + ((b[1] - a[1]) * (x1 - a[0])) / (b[0] - a[0])]],
    [(p) => p[1] >= y0, (a, b) => [a[0] + ((b[0] - a[0]) * (y0 - a[1])) / (b[1] - a[1]), y0]],
    [(p) => p[1] <= y1, (a, b) => [a[0] + ((b[0] - a[0]) * (y1 - a[1])) / (b[1] - a[1]), y1]],
  ]
  let out = poly
  for (const [keep, cut] of edges) {
    const src = out
    out = []
    for (let i = 0; i < src.length; i++) {
      const a = src[(i + src.length - 1) % src.length]
      const b = src[i]
      if (keep(b)) {
        if (!keep(a)) out.push(cut(a, b))
        out.push(b)
      } else if (keep(a)) out.push(cut(a, b))
    }
    if (out.length === 0) break
  }
  return out
}

const centroid = (poly: Pt[]): Pt => [poly.reduce((s, p) => s + p[0], 0) / poly.length, poly.reduce((s, p) => s + p[1], 0) / poly.length]

const label = (z: OdSource['zonas'][number]) => {
  const fora = z.bairro?.endsWith('(fora do Recife)')
  const nome = fora || !z.bairro ? z.municipio.toLowerCase().replace(/(^|\s)\S/g, (c) => c.toUpperCase()) : z.bairro
  return `${z.id} · ${nome}`
}

/**
 * Escolhe as zonas que têm vias no mapa carregado (recortadas à área da rede). As demais:
 * - sem `gateways`: são incorporadas à zona mantida mais próxima (quem vem de fora entra pela borda do mapa mais perto da sua origem);
 * - com `gateways`: entram e saem por portais nas vias principais que cruzam a divisa (`gatewayImport.ts`), e só ficam no mapa as zonas que
 *   ele cobre de fato (as de borda, com um canto de rede, passam a ser de fora).
 */
export function buildOdImport(src: OdSource, data: Pick<NetworkData, 'meta' | 'nodeXY' | 'segU32'>, opts: { gateways?: boolean } = {}): { zones: Zone[]; imp: OdImport } {
  const { meta, nodeXY } = data
  const [latC, lonC, kx] = meta.geo
  const proj = ([lon, lat]: Pt): Pt => [(lon - lonC) * kx, -(lat - latC) * M_PER_DEG]
  const b = meta.bounds
  const polys = src.zonas.map((z) => {
    const ring = z.ring.map(proj)
    if (ring.length > 1 && ring[0][0] === ring[ring.length - 1][0] && ring[0][1] === ring[ring.length - 1][1]) ring.pop()
    return ring
  })
  // nós da rede por zona (com pré-filtro pela caixa do polígono)
  const cap = opts.gateways ? Infinity : MIN_NODES
  const counts = polys.map((poly) => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
    for (const [x, y] of poly) {
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y)
    }
    if (x1 < b[0] || x0 > b[2] || y1 < b[1] || y0 > b[3]) return 0
    let n = 0
    for (let i = 0; i < nodeXY.length && n < cap; i += 2) {
      const p: Pt = [nodeXY[i], nodeXY[i + 1]]
      if (p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1 && inside(p, poly)) n++
    }
    return n
  })
  const keep = opts.gateways
    ? internalZones(polys, src.zonas.map((z) => z.municipio), counts, nodeXY, MIN_NODES)
    : counts.map((n) => n >= MIN_NODES)
  const keptIdx = src.zonas.map((_, i) => i).filter((i) => keep[i])
  if (keptIdx.length === 0) throw new Error('Nenhuma zona da Pesquisa OD 2018 cobre este mapa (só há dados para a Região Metropolitana do Recife).')
  const clipped = new Map(keptIdx.map((i) => [i, clipRect(polys[i], b)]))
  const idx = new Map(src.zonas.map((z, i) => [z.id, i]))
  const zones: Zone[] = keptIdx.map((i, n) => ({
    id: src.zonas[i].id,
    name: label(src.zonas[i]),
    poly: clipped.get(i)!.map(([x, y]) => [Math.round(x * 10) / 10, Math.round(y * 10) / 10] as Pt),
    gen: 0,
    attr: 1,
    profile: 'od2018',
    color: ZONE_COLORS[n % ZONE_COLORS.length],
  }))
  const merged = src.zonas.length - keptIdx.length
  const name = `Pesquisa OD ${src.ano} · carro dirigindo`
  const pack = (acc: Map<string, number>) => {
    const out: number[] = []
    for (const [k, v] of acc) {
      const [o, d] = k.split('>').map(Number)
      if (v >= 0.005) out.push(o, d, Math.round(v * 100) / 100)
    }
    return out
  }
  const add = (acc: Map<string, number>, o: number, d: number, v: number) => {
    const k = `${o}>${d}`
    acc.set(k, (acc.get(k) ?? 0) + v)
  }

  if (!opts.gateways) {
    const kc = keptIdx.map((i) => [i, centroid(clipped.get(i)!)] as const)
    const target = new Map<number, number>() // id da pesquisa -> id da zona mantida
    src.zonas.forEach((z, i) => {
      if (clipped.has(i)) return target.set(z.id, z.id)
      const c = centroid(polys[i])
      let best = kc[0]
      let bd = Infinity
      for (const k of kc) {
        const d = Math.hypot(k[1][0] - c[0], k[1][1] - c[1])
        if (d < bd) [bd, best] = [d, k]
      }
      target.set(z.id, src.zonas[best[0]].id)
    })
    const hourly = src.horas.map((flat) => {
      const acc = new Map<string, number>()
      for (let i = 0; i < flat.length; i += 3) {
        const o = target.get(flat[i])
        const d = target.get(flat[i + 1])
        if (o != null && d != null) add(acc, o, d, flat[i + 2])
      }
      return pack(acc)
    })
    return { zones, imp: { name, scale: 1, hourly, merged } }
  }

  const ext = src.zonas.map((z, i) => ({ i, poly: polys[i], municipio: z.municipio })).filter((z) => !keep[z.i])
  const gates = findGateways({ nodeXY, segU32: data.segU32, names: meta.names }, ext)
  if (gates.length === 0) throw new Error('Nenhuma via principal cruza a divisa do mapa: não há onde pôr entradas e saídas.')
  const cents = src.zonas.map((_, i) => centroid(clipped.get(i) ?? polys[i]))
  const sp = gatewaySplitter(gates, (id) => cents[idx.get(id)!])
  let dropped = 0
  const hourly = src.horas.map((flat) => {
    const acc = new Map<string, number>()
    for (let i = 0; i < flat.length; i += 3) {
      const [o, d, v] = [flat[i], flat[i + 1], flat[i + 2]]
      const io = idx.get(o)
      const id = idx.get(d)
      if (io == null || id == null) continue
      const [ko, kd] = [keep[io], keep[id]]
      if (ko && kd) add(acc, o, d, v)
      else if (kd) for (const [g, f] of sp.inbound(o, d)) add(acc, g, d, v * f)
      else if (ko) for (const [g, f] of sp.outbound(o, d)) add(acc, o, g, v * f)
      else if (o !== d) {
        const split = sp.through(o, d)
        if (split.length === 0) dropped += v
        for (const [pair, f] of split) {
          const [ga, gb] = sp.pairOf(pair)
          add(acc, ga, gb, v * f)
        }
      } else dropped += v
    }
    return pack(acc)
  })
  return {
    zones: [...zones, ...gatewayZones(gates)],
    imp: { name, scale: 1, hourly, merged, gateways: gates, dropped: Math.round(dropped) },
  }
}

/**
 * Células fixadas, perfil horário e geração de cada zona para a escala atual. Sem viagens intrazona, as
 * células i>i ficam de fora e o perfil é recalculado sem elas, para a geração bater com a soma da linha.
 */
export function applyOdImport(imp: OdImport, zones: Zone[], hour: number, intrazone: boolean) {
  const s = imp.scale
  const hourOf = (h: number) => {
    const m: Record<string, number> = {}
    const flat = imp.hourly[h]
    for (let i = 0; i < flat.length; i += 3) {
      if (!intrazone && flat[i] === flat[i + 1]) continue
      m[`${flat[i]}>${flat[i + 1]}`] = Math.round(flat[i + 2] * s * 100) / 100
    }
    return m
  }
  const tot = new Map<number, number[]>()
  imp.hourly.forEach((flat, h) => {
    for (let i = 0; i < flat.length; i += 3) {
      if (!intrazone && flat[i] === flat[i + 1]) continue
      const t = tot.get(flat[i]) ?? Array(24).fill(0)
      t[h] += flat[i + 2]
      tot.set(flat[i], t)
    }
  })
  const att = new Map<number, number>()
  for (const flat of imp.hourly) for (let i = 0; i < flat.length; i += 3) att.set(flat[i + 1], (att.get(flat[i + 1]) ?? 0) + flat[i + 2])
  const nextZones = zones.map((z) => {
    const t = tot.get(z.id) ?? Array(24).fill(0)
    const max = Math.max(...t)
    return {
      ...z,
      gen: Math.round(max * s * 10) / 10,
      attr: Math.round((att.get(z.id) ?? 0) / 10) / 10,
      profileValues: t.map((v) => (max > 0 ? Math.round((v / max) * 1000) / 1000 : 0)),
    }
  })
  const odHourly = Array.from({ length: 24 }, (_, h) => hourOf(h))
  // mesma interpolação linear entre horas que o núcleo usa no perfil e na matriz horária
  const hr = ((hour % 24) + 24) % 24
  const i = Math.floor(hr) % 24
  const f = hr - Math.floor(hr)
  const odManual: Record<string, number> = {}
  for (const [k, v] of Object.entries(odHourly[i])) odManual[k] = v * (1 - f)
  for (const [k, v] of Object.entries(odHourly[(i + 1) % 24])) odManual[k] = (odManual[k] ?? 0) + v * f
  for (const k in odManual) odManual[k] = Math.round(odManual[k] * 100) / 100
  return { zones: nextZones, odManual, odHourly }
}
