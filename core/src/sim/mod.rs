//! Simulação microscópica de tráfego (carros e ônibus) sobre a rede.
//!
//! Elementos de via ("arestas"): primeiro todas as faixas, depois cada conversão ligada (a trajetória
//! dentro do cruzamento). Cada veículo está em exatamente uma aresta, ordenado por posição da frente
//! (`s`). Longitudinal: IDM. Lateral: troca de faixa obrigatória (rota) e discricionária (simplificada).
//! Cruzamentos: semáforo (verde/amarelo/vermelho, esquerda permissiva) e preferência (via principal,
//! rotatória, direita) por aceitação de brecha, mais *keep-clear*. Tudo determinístico por semente.

pub mod metrics;
pub mod route;

use crate::demand;
use crate::geo::*;
use crate::net::*;
use route::*;
use serde::Deserialize;

thread_local! { pub static REASON: std::cell::Cell<u32> = std::cell::Cell::new(0); }
pub const SINK: u32 = u32::MAX - 1;
const LOOKAHEAD: f32 = 150.0;
const MIN_GAP: f32 = 0.3;

#[derive(Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct SimParams {
    pub seed: u64,
    /// hora do dia de início (h)
    pub hour: f64,
    pub bus_share: f64,
    pub beta: f64,
    pub intrazone: bool,
    pub od_manual: std::collections::HashMap<String, f64>,
}
impl Default for SimParams {
    fn default() -> Self {
        SimParams { seed: 1, hour: 8.0, bus_share: 2.0, beta: 0.08, intrazone: false, od_manual: Default::default() }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SimInit {
    pub zones: Vec<demand::ZoneIn>,
    pub params: SimParams,
}

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03)
    }
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn exp(&mut self, rate: f64) -> f64 {
        -((1.0 - self.f() as f64).max(1e-12)).ln() / rate
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

struct LaneRt {
    seg: u32,
    dir: u8,
    idx: u8,
    len: f32,
    /// deslocamento lateral do centro da faixa
    off: f32,
    /// arco (m) no eixo do trecho em que a faixa começa (dir 0) ou termina (dir 1)
    a0: f32,
    to_node: u32,
    end_id: u32,
    bus: bool,
    vfree: f32,
    rank: i16,
    ring: bool,
    dirseg: u32,
    /// arestas de saída (conversões ligadas)
    conns: Vec<u32>,
}

struct ConnRt {
    node: u32,
    from: u32,
    to: u32,
    kind: u8,
    len: f32,
    curve: [f32; 8],
    vturn: f32,
    rank: i16,
    ring: bool,
    /// direção de saída do nó pela ponta de chegada (para a regra da direita)
    u_me: V2,
}

#[derive(Clone, Copy)]
struct Conflict {
    other: u32,
    s_me: f32,
    s_other: f32,
    merge: bool,
}

struct SigRt {
    offset: f64,
    cycle: f64,
    phases: Vec<(f64, f64, f64, Vec<u32>)>,
}

#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub spawned: u64,
    pub finished: u64,
    pub lost: u64,
    pub queued: u64,
    pub travel_sum: f64,
    pub red_runs: u64,
    pub lane_changes: u64,
}

pub struct Sim {
    pub time: f64,
    pub dt: f32,
    pub stats: Stats,
    nl: usize,
    lanes: Vec<LaneRt>,
    conns: Vec<ConnRt>,
    edge_len: Vec<f32>,
    edge_veh: Vec<Vec<u32>>,
    node_conns: Vec<Vec<u32>>,
    conflicts: Vec<Option<std::rc::Rc<Vec<Conflict>>>>,
    router: Router,
    hops: Vec<Vec<u32>>,
    dists: Vec<Vec<f32>>,
    zone_ids: Vec<i64>,
    zone_poly: Vec<Vec<[f64; 2]>>,
    zone_profile: Vec<Vec<f64>>,
    zone_gen: Vec<f64>,
    zone_attr: Vec<f64>,
    sink: Vec<Vec<bool>>,
    gates: Vec<Vec<u32>>,
    params: SimParams,
    /// taxa de viagens (veículos/s) por par origem->destino na hora corrente
    rate: Vec<Vec<f64>>,
    next_spawn: Vec<Vec<f64>>,
    pending: Vec<std::collections::VecDeque<(u32, u16, u8, f32)>>,
    rng: Rng,
    next_matrix_at: f64,
    sigs: Vec<SigRt>,
    end_state: Vec<u8>,
    red_age: Vec<f32>,
    // veículos (SoA)
    alive: Vec<bool>,
    free: Vec<u32>,
    edge: Vec<u32>,
    s: Vec<f32>,
    v: Vec<f32>,
    lat: Vec<f32>,
    kind: Vec<u8>,
    vlen: Vec<f32>,
    vfac: Vec<f32>,
    tgap: Vec<f32>,
    amax: Vec<f32>,
    bcomf: Vec<f32>,
    dest: Vec<u16>,
    t_spawn: Vec<f32>,
    choice: Vec<u32>,
    wait: Vec<f32>,
    cool: Vec<f32>,
    acc: Vec<f32>,
    /// decisão de entrada em cruzamento reaproveitada por 1 passo (reavaliada a cada 2)
    enter_ok: Vec<bool>,
    tick: u64,
    multi: Vec<u32>,
    pub n_alive: usize,
    pub metrics: metrics::Metrics,
    t_free: Vec<f32>,
    origin: Vec<u16>,
    start_time: f64,
    /// tempo acumulado (ms) por fase do passo — só nativo, para os testes de desempenho
    pub prof: [f64; 7],
}

