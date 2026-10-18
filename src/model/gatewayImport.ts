/**
 * Entradas e saídas para as cidades vizinhas (Pesquisa OD 2018).
 *
 * As zonas da pesquisa que o mapa não cobre (Olinda, Jaboatão, Camaragibe, Paulista…) não viram zonas: o
 * tráfego delas entra e sai por **portais** postos onde uma via principal (via expressa, arterial ou
 * coletora: `motorway`…`secondary`) cruza a divisa do mapa, como BR-101, BR-232, Agamenon e Cruz Cabugá.
 *
 * Quantos carros passam por cada portal vem da matriz da OD 2018: cada viagem de/para uma zona de fora é
 * dividida entre os portais por um logit sobre a distância (origem → portal → destino, em linha reta) e a
 * capacidade do portal (faixas no sentido × peso da classe). Viagens entre duas cidades vizinhas só entram
 * quando atravessar o mapa é caminho (desvio ≤ 20 % da reta), de portal a portal. Isso é uma hipótese: a
 * pesquisa não diz por onde a viagem passa, e a rede fora do mapa não é conhecida.
 */
import type { Zone } from './demand'

type Pt = [number, number]

/** Ids dos portais (as zonas da pesquisa vão de 1 a 253). */
export const GATEWAY_ID0 = 1000
/** Classe máxima da via de um portal (0 motorway, 1 trunk, 2 primary, 3 secondary). */
const MAX_HW = 3
/** Peso de cada faixa por classe de via (vias expressas puxam mais tráfego). */
const CLASS_W = [1, 1, 0.8, 0.6]
/** Raio do polígono do portal em volta do nó de fora da divisa (m). */
const GATE_R = 30
/** Cruzamentos da mesma via a até esta distância viram um portal só (pistas de uma via duplicada). */
const CLUSTER_M = 400
/** Cruzamentos a até esta distância viram um portal só mesmo com nomes diferentes (pistas com nome trocado no OSM). */
const CLUSTER_ANY_M = 120
/** Escala do logit de escolha do portal (m): +1,5 km de volta divide o peso por e. */
const LAMBDA_M = 1500
/** Atravessar o mapa só conta se o caminho por dois portais não passa da reta × este fator. */
const THROUGH_DETOUR = 1.2
/** Portais de passagem precisam estar a pelo menos isto um do outro (m). */
const THROUGH_MIN_M = 1500
/** Fatias menores que isto são descartadas e o resto renormalizado. */
const MIN_SHARE = 0.02
/** Célula da pegada da rede (m) e raio do fechamento morfológico (células). */
const CELL = 250
const CLOSE_R = 4

export interface GatewayInfo {
  id: number
  /** nome da via na divisa */
  road: string
  /** município do outro lado */
  municipio: string
  hw: number
  /** faixas entrando no mapa / saindo dele */
  lanesIn: number
  lanesOut: number
  /** nó de fora da divisa (metros locais) */
  x: number
  y: number
  poly: Pt[]
}

export interface NetLike {
  nodeXY: Float32Array
  segU32: Uint32Array
  names: string[]
}

const SEG = 9

export const inside = (p: Pt, poly: Pt[]) => {
  let r = false
  for (let a = 0, b = poly.length - 1; a < poly.length; b = a++) {
    const [ax, ay] = poly[a]
    const [bx, by] = poly[b]
    if (ay > p[1] !== by > p[1] && p[0] < ((bx - ax) * (p[1] - ay)) / (by - ay) + ax) r = !r
  }
  return r
}

const bbox = (poly: Pt[]) => {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
  for (const [x, y] of poly) {
    x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y)
  }
  return [x0, y0, x1, y1]
}

const dist = (a: Pt, b: Pt) => Math.hypot(a[0] - b[0], a[1] - b[1])

const titleCase = (s: string) => s.toLowerCase().replace(/(^|\s)\S/g, (c) => c.toUpperCase()).replace(/\s(Da|De|Do|Dos|Das)\s/g, (m) => m.toLowerCase())

