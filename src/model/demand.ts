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
  color: number
}

export interface DemandParams {
  /** sensibilidade ao tempo de viagem (1/min): maior = viagens mais curtas */
  beta: number
  intrazone: boolean
  /** hora do dia mostrada/simulada (0–24) */
  hour: number
  /** % de ônibus no fluxo gerado (reservado para a simulação) */
  busShare: number
  seed: number
}

export const DEFAULT_PARAMS: DemandParams = { beta: 0.08, intrazone: false, hour: 8, busShare: 2, seed: 1 }

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
    zones: zones.map((z) => ({ id: z.id, poly: z.poly, gen: z.gen, attr: z.attr, profile: profileById(z.profile).values })),
    params: { beta: params.beta, intrazone: params.intrazone, hour: params.hour, odManual },
  }
}
