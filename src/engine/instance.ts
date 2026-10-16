import type { MapView } from './MapView'

/** Instância única do motor de desenho, acessível pela UI. */
export const viewRef: { current: MapView | null } = { current: null }
