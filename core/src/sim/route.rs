//! Roteamento: grafo de trechos direcionados ("dirseg" = trecho*2 + sentido) cujas transições são as
//! conversões ligadas. Um Dijkstra **reverso** por zona de destino gera `próximo[dirseg]`.

use crate::net::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub const NONE: u32 = u32::MAX;

#[derive(Copy, Clone, PartialEq)]
struct Item(f32, u32);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal).then(o.1.cmp(&self.1))
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

pub fn dirseg_of_lane(net: &Net, lane: u32) -> u32 {
    let l = &net.lanes[lane as usize];
    l.seg * 2 + l.dir as u32
}

/// Nó de chegada de um trecho direcionado.
pub fn dirseg_end_node(net: &Net, d: u32) -> u32 {
    let s = &net.segs[(d >> 1) as usize];
    if d & 1 == 0 {
        s.to
    } else {
        s.from
    }
}

/// Velocidade livre (m/s) usada no roteamento e como `v0` base da via.
pub fn free_speed(s: &Seg) -> f32 {
    let kmh = if s.maxspeed > 0 {
        s.maxspeed as f32
    } else {
        match s.hw {
            HW_MOTORWAY => 90.0,
            HW_TRUNK => 80.0,
            HW_PRIMARY => 60.0,
            HW_SECONDARY => 50.0,
            HW_TERTIARY => 40.0,
            HW_SERVICE => 20.0,
            _ => 30.0,
        }
    };
    kmh.min(110.0) / 3.6
}

pub struct Router {
    /// tempo de percurso livre de cada trecho direcionado (s); infinito se o sentido não existe
    pub time: Vec<f32>,
    /// adjacência reversa (CSR): quem chega em B
    radj_off: Vec<u32>,
    radj: Vec<(u32, f32)>,
}

fn turn_penalty(net: &Net, kind: u8, node: u32) -> f32 {
    let n = &net.nodes[node as usize];
    let base = match kind {
        K_LEFT => 5.0,
        K_RIGHT => 2.0,
        K_UTURN => 60.0,
        _ => 0.0,
    };
    let junction = if n.signal { 14.0 } else if n.ends.len() >= 3 { 3.0 } else { 0.0 };
    base + junction
}

impl Router {
    pub fn new(net: &Net) -> Router {
        let nd = net.segs.len() * 2;
        let mut time = vec![f32::INFINITY; nd];
        for (i, s) in net.segs.iter().enumerate() {
            let len = (s.poly.length() as f32).max(1.0);
            let t = len / (free_speed(s) * 0.85);
            if s.lf > 0 {
                time[i * 2] = t;
            }
            if s.lb > 0 {
                time[i * 2 + 1] = t;
            }
        }
        // arestas A -> B (menor penalidade entre conversões ligadas)
        let mut edges: Vec<(u32, u32, f32)> = Vec::with_capacity(net.conns.len());
        for c in &net.conns {
            if c.state != CONN_ALLOWED {
                continue;
            }
            let a = dirseg_of_lane(net, c.from_lane);
            let b = dirseg_of_lane(net, c.to_lane);
            edges.push((b, a, turn_penalty(net, c.kind, c.node)));
        }
        edges.sort_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)).then(x.2.partial_cmp(&y.2).unwrap()));
        edges.dedup_by(|x, y| x.0 == y.0 && x.1 == y.1);
        let mut radj_off = vec![0u32; nd + 1];
        for e in &edges {
            radj_off[e.0 as usize + 1] += 1;
        }
        for i in 0..nd {
            radj_off[i + 1] += radj_off[i];
        }
        let radj: Vec<(u32, f32)> = edges.iter().map(|e| (e.1, e.2)).collect();
        Router { time, radj_off, radj }
    }

    /// Dijkstra reverso a partir dos trechos que terminam num nó-destino.
    /// `dist[A]` = tempo de A até sair do último trecho; `hop[A]` = próximo trecho (NONE se A já é alvo ou não há caminho).
    pub fn tree(&self, targets: &[u32], hop: &mut Vec<u32>, dist: &mut Vec<f32>) {
        let nd = self.time.len();
        hop.clear();
        hop.resize(nd, NONE);
        dist.clear();
        dist.resize(nd, f32::INFINITY);
        let mut heap = BinaryHeap::new();
        for &t in targets {
            if self.time[t as usize].is_finite() {
                dist[t as usize] = self.time[t as usize];
                heap.push(Item(dist[t as usize], t));
            }
        }
        while let Some(Item(d, b)) = heap.pop() {
            if d > dist[b as usize] {
                continue;
            }
            let (lo, hi) = (self.radj_off[b as usize] as usize, self.radj_off[b as usize + 1] as usize);
            for &(a, pen) in &self.radj[lo..hi] {
                if !self.time[a as usize].is_finite() {
                    continue;
                }
                let nd2 = d + self.time[a as usize] + pen;
                if nd2 < dist[a as usize] {
                    dist[a as usize] = nd2;
                    hop[a as usize] = b;
                    heap.push(Item(nd2, a));
                }
            }
        }
    }
}