impl Sim {
    pub fn new(net: &Net, init: &SimInit) -> Result<Sim, String> {
        if net.conns.is_empty() {
            return Err("conexões não construídas".into());
        }
        let nl = net.lanes.len();
        // ---- faixas
        let mut lanes: Vec<LaneRt> = Vec::with_capacity(nl);
        for (li, l) in net.lanes.iter().enumerate() {
            let s = &net.segs[l.seg as usize];
            let len = ((s.poly.length() - s.cut_from - s.cut_to) as f32).max(1.0);
            let to_node = l.to;
            let end_id = l.seg * 2 + if l.dir == 0 { 1 } else { 0 };
            lanes.push(LaneRt {
                seg: l.seg,
                dir: l.dir,
                idx: l.idx,
                len,
                off: s.lane_off(l.dir, l.idx) as f32,
                a0: if l.dir == 0 { s.cut_from as f32 } else { (s.poly.length() - s.cut_to) as f32 },
                to_node,
                end_id,
                bus: l.bus,
                vfree: free_speed(s),
                rank: if s.roundabout { 0 } else { s.hw as i16 + 1 },
                ring: s.roundabout,
                dirseg: l.seg * 2 + l.dir as u32,
                conns: Vec::new(),
            });
            let _ = li;
        }
        // ---- conversões ligadas
        let mut conns: Vec<ConnRt> = Vec::new();
        let mut node_conns: Vec<Vec<u32>> = vec![Vec::new(); net.nodes.len()];
        for c in &net.conns {
            if c.state != CONN_ALLOWED {
                continue;
            }
            let (fl_vfree, fl_rank, fl_ring, fl_end) = {
                let fl = &lanes[c.from_lane as usize];
                (fl.vfree, fl.rank, fl.ring, fl.end_id)
            };
            let pts: Vec<V2> = (0..=8).map(|i| bez8(&c.curve, i as f32 / 8.0)).collect();
            let len = pts.windows(2).map(|w| len_v(sub(w[1], w[0]))).sum::<f64>() as f32;
            let vturn = match c.kind {
                K_STRAIGHT => fl_vfree.min(13.0),
                K_RIGHT => 6.5,
                K_LEFT => 5.5,
                _ => 3.5,
            };
            let eid = (nl + conns.len()) as u32;
            lanes[c.from_lane as usize].conns.push(eid);
            node_conns[c.node as usize].push(eid);
            conns.push(ConnRt {
                node: c.node,
                from: c.from_lane,
                to: c.to_lane,
                kind: c.kind,
                len: len.max(0.5),
                curve: c.curve,
                vturn,
                rank: fl_rank,
                ring: fl_ring,
                u_me: net.outward(fl_end),
            });
        }
        let mut edge_len: Vec<f32> = lanes.iter().map(|l| l.len).collect();
        edge_len.extend(conns.iter().map(|c| c.len));
        let ne = edge_len.len();

        // ---- zonas: nós de destino e portões de origem
        let nz = init.zones.len();
        let mut sink = Vec::with_capacity(nz);
        let mut gates = Vec::with_capacity(nz);
        for z in &init.zones {
            let inside: Vec<bool> = net.nodes.iter().map(|n| z.poly.len() >= 3 && demand::point_in_poly(n.pos, &z.poly)).collect();
            let mut g = Vec::new();
            for (li, l) in net.lanes.iter().enumerate() {
                let s = &net.segs[l.seg as usize];
                if inside[l.from as usize] && !s.roundabout && !l.bus && s.hw > HW_TRUNK && lanes[li].len > 12.0 {
                    g.push(li as u32);
                }
            }
            sink.push(inside);
            gates.push(g);
        }

        // ---- roteamento
        let router = Router::new(net);
        let mut hops = Vec::with_capacity(nz);
        let mut dists = Vec::with_capacity(nz);
        for sk in &sink {
            let targets: Vec<u32> = (0..net.segs.len() as u32 * 2).filter(|&d| router.time[d as usize].is_finite() && sk[dirseg_end_node(net, d) as usize]).collect();
            let (mut h, mut dd) = (Vec::new(), Vec::new());
            router.tree(&targets, &mut h, &mut dd);
            hops.push(h);
            dists.push(dd);
        }

        // ---- semáforos
        let mut sigs = Vec::new();
        for s in &net.signals {
            let phases: Vec<(f64, f64, f64, Vec<u32>)> = s.phases.iter().map(|p| (p.green as f64, p.yellow as f64, p.all_red as f64, p.ends.clone())).collect();
            let cycle = phases.iter().map(|p| p.0 + p.1 + p.2).sum();
            sigs.push(SigRt { offset: s.offset as f64, cycle, phases });
        }

        let mut sim = Sim {
            time: init.params.hour.rem_euclid(24.0) * 3600.0,
            dt: 0.2,
            stats: Stats::default(),
            nl,
            lanes,
            conns,
            edge_len,
            edge_veh: vec![Vec::new(); ne],
            node_conns,
            conflicts: vec![None; ne],
            router,
            hops,
            dists,
            zone_ids: init.zones.iter().map(|z| z.id).collect(),
            zone_poly: init.zones.iter().map(|z| z.poly.clone()).collect(),
            zone_profile: init.zones.iter().map(|z| z.profile.clone()).collect(),
            zone_gen: init.zones.iter().map(|z| z.gen).collect(),
            zone_attr: init.zones.iter().map(|z| z.attr).collect(),
            sink,
            gates,
            params: init.params.clone(),
            rate: vec![vec![0.0; nz]; nz],
            next_spawn: vec![vec![f64::INFINITY; nz]; nz],
            pending: vec![Default::default(); net.lanes.len()],
            rng: Rng::new(init.params.seed),
            next_matrix_at: 0.0,
            sigs,
            end_state: vec![0; net.segs.len() * 2],
            red_age: vec![0.0; net.segs.len() * 2],
            alive: Vec::new(),
            free: Vec::new(),
            edge: Vec::new(),
            s: Vec::new(),
            v: Vec::new(),
            lat: Vec::new(),
            kind: Vec::new(),
            vlen: Vec::new(),
            vfac: Vec::new(),
            tgap: Vec::new(),
            amax: Vec::new(),
            bcomf: Vec::new(),
            dest: Vec::new(),
            t_spawn: Vec::new(),
            choice: Vec::new(),
            wait: Vec::new(),
            cool: Vec::new(),
            acc: Vec::new(),
            enter_ok: Vec::new(),
            tick: 0,
            multi: Vec::new(),
            n_alive: 0,
            metrics: metrics::Metrics::new(net.segs.len() * 2, net.nodes.len(), nz),
            t_free: Vec::new(),
            origin: Vec::new(),
            start_time: 0.0,
            prof: [0.0; 7],
        };
        sim.start_time = sim.time;
        sim.refresh_matrix(net);
        Ok(sim)
    }

    // ------------------------------------------------------------------ demanda

    /// Tempo de viagem livre médio (min) entre duas zonas, pelo roteador.
    fn zone_minutes(&self, i: usize, j: usize) -> f64 {
        let g = &self.gates[i];
        if g.is_empty() {
            return 120.0;
        }
        let step = (g.len() / 8).max(1);
        let (mut sum, mut n) = (0.0, 0.0);
        for &l in g.iter().step_by(step) {
            let d = self.dists[j][self.lanes[l as usize].dirseg as usize];
            if d.is_finite() {
                sum += d as f64;
                n += 1.0;
            }
        }
        if n == 0.0 {
            120.0
        } else {
            sum / n / 60.0
        }
    }

    /// Recalcula a matriz OD para a hora atual e as taxas de chegada.
    fn refresh_matrix(&mut self, net: &Net) {
        let hour = (self.time / 3600.0).rem_euclid(24.0);
        let payload = demand::Payload {
            zones: (0..self.zone_ids.len())
                .map(|i| demand::ZoneIn { id: self.zone_ids[i], poly: self.zone_poly[i].clone(), gen: self.zone_gen[i], attr: self.zone_attr[i], profile: self.zone_profile[i].clone() })
                .collect(),
            params: demand::ParamsIn { beta: self.params.beta, intrazone: self.params.intrazone, hour, od_manual: self.params.od_manual.clone() },
        };
        let me: &Sim = self;
        let travel = |i: usize, j: usize, _c: &[[f64; 2]; 2]| me.zone_minutes(i, j);
        let r = demand::resolve(net, &payload, &travel);
        let nz = self.zone_ids.len();
        for i in 0..nz {
            for j in 0..nz {
                let veh_h = if self.gates[i].is_empty() { 0.0 } else { r.matrix[i][j] };
                let new_rate = veh_h / 3600.0;
                let old = self.rate[i][j];
                self.rate[i][j] = new_rate;
                // reprograma o próximo nascimento quando a taxa muda
                if new_rate <= 0.0 {
                    self.next_spawn[i][j] = f64::INFINITY;
                } else if old <= 0.0 || !self.next_spawn[i][j].is_finite() {
                    self.next_spawn[i][j] = self.time + self.rng.exp(new_rate);
                } else if (new_rate - old).abs() > 1e-12 {
                    let remaining = (self.next_spawn[i][j] - self.time).max(0.0);
                    self.next_spawn[i][j] = self.time + remaining * old / new_rate;
                }
            }
        }
        self.next_matrix_at = self.time + 600.0;
    }

