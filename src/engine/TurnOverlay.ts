import { Graphics } from 'pixi.js'
import type { NetworkData, NodeConns } from '../model/types'
import { CONN_ABSENT, CONN_ALLOWED, CONN_BLOCKED, CONN_STRIDE, KIND_RIGHT, KIND_STRAIGHT, KIND_UTURN } from '../model/types'
import { C } from './colors'

type Pt = [number, number]

function bez(c: Float32Array, o: number, t: number): Pt {
  const u = 1 - t
  const a = u * u * u
  const b = 3 * u * u * t
  const d = 3 * u * t * t
  const e = t * t * t
  return [a * c[o] + b * c[o + 2] + d * c[o + 4] + e * c[o + 6], a * c[o + 1] + b * c[o + 3] + d * c[o + 5] + e * c[o + 7]]
}

function dashed(g: Graphics, pts: Pt[], dash: number, gap: number) {
  let draw = true
  let left = dash
  g.moveTo(pts[0][0], pts[0][1])
  for (let i = 1; i < pts.length; i++) {
    let [x0, y0] = pts[i - 1]
    const [x1, y1] = pts[i]
    let seg = Math.hypot(x1 - x0, y1 - y0)
    const dx = (x1 - x0) / (seg || 1)
    const dy = (y1 - y0) / (seg || 1)
    while (seg > 1e-6) {
      const step = Math.min(left, seg)
      x0 += dx * step
      y0 += dy * step
      if (draw) g.lineTo(x0, y0)
      else g.moveTo(x0, y0)
      seg -= step
      left -= step
      if (left <= 1e-9) {
        draw = !draw
        left = draw ? dash : gap
      }
    }
  }
}

/** Seta de faixa: base em b, direção d, manobra `kind`, tamanho L (mundo). */
function arrow(g: Graphics, bx: number, by: number, dx: number, dy: number, kind: number, L: number, color: number, alpha: number, w: number) {
  const nx = -dy
  const ny = dx
  let tipX = bx + dx * L
  let tipY = by + dy * L
  let hx = dx
  let hy = dy
  g.moveTo(bx, by)
  if (kind === KIND_STRAIGHT) {
    g.lineTo(tipX, tipY)
  } else if (kind === KIND_UTURN) {
    const sx = bx + dx * L * 0.5
    const sy = by + dy * L * 0.5
    g.lineTo(sx, sy)
    tipX = sx - nx * L * 0.5 - dx * L * 0.1
    tipY = sy - ny * L * 0.5 - dy * L * 0.1
    g.lineTo(sx - nx * L * 0.5, sy - ny * L * 0.5)
    hx = -dx
    hy = -dy
  } else {
    const side = kind === KIND_RIGHT ? 1 : -1
    const sx = bx + dx * L * 0.55
    const sy = by + dy * L * 0.55
    g.lineTo(sx, sy)
    tipX = sx + nx * side * L * 0.5
    tipY = sy + ny * side * L * 0.5
    g.lineTo(tipX, tipY)
    hx = nx * side
    hy = ny * side
  }
  g.stroke({ width: w, color, alpha, cap: 'round', join: 'round' })
  const hs = L * 0.28
  const px = -hy
  const py = hx
  g.poly([tipX + hx * hs * 0.9, tipY + hy * hs * 0.9, tipX - hx * hs * 0.3 + px * hs, tipY - hy * hs * 0.3 + py * hs, tipX - hx * hs * 0.3 - px * hs, tipY - hy * hs * 0.3 - py * hs]).fill({ color, alpha })
}

/** Cores por faixa de origem (como no TM:PE, cada origem tem a sua). */
export const PALETTE = [0xff4d4d, 0x7cfc00, 0x4f6bff, 0xffd166, 0xff9a3d, 0x4cc9f0, 0xc77dff, 0xf78fb3, 0x2dd4bf, 0xa3e635]

export interface TurnView {
  /** conexão sob o mouse (realce) */
  highlight: number | null
  /** faixa de origem ativa: as demais conexões ficam esmaecidas */
  activeSrc: number | null
  /** arrasto em andamento: posição do cursor (mundo) e faixa de destino sob ele */
  drag: { x: number; y: number; dst: number | null } | null
}

export interface NodeLanes {
  src: number[]
  dst: number[]
}

/** Faixas de entrada (origem) e de saída (destino) do nó, ordenadas por id. */
export function nodeLanes(nc: NodeConns): NodeLanes {
  const src = new Set<number>()
  const dst = new Set<number>()
  for (let c = 0; c < nc.n; c++) {
    src.add(nc.u32[c * CONN_STRIDE + 1])
    dst.add(nc.u32[c * CONN_STRIDE + 2])
  }
  return { src: [...src].sort((a, b) => a - b), dst: [...dst].sort((a, b) => a - b) }
}

const laneEnd = (d: NetworkData, lane: number): Pt => [d.laneF32[lane * 8 + 4], d.laneF32[lane * 8 + 5]]
const laneStart = (d: NetworkData, lane: number): Pt => [d.laneF32[lane * 8], d.laneF32[lane * 8 + 1]]

