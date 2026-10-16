//! Conexões entre faixas em cada nó (estilo TM:PE), restrições OSM e semáforos.

use crate::geo::*;
use crate::net::*;
use crate::osm::RawOsm;
use std::collections::HashMap;

const LEFT: u8 = 1 << K_LEFT;
const STRAIGHT: u8 = 1 << K_STRAIGHT;
const RIGHT: u8 = 1 << K_RIGHT;
const UTURN: u8 = 1 << K_UTURN;

/// Classifica a manobra entre a direção de chegada e a de saída (coordenadas Y para baixo).
fn classify(in_dir: V2, out_dir: V2) -> u8 {
    let ang = cross(in_dir, out_dir).atan2(dot(in_dir, out_dir)).to_degrees();
    if ang.abs() < 30.0 {
        K_STRAIGHT
    } else if ang > 0.0 {
        K_RIGHT
    } else {
        K_LEFT
    }
}

fn lane_masks(tags: &[u8], n: usize, present: u8) -> Vec<u8> {
    let any_tag = tags.iter().any(|&t| t != 0);
    let mut m = vec![0u8; n];
    for i in 0..n {
        let t = tags.get(i).copied().unwrap_or(0);
        m[i] = if t != 0 {
            t
        } else if n == 1 {
            15
        } else if i == 0 {
            LEFT | if n == 2 { STRAIGHT } else { 0 }
        } else if i == n - 1 {
            RIGHT | if n == 2 { STRAIGHT } else { 0 }
        } else {
            STRAIGHT
        };
    }
    if !any_tag {
        if present & LEFT != 0 {
            m[0] |= LEFT;
        }
        if present & RIGHT != 0 {
            m[n - 1] |= RIGHT;
        }
        if present & STRAIGHT != 0 && !m.iter().any(|x| x & STRAIGHT != 0) {
            m[n / 2] |= STRAIGHT;
        }
    }
    for i in 0..n {
        m[i] &= present;
        if m[i] == 0 {
            let order: [u8; 4] = if i == 0 {
                [STRAIGHT, LEFT, RIGHT, UTURN]
            } else if i == n - 1 {
                [STRAIGHT, RIGHT, LEFT, UTURN]
            } else {
                [STRAIGHT, RIGHT, LEFT, UTURN]
            };
            if let Some(b) = order.iter().find(|b| present & **b != 0) {
                m[i] = *b;
            }
        }
    }
    m
}

fn target_index(kind: u8, t: usize, m: usize, lane_idx: usize, n_in: usize, n_out: usize) -> usize {
    let last = n_out - 1;
    match kind {
        K_LEFT => t.min(last),
        K_RIGHT => last - (m - 1 - t).min(last),
        K_UTURN => 0,
        _ => {
            if m == n_out {
                t
            } else if m > 1 {
                ((t as f64) * last as f64 / (m - 1) as f64).round() as usize
            } else if n_in > 1 {
                ((lane_idx as f64) * last as f64 / (n_in - 1) as f64).round() as usize
            } else {
                last / 2
            }
        }
    }
    .min(last)
}

fn bezier(p0: [f32; 2], d0: [f32; 2], p3: [f32; 2], d3: [f32; 2], kind: u8) -> [f32; 8] {
    let dx = p3[0] - p0[0];
    let dy = p3[1] - p0[1];
    let dist = (dx * dx + dy * dy).sqrt();
    let k = match kind {
        K_STRAIGHT => dist * 0.33,
        K_UTURN => 5.0,
        _ => dist * 0.45,
    };
    [p0[0], p0[1], p0[0] + d0[0] * k, p0[1] + d0[1] * k, p3[0] - d3[0] * k, p3[1] - d3[1] * k, p3[0], p3[1]]
}