/**
 * Pegada da rede: células de 250 m com algum nó, com fechamento morfológico (preenche matas, parques e
 * campi cercados de vias; os tocos de via que passam da divisa continuam finos e não ganham área).
 */
function footprint(nodeXY: Float32Array) {
  const key = (i: number, j: number) => (i + 0x8000) * 0x10000 + (j + 0x8000)
  const cells = new Set<number>()
  for (let k = 0; k < nodeXY.length; k += 2) cells.add(key(Math.floor(nodeXY[k] / CELL), Math.floor(nodeXY[k + 1] / CELL)))
  const disk: Pt[] = []
  for (let a = -CLOSE_R; a <= CLOSE_R; a++) for (let b = -CLOSE_R; b <= CLOSE_R; b++) if (a * a + b * b <= CLOSE_R * CLOSE_R) disk.push([a, b])
  const dil = new Set<number>()
  for (const c of cells) {
    const i = Math.floor(c / 0x10000) - 0x8000
    const j = (c % 0x10000) - 0x8000
    for (const [a, b] of disk) dil.add(key(i + a, j + b))
  }
  const foot = new Set(cells)
  for (const c of dil) {
    const i = Math.floor(c / 0x10000) - 0x8000
    const j = (c % 0x10000) - 0x8000
    if (disk.every(([a, b]) => dil.has(key(i + a, j + b)))) foot.add(c)
  }
  return (p: Pt) => foot.has(key(Math.floor(p[0] / CELL), Math.floor(p[1] / CELL)))
}

/** Fração da área do polígono dentro da pegada da rede (amostragem em grade). */
function coverage(poly: Pt[], inFoot: (p: Pt) => boolean) {
  const [x0, y0, x1, y1] = bbox(poly)
  const step = Math.max(100, Math.sqrt((x1 - x0) * (y1 - y0)) / 20)
  let tot = 0
  let cov = 0
  for (let y = y0 + step / 2; y < y1; y += step) {
    for (let x = x0 + step / 2; x < x1; x += step) {
      if (!inside([x, y], poly)) continue
      tot++
      if (inFoot([x, y])) cov++
    }
  }
  return tot > 0 ? cov / tot : 0
}

/**
 * Quais zonas ficam no mapa. Uma zona fica se tem ≥ `minNodes` nós e cobre ≥ 50 % da sua área com rede;
 * as do município principal do mapa (o de mais nós) ficam com ≥ 15 %, porque têm matas, campi e porto
 * (Várzea, Bairro do Recife) e não podem virar "cidade vizinha".
 */
export function internalZones(polys: Pt[][], municipios: string[], counts: number[], nodeXY: Float32Array, minNodes: number): boolean[] {
  const inFoot = footprint(nodeXY)
  const byMun = new Map<string, number>()
  counts.forEach((n, i) => byMun.set(municipios[i], (byMun.get(municipios[i]) ?? 0) + n))
  const main = [...byMun].sort((a, b) => b[1] - a[1])[0]?.[0]
  return polys.map((poly, i) => {
    if (counts[i] < minNodes) return false
    const c = coverage(poly, inFoot)
    return c >= 0.5 || (municipios[i] === main && c >= 0.15)
  })
}

/** Polígono convexo (casco) de octógonos de raio `r` em volta dos pontos. */
function hullAround(pts: Pt[], r: number): Pt[] {
  const cloud: Pt[] = []
  for (const [x, y] of pts) for (let k = 0; k < 8; k++) cloud.push([x + r * Math.cos((k * Math.PI) / 4), y + r * Math.sin((k * Math.PI) / 4)])
  cloud.sort((a, b) => a[0] - b[0] || a[1] - b[1])
  const cross = (o: Pt, a: Pt, b: Pt) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
  const lo: Pt[] = []
  for (const p of cloud) {
    while (lo.length >= 2 && cross(lo[lo.length - 2], lo[lo.length - 1], p) <= 0) lo.pop()
    lo.push(p)
  }
  const up: Pt[] = []
  for (let i = cloud.length - 1; i >= 0; i--) {
    const p = cloud[i]
    while (up.length >= 2 && cross(up[up.length - 2], up[up.length - 1], p) <= 0) up.pop()
    up.push(p)
  }
  return [...lo.slice(0, -1), ...up.slice(0, -1)].map(([x, y]) => [Math.round(x * 10) / 10, Math.round(y * 10) / 10] as Pt)
}

