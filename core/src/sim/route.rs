//! Roteamento por faixa: um nó por faixa, arestas = conversões ligadas e trocas de faixa. Um Dijkstra **reverso**
//! por zona de destino dá o tempo de cada faixa até a zona.

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
            _ => 30.0,
        }
    };
    kmh.min(110.0) / 3.6
}

/// Parte geométrica da penalidade (a manobra em si, sem fila).
pub fn turn_base(kind: u8) -> f32 {
    match kind {
        K_LEFT => 5.0,
        K_RIGHT => 2.0,
        K_UTURN => 60.0,
        _ => 0.0,
    }
}

/// Espera típica no cruzamento quando ainda não há medição (semáforo, cruzamento sem semáforo).
pub fn junction_penalty(net: &Net, node: u32) -> f32 {
    let n = &net.nodes[node as usize];
    if n.signal { 14.0 } else if n.ends.len() >= 3 { 3.0 } else { 0.0 }
}

pub fn turn_penalty(net: &Net, kind: u8, node: u32) -> f32 {
    turn_base(kind) + junction_penalty(net, node)
}

/// Roteamento por **faixa**: o grafo tem um nó por faixa; as arestas são as conversões ligadas e as trocas de faixa
/// entre faixas vizinhas do mesmo trecho (custo maior em trechos curtos, onde não há espaço para trocar).
/// `dist[faixa]` = tempo estimado desde o início da faixa até a zona de destino.
pub struct LaneRouter {
    /// tempo de percurso livre de cada faixa (s)
    pub time: Vec<f32>,
    /// custo (s) de trocar para uma faixa vizinha neste trecho
    pub change: Vec<f32>,
    /// multiplicador do custo de trocar **para** esta faixa (ocupação: entrar numa faixa cheia é mais difícil)
    pub change_mul: Vec<f32>,
    radj_off: Vec<u32>,
    /// (faixa de origem, penalidade da conversão)
    radj: Vec<(u32, f32)>,
    /// vizinhas (esquerda, direita) no mesmo trecho e sentido; NONE se não há
    pub nb: Vec<[u32; 2]>,
    /// posição em `radj` da k-ésima conversão passada a `new` (para trocar a penalidade com o trânsito)
    edge_of: Vec<u32>,
}

/// Custo (s) de uma troca de faixa em um trecho de `len` metros.
pub fn lane_change_cost(len: f32) -> f32 {
    // trocar de faixa só é realista com espaço e folga no trânsito: trechos curtos pesam muito, para o plano
    // preferir chegar à faixa certa (pela conversão anterior) em vez de contar com uma troca que não cabe
    if len >= 150.0 {
        4.0
    } else if len >= 80.0 {
        10.0
    } else if len >= 40.0 {
        35.0
    } else if len >= 20.0 {
        90.0
    } else {
        200.0
    }
}

