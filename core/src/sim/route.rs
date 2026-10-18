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

/// Dijkstra reverso de uma zona feito em fatias (alguns milhares de nós por passo da simulação), para recalcular
/// as rotas com o trânsito medido sem travar o passo. Ao terminar, `dist` substitui a árvore da zona.
pub struct TreeJob {
    pub zone: usize,
    pub dist: Vec<f32>,
    heap: BinaryHeap<Item>,
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

    /// Começa a árvore da zona `zone` (reaproveita o vetor `buf`).
    pub fn job_start(&self, zone: usize, targets: &[u32], mut buf: Vec<f32>) -> TreeJob {
        buf.clear();
        buf.resize(self.time.len(), f32::INFINITY);
        let mut heap = BinaryHeap::new();
        for &t in targets {
            buf[t as usize] = self.time[t as usize];
            heap.push(Item(buf[t as usize], t));
        }
        TreeJob { zone, dist: buf, heap }
    }

    /// Avança a árvore em até `budget` nós retirados da fila; `true` quando terminou.
    pub fn job_run(&self, job: &mut TreeJob, budget: usize) -> bool {
        let dist = &mut job.dist;
        for _ in 0..budget {
            let Some(Item(d, b)) = job.heap.pop() else { return true };
            if d > dist[b as usize] {
                continue;
            }
            let (lo, hi) = (self.radj_off[b as usize] as usize, self.radj_off[b as usize + 1] as usize);
            for &(a, pen) in &self.radj[lo..hi] {
                let nd = d + pen + self.time[a as usize];
                if nd < dist[a as usize] {
                    dist[a as usize] = nd;
                    job.heap.push(Item(nd, a));
                }
            }
            for &a in &self.nb[b as usize] {
                if a == NONE {
                    continue;
                }
                let nd = d + self.change[a as usize] * self.change_mul[b as usize];
                if nd < dist[a as usize] {
                    dist[a as usize] = nd;
                    job.heap.push(Item(nd, a));
                }
            }
        }
        job.heap.is_empty()
    }
}