    // ------------------------------------------------------------------ semáforos

    fn update_signals(&mut self, net: &Net) {
        let _ = net;
        for st in self.end_state.iter_mut() {
            if *st != 0 {
                *st = 0;
            }
        }
        let dt = self.dt;
        for sg in &self.sigs {
            if sg.cycle <= 0.0 {
                continue;
            }
            let mut r = (self.time + sg.offset).rem_euclid(sg.cycle);
            let mut active: Option<(usize, u8)> = None;
            for (pi, p) in sg.phases.iter().enumerate() {
                if r < p.0 {
                    active = Some((pi, 3));
                    break;
                }
                r -= p.0;
                if r < p.1 {
                    active = Some((pi, 2));
                    break;
                }
                r -= p.1;
                if r < p.2 {
                    break;
                }
                r -= p.2;
            }
            for (pi, p) in sg.phases.iter().enumerate() {
                for &e in &p.3 {
                    let st = match active {
                        Some((a, s)) if a == pi => s,
                        _ => 1,
                    };
                    self.end_state[e as usize] = st;
                }
            }
        }
        for (e, st) in self.end_state.iter().enumerate() {
            if *st == 1 {
                self.red_age[e] += dt;
            } else {
                self.red_age[e] = 0.0;
            }
        }
    }

    // ------------------------------------------------------------------ veículos

    fn spawn_vehicle(&mut self, lane: u32, origin: u16, dest: u16, kind: u8) -> bool {
        let l = &self.lanes[lane as usize];
        if kind == 0 && l.bus {
            return false;
        }
        let (vlen, tgap, amax, bcomf, vfac) = if kind == 1 {
            (12.0, self.rng.range(1.4, 1.8), self.rng.range(0.9, 1.2), self.rng.range(1.8, 2.4), self.rng.range(0.8, 0.95))
        } else {
            (self.rng.range(4.1, 4.9), self.rng.range(0.9, 1.5), self.rng.range(1.6, 2.6), self.rng.range(2.2, 3.4), self.rng.range(0.9, 1.15))
        };
        // há espaço na entrada?
        let list = &self.edge_veh[lane as usize];
        if let Some(&j) = list.first() {
            let j = j as usize;
            if self.s[j] - self.vlen[j] < vlen + 3.0 + self.v[j] * 0.8 {
                return false;
            }
        }
        let id = match self.free.pop() {
            Some(i) => i as usize,
            None => {
                let i = self.alive.len();
                self.alive.push(false);
                self.edge.push(0);
                self.s.push(0.0);
                self.v.push(0.0);
                self.lat.push(0.0);
                self.kind.push(0);
                self.vlen.push(0.0);
                self.vfac.push(0.0);
                self.tgap.push(0.0);
                self.amax.push(0.0);
                self.bcomf.push(0.0);
                self.dest.push(0);
                self.t_spawn.push(0.0);
                self.choice.push(NONE);
                self.wait.push(0.0);
                self.cool.push(0.0);
                self.acc.push(0.0);
                self.enter_ok.push(false);
                self.t_free.push(0.0);
                self.origin.push(0);
                i
            }
        };
        self.alive[id] = true;
        self.edge[id] = lane;
        self.s[id] = vlen + 0.5;
        self.v[id] = (self.lanes[lane as usize].vfree * vfac * 0.4).min(8.0);
        self.lat[id] = 0.0;
        self.kind[id] = kind;
        self.vlen[id] = vlen;
        self.vfac[id] = vfac;
        self.tgap[id] = tgap;
        self.amax[id] = amax;
        self.bcomf[id] = bcomf;
        self.dest[id] = dest;
        self.origin[id] = origin;
        self.t_free[id] = {
            let d = self.dists[dest as usize][self.lanes[lane as usize].dirseg as usize];
            if d.is_finite() { d } else { 0.0 }
        };
        self.t_spawn[id] = self.time as f32;
        self.wait[id] = 0.0;
        self.cool[id] = self.rng.range(0.0, 2.0);
        self.edge_veh[lane as usize].insert(0, id as u32);
        self.metrics.entered(self.lanes[lane as usize].dirseg as usize);
        self.n_alive += 1;
        self.stats.spawned += 1;
        self.choose_conn(id);
        true
    }

    fn remove_vehicle(&mut self, id: usize) {
        let e = self.edge[id] as usize;
        if let Some(p) = self.edge_veh[e].iter().position(|&x| x as usize == id) {
            self.edge_veh[e].remove(p);
        }
        self.alive[id] = false;
        self.free.push(id as u32);
        self.n_alive -= 1;
    }

    fn insert_sorted(&mut self, edge: u32, id: usize) {
        let s = self.s[id];
        let list = &self.edge_veh[edge as usize];
        let pos = list.partition_point(|&j| self.s[j as usize] < s);
        self.edge_veh[edge as usize].insert(pos, id as u32);
    }

    /// Escolhe a conversão que leva ao próximo trecho da rota a partir da faixa atual.
    fn choose_conn(&mut self, id: usize) {
        let e = self.edge[id] as usize;
        if e >= self.nl {
            return;
        }
        let lane = &self.lanes[e];
        let dest = self.dest[id] as usize;
        if self.sink[dest][lane.to_node as usize] {
            self.choice[id] = SINK;
            return;
        }
        let nh = self.hops[dest][lane.dirseg as usize];
        if nh == NONE {
            // sem rota a partir daqui: qualquer saída (vai se perder no fim)
            self.choice[id] = lane.conns.first().copied().unwrap_or(NONE);
            return;
        }
        let mut best = NONE;
        for &c in &lane.conns {
            let to = self.conns[c as usize - self.nl].to;
            if self.lanes[to as usize].dirseg == nh {
                best = c;
                break;
            }
        }
        self.choice[id] = best;
    }

    // ---- cruzamentos

