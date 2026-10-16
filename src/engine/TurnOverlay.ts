import { Graphics } from 'pixi.js'
import type { NetworkData } from '../model/types'
import { CONN_STRIDE, KIND_RIGHT, KIND_STRAIGHT, KIND_UTURN } from '../model/types'
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

/** Visualização das conversões do nó selecionado (estilo TM:PE). Tamanhos em px de tela / zoom. */
export function drawTurns(g: Graphics, d: NetworkData, node: number, zoom: number, highlight: number | null = null) {
  g.clear()
  const px = 1 / zoom
  const a = d.connOff[node]
  const b = d.connOff[node + 1]
  const nx = d.nodeXY[node * 2]
  const ny = d.nodeXY[node * 2 + 1]

  // curvas tracejadas (bloqueadas primeiro, para ficarem abaixo)
  for (const pass of [0, 1]) {
    for (let c = a; c < b; c++) {
      const allowed = d.connU32[c * CONN_STRIDE + 4] === 1
      if ((pass === 1) !== allowed) continue
      const pts: Pt[] = []
      for (let i = 0; i <= 24; i++) pts.push(bez(d.connF32, c * 8, i / 24))
      if (allowed) dashed(g, pts, 7 * px, 4 * px)
      else dashed(g, pts, 3 * px, 5 * px)
      const hot = c === highlight
      g.stroke({ width: (hot ? 4 : allowed ? 2.4 : 1.6) * px, color: hot ? 0xffffff : allowed ? C.curveOk : C.curveBlocked, alpha: hot ? 1 : allowed ? 0.95 : 0.55, cap: 'round' })
    }
  }

  // setas por faixa de entrada
  const byLane = new Map<number, { kind: number; allowed: boolean }[]>()
  const outLanes = new Set<number>()
  for (let c = a; c < b; c++) {
    const o = c * CONN_STRIDE
    const lane = d.connU32[o + 1]
    outLanes.add(d.connU32[o + 2])
    let arr = byLane.get(lane)
    if (!arr) byLane.set(lane, (arr = []))
    arr.push({ kind: d.connU32[o + 3], allowed: d.connU32[o + 4] === 1 })
  }
  for (const [lane, list] of byLane) {
    const o = lane * 8
    const ex = d.laneF32[o + 4]
    const ey = d.laneF32[o + 5]
    const dx = d.laneF32[o + 6]
    const dy = d.laneF32[o + 7]
    const L = Math.min(Math.max(13 * px, 3.2 * zoom * px * 1.6), 34 * px) // ~ largura da faixa
    // recua além da faixa de pedestres (~3,6 m) para a seta ficar sobre a pista
    const back = 3.8 + L * 1.15 + 6 * px
    const bx = ex - dx * back
    const by = ey - dy * back
    for (const k of [...new Set(list.map((x) => x.kind))]) {
      const ok = list.some((x) => x.kind === k && x.allowed)
      arrow(g, bx, by, dx, dy, k, L, ok ? C.arrow : C.curveBlocked, ok ? 1 : 0.6, 2.4 * px)
    }
  }

  // pontos nas pontas das faixas
  for (const lane of byLane.keys()) {
    const o = lane * 8
    g.circle(d.laneF32[o + 4], d.laneF32[o + 5], 5 * px).fill(C.laneIn).stroke({ width: 1.5 * px, color: 0x06130d })
  }
  for (const lane of outLanes) {
    const o = lane * 8
    g.circle(d.laneF32[o], d.laneF32[o + 1], 4.5 * px).fill(C.laneOut).stroke({ width: 1.5 * px, color: 0x06130d })
  }

  // nó selecionado com halo
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
export function pickConn(d: NetworkData, node: number, wx: number, wy: number, tol: number): number | null {
  let best: number | null = null
  let bd = tol * tol
  for (let c = d.connOff[node]; c < d.connOff[node + 1]; c++) {
    let [px, py] = bez(d.connF32, c * 8, 0)
    for (let i = 1; i <= 16; i++) {
      const [qx, qy] = bez(d.connF32, c * 8, i / 16)
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