function nearest(pts: Pt[], ids: number[], wx: number, wy: number, tol: number): number | null {
  let best: number | null = null
  let bd = tol * tol
  for (let i = 0; i < ids.length; i++) {
    const dd = (pts[i][0] - wx) ** 2 + (pts[i][1] - wy) ** 2
    if (dd < bd) {
      bd = dd
      best = ids[i]
    }
  }
  return best
}

export function pickSrc(d: NetworkData, lanes: NodeLanes, wx: number, wy: number, tol: number) {
  return nearest(lanes.src.map((l) => laneEnd(d, l)), lanes.src, wx, wy, tol)
}
export function pickDst(d: NetworkData, lanes: NodeLanes, wx: number, wy: number, tol: number) {
  return nearest(lanes.dst.map((l) => laneStart(d, l)), lanes.dst, wx, wy, tol)
}

function polyline(g: Graphics, pts: Pt[]) {
  g.moveTo(pts[0][0], pts[0][1])
  for (let i = 1; i < pts.length; i++) g.lineTo(pts[i][0], pts[i][1])
}

/** Visualização e edição das conversões do nó selecionado (estilo TM:PE). Tamanhos em px de tela / zoom. */
export function drawTurns(g: Graphics, d: NetworkData, nc: NodeConns | null, node: number, zoom: number, view: TurnView) {
  g.clear()
  const px = 1 / zoom
  const nx = d.nodeXY[node * 2]
  const ny = d.nodeXY[node * 2 + 1]
  if (!nc) {
    // conexões ainda chegando do núcleo: só o nó selecionado com halo
    g.circle(nx, ny, 15 * px).fill({ color: C.nodeJunction, alpha: 0.22 })
    g.circle(nx, ny, 7.5 * px).fill(C.nodeJunction).stroke({ width: 2 * px, color: 0xffffff, alpha: 0.9 })
    return
  }
  const a = 0
  const b = nc.n
  const lanes = nodeLanes(nc)
  const color = (lane: number) => PALETTE[lanes.src.indexOf(lane) % PALETTE.length]
  const sample = (c: number): Pt[] => {
    const pts: Pt[] = []
    for (let i = 0; i <= 24; i++) pts.push(bez(nc.f32, c * 8, i / 24))
    return pts
  }
  const active = view.activeSrc

  // 1) proibidas por restrição OSM: tracejado vermelho fino
  for (let c = a; c < b; c++) {
    if (nc.u32[c * CONN_STRIDE + 4] !== CONN_BLOCKED) continue
    dashed(g, sample(c), 3 * px, 5 * px)
    g.stroke({ width: 1.6 * px, color: C.curveBlocked, alpha: active == null ? 0.6 : 0.25, cap: 'round' })
  }

  // 2) ligadas: contorno escuro + cor da faixa de origem
  const lit: number[] = []
  for (let c = a; c < b; c++) if (nc.u32[c * CONN_STRIDE + 4] === CONN_ALLOWED) lit.push(c)
  const alphaOf = (c: number) => (active == null || nc.u32[c * CONN_STRIDE + 1] === active ? 1 : 0.18)
  for (const c of lit) {
    polyline(g, sample(c))
    g.stroke({ width: 6.5 * px, color: 0x05090a, alpha: 0.65 * alphaOf(c), cap: 'round', join: 'round' })
  }
  for (const c of lit) {
    const hot = c === view.highlight
    polyline(g, sample(c))
    g.stroke({ width: (hot ? 5 : 3.4) * px, color: hot ? 0xffffff : color(nc.u32[c * CONN_STRIDE + 1]), alpha: alphaOf(c), cap: 'round', join: 'round' })
  }

  // 3) setas por faixa de entrada (manobras ligadas/proibidas)
  const byLane = new Map<number, { kind: number; ok: boolean }[]>()
  for (let c = a; c < b; c++) {
    const o = c * CONN_STRIDE
    const st = nc.u32[o + 4]
    if (st === CONN_ABSENT) continue
    const lane = nc.u32[o + 1]
    const arr = byLane.get(lane) ?? []
    byLane.set(lane, arr)
    arr.push({ kind: nc.u32[o + 3], ok: st === CONN_ALLOWED })
  }
  for (const [lane, list] of byLane) {
    const o = lane * 8
    const dx = d.laneF32[o + 6]
    const dy = d.laneF32[o + 7]
    const L = Math.min(Math.max(13 * px, 3.2 * zoom * px * 1.6), 34 * px)
    const back = 3.8 + L * 1.15 + 14 * px
    const bx = d.laneF32[o + 4] - dx * back
    const by = d.laneF32[o + 5] - dy * back
    for (const k of [...new Set(list.map((x) => x.kind))]) {
      const ok = list.some((x) => x.kind === k && x.ok)
      arrow(g, bx, by, dx, dy, k, L, ok ? C.arrow : C.curveBlocked, ok ? 0.95 : 0.6, 2.4 * px)
    }
  }

  // 4) destinos: pontos pequenos (preenchidos na cor da origem ativa quando ligados a ela)
  const activeDst = new Set<number>()
  if (active != null) {
    for (let c = a; c < b; c++) {
      if (nc.u32[c * CONN_STRIDE + 1] === active && nc.u32[c * CONN_STRIDE + 4] === CONN_ALLOWED) activeDst.add(nc.u32[c * CONN_STRIDE + 2])
    }
  }
  const dragging = view.drag != null
  for (const lane of lanes.dst) {
    const [x, y] = laneStart(d, lane)
    const on = activeDst.has(lane)
    const hover = dragging && view.drag!.dst === lane
    if (hover) g.circle(x, y, 11 * px).fill({ color: 0xffffff, alpha: 0.35 }).stroke({ width: 2 * px, color: 0xffffff })
    const r = on ? 5.5 : dragging || active != null ? 4.5 : 3.5
    g.circle(x, y, r * px).fill({ color: on ? color(active!) : C.laneOut, alpha: on || dragging || active != null ? 1 : 0.75 }).stroke({ width: 1.2 * px, color: 0x06130d })
  }

  // 5) origens: pontos grandes e coloridos, com contorno branco
  for (const lane of lanes.src) {
    const [x, y] = laneEnd(d, lane)
    const isActive = lane === active
    if (isActive) g.circle(x, y, 15 * px).stroke({ width: 2.2 * px, color: 0xffffff, alpha: 0.95 })
    g.circle(x, y, 9 * px).fill(color(lane)).stroke({ width: 2.4 * px, color: 0xffffff })
  }

  // 6) prévia do arrasto
  if (view.drag && active != null) {
    const o = active * 8
    const p0: Pt = [d.laneF32[o + 4], d.laneF32[o + 5]]
    const d0: Pt = [d.laneF32[o + 6], d.laneF32[o + 7]]
    let p3: Pt = [view.drag.x, view.drag.y]
    let d3: Pt | null = null
    if (view.drag.dst != null) {
      const q = view.drag.dst * 8
      p3 = [d.laneF32[q], d.laneF32[q + 1]]
      d3 = [d.laneF32[q + 2], d.laneF32[q + 3]]
    }
    const dist = Math.hypot(p3[0] - p0[0], p3[1] - p0[1])
    const k = dist * 0.4
    const e3: Pt = d3 ?? [(p3[0] - p0[0]) / (dist || 1), (p3[1] - p0[1]) / (dist || 1)]
    const f = new Float32Array([p0[0], p0[1], p0[0] + d0[0] * k, p0[1] + d0[1] * k, p3[0] - e3[0] * k, p3[1] - e3[1] * k, p3[0], p3[1]])
    const pts: Pt[] = []
    for (let i = 0; i <= 28; i++) pts.push(bez(f, 0, i / 28))
    polyline(g, pts)
    g.stroke({ width: 7 * px, color: 0x05090a, alpha: 0.6, cap: 'round' })
    polyline(g, pts)
    g.stroke({ width: 3.6 * px, color: 0xffffff, alpha: view.drag.dst != null ? 1 : 0.7, cap: 'round' })
  }

  // 7) nó selecionado com halo
  g.circle(nx, ny, 15 * px).fill({ color: C.nodeJunction, alpha: 0.22 })
  g.circle(nx, ny, 7.5 * px).fill(C.nodeJunction).stroke({ width: 2 * px, color: 0xffffff, alpha: 0.9 })
}

