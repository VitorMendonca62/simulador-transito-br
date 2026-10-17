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
fn angle(in_dir: V2, out_dir: V2) -> f64 {
    cross(in_dir, out_dir).atan2(dot(in_dir, out_dir)).to_degrees()
}

fn classify(in_dir: V2, out_dir: V2) -> u8 {
    let ang = angle(in_dir, out_dir);
    if ang.abs() < 30.0 {
        K_STRAIGHT
    } else if ang > 0.0 {
        K_RIGHT
    } else {
        K_LEFT
    }
}

/// Quando várias saídas têm a mesma manobra (bifurcação em Y: dois ramos "em frente"), cada uma recebe um grupo
/// contíguo de faixas, da esquerda para a direita na mesma ordem dos ramos; a faixa do meio é compartilhada
/// quando a divisão não é exata. Com menos faixas que ramos, cada faixa leva a todos os ramos que lhe cabem.
/// Retorna o intervalo `lo..hi` das `m` faixas que vão para o ramo `j` de `k` (ordenados da esquerda para a direita).
fn lane_group(m: usize, k: usize, j: usize) -> (usize, usize) {
    if k <= 1 || m == 0 {
        return (0, m);
    }
    (j * m / k, ((j + 1) * m + k - 1) / k)
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

/// Restrições OSM indexadas pelo nó "via" (id OSM) -> índices em `raw.restrictions`.
pub fn restriction_index(raw: &RawOsm) -> HashMap<i64, Vec<usize>> {
    let mut m: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in raw.restrictions.iter().enumerate() {
        m.entry(r.via).or_default().push(i);
    }
    m
}

/// Chave estável de uma faixa: way:de:para.sentido.posição (orientação original do trecho).
pub fn lane_key(net: &Net, lane: u32) -> String {
    let l = &net.lanes[lane as usize];
    let k = net.segs[l.seg as usize].key;
    format!("{}:{}:{}.{}.{}", k[0], k[1], k[2], l.dir, l.idx)
}

/// Chave estável de uma conversão: nóOsm|faixaOrigem>faixaDestino.
pub fn conn_key(net: &Net, node: u32, from: u32, to: u32) -> String {
    format!("{}|{}>{}", net.nodes[node as usize].osm_id, lane_key(net, from), lane_key(net, to))
}

/// Todas as conexões do nó `ni` (padrão, candidatas e proibidas por restrição), já com as edições do usuário.
pub fn node_conns(net: &Net, ni: usize, raw: &RawOsm, restr: &HashMap<i64, Vec<usize>>, edits: &Edits) -> Vec<Conn> {
    let mut conns: Vec<Conn> = Vec::new();
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
            // ramos desta manobra, da esquerda para a direita
            let mut tg: Vec<(usize, f64)> = targets.iter().filter(|x| x.1 == kind).map(|&(b, _)| (b, if b == a { 0.0 } else { angle(mul(outw[a], -1.0), outw[b]) })).collect();
            tg.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
            for (j, &(b, _)) in tg.iter().enumerate() {
                let (lo, hi) = lane_group(who.len(), tg.len(), j);
                let sub = &who[lo..hi];
                for (t, &li) in sub.iter().enumerate() {
                    let outs = &ap[b].1;
                    let oi = target_index(kind, t, sub.len(), li, ins.len(), outs.len());
                    let (fl, tl) = (ins[li], outs[oi]);
                    let (l0, l1) = (&net.lanes[fl as usize], &net.lanes[tl as usize]);
                    conns.push(Conn { node: ni as u32, from_lane: fl, to_lane: tl, kind, state: CONN_ALLOWED, base: CONN_ALLOWED, curve: bezier(l0.end, l0.edir, l1.start, l1.sdir, kind) });
                }
            }
        }
    }

    // candidatas: todo par entrada -> saída que ainda não existe (estado ausente)
    let existing: std::collections::HashSet<(u32, u32)> = conns.iter().map(|c| (c.from_lane, c.to_lane)).collect();
    for a in 0..n {
        for b in 0..n {
            let kind = if a == b { K_UTURN } else { classify(mul(outw[a], -1.0), outw[b]) };
            for &fl in &ap[a].0 {
                for &tl in &ap[b].1 {
                    if existing.contains(&(fl, tl)) {
                        continue;
                    }
                    let (l0, l1) = (&net.lanes[fl as usize], &net.lanes[tl as usize]);
                    conns.push(Conn { node: ni as u32, from_lane: fl, to_lane: tl, kind, state: CONN_ABSENT, base: CONN_ABSENT, curve: bezier(l0.end, l0.edir, l1.start, l1.sdir, kind) });
                }
            }
        }
    }

    // restrições do OSM
    let osm = net.nodes[ni].osm_id;
    if let Some(list) = restr.get(&osm) {
        for &ri in list {
            let r = &raw.restrictions[ri];
            for c in conns.iter_mut() {
                let in_way = net.segs[net.lanes[c.from_lane as usize].seg as usize].way_id;
                let out_way = net.segs[net.lanes[c.to_lane as usize].seg as usize].way_id;
                if in_way != r.from {
                    continue;
                }
                let hit = if r.kind.starts_with("only_") { out_way != r.to } else if r.kind.starts_with("no_") { out_way == r.to } else { false };
                if hit && c.state == CONN_ALLOWED {
                    c.state = CONN_BLOCKED;
                }
            }
        }
    }

    for c in conns.iter_mut() {
        c.base = c.state;
    }

    // edições do usuário (ligar/remover conversões)
    if let Some(list) = edits.conns.get(&osm) {
        for c in conns.iter_mut() {
            let key = conn_key(net, ni as u32, c.from_lane, c.to_lane);
            if let Some((_, st)) = list.iter().find(|(k, _)| *k == key) {
                c.state = *st;
            }
        }
    }
    conns
}

