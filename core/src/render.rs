//! Geração das malhas estáticas (asfalto, marcações) a partir do grafo, em ladrilhos.
//!
//! Os ladrilhos (1 km) permitem enviar ao desenho só o que mudou depois de uma edição: cada trecho/nó
//! tem uma assinatura (hash do que influencia o seu desenho); comparando com a rede anterior achamos
//! os ladrilhos sujos e regeneramos apenas esses.

use crate::geo::*;
use crate::net::*;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Hash rápido (FxHash) para os mapas de assinatura: chaves são inteiros pequenos, sem ataque a considerar.
#[derive(Default, Clone, Copy)]
pub struct Fx(u64);
impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517cc1b727220a95);
        }
    }
    fn write_i64(&mut self, v: i64) {
        self.0 = (self.0.rotate_left(5) ^ v as u64).wrapping_mul(0x517cc1b727220a95);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517cc1b727220a95);
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}
type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

pub const TILE_M: f64 = 1024.0;

/// Identificador do ladrilho que contém `p` (x,y empacotados em 16 bits cada).
pub fn tile_of(p: V2) -> u32 {
    let tx = (p[0] / TILE_M).floor() as i32;
    let ty = (p[1] / TILE_M).floor() as i32;
    (((tx + 32768) as u32 & 0xffff) << 16) | ((ty + 32768) as u32 & 0xffff)
}

/// Conteúdo desenhável de um ladrilho.
#[derive(Default)]
pub struct TileMesh {
    pub asphalt: Mesh,
    pub bus: Mesh,
    pub white: Mesh,
    pub yellow: Mesh,
    /// pontos (xy) dos vértices intermediários das vias
    pub shape: Vec<f32>,
    /// nós de grau < 3 e cruzamentos (grau >= 3)
    pub minor: Vec<f32>,
    pub major: Vec<f32>,
}

struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf29ce484222325)
    }
    fn u(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn f(&mut self, v: f64) {
        self.u(v.to_bits());
    }
}

fn seg_hash(net: &Net, s: &Seg) -> u64 {
    let mut h = Fnv::new();
    for k in s.key {
        h.u(k as u64);
    }
    h.u(s.hw as u64 | (s.oneway as u64) << 8 | (s.roundabout as u64) << 9 | (s.link as u64) << 10 | (s.lf as u64) << 16 | (s.lb as u64) << 24);
    h.f(s.w);
    h.f(s.cut_from);
    h.f(s.cut_to);
    for b in s.bus_f.iter().chain(&s.bus_b) {
        h.u(*b as u64);
    }
    for p in &s.poly.pts {
        h.f(p[0]);
        h.f(p[1]);
    }
    // marcações de ponta dependem do nó vizinho
    for ni in [s.from, s.to] {
        let n = &net.nodes[ni as usize];
        h.u(n.signal as u64 | (n.crossing as u64) << 1 | (n.ends.len() as u64) << 8);
    }
    h.0
}

fn node_hash(net: &Net, ni: usize, seg_h: &[u64]) -> u64 {
    let n = &net.nodes[ni];
    let mut h = Fnv::new();
    h.u(n.osm_id as u64);
    h.f(n.pos[0]);
    h.f(n.pos[1]);
    h.u(n.signal as u64 | (n.crossing as u64) << 1);
    for &e in &n.ends {
        h.u(e as u64);
        h.u(seg_h[(e >> 1) as usize]);
    }
    h.0
}