export function drawHover(g: Graphics, d: NetworkData, node: number | null, zoom: number) {
  g.clear()
  if (node == null) return
  const px = 1 / zoom
  g.circle(d.nodeXY[node * 2], d.nodeXY[node * 2 + 1], 11 * px).stroke({ width: 2 * px, color: C.nodeJunction, alpha: 0.9 })
}

/** Conversão do nó cuja curva passa a menos de `tol` (unidades do mundo) do ponto (wx, wy). */
export function pickConn(nc: NodeConns, wx: number, wy: number, tol: number): number | null {
  let best: number | null = null
  let bd = tol * tol
  for (let c = 0; c < nc.n; c++) {
    if (nc.u32[c * CONN_STRIDE + 4] === CONN_ABSENT) continue
    let [px, py] = bez(nc.f32, c * 8, 0)
    for (let i = 1; i <= 16; i++) {
      const [qx, qy] = bez(nc.f32, c * 8, i / 16)
      const vx = qx - px
      const vy = qy - py
      const l2 = vx * vx + vy * vy || 1
      const t = Math.max(0, Math.min(1, ((wx - px) * vx + (wy - py) * vy) / l2))
      const dx = px + vx * t - wx
      const dy = py + vy * t - wy
      const dd = dx * dx + dy * dy
      if (dd < bd) {
        bd = dd
        best = c
      }
      px = qx
      py = qy
    }
  }
  return best
}