impl LaneRouter {
    /// `lane_len`/`lane_speed` por faixa e `conns` = (de, para, penalidade) das conversões ligadas.
    pub fn new(net: &Net, lane_len: &[f32], lane_speed: &[f32], conns: &[(u32, u32, f32)]) -> LaneRouter {
        let nl = net.lanes.len();
        let time: Vec<f32> = (0..nl).map(|l| lane_len[l].max(1.0) / (lane_speed[l] * 0.85)).collect();
        let change: Vec<f32> = (0..nl).map(|l| lane_change_cost(lane_len[l])).collect();
        let mut nb = vec![[NONE; 2]; nl];
        for l in 0..nl {
            let (a, b) = (&net.lanes[l], if l + 1 < nl { Some(&net.lanes[l + 1]) } else { None });
            if let Some(b) = b {
                if a.seg == b.seg && a.dir == b.dir {
                    nb[l][1] = (l + 1) as u32;
                    nb[l + 1][0] = l as u32;
                }
            }
        }
        let mut edges: Vec<(u32, u32, f32, u32)> = conns.iter().enumerate().map(|(k, &(f, t, p))| (t, f, p, k as u32)).collect();
        edges.sort_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)).then(x.2.partial_cmp(&y.2).unwrap()));
        let mut edge_of = vec![0u32; conns.len()];
        let mut kept: Vec<(u32, u32, f32, u32)> = Vec::with_capacity(edges.len());
        for e in edges {
            match kept.last() {
                Some(l) if l.0 == e.0 && l.1 == e.1 => {}
                _ => kept.push(e),
            }
            edge_of[e.3 as usize] = (kept.len() - 1) as u32;
        }
        let edges = kept;
        let mut radj_off = vec![0u32; nl + 1];
        for e in &edges {
            radj_off[e.0 as usize + 1] += 1;
        }
        for i in 0..nl {
            radj_off[i + 1] += radj_off[i];
        }
        let radj = edges.iter().map(|e| (e.1, e.2)).collect();
        LaneRouter { change_mul: vec![1.0; nl], time, change, radj_off, radj, nb, edge_of }
    }

    /// Dijkstra reverso a partir das faixas que terminam na zona de destino.
    pub fn tree(&self, targets: &[u32], dist: &mut Vec<f32>) {
        let nl = self.time.len();
        dist.clear();
        dist.resize(nl, f32::INFINITY);
        let mut heap = BinaryHeap::new();
        for &t in targets {
            dist[t as usize] = self.time[t as usize];
            heap.push(Item(dist[t as usize], t));
        }
        while let Some(Item(d, b)) = heap.pop() {
            if d > dist[b as usize] {
                continue;
            }
            let (lo, hi) = (self.radj_off[b as usize] as usize, self.radj_off[b as usize + 1] as usize);
            for &(a, pen) in &self.radj[lo..hi] {
                let nd = d + pen + self.time[a as usize];
                if nd < dist[a as usize] {
                    dist[a as usize] = nd;
                    heap.push(Item(nd, a));
                }
            }
            for &a in &self.nb[b as usize] {
                if a == NONE {
                    continue;
                }
                let nd = d + self.change[a as usize] * self.change_mul[b as usize];
                if nd < dist[a as usize] {
                    dist[a as usize] = nd;
                    heap.push(Item(nd, a));
                }
            }
        }
    }
    /// Como `tree`, mas só percorre as faixas para as quais `local_of` dá um índice (as de uma zona);
    /// `dist[i]` é a do i-ésimo membro. Barato: custa o tamanho da zona, não o da rede.
    pub fn tree_local(&self, targets: &[u32], local_of: &dyn Fn(u32) -> u32, size: usize) -> Vec<f32> {
        let mut dist = vec![f32::INFINITY; size];
        let mut heap = BinaryHeap::new();
        for &t in targets {
            let li = local_of(t);
            if li != NONE {
                dist[li as usize] = self.time[t as usize];
                heap.push(Item(dist[li as usize], t));
            }
        }
        while let Some(Item(d, b)) = heap.pop() {
            let lb = local_of(b);
            if d > dist[lb as usize] {
                continue;
            }
            let (lo, hi) = (self.radj_off[b as usize] as usize, self.radj_off[b as usize + 1] as usize);
            for &(a, pen) in &self.radj[lo..hi] {
                let la = local_of(a);
                if la == NONE {
                    continue;
                }
                let nd = d + pen + self.time[a as usize];
                if nd < dist[la as usize] {
                    dist[la as usize] = nd;
                    heap.push(Item(nd, a));
                }
            }
            for &a in &self.nb[b as usize] {
                if a == NONE {
                    continue;
                }
                let la = local_of(a);
                if la == NONE {
                    continue;
                }
                let nd = d + self.change[a as usize] * self.change_mul[b as usize];
                if nd < dist[la as usize] {
                    dist[la as usize] = nd;
                    heap.push(Item(nd, a));
                }
            }
        }
        dist
    }

    /// Troca a penalidade da k-ésima conversão (na ordem passada a `new`).
    pub fn set_pen(&mut self, k: usize, pen: f32) {
        self.radj[self.edge_of[k] as usize].1 = pen;
    }

    /// Pesos vivos de uma vez: penalidade de cada conversão (na ordem de `new`, aplicadas em ordem, como `set_pen`)
    /// e multiplicador da troca de faixa. Uma cópia do roteador em outro worker fica igual à da simulação.
    pub fn set_weights(&mut self, pen: &[f32], change_mul: &[f32]) {
        for (k, &p) in pen.iter().enumerate() {
            self.set_pen(k, p);
        }
        self.change_mul.copy_from_slice(change_mul);
    }

    /// Grafo inteiro em bytes (little-endian), para montar uma cópia em outro worker com `from_bytes`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let nl = self.time.len();
        let mut b = Vec::with_capacity(12 + nl * 25 + self.radj.len() * 8 + self.edge_of.len() * 4);
        for n in [nl, self.radj.len(), self.edge_of.len()] {
            b.extend_from_slice(&(n as u32).to_le_bytes());
        }
        for v in [&self.time, &self.change, &self.change_mul] {
            v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
        }
        self.radj_off.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
        for &(a, p) in &self.radj {
            b.extend_from_slice(&a.to_le_bytes());
            b.extend_from_slice(&p.to_le_bytes());
        }
        for nb in &self.nb {
            b.extend_from_slice(&nb[0].to_le_bytes());
            b.extend_from_slice(&nb[1].to_le_bytes());
        }
        self.edge_of.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<LaneRouter> {
        let mut words = b.chunks_exact(4).map(|w| [w[0], w[1], w[2], w[3]]);
        let mut u = || words.next().map(u32::from_le_bytes);
        let (nl, nr, nc) = (u()? as usize, u()? as usize, u()? as usize);
        if b.len() != 12 + nl * 4 * 3 + (nl + 1) * 4 + nr * 8 + nl * 8 + nc * 4 {
            return None;
        }
        let mut f32s = |n: usize| (0..n).map(|_| f32::from_bits(u().unwrap())).collect::<Vec<f32>>();
        let (time, change, change_mul) = (f32s(nl), f32s(nl), f32s(nl));
        let mut u = || words.next().map(u32::from_le_bytes).unwrap();
        let radj_off: Vec<u32> = (0..nl + 1).map(|_| u()).collect();
        let radj = (0..nr).map(|_| (u(), f32::from_bits(u()))).collect();
        let nb = (0..nl).map(|_| [u(), u()]).collect();
        let edge_of = (0..nc).map(|_| u()).collect();
        Some(LaneRouter { time, change, change_mul, radj_off, radj, nb, edge_of })
    }
}
