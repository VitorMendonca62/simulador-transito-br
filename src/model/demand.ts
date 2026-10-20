/** Zonas e demanda de viagens (modelo gravitacional resolvido no núcleo). */

export interface Zone {
  id: number
  name: string
  /** polígono em metros locais */
  poly: [number, number][]
  /** veículos/h gerados no pico (fator 1 do perfil) */
  gen: number
  /** peso de atração */
  attr: number
  /** id do perfil horário */
  profile: string
  /** perfil próprio (24 fatores), usado no lugar de `profile` (zonas importadas da Pesquisa OD) */
  profileValues?: number[]
  color: number
  /** portal na divisa do mapa (entrada/saída das cidades vizinhas): pode nascer em via expressa */
  gateway?: boolean
}

export interface DemandParams {
  /** sensibilidade ao tempo de viagem (1/min): maior = viagens mais curtas */
  beta: number
  intrazone: boolean
  /** hora do dia mostrada/simulada (0–24) */
  hour: number
  /** % de ônibus no fluxo gerado (reservado para a simulação) */
  busShare: number
  /** % de motoristas que ignoram a proibição de conversão quando estão na faixa errada */
  illegalPct: number
  /** % de motoristas que entram no cruzamento sem espaço para sair e podem ficar parados nele (CTB art. 45) */
  boxBlockPct?: number
  seed: number
  /** recalcula as rotas com o tempo medido durante a simulação (estilo Waze) */
  reroute?: boolean
  /** % de motoristas que seguem o tempo real; os demais usam o tempo de fluxo livre */
  appPct?: number
  /** erro de percepção (0–1) que espalha motoristas entre caminhos parecidos */
  perception?: number
}

/** Comportamento típico do trânsito do Recife (estimativas, não medições):
 * - beta 0,08/min: faixa usual de modelos gravitacionais de carro (0,05–0,1/min); com as zonas da OD 2018 a matriz é a da pesquisa
 * - viagens na mesma zona: as zonas da OD são do tamanho de bairros e têm viagens de carro internas
 * - ônibus 3 % dos veículos: ~2,8 mil ônibus no sistema, 4–6 % nos corredores e menos nas vias locais
 * - app de navegação 50 %: a maioria dos motoristas tem Waze/Google Maps, mas no trajeto diário só parte segue a rota em tempo real
 * - ignoram a sinalização 5 %; fecham o cruzamento 20 %: infrações comuns nos cruzamentos do centro e da Agamenon */
export const TYPICAL_PARAMS = { beta: 0.08, intrazone: true, busShare: 3, illegalPct: 5, boxBlockPct: 20, reroute: true, appPct: 50, perception: 0.3 } satisfies Partial<DemandParams>

export const DEFAULT_PARAMS: DemandParams = { ...TYPICAL_PARAMS, hour: 7, seed: 1 }

export interface Profile {
  id: string
  label: string
  /** 24 fatores relativos (0–1) */
  values: number[]
}

const ramp = (f: (h: number) => number) => Array.from({ length: 24 }, (_, h) => Math.round(f(h) * 100) / 100)
const bump = (h: number, c: number, w: number) => Math.exp(-((h - c) ** 2) / (2 * w * w))

export const PROFILES: Profile[] = [
  { id: 'flat', label: 'Plano (24 h)', values: ramp(() => 1) },
  { id: 'am', label: 'Residencial (pico da manhã)', values: ramp((h) => Math.min(1, 0.08 + bump(h, 7.5, 1.3) + 0.3 * bump(h, 18, 2))) },
  { id: 'pm', label: 'Comercial (pico da tarde)', values: ramp((h) => Math.min(1, 0.08 + bump(h, 18, 1.4) + 0.35 * bump(h, 12.5, 1.5))) },
  { id: 'workday', label: 'Dia útil (duplo pico)', values: ramp((h) => Math.min(1, 0.12 + 0.95 * bump(h, 7.8, 1.3) + 0.95 * bump(h, 17.8, 1.5) + 0.35 * bump(h, 12.5, 2))) },
  { id: 'night', label: 'Noturno / baixo', values: ramp((h) => 0.1 + 0.25 * bump(h, 21, 2.5)) },
]

export const profileById = (id: string): Profile => PROFILES.find((p) => p.id === id) ?? PROFILES[0]

export const ZONE_COLORS = [0x60a5fa, 0xf472b6, 0xfbbf24, 0xa78bfa, 0x2dd4bf, 0xfb923c, 0x4ade80, 0xf87171]

export const hexColor = (c: number) => `#${c.toString(16).padStart(6, '0')}`

/** Payload para o núcleo (`demand_json`). */
export function demandPayload(zones: Zone[], params: DemandParams, odManual: Record<string, number>) {
  return {
    zones: zones.map((z) => ({ id: z.id, poly: z.poly, gen: z.gen, attr: z.attr, profile: z.profileValues ?? profileById(z.profile).values, gateway: z.gateway ?? false })),
    params: { beta: params.beta, intrazone: params.intrazone, hour: params.hour, odManual },
  }
}