/**
 * Portais: trechos de via principal com um nó dentro de uma zona externa e o outro fora de todas (dentro
 * do mapa ou nas frestas entre polígonos). Cruzamentos da mesma via próximos (pistas de via duplicada)
 * viram um portal só.
 */
export function findGateways(net: NetLike, extPolys: { poly: Pt[]; municipio: string }[]): GatewayInfo[] {
  const { nodeXY, segU32, names } = net
  const nn = nodeXY.length / 2
  const boxes = extPolys.map((z) => bbox(z.poly))
  const extOf = new Int32Array(nn).fill(-1)
  const degree = new Uint32Array(nn)
  for (let s = 0; s < segU32.length; s += SEG) {
    degree[segU32[s]]++
    degree[segU32[s + 1]]++
  }
  for (let n = 0; n < nn; n++) {
    const p: Pt = [nodeXY[n * 2], nodeXY[n * 2 + 1]]
    for (let k = 0; k < extPolys.length; k++) {
      const b = boxes[k]
      if (p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3] && inside(p, extPolys[k].poly)) {
        extOf[n] = k
        break
      }
    }
  }
  interface Crossing { road: string; ext: number; hw: number; lin: number; lout: number; p: Pt }
  const crossings: Crossing[] = []
  for (let s = 0; s < segU32.length; s += SEG) {
    const [a, b, lf, lb, hw, name, flags] = [segU32[s], segU32[s + 1], segU32[s + 2], segU32[s + 3], segU32[s + 4], segU32[s + 5], segU32[s + 6]]
    if (hw > MAX_HW || flags & 4) continue // só vias principais, sem alças
    const ea = extOf[a] >= 0
    if (ea === extOf[b] >= 0) continue
    const [out, inn] = ea ? [a, b] : [b, a]
    if (degree[inn] < 2) continue // o lado de dentro precisa seguir para a rede
    // faixas no sentido de quem entra: de `out` para `inn`
    const lin = out === a ? lf : lb
    const lout = out === a ? lb : lf
    crossings.push({ road: names[name] ?? '', ext: extOf[out], hw, lin, lout, p: [nodeXY[out * 2], nodeXY[out * 2 + 1]] })
  }
  // agrupa por via (mesmo nome) e proximidade
  const groups: Crossing[][] = []
  for (const c of crossings.sort((x, y) => x.hw - y.hw)) {
    const g = groups.find((g) => g.some((o) => dist(o.p, c.p) < (o.road === c.road ? CLUSTER_M : CLUSTER_ANY_M)))
    if (g) g.push(c)
    else groups.push([c])
  }
  return groups
    .map((g) => {
      const pts = g.map((c) => c.p)
      const lanesIn = g.reduce((s, c) => s + c.lin, 0)
      const lanesOut = g.reduce((s, c) => s + c.lout, 0)
      // nome e município da pista principal do grupo (maior classe, depois mais faixas, depois com nome)
      const main = g.reduce((m, c) => (c.hw < m.hw || (c.hw === m.hw && (c.lin + c.lout > m.lin + m.lout || (!m.road && c.road))) ? c : m), g[0])
      return {
        id: 0,
        road: main.road || g.find((c) => c.road)?.road || 'via sem nome',
        municipio: titleCase(extPolys[main.ext].municipio),
        hw: Math.min(...g.map((c) => c.hw)),
        lanesIn,
        lanesOut,
        x: Math.round((pts.reduce((s, p) => s + p[0], 0) / pts.length) * 10) / 10,
        y: Math.round((pts.reduce((s, p) => s + p[1], 0) / pts.length) * 10) / 10,
        poly: hullAround(pts, GATE_R),
      }
    })
    .filter((g) => g.lanesIn + g.lanesOut > 0)
    .map((g, k) => ({ ...g, id: GATEWAY_ID0 + k + 1 }))
}

