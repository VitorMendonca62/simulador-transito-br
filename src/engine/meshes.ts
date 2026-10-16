import { Mesh, MeshGeometry, Texture } from 'pixi.js'
import type { MeshData } from '../model/types'

export function makeMesh(m: MeshData, color: number): Mesh {
  const geometry = new MeshGeometry({ positions: m.pos, uvs: new Float32Array(m.pos.length), indices: m.idx })
  const mesh = new Mesh({ geometry, texture: Texture.WHITE })
  mesh.tint = color
  return mesh
}

/** Círculos (polígonos de `sides` lados) em cada ponto de `xy`. Geometria estática em unidades do mundo. */
export function makeDots(xy: ArrayLike<number>, radius: number, color: number, sides = 10): Mesh | null {
  const n = xy.length / 2
  if (n === 0) return null
  const pos = new Float32Array(n * (sides + 1) * 2)
  const idx = new Uint32Array(n * sides * 3)
  let p = 0
  let t = 0
  for (let i = 0; i < n; i++) {
    const cx = xy[i * 2]
    const cy = xy[i * 2 + 1]
    const base = p / 2
    pos[p++] = cx
    pos[p++] = cy
    for (let k = 0; k < sides; k++) {
      const a = (k / sides) * Math.PI * 2
      pos[p++] = cx + Math.cos(a) * radius
      pos[p++] = cy + Math.sin(a) * radius
      idx[t++] = base
      idx[t++] = base + 1 + k
      idx[t++] = base + 1 + ((k + 1) % sides)
    }
  }
  return makeMesh({ pos, idx }, color)
}

export function pick(xy: Float32Array, ids: ArrayLike<number>): Float32Array {
  const out = new Float32Array(ids.length * 2)
  for (let i = 0; i < ids.length; i++) {
    out[i * 2] = xy[ids[i] * 2]
    out[i * 2 + 1] = xy[ids[i] * 2 + 1]
  }
  return out
}