    fn conflicts_of(&mut self, net: &Net, c: u32) -> std::rc::Rc<Vec<Conflict>> {
        if let Some(v) = &self.conflicts[c as usize] {
            return v.clone();
        }
        let me = &self.conns[c as usize - self.nl];
        let pts_me: Vec<V2> = (0..=12).map(|i| bez8(&me.curve, i as f32 / 12.0)).collect();
        let cum = |p: &[V2]| -> Vec<f64> {
            let mut a = vec![0.0];
            for w in p.windows(2) {
                a.push(a.last().unwrap() + len_v(sub(w[1], w[0])));
            }
            a
        };
        let cum_me = cum(&pts_me);
        let scale_me = me.len as f64 / cum_me.last().copied().unwrap_or(1.0).max(1e-6);
        let mut out = Vec::new();
        for &o in &self.node_conns[me.node as usize] {
            if o == c {
                continue;
            }
            let other = &self.conns[o as usize - self.nl];
            if other.from == me.from {
                continue; // divergem da mesma faixa
            }
            if other.to == me.to {
                out.push(Conflict { other: o, s_me: me.len, s_other: other.len, merge: true });
                continue;
            }
            let pts_o: Vec<V2> = (0..=12).map(|i| bez8(&other.curve, i as f32 / 12.0)).collect();
            let cum_o = cum(&pts_o);
            let scale_o = other.len as f64 / cum_o.last().copied().unwrap_or(1.0).max(1e-6);
            'search: for a in 0..pts_me.len() - 1 {
                for b in 0..pts_o.len() - 1 {
                    if let Some((t, u)) = seg_cross_t(pts_me[a], pts_me[a + 1], pts_o[b], pts_o[b + 1]) {
                        let s_me = (cum_me[a] + t * (cum_me[a + 1] - cum_me[a])) * scale_me;
                        let s_o = (cum_o[b] + u * (cum_o[b + 1] - cum_o[b])) * scale_o;
                        out.push(Conflict { other: o, s_me: s_me as f32, s_other: s_o as f32, merge: false });
                        break 'search;
                    }
                }
            }
        }
        let _ = net;
        let rc = std::rc::Rc::new(out);
        self.conflicts[c as usize] = Some(rc.clone());
        rc
    }

    /// `o` tem preferência sobre `me`?
    fn has_priority(&self, me: &ConnRt, o: &ConnRt, signaled: bool) -> bool {
        if signaled {
            // fase verde: só a esquerda permissiva cede a quem segue em frente/direita
            return me.kind == K_LEFT && (o.kind == K_STRAIGHT || o.kind == K_RIGHT);
        }
        if o.rank != me.rank {
            return o.rank < me.rank;
        }
        if self.lanes[o.from as usize].seg == self.lanes[me.from as usize].seg {
            return false;
        }
        // mesma hierarquia: preferência a quem vem da direita (CTB art. 29)
        cross(me.u_me, o.u_me) < 0.0
    }

    /// Pode entrar na conversão `cc` agora? (semáforo, preferência por brecha, espaço de saída)
    fn may_enter(&mut self, net: &Net, id: usize, cc: u32) -> bool {
        if cc == NONE {
            { REASON.with(|r| r.set(1)); return false; }
        }
        if cc == SINK {
            return true;
        }
        let lane_id = self.edge[id] as usize;
        let end = self.lanes[lane_id].end_id as usize;
        let st = self.end_state[end];
        let v = self.v[id];
        let d_end = self.lanes[lane_id].len - self.s[id];
        let b = self.bcomf[id];
        let signaled = st != 0;
        if st == 1 {
            { REASON.with(|r| r.set(2)); return false; }
        }
        if st == 2 && v * v / (2.0 * b) < d_end - 1.0 {
            { REASON.with(|r| r.set(3)); return false; } // amarelo e dá para parar
        }
        let (me_idx, to_lane) = (cc as usize - self.nl, self.conns[cc as usize - self.nl].to);
        // fila justa na saída: quem espera há mais tempo, vindo de outra faixa para a mesma faixa de saída, passa antes
        {
            let node = self.conns[me_idx].node as usize;
            let my_wait = self.wait[id];
            for &o in &self.node_conns[node] {
                let oc = &self.conns[o as usize - self.nl];
                if oc.to != to_lane || oc.from as usize == lane_id {
                    continue;
                }
                if self.end_state[self.lanes[oc.from as usize].end_id as usize] == 1 {
                    continue;
                }
                if let Some(&j) = self.edge_veh[oc.from as usize].last() {
                    let j = j as usize;
                    if self.choice[j] == o && self.v[j] < 0.5 && self.enter_ok[j] && (self.wait[j] > my_wait + 2.0 || (self.wait[j] >= my_wait - 2.0 && j < id)) {
                        REASON.with(|r| r.set(9));
                        return false;
                    }
                }
            }
        }
        // espaço de saída (keep-clear): fim da faixa de destino e entrada da conversão
        // impasse prolongado (anel/entroncamento travado): aceita "bloquear o cruzamento", reduzindo a folga exigida
        let need = (self.vlen[id] + 2.0) * (1.0 - ((self.wait[id] - 30.0) / 50.0).clamp(0.0, 1.0)).max(0.1);
        if let Some(&j) = self.edge_veh[to_lane as usize].first() {
            let j = j as usize;
            if self.s[j] - self.vlen[j] < need {
                { REASON.with(|r| r.set(4)); return false; }
            }
        }
        if let Some(&j) = self.edge_veh[cc as usize].first() {
            let j = j as usize;
            if self.s[j] - self.vlen[j] < 2.0 {
                { REASON.with(|r| r.set(5)); return false; }
            }
        }
        let relaxed = self.wait[id] > 25.0;
        let conflicts = self.conflicts_of(net, cc);
        let t_gap = if relaxed { 1.0 } else { 2.2 };
        let v_me = v.max(2.0);
        for &cf in conflicts.iter() {
            let oi = cf.other as usize - self.nl;
            let (me, o) = (&self.conns[me_idx], &self.conns[oi]);
            if !self.has_priority(me, o, signaled) {
                // convergência na mesma faixa sem preferência: quem chega primeiro entra (desempate pelo id)
                if cf.merge && !signaled_green_streams(signaled, me, o) {
                    let t_me = (d_end + cf.s_me) / v_me;
                    let blocked = self.edge_veh[cf.other as usize].iter().any(|&j| {
                        let j = j as usize;
                        let t_j = ((cf.s_other - self.s[j]).max(0.0)) / self.v[j].max(2.0);
                        t_j < t_me || (t_j == t_me && cf.other < cc)
                    }) || self.edge_veh[o.from as usize].iter().rev().take(2).any(|&j| {
                        let j = j as usize;
                        if self.v[j] < 0.5 {
                            return false; // parado na linha: a ordem de espera (acima) arbitra
                        }
                        let t_j = (self.lanes[o.from as usize].len - self.s[j] + cf.s_other) / self.v[j].max(2.0);
                        t_j < t_me || (t_j == t_me && cf.other < cc)
                    });
                    if blocked {
                        { REASON.with(|r| r.set(6)); return false; }
                    }
                }
                continue;
            }
            // chegada minha ao ponto de conflito
            let t_me = (d_end + cf.s_me) / v_me;
            // veículos já na conversão concorrente
            for &j in &self.edge_veh[cf.other as usize] {
                let j = j as usize;
                let (sj, lj) = (self.s[j], self.vlen[j]);
                if sj - lj > cf.s_other + 1.5 {
                    continue; // já liberou o ponto
                }
                let t_j = if sj >= cf.s_other { 0.0 } else { (cf.s_other - sj) / self.v[j].max(2.0) };
                if !(t_me + t_gap < t_j) {
                    { REASON.with(|r| r.set(7)); return false; }
                }
            }
            // os que estão chegando pela faixa que alimenta a conversão
            let fl = o.from as usize;
            // aproximação concorrente com sinal vermelho não disputa a brecha (só quem já está na conversão)
            if signaled && self.end_state[self.lanes[fl].end_id as usize] == 1 {
                continue;
            }
            for &j in self.edge_veh[fl].iter().rev().take(2) {
                let j = j as usize;
                // desempate anti-impasse (ex.: quatro aproximações cedendo à direita): quem espera há mais tempo passa
                if !signaled && self.v[j] < 0.5 && self.wait[j] + 3.0 < self.wait[id] {
                    continue;
                }
                // quem está parado na linha há tempo está bloqueado (não vai ocupar o cruzamento já): não prende quem espera muito
                if self.v[j] < 0.5 && self.wait[j] > 8.0 && self.wait[id] > 25.0 {
                    continue;
                }
                let d_j = self.lanes[fl].len - self.s[j];
                let t_j = (d_j + cf.s_other) / self.v[j].max(2.0);
                if !(t_me + t_gap < t_j) {
                    { REASON.with(|r| r.set(8)); return false; }
                }
            }
        }
        true
    }

    // ---- passo

    /// Líder (folga até a traseira dele, velocidade) para o veículo `i`, ou None.
    fn leader(&mut self, net: &Net, i: usize, e: usize, k: usize, front_decides: bool) -> Option<(f32, f32)> {
        let list = &self.edge_veh[e];
        if k + 1 < list.len() {
            let j = list[k + 1] as usize;
            return Some((self.s[j] - self.vlen[j] - self.s[i], self.v[j]));
        }
        let elen = self.edge_len[e];
        let rest = elen - self.s[i];
        if e < self.nl {
            let cc = self.choice[i];
            if cc == SINK {
                return None; // sai da rede ao fim da faixa; sem líder virtual
            }
            // linha de retenção virtual quando não pode entrar
            if rest < LOOKAHEAD && front_decides {
                let d_check = (self.v[i] * self.v[i] / (2.0 * self.bcomf[i]) + 20.0).max(25.0);
                if rest <= d_check {
                    if (self.tick + i as u64) & 1 == 0 || self.wait[i] > 0.0 && false {
                        self.enter_ok[i] = self.may_enter(net, i, cc);
                    }
                    if !self.enter_ok[i] {
                        return Some((rest.max(0.0), 0.0));
                    }
                } else {
                    self.enter_ok[i] = false;
                }
            }
            if cc == NONE {
                return Some((rest.max(0.0), 0.0));
            }
            // dentro da conversão escolhida
            if let Some(&j) = self.edge_veh[cc as usize].first() {
                let j = j as usize;
                return Some((rest + self.s[j] - self.vlen[j], self.v[j]));
            }
            let to = self.conns[cc as usize - self.nl].to;
            let clen = self.edge_len[cc as usize];
            if let Some(&j) = self.edge_veh[to as usize].first() {
                let j = j as usize;
                return Some((rest + clen + self.s[j] - self.vlen[j], self.v[j]));
            }
            None
        } else {
            let to = self.conns[e - self.nl].to;
            if let Some(&j) = self.edge_veh[to as usize].first() {
                let j = j as usize;
                return Some((rest + self.s[j] - self.vlen[j], self.v[j]));
            }
            None
        }
    }

    fn desired_speed(&self, i: usize) -> f32 {
        let e = self.edge[i] as usize;
        if e < self.nl {
            self.lanes[e].vfree * self.vfac[i]
        } else {
            let c = &self.conns[e - self.nl];
            (self.lanes[c.from as usize].vfree * self.vfac[i]).min(c.vturn)
        }
    }

    pub fn step(&mut self, net: &Net) {
        #[cfg(not(target_arch = "wasm32"))]
        let mut _t = std::time::Instant::now();
        macro_rules! lap {
            ($k:expr) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.prof[$k] += _t.elapsed().as_secs_f64() * 1e3;
                    _t = std::time::Instant::now();
                }
            };
        }
        let dt = self.dt;
        if self.time >= self.next_matrix_at {
            self.refresh_matrix(net);
        }
        self.update_signals(net);
        lap!(0);
        self.spawn(net);
        lap!(1);

        // 1) acelerações (com o estado do início do passo), percorrendo as arestas
        let n = self.alive.len();
        self.multi.clear();
        for e in 0..self.edge_veh.len() {
            let len = self.edge_veh[e].len();
            if len == 0 {
                continue;
            }
            if len >= 2 {
                self.multi.push(e as u32);
            }
            for k in 0..len {
                let i = self.edge_veh[e][k] as usize;
                let front = k + 1 == len;
                let lead = self.leader(net, i, e, k, front);
                let v = self.v[i];
                let v0 = self.desired_speed(i).max(0.5);
                let (a_max, b, t) = (self.amax[i], self.bcomf[i], self.tgap[i]);
                let free = 1.0 - (v / v0).powi(4);
                let mut interact = 0.0;
                if let Some((gap, vl)) = lead {
                    let gap = gap.max(0.1);
                    let s_star = 2.0 + (v * t + v * (v - vl) / (2.0 * (a_max * b).sqrt())).max(0.0);
                    interact = (s_star / gap).powi(2);
                }
                self.acc[i] = (a_max * (free - interact)).clamp(-9.0, a_max);
            }
        }
        // 2) integração
        for i in 0..n {
            if !self.alive[i] {
                continue;
            }
            let v = self.v[i];
            let nv = (v + self.acc[i] * dt).max(0.0);
            self.s[i] += 0.5 * (v + nv) * dt;
            self.v[i] = nv;
            if (self.edge[i] as usize) < self.nl {
                let l = &self.lanes[self.edge[i] as usize];
                self.metrics.sample(l.dirseg as usize, nv, dt, l.to_node as usize, l.len - self.s[i]);
            }
            if nv < 0.3 {
                self.wait[i] += dt;
            } else {
                self.wait[i] = 0.0;
            }
            // suaviza a troca lateral
            let l = self.lat[i];
            if l != 0.0 {
                let step = 1.8 * dt;
                self.lat[i] = if l.abs() <= step { 0.0 } else { l - l.signum() * step };
            }
            if self.cool[i] > 0.0 {
                self.cool[i] -= dt;
            }
        }

        lap!(3);
        // 3) sem sobreposição na mesma aresta (do líder para trás)
        self.enforce_gaps();
        lap!(4);

        // 4) mudanças de aresta
        let mut movers: Vec<(u32, u32)> = Vec::new();
        for i in 0..n {
            if self.alive[i] && self.s[i] >= self.edge_len[self.edge[i] as usize] {
                movers.push((self.edge[i], i as u32));
            }
        }
        movers.sort_by(|a, b| a.0.cmp(&b.0).then(self.s[b.1 as usize].partial_cmp(&self.s[a.1 as usize]).unwrap()));
        for (_, vid) in movers {
            self.advance(net, vid as usize);
        }

        self.enforce_gaps();
        lap!(5);

        // 5) trocas de faixa
        for i in 0..n {
            if self.alive[i] && (self.edge[i] as usize) < self.nl && self.cool[i] <= 0.0 {
                self.consider_lane_change(i);
            }
        }
        lap!(6);
        self.time += dt as f64;
        self.tick += 1;
        {
            let lanes = &self.lanes;
            let segs = &net.segs;
            let fs = |d: usize| free_speed(&segs[d / 2]) * 0.9;
            let _ = lanes;
            let queued: u64 = self.pending.iter().map(|q| q.len() as u64).sum();
            self.metrics.end_tick(self.time, &fs, self.n_alive, queued);
        }
    }

    /// Garante folga mínima entre veículos consecutivos nas arestas com 2+ veículos.
    fn enforce_gaps(&mut self) {
        for mi in 0..self.multi.len() {
            let e = self.multi[mi] as usize;
            let len = self.edge_veh[e].len();
            if len < 2 {
                continue;
            }
            for k in (0..len - 1).rev() {
                let (a, b) = (self.edge_veh[e][k] as usize, self.edge_veh[e][k + 1] as usize);
                let limit = self.s[b] - self.vlen[b] - MIN_GAP;
                if self.s[a] > limit {
                    self.s[a] = limit.max(self.s[a] - 5.0);
                    self.v[a] = self.v[a].min(self.v[b]);
                }
            }
        }
    }

    fn advance(&mut self, net: &Net, i: usize) {
        let _ = net;
        for _ in 0..4 {
            let e = self.edge[i] as usize;
            if self.s[i] < self.edge_len[e] {
                return;
            }
            if e < self.nl {
                let cc = self.choice[i];
                if cc == SINK {
                    self.stats.finished += 1;
                    let travel = self.time - self.t_spawn[i] as f64;
                    self.stats.travel_sum += travel;
                    self.metrics.trip_done(self.origin[i] as usize, self.dest[i] as usize, travel, self.t_free[i] as f64);
                    self.remove_vehicle(i);
                    return;
                }
                if cc == NONE {
                    // beco sem saída para a rota: fica parado até desistir
                    self.s[i] = self.edge_len[e] - 0.01;
                    self.v[i] = 0.0;
                    if self.wait[i] > 40.0 {
                        self.stats.lost += 1;
                        self.remove_vehicle(i);
                    } else {
                        self.reroute_if_stuck(i);
                    }
                    return;
                }
                let end = self.lanes[e].end_id as usize;
                if self.end_state[end] == 1 && self.red_age[end] > 1.5 {
                    self.stats.red_runs += 1;
                }
                let over = self.s[i] - self.edge_len[e];
                let p = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
                self.edge_veh[e].remove(p);
                self.edge[i] = cc;
                self.s[i] = over;
                self.insert_sorted(cc, i);
                self.multi.push(cc);
            } else {
                let c = &self.conns[e - self.nl];
                let to = c.to;
                let over = self.s[i] - self.edge_len[e];
                let p = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
                self.edge_veh[e].remove(p);
                self.edge[i] = to;
                self.s[i] = over;
                self.insert_sorted(to, i);
                self.multi.push(to);
                self.metrics.entered(self.lanes[to as usize].dirseg as usize);
                self.choose_conn(i);
            }
        }
    }

    /// Parado no fim da faixa sem conversão para a rota: aceita qualquer saída e deixa o roteador refazer o caminho.
    fn reroute_if_stuck(&mut self, i: usize) {
        if self.wait[i] > 12.0 {
            let e = self.edge[i] as usize;
            if let Some(&c) = self.lanes[e].conns.first() {
                self.choice[i] = c;
            }
        }
    }

    fn consider_lane_change(&mut self, i: usize) {
        let e = self.edge[i] as usize;
        let lane = &self.lanes[e];
        let dest = self.dest[i] as usize;
        let mandatory_target = if self.choice[i] == NONE || self.choice[i] == SINK && false {
            let nh = self.hops[dest][lane.dirseg as usize];
            if nh == NONE {
                None
            } else {
                self.lane_toward(e, nh)
            }
        } else {
            None
        };
        let rest = lane.len - self.s[i];
        if let Some(target) = mandatory_target {
            let forced = rest < 40.0 || self.wait[i] > 6.0;
            if self.try_change(i, target, forced) {
                self.cool[i] = 1.0;
            } else {
                self.cool[i] = 0.4;
            }
            return;
        }
        // discricionária: leva-se em conta o líder lento
        if self.choice[i] == SINK || rest < 60.0 || self.v[i] > 0.85 * self.desired_speed(i) {
            self.cool[i] = 1.5;
            return;
        }
        let list = &self.edge_veh[e];
        let Some(k) = list.iter().rposition(|&x| x as usize == i) else { return };
        let slow_leader = k + 1 < list.len() && {
            let j = list[k + 1] as usize;
            self.s[j] - self.vlen[j] - self.s[i] < 20.0 + self.v[i] * 1.0 && self.v[j] < self.v[i] + 1.0
        };
        if !slow_leader {
            self.cool[i] = 1.5;
            return;
        }
        let seg_lanes = self.lanes_of(e);
        let (lo, hi) = seg_lanes;
        for cand in [e as i32 - 1, e as i32 + 1] {
            if cand < lo as i32 || cand >= hi as i32 {
                continue;
            }
            let cl = cand as usize;
            if !self.compatible(i, cl) {
                continue;
            }
            // só se a faixa vizinha tem espaço maior à frente
            let ahead = self.gap_ahead_in(cl, self.s[i], i);
            if ahead > 35.0 && self.try_change(i, cl, false) {
                self.cool[i] = 4.0;
                return;
            }
        }
        self.cool[i] = 2.0;
    }

    /// Faixas (intervalo de ids) do mesmo trecho e sentido que a faixa `e`.
    fn lanes_of(&self, e: usize) -> (usize, usize) {
        let l = &self.lanes[e];
        let mut lo = e;
        while lo > 0 && self.lanes[lo - 1].seg == l.seg && self.lanes[lo - 1].dir == l.dir {
            lo -= 1;
        }
        let mut hi = e + 1;
        while hi < self.nl && self.lanes[hi].seg == l.seg && self.lanes[hi].dir == l.dir {
            hi += 1;
        }
        (lo, hi)
    }

    /// A faixa `cl` é utilizável por este veículo (ônibus só para ônibus em faixa exclusiva)?
    fn compatible(&self, i: usize, cl: usize) -> bool {
        !(self.lanes[cl].bus && self.kind[i] == 0)
    }

    /// Folga à frente de `s` na faixa `cl` (até o líder dessa faixa).
    fn gap_ahead_in(&self, cl: usize, s: f32, me: usize) -> f32 {
        let list = &self.edge_veh[cl];
        let p = list.partition_point(|&j| self.s[j as usize] < s);
        match list.get(p) {
            Some(&j) if j as usize != me => self.s[j as usize] - self.vlen[j as usize] - s,
            _ => LOOKAHEAD,
        }
    }

    /// Faixa vizinha (um passo) na direção da faixa do mesmo trecho/sentido que tem conversão para `nh`.
    fn lane_toward(&self, e: usize, nh: u32) -> Option<usize> {
        let (lo, hi) = self.lanes_of(e);
        let mut best: Option<(usize, usize)> = None;
        for cl in lo..hi {
            let ok = self.lanes[cl].conns.iter().any(|&c| self.lanes[self.conns[c as usize - self.nl].to as usize].dirseg == nh);
            if ok {
                let d = (cl as i32 - e as i32).unsigned_abs() as usize;
                if best.map_or(true, |b| d < b.1) {
                    best = Some((cl, d));
                }
            }
        }
        let (target, _) = best?;
        if target == e {
            return None;
        }
        Some(if target > e { e + 1 } else { e - 1 })
    }

    fn try_change(&mut self, i: usize, to: usize, forced: bool) -> bool {
        let e = self.edge[i] as usize;
        if !self.compatible(i, to) {
            return false;
        }
        let s = self.s[i];
        let list = &self.edge_veh[to];
        let p = list.partition_point(|&j| self.s[j as usize] < s);
        let margin = if forced { 0.5 } else { 1.0 };
        if let Some(&j) = list.get(p) {
            let j = j as usize;
            let gap = self.s[j] - self.vlen[j] - s;
            if gap < (4.0 + 0.4 * self.v[i]) * margin {
                return false;
            }
        }
        if p > 0 {
            let f = list[p - 1] as usize;
            let gap = s - self.vlen[i] - self.s[f];
            let need = (3.0 + self.v[f] * 0.9 - (self.v[f] - self.v[i]).min(0.0)) * margin;
            if gap < need {
                return false;
            }
        }
        // executa
        let old_off = self.lanes[e].off;
        let new_off = self.lanes[to].off;
        let pos = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
        self.edge_veh[e].remove(pos);
        self.edge[i] = to as u32;
        self.insert_sorted(to as u32, i);
        self.lat[i] += old_off - new_off;
        self.choose_conn(i);
        self.stats.lane_changes += 1;
        true
    }

    // ---- geração de viagens

    fn spawn(&mut self, net: &Net) {
        let _ = net;
        let nz = self.zone_ids.len();
        // filas de entrada (primeiro as pendentes)
        for g in 0..self.pending.len() {
            while let Some(&(origin, dest, kind, _)) = self.pending[g].front() {
                if self.spawn_vehicle(g as u32, origin as u16, dest, kind) {
                    self.pending[g].pop_front();
                    self.stats.queued = self.stats.queued.saturating_sub(1);
                } else {
                    break;
                }
            }
        }
        for i in 0..nz {
            for j in 0..nz {
                while self.next_spawn[i][j] <= self.time {
                    let rate = self.rate[i][j];
                    if rate <= 0.0 || self.gates[i].is_empty() {
                        self.next_spawn[i][j] = f64::INFINITY;
                        break;
                    }
                    let gate = self.gates[i][self.rng.below(self.gates[i].len())];
                    let kind = if (self.rng.f() as f64) < self.params.bus_share / 100.0 { 1 } else { 0 };
                    if !self.spawn_vehicle(gate, i as u16, j as u16, kind) {
                        if self.pending[gate as usize].len() < 200 {
                            self.pending[gate as usize].push_back((i as u32, j as u16, kind, self.time as f32));
                            self.stats.queued += 1;
                        }
                    }
                    self.next_spawn[i][j] += self.rng.exp(rate);
                }
            }
        }
    }

    // ------------------------------------------------------------------ saída

    /// Posição (x, y, rumo) do veículo `i`.
    fn pose(&self, net: &Net, i: usize) -> (f32, f32, f32) {
        let e = self.edge[i] as usize;
        if e < self.nl {
            let l = &self.lanes[e];
            let seg = &net.segs[l.seg as usize];
            let s = self.s[i].clamp(0.0, l.len);
            let a = if l.dir == 0 { l.a0 as f64 + s as f64 } else { l.a0 as f64 - s as f64 };
            let (p, t) = seg.poly.at(a);
            let sign = if l.dir == 0 { 1.0 } else { -1.0 };
            let q = add(p, mul(right(t), (l.off + self.lat[i]) as f64));
            (q[0] as f32, q[1] as f32, (t[1] * sign).atan2(t[0] * sign) as f32)
        } else {
            let c = &self.conns[e - self.nl];
            let u = (self.s[i] / c.len).clamp(0.0, 1.0);
            let p = bez8(&c.curve, u);
            let q = bez8(&c.curve, (u + 0.02).min(1.0));
            let p0 = bez8(&c.curve, (u - 0.02).max(0.0));
            let d = sub(q, p0);
            (p[0] as f32, p[1] as f32, d[1].atan2(d[0]) as f32)
        }
    }

    /// Estado dos veículos: por veículo [x, y, rumo, velocidade, tipo, comprimento, id].
    pub fn snapshot(&self, net: &Net, out: &mut Vec<f32>) {
        out.clear();
        for i in 0..self.alive.len() {
            if !self.alive[i] {
                continue;
            }
            let (x, y, a) = self.pose(net, i);
            out.extend_from_slice(&[x, y, a, self.v[i], self.kind[i] as f32, self.vlen[i], i as f32]);
        }
    }

    /// Rota restante do veículo `i` até a zona de destino: (pontos xy planos, id da zona, tipo, velocidade, espera).
    pub fn track(&self, net: &Net, i: usize) -> Option<(Vec<f32>, i64, u8, f32, f32)> {
        if i >= self.alive.len() || !self.alive[i] {
            return None;
        }
        let dest = self.dest[i] as usize;
        let mut pts: Vec<V2> = Vec::new();
        let (x, y, _) = self.pose(net, i);
        pts.push([x as f64, y as f64]);
        let lane_pts = |l: &LaneRt, from_s: f32| -> Vec<V2> {
            let seg = &net.segs[l.seg as usize];
            let mut v = if l.dir == 0 {
                seg.poly.slice(l.a0 as f64 + from_s as f64, l.a0 as f64 + l.len as f64)
            } else {
                let mut v = seg.poly.slice(l.a0 as f64 - l.len as f64, l.a0 as f64 - from_s as f64);
                v.reverse();
                v
            };
            if v.len() >= 2 {
                let m = miter(&v);
                // `off` é medido à direita do eixo no sentido do trecho; para dir 1 a lista foi invertida (direita trocada)
                let off = if l.dir == 0 { l.off as f64 } else { -(l.off as f64) };
                for (p, d) in v.iter_mut().zip(m) {
                    *p = add(*p, mul(d, off));
                }
            }
            v
        };
        let curve_pts = |c: &ConnRt, from_u: f32| -> Vec<V2> {
            let n = 10;
            (0..=n).map(|k| from_u + (1.0 - from_u) * k as f32 / n as f32).map(|u| {
                let p = bez8(&c.curve, u);
                [p[0], p[1]]
            }).collect()
        };
        let e = self.edge[i] as usize;
        let mut next_dirseg: u32 = NONE;
        let mut done = false;
        if e < self.nl {
            let l = &self.lanes[e];
            pts.extend(lane_pts(l, self.s[i]));
            let cc = self.choice[i];
            if cc == SINK {
                done = true;
            } else if cc != NONE {
                let c = &self.conns[cc as usize - self.nl];
                pts.extend(curve_pts(c, 0.0));
                let tl = &self.lanes[c.to as usize];
                pts.extend(lane_pts(tl, 0.0));
                next_dirseg = tl.dirseg;
                if self.sink[dest][tl.to_node as usize] {
                    done = true;
                }
            } else {
                next_dirseg = self.hops[dest][l.dirseg as usize];
            }
        } else {
            let c = &self.conns[e - self.nl];
            pts.extend(curve_pts(c, (self.s[i] / c.len).clamp(0.0, 1.0)));
            let tl = &self.lanes[c.to as usize];
            pts.extend(lane_pts(tl, 0.0));
            next_dirseg = tl.dirseg;
            if self.sink[dest][tl.to_node as usize] {
                done = true;
            }
        }
        let mut guard = 0;
        if !done && next_dirseg != NONE {
            // `next_dirseg` já foi desenhado quando veio de uma conversão; senão é o próximo trecho a percorrer
            let mut cur = if e < self.nl && self.choice[i] == NONE { next_dirseg } else { self.hops[dest][next_dirseg as usize] };
            while cur != NONE && guard < 4000 {
                guard += 1;
                let seg = &net.segs[(cur >> 1) as usize];
                let mut v = seg.poly.pts.clone();
                if cur & 1 == 1 {
                    v.reverse();
                }
                if v.len() >= 2 {
                    let m = miter(&v);
                    for (p, d) in v.iter_mut().zip(m) {
                        *p = add(*p, mul(d, 1.75));
                    }
                }
                pts.extend(v);
                let end = route::dirseg_end_node(net, cur);
                if self.sink[dest][end as usize] {
                    break;
                }
                cur = self.hops[dest][cur as usize];
            }
        }
        let flat: Vec<f32> = pts.iter().flat_map(|p| [p[0] as f32, p[1] as f32]).collect();
        Some((flat, self.zone_ids[dest], self.kind[i], self.v[i], self.wait[i]))
    }

    /// Relatório consolidado (JSON) da execução até agora.
    pub fn report(&self, net: &Net) -> serde_json::Value {
        let queued: u64 = self.pending.iter().map(|q| q.len() as u64).sum();
        let mut r = self.metrics.report(&self.zone_ids, &|n| net.nodes[n].osm_id, self.time - self.start_time, self.n_alive, queued, self.stats.spawned);
        // Atraso dos que ainda circulam (tempo decorrido menos o que levariam em fluxo livre até aqui) e dos
        // que esperam para entrar na rede: sem isso, um gargalo "melhora" a média só dos que conseguiram chegar.
        let mut alive_delay = 0.0f64;
        for i in 0..self.alive.len() {
            if !self.alive[i] {
                continue;
            }
            let e = self.edge[i] as usize;
            let dirseg = if e < self.nl { self.lanes[e].dirseg } else { self.lanes[self.conns[e - self.nl].from as usize].dirseg };
            let remaining = self.dists[self.dest[i] as usize][dirseg as usize];
            let remaining = if remaining.is_finite() { remaining } else { 0.0 };
            let free_so_far = (self.t_free[i] - remaining).max(0.0) as f64;
            alive_delay += ((self.time - self.t_spawn[i] as f64) - free_so_far).max(0.0);
        }
        let queue_delay: f64 = self.pending.iter().flat_map(|q| q.iter()).map(|p| (self.time - p.3 as f64).max(0.0)).sum();
        let finished_delay = r["totalDelayVehH"].as_f64().unwrap_or(0.0);
        let total_h = finished_delay + alive_delay / 3600.0 + queue_delay / 3600.0;
        let gen = (self.stats.spawned + queued).max(1) as f64;
        r["aliveDelayVehH"] = serde_json::json!(alive_delay / 3600.0);
        r["queueDelayVehH"] = serde_json::json!(queue_delay / 3600.0);
        r["totalDelayAllVehH"] = serde_json::json!(total_h);
        r["meanDelayAll"] = serde_json::json!(total_h * 3600.0 / gen);
        r["meanSpeedKmh"] = serde_json::json!(self.metrics.mean_speed_kmh());
        r
    }
    /// Razão média de velocidade por trecho direcionado em toda a execução (0..254; 255 = sem dados).
    pub fn run_ratio(&self, net: &Net) -> Vec<u8> {
        let fs = |d: usize| free_speed(&net.segs[d / 2]) * 0.9;
        self.metrics.run_ratio(&fs)
    }

    /// Estado de verificação dos testes: (aresta, s, v) por veículo vivo.
    /// Diagnóstico: veículos na frente da faixa parados há mais de `min_wait` s.
    pub fn stuck_report(&mut self, net: &Net, min_wait: f32) -> Vec<String> {
        let mut out = Vec::new();
        for e in 0..self.nl {
            let Some(&j) = self.edge_veh[e].last() else { continue };
            let j = j as usize;
            if self.wait[j] < min_wait {
                continue;
            }
            let cc = self.choice[j];
            let ok = if cc == NONE || cc == SINK { cc == SINK } else { self.may_enter(net, j, cc) };
            let l = &self.lanes[e];
            if cc < SINK {
                let to = self.conns[cc as usize - self.nl].to as usize;
                let tl = &self.lanes[to];
                let f = self.edge_veh[to].first().map(|&k| { let k = k as usize; format!("first(veh{} s={:.1} len={:.1} v={:.1} wait={:.0} choice={})", k, self.s[k], self.vlen[k], self.v[k], self.wait[k], self.choice[k]) });
                out.push(format!("   -> exit lane {} seg {} dir {} idx {} n={} {:?}", to, tl.seg, tl.dir, tl.idx, self.edge_veh[to].len(), f));
            }
            out.push(format!(
                "veh {} lane {} seg {} dir {} idx {} s={:.1}/{:.1} v={:.1} wait={:.0} choice={} kind={} end_state={} may_enter={} why={} nlane_veh={} dest={}",
                j, e, l.seg, l.dir, l.idx, self.s[j], l.len, self.v[j], self.wait[j],
                if cc == NONE { "NONE".into() } else if cc == SINK { "SINK".into() } else { format!("conn{}", cc) },
                if cc < SINK { self.conns[cc as usize - self.nl].kind as i32 } else { -1 },
                self.end_state[l.end_id as usize], ok, REASON.with(|r| r.get()), self.edge_veh[e].len(), self.dest[j]
            ));
        }
        out
    }

    pub fn od_of(&self, i: usize) -> Option<(u16, u16, u32, f32)> {
        if i < self.alive.len() && self.alive[i] { Some((self.origin[i], self.dest[i], self.edge[i], self.s[i])) } else { None }
    }

    pub fn debug_vehicles(&self) -> Vec<(u32, f32, f32, f32)> {
        (0..self.alive.len()).filter(|&i| self.alive[i]).map(|i| (self.edge[i], self.s[i], self.v[i], self.vlen[i])).collect()
    }
    pub fn edge_lists(&self) -> &Vec<Vec<u32>> {
        &self.edge_veh
    }
    pub fn vehicle_s(&self, i: u32) -> (f32, f32) {
        (self.s[i as usize], self.vlen[i as usize])
    }
    pub fn lane_count(&self) -> usize {
        self.nl
    }
    pub fn end_state_of(&self, end: u32) -> u8 {
        self.end_state[end as usize]
    }
}

/// Em fase verde, correntes que não são esquerda-contra-oposto não precisam disputar (a convergência continua sendo tratada pelo espaço de saída).
fn signaled_green_streams(signaled: bool, _me: &ConnRt, _o: &ConnRt) -> bool {
    let _ = signaled;
    false
}

fn len_v(a: V2) -> f64 {
    len(a)
}

/// Bézier cúbica (8 floats) no parâmetro `t`.
fn bez8(c: &[f32; 8], t: f32) -> V2 {
    let u = 1.0 - t;
    let (a, b, d, e) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [(a * c[0] + b * c[2] + d * c[4] + e * c[6]) as f64, (a * c[1] + b * c[3] + d * c[5] + e * c[7]) as f64]
}

/// Interseção de dois segmentos, devolvendo os parâmetros t, u em (0,1).
fn seg_cross_t(a: V2, b: V2, c: V2, d: V2) -> Option<(f64, f64)> {
    let r = sub(b, a);
    let s = sub(d, c);
    let den = cross(r, s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(c, a), s) / den;
    let u = cross(sub(c, a), r) / den;
    (t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0).then_some((t, u))
}