/// Constrói as conexões de todos os nós (usado pelos testes e pela simulação; a interface pede por nó).
pub fn build_connections(net: &mut Net, raw: &RawOsm, edits: &Edits) {
    let restr = restriction_index(raw);
    let mut all: Vec<Conn> = Vec::new();
    let mut off = Vec::with_capacity(net.nodes.len() + 1);
    for ni in 0..net.nodes.len() {
        off.push(all.len() as u32);
        all.extend(node_conns(net, ni, raw, &restr, edits));
    }
    off.push(all.len() as u32);
    net.conns = all;
    net.conn_off = off;
}

/// Semáforos: fases padrão (uma por eixo de aproximação), substituídas pela configuração do usuário quando houver.
pub fn build_signals(net: &mut Net, edits: &Edits) {
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
        let edit = edits.sigs.get(&node.osm_id);
        let mut phases: Vec<Phase> = match edit.and_then(|e| e.phases.as_ref()) {
            Some(list) => {
                let keys: Vec<(u32, String)> = approaches.iter().map(|(e, _)| (*e, approach_key(net, ni as u32, *e))).collect();
                let mut ps: Vec<Phase> = list
                    .iter()
                    .map(|p| Phase {
                        green: p.green.clamp(0.0, 600.0),
                        yellow: p.yellow.clamp(0.0, 60.0),
                        all_red: p.all_red.clamp(0.0, 60.0),
                        ends: p.approaches.iter().filter_map(|k| keys.iter().find(|x| &x.1 == k).map(|x| x.0)).collect(),
                    })
                    .collect();
                // toda aproximação precisa ser atendida: as não listadas ganham uma fase própria
                let listed: std::collections::HashSet<u32> = ps.iter().flat_map(|p| p.ends.iter().copied()).collect();
                let rest: Vec<u32> = approaches.iter().map(|a| a.0).filter(|e| !listed.contains(e)).collect();
                if !rest.is_empty() {
                    ps.push(Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: rest });
                }
                ps
            }
            None => groups.into_iter().map(|g| Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: g.1 }).collect(),
        };
        if phases.is_empty() {
            phases.push(Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: approaches.iter().map(|a| a.0).collect() });
        }
        // semáforo em trecho (sem cruzamento): fase própria para a travessia de pedestres, que para os dois sentidos
        if edit.map_or(true, |e| e.phases.is_none()) && approaches.len() == 2 && phases.len() == 1 {
            phases.push(Phase { green: 12.0, yellow: 0.0, all_red: 0.0, ends: Vec::new() });
        }
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
            ends.push(SignalEnd { end: *e, key: approach_key(net, ni as u32, *e), pos: [q[0] as f32, q[1] as f32], angle: d[1].atan2(d[0]) as f32 });
        }
        signals.push(Signal { node: ni as u32, offset: edit.map_or(0.0, |e| e.offset) as f32, phases, ends });
    }
    net.signals = signals;
}

#[cfg(test)]
mod tests {
    use super::lane_group;

    #[test]
    fn fork_splits_lanes_left_to_right() {
        // 4 faixas, 2 ramos: 2 + 2
        assert_eq!((lane_group(4, 2, 0), lane_group(4, 2, 1)), ((0, 2), (2, 4)));
        // 3 faixas, 2 ramos: a do meio é compartilhada, as das pontas não cruzam
        assert_eq!((lane_group(3, 2, 0), lane_group(3, 2, 1)), ((0, 2), (1, 3)));
        // 1 faixa: leva a todos os ramos; 2 faixas, 3 ramos: ninguém fica sem faixa
        assert_eq!((lane_group(1, 2, 0), lane_group(1, 2, 1)), ((0, 1), (0, 1)));
        assert_eq!((lane_group(2, 3, 0), lane_group(2, 3, 1), lane_group(2, 3, 2)), ((0, 1), (0, 2), (1, 2)));
        // um só ramo: todas as faixas
        assert_eq!(lane_group(5, 1, 0), (0, 5));
    }
}