/** Zonas do simulador para os portais (geração e perfil vêm depois, de `applyOdImport`). */
export function gatewayZones(gates: GatewayInfo[]): Zone[] {
  return gates.map((g) => ({
    id: g.id,
    name: `Divisa · ${g.road} (${g.municipio})`,
    poly: g.poly,
    gen: 0,
    attr: 1,
    profile: 'od2018',
    color: 0xe5e7eb,
    gateway: true,
  }))
}

type Split = [number, number][]

/**
 * Divide as viagens das zonas de fora entre os portais. `centroid(id)` dá o ponto de cada zona da pesquisa
 * (as de dentro, recortadas ao mapa).
 */
export function gatewaySplitter(gates: GatewayInfo[], centroid: (id: number) => Pt) {
  const gp = gates.map((g): Pt => [g.x, g.y])
  const capIn = gates.map((g) => g.lanesIn * CLASS_W[g.hw])
  const capOut = gates.map((g) => g.lanesOut * CLASS_W[g.hw])
  const logit = (items: { key: number; cost: number; cap: number }[]): Split => {
    const ok = items.filter((x) => x.cap > 0)
    if (ok.length === 0) return []
    const min = Math.min(...ok.map((x) => x.cost))
    let w = ok.map((x) => [x.key, x.cap * Math.exp(-(x.cost - min) / LAMBDA_M)] as [number, number])
    let sum = w.reduce((s, x) => s + x[1], 0)
    w = w.filter((x) => x[1] / sum >= MIN_SHARE)
    sum = w.reduce((s, x) => s + x[1], 0)
    return w.map(([k, v]) => [k, v / sum])
  }
  const cache = new Map<string, Split>()
  /** de fora para dentro: [portal, fração] */
  const inbound = (o: number, d: number): Split => {
    const k = `i${o}>${d}`
    let r = cache.get(k)
    if (!r) {
      const [po, pd] = [centroid(o), centroid(d)]
      r = logit(gates.map((g, i) => ({ key: g.id, cost: dist(po, gp[i]) + dist(gp[i], pd), cap: capIn[i] })))
      cache.set(k, r)
    }
    return r
  }
  /** de dentro para fora */
  const outbound = (o: number, d: number): Split => {
    const k = `o${o}>${d}`
    let r = cache.get(k)
    if (!r) {
      const [po, pd] = [centroid(o), centroid(d)]
      r = logit(gates.map((g, i) => ({ key: g.id, cost: dist(po, gp[i]) + dist(gp[i], pd), cap: capOut[i] })))
      cache.set(k, r)
    }
    return r
  }
  /** de fora para fora, atravessando o mapa: chave = par de portais (entrada * n + saída), ver `pairOf` */
  const through = (o: number, d: number): Split => {
    const k = `t${o}>${d}`
    let r = cache.get(k)
    if (!r) {
      const [po, pd] = [centroid(o), centroid(d)]
      const direct = dist(po, pd)
      const items: { key: number; cost: number; cap: number }[] = []
      const n = gates.length
      for (let a = 0; a < n; a++) {
        if (capIn[a] <= 0) continue
        const ca = dist(po, gp[a])
        for (let b = 0; b < n; b++) {
          if (a === b || capOut[b] <= 0) continue
          const ab = dist(gp[a], gp[b])
          if (ab < THROUGH_MIN_M) continue
          items.push({ key: a * n + b, cost: ca + ab + dist(gp[b], pd), cap: Math.min(capIn[a], capOut[b]) })
        }
      }
      const best = Math.min(...items.map((x) => x.cost))
      r = items.length > 0 && best <= direct * THROUGH_DETOUR ? logit(items) : []
      cache.set(k, r)
    }
    return r
  }
  const pairOf = (key: number): [number, number] => [gates[Math.floor(key / gates.length)].id, gates[key % gates.length].id]
  return { inbound, outbound, through, pairOf }
}