pub fn build_connections(net: &mut Net, raw: &RawOsm) {
    let mut conns: Vec<Conn> = Vec::new();
    let mut off = Vec::with_capacity(net.nodes.len() + 1);

    for ni in 0..net.nodes.len() {
        off.push(conns.len() as u32);
        let ends = net.nodes[ni].ends.clone();
        let n = ends.len();
        let ap: Vec<(Vec<u32>, Vec<u32>)> = ends.iter().map(|&e| net.approach_lanes(e)).collect();
        let outw: Vec<V2> = ends.iter().map(|&e| net.outward(e)).collect();

        for a in 0..n {
            let ins = &ap[a].0;
            if ins.is_empty() {
                continue;
            }
            // alvos: outras pontas com faixas de saída
            let mut targets: Vec<(usize, u8)> = Vec::new();
            for b in 0..n {
                if b == a || ap[b].1.is_empty() {
                    continue;
                }
                targets.push((b, classify(mul(outw[a], -1.0), outw[b])));
            }
            // beco sem saída: retorno
            if n == 1 && !ap[a].1.is_empty() {
                targets.push((a, K_UTURN));
            }
            let present = targets.iter().fold(0u8, |m, t| m | (1 << t.1));
            if present == 0 {
                continue;
            }
            let tags: Vec<u8> = ins.iter().map(|&l| net.lanes[l as usize].tmask).collect();
            let masks = lane_masks(&tags, ins.len(), present);
            for kind in 0..4u8 {
                let bit = 1u8 << kind;
                let who: Vec<usize> = (0..ins.len()).filter(|&i| masks[i] & bit != 0).collect();
                for (t, &li) in who.iter().enumerate() {
                    for &(b, k) in targets.iter().filter(|x| x.1 == kind) {
                        let _ = k;
                        let outs = &ap[b].1;
                        let oi = target_index(kind, t, who.len(), li, ins.len(), outs.len());
                        let fl = ins[li];
                        let tl = outs[oi];
                        let (l0, l1) = (&net.lanes[fl as usize], &net.lanes[tl as usize]);
                        conns.push(Conn {
                            node: ni as u32,
                            from_lane: fl,
                            to_lane: tl,
                            kind,
                            allowed: true,
                            curve: bezier(l0.end, l0.edir, l1.start, l1.sdir, kind),
                        });
                    }
                }
            }
        }
    }
    off.push(conns.len() as u32);
    net.conns = conns;
    net.conn_off = off;
    apply_restrictions(net, raw);
}

fn apply_restrictions(net: &mut Net, raw: &RawOsm) {
    let node_by_osm: HashMap<i64, u32> = net.nodes.iter().enumerate().map(|(i, n)| (n.osm_id, i as u32)).collect();
    for r in &raw.restrictions {
        let Some(&ni) = node_by_osm.get(&r.via) else { continue };
        let (a, b) = (net.conn_off[ni as usize] as usize, net.conn_off[ni as usize + 1] as usize);
        for ci in a..b {
            let c = &net.conns[ci];
            let in_way = net.segs[net.lanes[c.from_lane as usize].seg as usize].way_id;
            let out_way = net.segs[net.lanes[c.to_lane as usize].seg as usize].way_id;
            if in_way != r.from {
                continue;
            }
            let hit = if r.kind.starts_with("only_") { out_way != r.to } else if r.kind.starts_with("no_") { out_way == r.to } else { false };
            if hit {
                net.conns[ci].allowed = false;
            }
        }
    }
}

/// Fases padrão: uma por eixo de aproximação.
pub fn build_signals(net: &mut Net) {
    let mut signals = Vec::new();
    for (ni, node) in net.nodes.iter().enumerate() {
        if !node.signal {
            continue;
        }
        let mut approaches: Vec<(u32, f64)> = Vec::new();
        for &e in &node.ends {
            let (ins, _) = net.approach_lanes(e);
            if ins.is_empty() {
                continue;
            }
            let u = net.outward(e);
            approaches.push((e, u[1].atan2(u[0])));
        }
        if approaches.is_empty() {
            continue;
        }
        let mut groups: Vec<(f64, Vec<u32>)> = Vec::new();
        for (e, ang) in &approaches {
            let found = groups.iter_mut().find(|g| {
                let d = (g.0 - ang).rem_euclid(std::f64::consts::PI);
                d.min(std::f64::consts::PI - d) < 0.6
            });
            match found {
                Some(g) => g.1.push(*e),
                None => groups.push((*ang, vec![*e])),
            }
        }
        let phases = groups.into_iter().map(|g| Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: g.1 }).collect();
        let mut ends = Vec::new();
        for (e, _) in &approaches {
            let s = &net.segs[(*e >> 1) as usize];
            let l = s.poly.length();
            let (sv, lat, sign) = if e & 1 == 1 {
                (l - s.cut_to, s.p0() + s.lf as f64 * s.w + 0.9, 1.0)
            } else {
                (s.cut_from, -(s.lb as f64) * s.w - 0.9, -1.0)
            };
            let (p, t) = s.poly.at(sv);
            let q = add(p, mul(right(t), lat));
            let d = mul(t, sign);
            ends.push(SignalEnd { end: *e, pos: [q[0] as f32, q[1] as f32], angle: d[1].atan2(d[0]) as f32 });
        }
        signals.push(Signal { node: ni as u32, phases, ends });
    }
    net.signals = signals;
}
