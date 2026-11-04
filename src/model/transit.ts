/**
 * Linhas de ônibus na interface: dados do encaixe vindos do núcleo (`transit_lines_json`, `transit_stops_json`,
 * polilinhas) e as edições do usuário (linhas desligadas, fator de frequência por hora, cor, paradas editadas e criadas).
 * As edições vão ao núcleo por `set_transit_edits` (sem a cor, que é só da interface) e valem na próxima simulação.
 */

/** Dados das linhas de ônibus do Recife (`scripts/onibus_recife/`). */
export const TRANSIT_URL = `${import.meta.env.BASE_URL}data/onibus_recife.json`

export interface TransitTrecho {
  ok: boolean
  aviso: string | null
  paradas: number
  /** índice da polilinha em `TransitData.off` (−1 = não encaixou) */
  pts: number
}
export interface TransitPadrao {
  id: string
  destino: string
  trechos: TransitTrecho[]
}
export interface TransitLine {
  id: string
  numero: string
  nome: string
  padroes: TransitPadrao[]
}
export interface TransitStop {
  id: string
  nome: string
  x: number
  y: number
  baia: boolean
  linhas: string[]
  editada: boolean
  nova: boolean
}
export interface TransitSummary {
  trechos: number
  ok: number
  paradas: number
  paradas_fora: number
  paradas_baia: number
  falhas: { linha: string; padrao: string; trecho: number; aviso: string }[]
  excecoes: string[]
}
export interface TransitData {
  lines: TransitLine[]
  stops: TransitStop[]
  summary: TransitSummary
  /** polilinhas dos trechos encaixados (xy planos) e o início de cada uma, em pontos */
  pts: Float32Array
  off: Uint32Array
}

export interface LineEditRec {
  off?: boolean
  /** fator de frequência por hora do dia (24; 1 = como no GTFS) */
  factor?: number[]
  /** cor (só na interface) */
  color?: number
}
export interface StopEditRec {
  removed?: boolean
  /** posição nova [lat, lon] */
  pos?: [number, number]
  baia?: boolean
}
export interface AddedStopRec {
  id: string
  nome: string
  pos: [number, number]
  baia: boolean
}
export interface TransitRecord {
  lines: Record<string, LineEditRec>
  stops: Record<string, StopEditRec>
  added: AddedStopRec[]
}

export const emptyTransit = (): TransitRecord => ({ lines: {}, stops: {}, added: [] })

/** Edições no formato do núcleo (`TransitEdits`): sem a cor e sem entradas vazias. */
export function transitEditsJson(r: TransitRecord): string {
  const lines: Record<string, { off: boolean; factor: number[] }> = {}
  for (const [id, l] of Object.entries(r.lines)) {
    const factor = l.factor && l.factor.some((f) => f !== 1) ? l.factor : []
    if (l.off || factor.length) lines[id] = { off: !!l.off, factor }
  }
  return JSON.stringify({ lines, stops: r.stops, added: r.added })
}

/** Paleta das linhas (cor padrão sorteada pelo número da linha). */
export const LINE_COLORS = [0xf59e0b, 0x22d3ee, 0xa78bfa, 0xf472b6, 0x4ade80, 0xfb923c, 0x60a5fa, 0xfacc15, 0x2dd4bf, 0xf87171]

export function lineColor(numero: string, rec?: LineEditRec): number {
  if (rec?.color != null) return rec.color
  let h = 0
  for (const c of numero) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return LINE_COLORS[h % LINE_COLORS.length]
}

export const lineOn = (r: TransitRecord, id: string) => !r.lines[id]?.off

const M_PER_DEG = 111_319.490_793_273_57

/** Metros locais -> [lat, lon] (inverso da projeção do núcleo, `meta.geo`). */
export function xyToLatLon(geo: [number, number, number], x: number, y: number): [number, number] {
  const [latC, lonC, kx] = geo
  return [latC - y / M_PER_DEG, lonC + x / kx]
}

/** Parada na simulação (`sim_stop_info`). */
export interface StopInfo {
  id: string
  nome: string
  linhas: string[]
  proximos: { veh: number; linha: string; dist: number; eta: number; embarcando: boolean }[]
  espera: { linha: string; intervalos: number; intervaloMedio: number | null; esperaMedia: number | null }[]
  naBaia: number
}

/** Métricas por linha na simulação (`sim_bus_report`). */
export interface BusLineReport {
  linha: string
  viagens: number
  tempoMedio: number
  velocidadeKmh: number
  paradoNasParadas: number
  paradoForaDasParadas: number
  paradas: number
  paradasPerdidas: number
  intervaloMedio: number
  intervaloCV: number
  comboios: number
}
export interface BusReport {
  ativos: number
  partidas: number
  naFila: number
  concluidas: number
  velocidadeKmh: number
  paradas: number
  paradasPerdidas: number
  linhas: BusLineReport[]
}

/** Texto para busca: minúsculas sem acentos. */
export const fold = (s: string) => s.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')