fn emit_seg(net: &Net, s: &Seg, m: &mut TileMesh) {
    let l = s.poly.length();
    // vértices intermediários (nós de forma)
    let n = s.poly.pts.len();
    for p in s.poly.pts.iter().take(n.saturating_sub(1)).skip(1) {
        m.shape.push(p[0] as f32);
        m.shape.push(p[1] as f32);
    }
    let (s0, s1) = (s.cut_from, l - s.cut_to);
    if s1 - s0 < 0.5 {
        return;
    }
    let sl = s.poly.slice(s0, s1);
    if sl.len() < 2 {
        return;
    }
    let (left, right_e, w, p0) = (s.left_ext(), s.right_ext(), s.w, s.p0());
    m.asphalt.strip(&sl, -left, right_e);

    for i in 0..s.lf as usize {
        if s.bus_f[i] {
            let a = p0 + i as f64 * w;
            m.bus.strip(&sl, a, a + w);
        }
    }
    for j in 0..s.lb as usize {
        if s.bus_b[j] {
            m.bus.strip(&sl, -(j as f64 + 1.0) * w, -(j as f64) * w);
        }
    }

    // separadores brancos entre faixas do mesmo sentido
    for k in 1..s.lf as i32 {
        m.white.dashes(&offset(&sl, p0 + k as f64 * w), 3.0, 5.0, 0.13);
    }
    for k in 1..s.lb as i32 {
        m.white.dashes(&offset(&sl, -(k as f64) * w), 3.0, 5.0, 0.13);
    }

    // eixo amarelo entre sentidos opostos
    if !s.oneway {
        let big = s.hw <= HW_SECONDARY || s.lf as u32 + s.lb as u32 >= 4;
        if big {
            m.yellow.strip(&sl, -0.34, -0.12);
            m.yellow.strip(&sl, 0.12, 0.34);
        } else if s.hw != HW_SERVICE {
            m.yellow.dashes(&sl, 3.0, 5.0, 0.13);
        }
    }

    // linhas de borda em vias principais
    if s.hw <= HW_SECONDARY && !s.link {
        m.white.strip(&sl, -left + 0.3, -left + 0.5);
        m.white.strip(&sl, right_e - 0.5, right_e - 0.3);
    }

    // faixa de retenção e faixa de pedestres nas pontas
    let inner_len = s1 - s0;
    for at_to in [false, true] {
        let ni = if at_to { s.to } else { s.from };
        let node = &net.nodes[ni as usize];
        let deg = node.ends.len();
        let mw = s.hw == HW_MOTORWAY || s.link;
        let has_in = if at_to { s.lf > 0 } else { s.lb > 0 };
        let zebra = (node.signal || node.crossing) && deg >= 2 && !mw;
        let stop = deg >= 3 && has_in && !mw && !s.roundabout;
        let (se, dir) = if at_to { (s1, -1.0) } else { (s0, 1.0) };
        let mut cursor = 0.4;
        if zebra && inner_len > 7.0 {
            let (a, b) = (se + dir * cursor, se + dir * (cursor + 2.4));
            let part = s.poly.slice(a.min(b), a.max(b));
            let mut p = -left + 0.45;
            while p + 0.225 <= right_e {
                m.white.strip(&part, p - 0.225, p + 0.225);
                p += 0.9;
            }
            cursor += 3.0;
        }
        if stop && inner_len > cursor + 2.0 {
            let (a, b) = (se + dir * cursor, se + dir * (cursor + 0.5));
            let part = s.poly.slice(a.min(b), a.max(b));
            let (lo, hi) = if at_to { (p0 + 0.15, p0 + s.lf as f64 * w - 0.15) } else { (-(s.lb as f64) * w + 0.15, -0.15) };
            m.white.strip(&part, lo, hi);
        }
    }
}

fn emit_node(net: &Net, ni: usize, m: &mut TileMesh) {
    let node = &net.nodes[ni];
    let dots = if node.ends.len() >= 3 { &mut m.major } else { &mut m.minor };
    dots.push(node.pos[0] as f32);
    dots.push(node.pos[1] as f32);
    let n = node.ends.len();
    if n < 2 {
        return;
    }
    let mut ring: Vec<V2> = Vec::with_capacity(n * 2);
    for &e in &node.ends {
        let s = &net.segs[(e >> 1) as usize];
        let l = s.poly.length();
        let (sv, sign) = if e & 1 == 0 { (s.cut_from, 1.0) } else { (l - s.cut_to, -1.0) };
        let (p, t) = s.poly.at(sv);
        let u = mul(t, sign);
        let (cw, ccw) = net.ext_cw_ccw(e);
        ring.push(sub(p, mul(right(u), ccw)));
        ring.push(add(p, mul(right(u), cw)));
    }
    m.asphalt.fan(node.pos, &ring);
}

/// Conjunto de ladrilhos + assinaturas para detectar o que mudou entre reconstruções.
#[derive(Default)]
pub struct TileSet {
    pub tiles: HashMap<u32, TileMesh>,
    seg_sig: FastMap<SegKey, (u64, u32)>,
    node_sig: FastMap<i64, (u64, u32)>,
}

/// Ladrilhos regenerados e ladrilhos que ficaram vazios na última atualização.
#[derive(Default, Clone, Debug)]
pub struct TileDelta {
    pub dirty: Vec<u32>,
    pub removed: Vec<u32>,
}

impl TileDelta {
    /// Acumula uma atualização posterior (o JS só consome o delta depois de várias construções).
    pub fn merge(&mut self, next: TileDelta, existing: impl Fn(u32) -> bool) {
        for id in next.dirty {
            if !self.dirty.contains(&id) {
                self.dirty.push(id);
            }
            self.removed.retain(|r| *r != id);
        }
        for id in next.removed {
            self.dirty.retain(|d| *d != id);
            if !self.removed.contains(&id) {
                self.removed.push(id);
            }
        }
        self.dirty.retain(|d| existing(*d));
    }
}

impl TileSet {
    pub fn asphalt_tris(&self) -> usize {
        self.tiles.values().map(|t| t.asphalt.idx.len() / 3).sum()
    }
    pub fn vertex_count(&self, pick: impl Fn(&TileMesh) -> usize) -> usize {
        self.tiles.values().map(pick).sum()
    }
}

/// Atualiza os ladrilhos para a rede `net`. Com `prev = None` tudo é gerado.
pub fn update_tiles(net: &Net, prev: Option<TileSet>) -> (TileSet, TileDelta) {
    let seg_h: Vec<u64> = net.segs.iter().map(|s| seg_hash(net, s)).collect();
    let seg_tile: Vec<u32> = net.segs.iter().map(|s| tile_of(s.poly.at(s.poly.length() / 2.0).0)).collect();
    let node_h: Vec<u64> = (0..net.nodes.len()).map(|i| node_hash(net, i, &seg_h)).collect();
    let node_tile: Vec<u32> = net.nodes.iter().map(|n| tile_of(n.pos)).collect();

    let mut set = prev.unwrap_or_default();
    let had_prev = !set.seg_sig.is_empty() || !set.node_sig.is_empty();
    let mut dirty: std::collections::BTreeSet<u32> = Default::default();

    let mut new_seg_sig: FastMap<SegKey, (u64, u32)> = FastMap::with_capacity_and_hasher(net.segs.len(), Default::default());
    for (i, s) in net.segs.iter().enumerate() {
        let sig = (seg_h[i], seg_tile[i]);
        if set.seg_sig.get(&s.key) != Some(&sig) {
            dirty.insert(sig.1);
            if let Some(old) = set.seg_sig.get(&s.key) {
                dirty.insert(old.1);
            }
        }
        new_seg_sig.insert(s.key, sig);
    }
    let mut new_node_sig: FastMap<i64, (u64, u32)> = FastMap::with_capacity_and_hasher(net.nodes.len(), Default::default());
    for (i, n) in net.nodes.iter().enumerate() {
        let sig = (node_h[i], node_tile[i]);
        if set.node_sig.get(&n.osm_id) != Some(&sig) {
            dirty.insert(sig.1);
            if let Some(old) = set.node_sig.get(&n.osm_id) {
                dirty.insert(old.1);
            }
        }
        new_node_sig.insert(n.osm_id, sig);
    }
    if had_prev {
        for (k, old) in &set.seg_sig {
            if !new_seg_sig.contains_key(k) {
                dirty.insert(old.1);
            }
        }
        for (k, old) in &set.node_sig {
            if !new_node_sig.contains_key(k) {
                dirty.insert(old.1);
            }
        }
    }

    // gera só os ladrilhos sujos
    let mut buckets: HashMap<u32, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (i, t) in seg_tile.iter().enumerate() {
        if dirty.contains(t) {
            buckets.entry(*t).or_default().0.push(i);
        }
    }
    for (i, t) in node_tile.iter().enumerate() {
        if dirty.contains(t) {
            buckets.entry(*t).or_default().1.push(i);
        }
    }
    let mut delta = TileDelta::default();
    for t in &dirty {
        match buckets.remove(t) {
            Some((segs, nodes)) => {
                let mut m = TileMesh::default();
                for si in segs {
                    emit_seg(net, &net.segs[si], &mut m);
                }
                for ni in nodes {
                    emit_node(net, ni, &mut m);
                }
                set.tiles.insert(*t, m);
                delta.dirty.push(*t);
            }
            None => {
                if set.tiles.remove(t).is_some() {
                    delta.removed.push(*t);
                }
            }
        }
    }
    set.seg_sig = new_seg_sig;
    set.node_sig = new_node_sig;
    (set, delta)
}
