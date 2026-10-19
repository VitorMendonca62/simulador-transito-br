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

thread_local! {
    pub static REASON: std::cell::Cell<u32> = std::cell::Cell::new(0);
    /// veículo responsável pelo último bloqueio em `may_enter` (-1 = nenhum)
    pub static BLOCKER: std::cell::Cell<i64> = std::cell::Cell::new(-1);
}
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
    /// % de motoristas que ignoram a proibição de conversão quando estão na faixa errada
    pub illegal_pct: f64,
    pub od_manual: std::collections::HashMap<String, f64>,
    /// Matriz fixada por hora do dia (24 entradas, veíc./h por "idOrigem>idDestino"). Quando presente,
    /// substitui `od_manual` e acompanha o relógio da simulação (importação da Pesquisa OD).
    pub od_hourly: Vec<std::collections::HashMap<String, f64>>,
    /// recalcula as rotas com o tempo medido nos trechos durante a simulação (estilo Waze)
    pub reroute: bool,
    /// % de motoristas que seguem o tempo real (app); os demais usam o tempo de fluxo livre
    pub app_pct: f64,
    /// erro de percepção (fração do tempo da próxima faixa) que espalha motoristas entre caminhos parecidos
    pub perception: f64,
    /// % de motoristas que entram no cruzamento sem espaço garantido na saída e podem ficar parados dentro dele
    /// ("fechar o cruzamento", proibido pelo CTB art. 45); os demais só entram com espaço para sair
    pub box_block_pct: f64,
}
impl Default for SimParams {
    fn default() -> Self {
        SimParams { seed: 1, hour: 8.0, bus_share: 2.0, beta: 0.08, intrazone: false, illegal_pct: 0.0, od_manual: Default::default(), od_hourly: Vec::new(), reroute: true, app_pct: 60.0, perception: 0.3, box_block_pct: 0.0 }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SimInit {
    pub zones: Vec<demand::ZoneIn>,
    pub params: SimParams,
    /// placas de preferência escolhidas pelo usuário (`nóOsm|approach_key` -> 1 preferencial, 2 dê a preferência, 3 pare)
    #[serde(skip)]
    pub prios: std::collections::HashMap<String, u8>,
}

/// Hierarquia padrão de uma aproximação (menor = mais preferencial): rotatória, depois classe da via.
pub fn approach_rank(net: &Net, end: u32) -> i16 {
    let s = &net.segs[(end >> 1) as usize];
    if s.roundabout { 0 } else { s.hw as i16 + 1 }
}

/// O que a escolha de rota precisa saber do motorista.
#[derive(Clone, Copy)]
struct Drv {
    tgt: usize,
    kind: u8,
    /// segue o tempo real (árvores recalculadas) ou o de fluxo livre
    app: bool,
    seed: u32,
}

/// Ruído determinístico em [-1, 1] por (motorista, faixa): a mesma faixa sempre "parece" igual ao mesmo motorista.
fn noise(seed: u32, x: u32) -> f32 {
    let mut z = ((seed as u64) << 32 | x as u64).wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

/// Tempo em meio segundo (u16) para a cópia de fluxo livre das árvores.
fn enc_t(d: f32) -> u16 {
    if d.is_finite() { (d * 2.0).round().min(65534.0) as u16 } else { u16::MAX }
}
fn dec_t(v: u16) -> f32 {
    if v == u16::MAX { f32::INFINITY } else { v as f32 * 0.5 }
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

/// Ponto de destino: um nó dentro da zona `zone`. `local[i]` é o tempo (s) da i-ésima faixa interna da zona até ele
/// (infinito se não chega por dentro da zona); `mean` é a média dos finitos (estimativa do trecho interno).
struct Target {
    node: u32,
    zone: u16,
    local: Vec<f32>,
    mean: f32,
    /// fração das faixas internas que chegam ao ponto
    reach: f32,
}

/// Teto de floats nas árvores locais; passado dele, só se reaproveitam pontos já criados na zona.
const LOCAL_BUDGET: usize = 16_000_000;

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
    /// placa "Pare": exige parada total antes de entrar
    stop: bool,
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
    pen: f32,
    /// parte da penalidade que é espera no cruzamento (substituída pela medição quando há dados)
    jpen: f32,
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
    /// meia extensão (m, ao longo das duas trajetórias) da área que um veículo ocupa ao passar pelo ponto
    w: f32,
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
    pub illegal_moves: u64,
    /// freadas bruscas (desaceleração > 5 m/s²), contadas uma vez por episódio
    pub harsh_brakes: u64,
    /// trocas de faixa na linha contínua antes do nó (quem ficou preso na fila da faixa errada)
    pub solid_crossings: u64,
    /// viagens da matriz que não nasceram: nenhuma faixa de origem da zona tem caminho até o destino
    pub skipped: u64,
    /// pares de veículos com as carrocerias sobrepostas dentro de um cruzamento, em conversões cujas trajetórias se
    /// cruzam (um por episódio): falha de comportamento, deve ser 0
    pub junction_overlaps: u64,
    /// idem, entre conversões que não se cruzam mas passam perto demais (geometria das curvas)
    pub junction_overlaps_geom: u64,
    /// veículos que ficaram parados (> 2 s) dentro de um cruzamento (um por episódio)
    pub box_stops: u64,
    /// entradas no cruzamento sem espaço para sair do outro lado (CTB art. 45), das quais por quem "fecha o cruzamento"
    pub box_entries: u64,
    pub box_entries_blockers: u64,
    /// veículos-segundo parados à espera de um ponto de conflito ocupado por quem está dentro do cruzamento
    pub box_wait_s: f64,
    /// impasses dentro do cruzamento desfeitos ignorando a ocupação (após `OCC_DEADLOCK` s parado)
    pub occ_deadlocks: u64,
}

/// floats por veículo no quadro enviado à interface (ver `Sim::snapshot`)
pub const SNAP_STRIDE: usize = 6;

fn lc_block_text(c: u8) -> &'static str {
    match c {
        1 => "faixa incompatível",
        2 => "linha contínua perto do nó",
        3 => "seta ligada, aguardando",
        4 => "sem vão à frente na faixa vizinha",
        5 => "sem vão atrás na faixa vizinha",
        _ => "nenhuma",
    }
}

/// zíper: folga (s) entre chegadas ao ponto de convergência e espera (s) a partir da qual a outra corrente cede
const MERGE_GAP: f32 = 1.0;
const MERGE_WAIT: f32 = 2.0;
/// abaixo desta velocidade (m/s) a corrente está em fila e alterna um a um; acima, só cede após `MERGE_COURTESY` s de espera
const MERGE_SLOW: f32 = 5.0;
const MERGE_COURTESY: f32 = 20.0;

/// MOBIL: polidez (peso do incômodo aos outros), limiar (m/s²), viés de manter a direita, freada máxima imposta ao novo seguidor
const MOBIL_P: f32 = 0.3;
const MOBIL_TH: f32 = 0.2;
const MOBIL_BIAS: f32 = 0.3;
const MOBIL_SAFE: f32 = 4.0;
/// fração de motoristas que cedem passagem a quem está com seta para uma troca obrigatória
const COURTESY: f32 = 0.7;

/// quanto a ocupação da faixa de destino encarece a troca de faixa (faixa cheia = custo × (1 + k))
const LC_OCC_K: f32 = 4.0;

/// desaceleração (m/s²) a partir da qual a freada conta como brusca
const HARSH: f32 = 5.0;

/// meia largura de um veículo mais folga (m): base da área ocupada num ponto de conflito
const OCC_HALF_W: f32 = 1.2;
/// curvas que não se cruzam mas passam a menos disto (m, entre os eixos) contam como conflito
const NEAR_CONFLICT: f64 = 1.8;
/// dentro do cruzamento, depois de tanto tempo parado o veículo deixa de respeitar a ocupação (sai do impasse)
const OCC_DEADLOCK: f32 = 40.0;

/// metros finais da faixa, antes do nó, em que não se troca de faixa (linha contínua)
const LC_SOLID_ZONE: f32 = 12.0;
/// tempo mínimo de seta ligada antes de mudar de faixa (s)
const LC_BLINK: f32 = 1.0;

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
    /// tempo até a zona de destino por faixa (até o primeiro nó dela): vale enquanto o veículo está fora da zona
    zone_dist: Vec<Vec<f32>>,
    /// cópia de fluxo livre de `zone_dist` (meio segundo), para quem não usa app e para o tempo livre da viagem
    zone_free: Vec<Vec<u16>>,
    /// faixas-alvo de cada zona (as que terminam num nó dela) e árvore em recálculo
    zone_tl: Vec<Vec<u32>>,
    job: Option<route::TreeJob>,
    job_buf: Vec<f32>,
    route_z: usize,
    pub reroutes: u64,
    /// atraso medido por conversão (s, média móvel de quem passou), instante da última amostra e penalidade viva
    conn_delay: Vec<f32>,
    conn_seen: Vec<f32>,
    live_pen: Vec<f32>,
    /// pontos de destino sorteados (nó dentro de uma zona + roteamento local até ele), criados quando alguém os sorteia
    targets: Vec<Target>,
    target_of: std::collections::HashMap<(u16, u32), u32>,
    zone_targets: Vec<Vec<u32>>,
    /// nós candidatos a destino de cada zona
    zone_cand: Vec<Vec<u32>>,
    /// faixas internas de cada zona (as duas pontas dentro) e, por faixa, a zona e o índice nela
    zone_lanes: Vec<Vec<u32>>,
    lane_zone: Vec<u16>,
    lane_local: Vec<u32>,
    lrouter: LaneRouter,
    local_used: usize,
    /// destino de cada veículo (índice em `targets`), fixo da origem ao fim da viagem
    tgt: Vec<u32>,
    /// custo de trocar de faixa em cada faixa (s)
    lane_w: Vec<f32>,
    rule_breaker: Vec<bool>,
    /// motorista que "fecha o cruzamento" (ver `SimParams::box_block_pct`)
    box_blocker: Vec<bool>,
    /// parado neste passo por um ponto de conflito ocupado (para `Stats::box_wait_s`)
    held_box: Vec<bool>,
    /// pares sobrepostos na última verificação (episódios de `Stats::junction_overlaps`)
    overlap_prev: std::collections::HashSet<(u32, u32)>,
    app: Vec<bool>,
    pseed: Vec<u32>,
    /// instante em que o veículo chegaria ao fim da faixa atual em fluxo livre (base do atraso por conversão)
    t_due: Vec<f32>,
    /// instante em que a viagem foi pedida (entra na fila de entrada) e se o veículo já saiu da faixa onde nasceu
    t_req: Vec<f32>,
    left_gate: Vec<bool>,
    /// por zona de origem: (espera na fila de entrada, tempo na faixa de nascimento, veículos) — diagnóstico das divisas
    pub entry_stats: Vec<(f64, f64, u32)>,
    dists: Vec<Vec<f32>>,
    zone_ids: Vec<i64>,
    zone_poly: Vec<Vec<[f64; 2]>>,
    zone_gateway: Vec<bool>,
    zone_profile: Vec<Vec<f64>>,
    zone_gen: Vec<f64>,
    zone_attr: Vec<f64>,
    sink: Vec<Vec<bool>>,
    gates: Vec<Vec<u32>>,
    /// comprimento acumulado das faixas de origem de cada zona (sorteio proporcional à extensão de rua)
    gate_cum: Vec<Vec<f32>>,
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
    /// seta ligada para trocar de faixa: faixa de destino e instante em que foi acionada (CTB arts. 34 e 35)
    blink_to: Vec<u32>,
    blink_t: Vec<f32>,
    /// por que a última tentativa de troca de faixa falhou (0 = nenhuma; ver `try_change`)
    lc_why: Vec<u8>,
    /// a troca de faixa pendente é obrigatória (rota), não por conforto
    mand: Vec<bool>,
    /// pedidos de passagem do passo: (faixa de destino, veículo com seta), ordenados
    wants: Vec<(u32, u32)>,
    acc: Vec<f32>,
    /// decisão de entrada em cruzamento reaproveitada por 1 passo (reavaliada a cada 2)
    enter_ok: Vec<bool>,
    /// pode passar pelo nó seguinte (sem semáforo) ao fim de uma faixa curta à frente?
    /// faixa (+1) em cuja linha de "Pare" o veículo já parou
    stop_ok: Vec<u32>,
    tick: u64,
    multi: Vec<u32>,
    pub n_alive: usize,
    pub metrics: metrics::Metrics,
    t_free: Vec<f32>,
    origin: Vec<u16>,
    start_time: f64,
    /// tempo acumulado (ms) por fase do passo — só nativo, para os testes de desempenho
    pub prof: [f64; 7],
    pub red_log: Vec<(u32, f32, u32, f32, f32)>,
    /// diagnóstico (testes): descrição de cada sobreposição nova, se ligado
    pub overlap_log: Option<Vec<String>>,
    /// diagnóstico (só com `overlap_log`): últimos estados (aresta, s, v, pode entrar) de cada veículo
    hist: Vec<std::collections::VecDeque<(u32, f32, f32, bool)>>,
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
                rank: approach_rank(net, end_id),
                stop: false,
                ring: s.roundabout,
                dirseg: l.seg * 2 + l.dir as u32,
                conns: Vec::new(),
            });
            let _ = li;
        }
        // ---- placas de preferência do usuário: preferencial vence tudo, pare/dê a preferência cedem a todos
        if !init.prios.is_empty() {
            for l in lanes.iter_mut() {
                let osm = net.nodes[l.to_node as usize].osm_id;
                let key = approach_key(net, l.to_node, l.end_id);
                match init.prios.get(&format!("{osm}|{key}")) {
                    Some(1) => l.rank = -1,
                    Some(2) => l.rank = 100,
                    Some(3) => {
                        l.rank = 100;
                        l.stop = true;
                    }
                    _ => {}
                }
            }
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
                pen: route::turn_penalty(net, c.kind, c.node),
                jpen: route::junction_penalty(net, c.node),
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
                // portais da divisa nascem na via que entra no mapa, mesmo que seja via expressa (BR-101, BR-232)
                if inside[l.from as usize] && !s.roundabout && !l.bus && (z.gateway || s.hw > HW_TRUNK) && lanes[li].len > 12.0 {
                    g.push(li as u32);
                }
            }
            sink.push(inside);
            gates.push(g);
        }

        // ---- roteamento
        let router = Router::new(net);
        let mut dists = Vec::with_capacity(nz);
        for sk in &sink {
            let targets: Vec<u32> = (0..net.segs.len() as u32 * 2).filter(|&d| router.time[d as usize].is_finite() && sk[dirseg_end_node(net, d) as usize]).collect();
            let (mut h, mut dd) = (Vec::new(), Vec::new());
            router.tree(&targets, &mut h, &mut dd);
            dists.push(dd);
        }
        // por faixa: conversões ligadas + trocas de faixa
        let lane_len: Vec<f32> = lanes.iter().map(|l| l.len).collect();
        let lane_speed: Vec<f32> = lanes.iter().map(|l| l.vfree).collect();
        let conn_list: Vec<(u32, u32, f32)> = conns.iter().map(|c| (c.from, c.to, c.pen)).collect();
        let lrouter = LaneRouter::new(net, &lane_len, &lane_speed, &conn_list);
        // faixas internas de cada zona (as duas pontas dentro dela) e árvore até a zona inteira
        let mut lane_zone = vec![u16::MAX; nl];
        let mut lane_local = vec![u32::MAX; nl];
        let mut zone_lanes: Vec<Vec<u32>> = vec![Vec::new(); nz];
        for (li, l) in net.lanes.iter().enumerate() {
            if let Some(z) = sink.iter().position(|sk| sk[l.from as usize] && sk[l.to as usize]) {
                lane_zone[li] = z as u16;
                lane_local[li] = zone_lanes[z].len() as u32;
                zone_lanes[z].push(li as u32);
            }
        }
        let mut incoming = vec![false; net.nodes.len()];
        for l in &lanes {
            if !l.bus {
                incoming[l.to_node as usize] = true;
            }
        }
        let mut zone_dist: Vec<Vec<f32>> = Vec::with_capacity(nz);
        let mut zone_tl: Vec<Vec<u32>> = Vec::with_capacity(nz);
        let mut zone_cand: Vec<Vec<u32>> = Vec::with_capacity(nz);
        for (z, sk) in sink.iter().enumerate() {
            let targets: Vec<u32> = (0..nl as u32).filter(|&l| sk[lanes[l as usize].to_node as usize] && !lanes[l as usize].bus).collect();
            let mut dd = Vec::new();
            lrouter.tree(&targets, &mut dd);
            zone_dist.push(dd);
            zone_tl.push(targets);
            // candidatos: nós onde chega alguma faixa interna da zona (dá para alcançá-los por dentro); senão, qualquer nó dela com entrada
            let mut cand: Vec<u32> = zone_lanes[z].iter().filter(|&&l| !lanes[l as usize].bus).map(|&l| lanes[l as usize].to_node).collect();
            cand.sort_unstable();
            cand.dedup();
            if cand.is_empty() {
                cand = (0..net.nodes.len() as u32).filter(|&n| sk[n as usize] && incoming[n as usize]).collect();
            }
            zone_cand.push(cand);
        }
        let lane_w = lrouter.change.clone();
        let gate_cum: Vec<Vec<f32>> = gates
            .iter()
            .map(|g: &Vec<u32>| {
                let mut a = 0.0f32;
                g.iter().map(|&l| { a += lanes[l as usize].len; a }).collect()
            })
            .collect();

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
            zone_free: if init.params.reroute { zone_dist.iter().map(|d| d.iter().map(|&x| enc_t(x)).collect()).collect() } else { Vec::new() },
            zone_dist,
            zone_tl,
            job: None,
            job_buf: Vec::new(),
            route_z: 0,
            reroutes: 0,
            conn_delay: Vec::new(),
            conn_seen: Vec::new(),
            live_pen: Vec::new(),
            targets: Vec::new(),
            target_of: Default::default(),
            zone_targets: vec![Vec::new(); nz],
            zone_cand,
            zone_lanes,
            lane_zone,
            lane_local,
            lrouter,
            local_used: 0,
            tgt: Vec::new(),
            lane_w,
            rule_breaker: Vec::new(),
            box_blocker: Vec::new(),
            held_box: Vec::new(),
            overlap_prev: Default::default(),
            overlap_log: None,
            hist: Vec::new(),
            app: Vec::new(),
            pseed: Vec::new(),
            t_due: Vec::new(),
            t_req: Vec::new(),
            left_gate: Vec::new(),
            entry_stats: vec![(0.0, 0.0, 0); nz],
            dists,
            zone_ids: init.zones.iter().map(|z| z.id).collect(),
            zone_poly: init.zones.iter().map(|z| z.poly.clone()).collect(),
            zone_gateway: init.zones.iter().map(|z| z.gateway).collect(),
            zone_profile: init.zones.iter().map(|z| z.profile.clone()).collect(),
            zone_gen: init.zones.iter().map(|z| z.gen).collect(),
            zone_attr: init.zones.iter().map(|z| z.attr).collect(),
            sink,
            gate_cum,
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
            blink_to: Vec::new(),
            blink_t: Vec::new(),
            lc_why: Vec::new(),
            mand: Vec::new(),
            wants: Vec::new(),
            acc: Vec::new(),
            enter_ok: Vec::new(),
            stop_ok: Vec::new(),
            tick: 0,
            multi: Vec::new(),
            n_alive: 0,
            metrics: metrics::Metrics::new(net.segs.len() * 2, net.nodes.len(), nz),
            t_free: Vec::new(),
            origin: Vec::new(),
            start_time: 0.0,
            prof: [0.0; 7],
            red_log: Vec::new(),
        };
        sim.start_time = sim.time;
        let nc = sim.conns.len();
        sim.conn_delay = vec![0.0; nc];
        sim.conn_seen = vec![f32::NEG_INFINITY; nc];
        sim.live_pen = sim.conns.iter().map(|c| c.pen).collect();
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

    /// Células fixadas da matriz na hora `hour`: a fatia horária (interpolada como o perfil das
    /// zonas, ver `demand::profile_at`), se houver, ou `od_manual`.
    fn od_at(&self, hour: f64) -> std::collections::HashMap<String, f64> {
        let h = &self.params.od_hourly;
        if h.len() != 24 {
            return self.params.od_manual.clone();
        }
        let hr = hour.rem_euclid(24.0);
        let (i, f) = (hr.floor() as usize % 24, hr - hr.floor());
        let mut out: std::collections::HashMap<String, f64> = h[i].iter().map(|(k, v)| (k.clone(), v * (1.0 - f))).collect();
        for (k, v) in &h[(i + 1) % 24] {
            *out.entry(k.clone()).or_insert(0.0) += v * f;
        }
        out
    }

    /// Recalcula a matriz OD para a hora atual e as taxas de chegada.
    fn refresh_matrix(&mut self, net: &Net) {
        let hour = (self.time / 3600.0).rem_euclid(24.0);
        let payload = demand::Payload {
            zones: (0..self.zone_ids.len())
                .map(|i| demand::ZoneIn { id: self.zone_ids[i], poly: self.zone_poly[i].clone(), gen: self.zone_gen[i], attr: self.zone_attr[i], profile: self.zone_profile[i].clone(), gateway: self.zone_gateway[i] })
                .collect(),
            params: demand::ParamsIn { beta: self.params.beta, intrazone: self.params.intrazone, hour, od_manual: self.od_at(hour) },
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

    // ------------------------------------------------------------------ rotas em tempo real

    /// Recalcula, em fatias, a árvore de uma zona por vez com o tempo medido em cada trecho (média móvel do mapa de
    /// calor). Como o veículo escolhe a conversão a cada faixa olhando a árvore atual, quem usa app desvia no caminho.
    fn route_step(&mut self) {
        let nz = self.zone_dist.len();
        if !self.params.reroute || nz == 0 {
            return;
        }
        if self.job.is_none() {
            // pesos do momento: faixa em fluxo livre + atraso medido de cada conversão (o atraso já inclui a
            // lentidão no trecho de quem fez aquela manobra, então não se soma também a velocidade do trecho)
            self.refresh_live_pen();
            let z = self.route_z % nz;
            let buf = std::mem::take(&mut self.job_buf);
            self.job = Some(self.lrouter.job_start(z, &self.zone_tl[z], buf));
        }
        // volta completa em ~3 min simulados (900 passos), com teto de custo por passo
        let budget = ((nz * self.nl) / 900).clamp(2_000, 12_000);
        let mut job = self.job.take().unwrap();
        if self.lrouter.job_run(&mut job, budget) {
            std::mem::swap(&mut self.zone_dist[job.zone], &mut job.dist);
            self.job_buf = job.dist;
            self.route_z = job.zone + 1;
            self.reroutes += 1;
        } else {
            self.job = Some(job);
        }
    }

    /// Diagnóstico das entradas por zona: espera média na fila de entrada, tempo médio até sair da faixa de nascimento,
    /// fila atual e faixas de origem.
    pub fn gate_report(&self, net: &Net) -> String {
        let _ = net;
        let mut out = String::new();
        for z in 0..self.zone_ids.len() {
            let (qw, lt, k) = self.entry_stats[z];
            let queued: usize = self.gates[z].iter().map(|&g| self.pending[g as usize].len()).sum();
            if k == 0 && queued == 0 {
                continue;
            }
            let on_gate: usize = self.gates[z].iter().map(|&g| self.edge_veh[g as usize].len()).sum();
            out += &format!(
                "zona {}{}: {} saíram da faixa de origem, espera na fila {:.0} s, tempo na faixa de origem {:.0} s, fila agora {}, na faixa de origem agora {} ({} faixas de origem)\n",
                self.zone_ids[z],
                if self.zone_gateway[z] { " (portal)" } else { "" },
                k,
                if k > 0 { qw / k as f64 } else { 0.0 },
                if k > 0 { lt / k as f64 } else { 0.0 },
                queued,
                on_gate,
                self.gates[z].len()
            );
        }
        out
    }

    /// Tempo (s) para percorrer o resto da faixa `l` a partir de `s` em fluxo livre (mesma base do roteador).
    fn free_left(&self, l: usize, s: f32) -> f32 {
        let ln = &self.lanes[l];
        (ln.len - s).max(0.0) / (ln.vfree * 0.85)
    }

    /// Atraso de quem acabou de entrar na conversão `ci`: tempo na faixa além do de fluxo livre (fila, semáforo,
    /// brecha, lentidão no trecho). Média móvel por conversão.
    fn sample_conn_delay(&mut self, ci: usize, d: f32) {
        let d = d.clamp(0.0, 600.0);
        let now = self.time as f32;
        self.conn_delay[ci] = if self.conn_seen[ci].is_finite() { 0.75 * self.conn_delay[ci] + 0.25 * d } else { d };
        self.conn_seen[ci] = now;
    }

    /// Multiplicador do custo de trocar para a faixa `l`: cresce com a ocupação dela (fila = difícil de entrar).
    fn lc_mul(&self, l: usize) -> f32 {
        let occ = (self.edge_veh[l].len() as f32 * 7.0 / self.lanes[l].len.max(7.0)).min(1.0);
        1.0 + LC_OCC_K * occ
    }

    /// Penalidade viva de cada conversão = manobra + atraso medido (ou a espera típica do cruzamento sem dados).
    /// Quem está parado agora na frente da fila também conta (fila que ainda não escoou ninguém).
    fn refresh_live_pen(&mut self) {
        let now = self.time as f32;
        for l in 0..self.nl {
            let Some(&j) = self.edge_veh[l].last() else { continue };
            let j = j as usize;
            let cc = self.choice[j];
            if cc != NONE && cc != SINK {
                let ci = cc as usize - self.nl;
                let ongoing = now - self.t_due[j];
                if ongoing > self.conn_delay[ci] {
                    self.conn_delay[ci] = ongoing.min(600.0);
                    if !self.conn_seen[ci].is_finite() {
                        self.conn_seen[ci] = now;
                    }
                }
            }
        }
        for l in 0..self.nl {
            let m = self.lc_mul(l);
            self.lrouter.change_mul[l] = m;
        }
        for ci in 0..self.conns.len() {
            let c = &self.conns[ci];
            let age = now - self.conn_seen[ci];
            let pen = if age.is_finite() {
                // sem ninguém passando há tempo, a medição perde valor e volta ao típico
                let w = (-(age - 60.0).max(0.0) / 300.0).exp();
                (c.pen - c.jpen) + w * self.conn_delay[ci] + (1.0 - w) * c.jpen
            } else {
                c.pen
            };
            self.live_pen[ci] = pen;
            self.lrouter.set_pen(ci, pen);
        }
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

    fn spawn_vehicle(&mut self, lane: u32, origin: u16, dest: u16, kind: u8, t_req: f32) -> bool {
        let Some(tree) = self.pick_target(dest as usize, lane as usize) else { return false };
        let drv = Drv { tgt: tree as usize, kind, app: (self.rng.f() as f64) < self.params.app_pct / 100.0, seed: self.rng.next() as u32 };
        // nasce na faixa do mesmo trecho que melhor leva ao destino escolhido
        let lane = {
            let (lo, hi) = self.lanes_of(lane as usize);
            let mut best = (f32::INFINITY, lane as usize);
            for cl in lo..hi {
                if kind == 0 && self.lanes[cl].bus {
                    continue;
                }
                let c = self.stay_cost(drv, cl).0;
                if c < best.0 {
                    best = (c, cl);
                }
            }
            best.1 as u32
        };
        let l = &self.lanes[lane as usize];
        if kind == 0 && l.bus {
            return false;
        }
        let (vlen, tgap, amax, bcomf, vfac) = if kind == 1 {
            (12.0, self.rng.range(1.4, 1.8), self.rng.range(0.9, 1.2), self.rng.range(1.8, 2.4), self.rng.range(0.8, 0.95))
        } else {
            (self.rng.range(4.1, 4.9), self.rng.range(0.9, 1.5), self.rng.range(1.6, 2.6), self.rng.range(2.2, 3.4), self.rng.range(0.9, 1.15))
        };
        // posição de nascimento: em faixas longas, um ponto qualquer ao longo dela (não só logo após o cruzamento)
        let llen = self.lanes[lane as usize].len;
        let mut s0 = vlen + 0.5;
        let mut placed = false;
        let attempts = if llen >= 45.0 { 4 } else { 1 };
        for k in 0..attempts {
            let cand = if llen >= 45.0 && k < attempts - 1 { self.rng.range((vlen + 12.0).min(llen * 0.4), llen - 12.0) } else { vlen + 0.5 };
            let list = &self.edge_veh[lane as usize];
            let p = list.partition_point(|&j| self.s[j as usize] < cand);
            let lead_ok = list.get(p).map_or(true, |&j| {
                let j = j as usize;
                self.s[j] - self.vlen[j] - cand >= 6.0 + self.v[j] * 0.8
            });
            let foll_ok = if p > 0 {
                let f = list[p - 1] as usize;
                cand - vlen - self.s[f] >= 4.0 + self.v[f] * 1.0
            } else {
                true
            };
            if lead_ok && foll_ok {
                s0 = cand;
                placed = true;
                break;
            }
        }
        if !placed {
            return false;
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
                self.blink_to.push(NONE);
                self.blink_t.push(0.0);
                self.lc_why.push(0);
                self.mand.push(false);
                self.acc.push(0.0);
                self.enter_ok.push(false);
                self.stop_ok.push(0);
                self.t_free.push(0.0);
                self.origin.push(0);
                self.rule_breaker.push(false);
                self.box_blocker.push(false);
                self.held_box.push(false);
                self.app.push(false);
                self.pseed.push(0);
                self.t_due.push(0.0);
                self.t_req.push(0.0);
                self.left_gate.push(false);
                self.tgt.push(0);
                i
            }
        };
        self.alive[id] = true;
        self.edge[id] = lane;
        self.s[id] = s0;
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
        self.rule_breaker[id] = (self.rng.f() as f64) < self.params.illegal_pct / 100.0;
        // sorteio fora do PRNG da simulação: com 0 % o resto da simulação não muda
        self.box_blocker[id] = ((noise(drv.seed, 0xB0C5) + 1.0) * 50.0) < self.params.box_block_pct as f32;
        self.held_box[id] = false;
        self.tgt[id] = tree;
        self.app[id] = drv.app;
        self.pseed[id] = drv.seed;
        self.t_free[id] = {
            let d = self.dist(tree as usize, lane as usize, false);
            if d.is_finite() { d } else { 0.0 }
        };
        self.t_spawn[id] = self.time as f32;
        self.wait[id] = 0.0;
        self.stop_ok[id] = 0;
        self.cool[id] = self.rng.range(0.0, 2.0);
        self.blink_to[id] = NONE;
        self.lc_why[id] = 0;
        self.mand[id] = false;
        self.insert_sorted(lane, id);
        self.t_due[id] = self.time as f32 + self.free_left(lane as usize, s0);
        self.t_req[id] = t_req;
        self.left_gate[id] = false;
        self.metrics.entered(self.lanes[lane as usize].dirseg as usize);
        self.n_alive += 1;
        self.stats.spawned += 1;
        self.choose_conn(id);
        true
    }

    /// Faixa de origem sorteada com probabilidade proporcional ao comprimento.
    fn pick_gate(&mut self, zone: usize) -> u32 {
        let cum = &self.gate_cum[zone];
        let total = cum.last().copied().unwrap_or(0.0);
        let r = self.rng.f() * total;
        let k = cum.partition_point(|&c| c <= r).min(cum.len() - 1);
        self.gates[zone][k]
    }

    fn zone_reaches(&self, zone: usize, lane: usize) -> bool {
        self.zone_dist[zone][lane].is_finite()
    }

    /// Cria o ponto de destino `node` da zona `z` (árvore local só com as faixas internas dela).
    fn make_target(&mut self, z: usize, node: u32) -> u32 {
        let members = &self.zone_lanes[z];
        let zid = z as u16;
        let ends: Vec<u32> = members.iter().copied().filter(|&l| self.lanes[l as usize].to_node == node && !self.lanes[l as usize].bus).collect();
        let local_of = |l: u32| if self.lane_zone[l as usize] == zid { self.lane_local[l as usize] } else { NONE };
        let local = self.lrouter.tree_local(&ends, &local_of, members.len());
        let (sum, cnt) = local.iter().filter(|d| d.is_finite()).fold((0.0f64, 0usize), |a, &d| (a.0 + d as f64, a.1 + 1));
        let t = Target { node, zone: zid, mean: if cnt > 0 { (sum / cnt as f64) as f32 } else { 0.0 }, reach: cnt as f32 / local.len().max(1) as f32, local };
        self.local_used += t.local.len();
        let id = self.targets.len() as u32;
        self.targets.push(t);
        self.target_of.insert((zid, node), id);
        self.zone_targets[z].push(id);
        id
    }

    /// Sorteia o ponto de destino do veículo que vai nascer na faixa `lane` rumo à zona `dest`: um nó qualquer da
    /// zona, que fica o mesmo até o fim da viagem.
    fn pick_target(&mut self, dest: usize, lane: usize) -> Option<u32> {
        if !self.zone_dist[dest][lane].is_finite() {
            return None;
        }
        let nc = self.zone_cand[dest].len();
        if nc == 0 {
            return None;
        }
        if self.local_used > LOCAL_BUDGET && !self.zone_targets[dest].is_empty() {
            let k = self.rng.below(self.zone_targets[dest].len());
            return Some(self.zone_targets[dest][k]);
        }
        let mut last = None;
        for _ in 0..6 {
            let node = self.zone_cand[dest][self.rng.below(nc)];
            let id = match self.target_of.get(&(dest as u16, node)) {
                Some(&t) => t,
                None => self.make_target(dest, node),
            };
            last = Some(id);
            // evita pontos isolados do resto da zona
            if self.targets[id as usize].reach >= 0.3 {
                break;
            }
        }
        last
    }

    /// Tempo livre (s) da faixa `l` até o destino `tg`: pelo roteamento local dentro da zona; fora dela, até a zona mais o trecho interno típico.
    /// Árvore até a zona `z` na visão do motorista (tempo real ou fluxo livre).
    #[inline]
    fn zone_d(&self, z: usize, l: usize, live: bool) -> f32 {
        if live || self.zone_free.is_empty() { self.zone_dist[z][l] } else { dec_t(self.zone_free[z][l]) }
    }

    fn drv(&self, i: usize) -> Drv {
        Drv { tgt: self.tgt[i] as usize, kind: self.kind[i], app: self.app[i], seed: self.pseed[i] }
    }

    fn dist(&self, tg: usize, l: usize, live: bool) -> f32 {
        let t = &self.targets[tg];
        if self.lane_zone[l] == t.zone {
            let d = t.local[self.lane_local[l] as usize];
            if d.is_finite() {
                return d;
            }
        }
        let d = self.zone_d(t.zone as usize, l, live);
        if d.is_finite() { d + t.mean } else { d }
    }

    /// (id, instante de nascimento, nó de destino) dos veículos em circulação.
    pub fn vehicle_targets(&self) -> Vec<(usize, u32, u32)> {
        (0..self.alive.len()).filter(|&i| self.alive[i]).map(|i| (i, self.t_spawn[i].to_bits(), self.targets[self.tgt[i] as usize].node)).collect()
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

    /// Melhor conversão da faixa `l` para o destino: (custo até a zona, conversão). Carros ignoram faixas de ônibus.
    fn stay_cost(&self, drv: Drv, l: usize) -> (f32, u32) {
        let (tree, kind) = (drv.tgt, drv.kind);
        let t = &self.targets[tree];
        let lane = &self.lanes[l];
        if lane.to_node == t.node {
            return (0.0, SINK);
        }
        if kind == 0 && lane.bus {
            return (f32::INFINITY, NONE);
        }
        let z = t.zone as usize;
        let sigma = self.params.perception as f32;
        // quem pode seguir por dentro da zona até o ponto sempre prefere isso; senão vai rumo à zona
        let (mut local, mut other) = ((f32::INFINITY, NONE), (f32::INFINITY, NONE));
        for &c in &lane.conns {
            let cn = &self.conns[c as usize - self.nl];
            if kind == 0 && self.lanes[cn.to as usize].bus {
                continue;
            }
            let nx = cn.to as usize;
            // percepção: a próxima via parece um pouco mais rápida ou lenta para cada motorista (mesmo erro em
            // todas as faixas dela, para não inventar trocas de faixa entre faixas equivalentes)
            // a conversão imediata usa a penalidade fixa: o atraso medido nela inclui a espera do próprio veículo
            // (quem está na frente da fila); o atraso dos cruzamentos seguintes já está na árvore (`zone_d`)
            let pen = cn.pen;
            let err = if sigma > 0.0 { sigma * noise(drv.seed, self.lanes[nx].dirseg) * (cn.pen + self.lrouter.time[nx]) } else { 0.0 };
            if self.lane_zone[nx] == t.zone {
                let d = t.local[self.lane_local[nx] as usize];
                if d.is_finite() {
                    if pen + d + err < local.0 {
                        local = (pen + d + err, c);
                    }
                    continue;
                }
            }
            let d = pen + self.zone_d(z, nx, drv.app) + t.mean + err;
            if d < other.0 {
                other = (d, c);
            }
        }
        if local.1 != NONE {
            return local;
        }
        // chegou a um nó da zona de destino e não há caminho por dentro dela até o ponto sorteado: sai aqui (ex.: portal
        // da divisa que cobre as duas pistas — o ponto sorteado pode ser a ponta da outra pista, inalcançável)
        if self.sink[t.zone as usize][lane.to_node as usize] {
            return (0.0, SINK);
        }
        other
    }

    /// Escolhe a conversão da faixa atual com menor custo até o destino (se a faixa não leva ao destino por
    /// um caminho curto, ela leva por um mais longo: o veículo "dá a volta" em vez de empacar).
    fn choose_conn(&mut self, id: usize) {
        let e = self.edge[id] as usize;
        if e >= self.nl {
            return;
        }
        let (cost, c) = self.stay_cost(self.drv(id), e);
        self.choice[id] = if c == SINK {
            SINK
        } else if cost.is_finite() {
            c
        } else {
            NONE
        };
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
                out.push(Conflict { other: o, s_me: me.len, s_other: other.len, merge: true, w: 0.0 });
                continue;
            }
            let pts_o: Vec<V2> = (0..=12).map(|i| bez8(&other.curve, i as f32 / 12.0)).collect();
            let cum_o = cum(&pts_o);
            let scale_o = other.len as f64 / cum_o.last().copied().unwrap_or(1.0).max(1e-6);
            let mut found = false;
            'search: for a in 0..pts_me.len() - 1 {
                for b in 0..pts_o.len() - 1 {
                    if let Some((t, u)) = seg_cross_t(pts_me[a], pts_me[a + 1], pts_o[b], pts_o[b + 1]) {
                        let s_me = (cum_me[a] + t * (cum_me[a + 1] - cum_me[a])) * scale_me;
                        let s_o = (cum_o[b] + u * (cum_o[b + 1] - cum_o[b])) * scale_o;
                        // quanto mais rasante o ângulo, mais longa a área comum às duas trajetórias
                        let (da, db) = (sub(pts_me[a + 1], pts_me[a]), sub(pts_o[b + 1], pts_o[b]));
                        let sin = (cross(da, db).abs() / (len_v(da) * len_v(db)).max(1e-9)) as f32;
                        let w = (OCC_HALF_W / sin.max(0.2)).clamp(OCC_HALF_W, 5.5);
                        out.push(Conflict { other: o, s_me: s_me as f32, s_other: s_o as f32, merge: false, w });
                        found = true;
                        break 'search;
                    }
                }
            }
            // não se cruzam, mas passam tão perto que as carrocerias se tocariam: conflito no ponto mais próximo
            if !found {
                let mut best = (f64::MAX, 0.0, 0.0);
                for (a, &p) in pts_me.iter().enumerate() {
                    for b in 0..pts_o.len() - 1 {
                        let (q0, q1) = (pts_o[b], pts_o[b + 1]);
                        let d = sub(q1, q0);
                        let t = (dot(sub(p, q0), d) / dot(d, d).max(1e-9)).clamp(0.0, 1.0);
                        let dd = len_v(sub(p, add(q0, mul(d, t))));
                        if dd < best.0 {
                            best = (dd, cum_me[a] * scale_me, (cum_o[b] + t * (cum_o[b + 1] - cum_o[b])) * scale_o);
                        }
                    }
                }
                // longe das pontas: perto do começo é o mesmo trecho de chegada; perto do fim, a mesma saída
                if best.0 < NEAR_CONFLICT && best.1 > 0.5 && best.2 > 0.5 && best.1 < me.len as f64 - 0.5 && best.2 < other.len as f64 - 0.5 {
                    out.push(Conflict { other: o, s_me: best.1 as f32, s_other: best.2 as f32, merge: false, w: OCC_HALF_W * 1.5 });
                }
            }
        }
        let _ = net;
        let rc = std::rc::Rc::new(out);
        self.conflicts[c as usize] = Some(rc.clone());
        rc
    }

    /// Veículo da conversão `cf.other` com a carroceria sobre a área do ponto de conflito que ainda não a terá
    /// liberado daqui a `t_arr` s (quando quem pergunta chegaria lá). Inclui quem já passou à faixa de saída com a
    /// traseira ainda na curva.
    fn occ_by(&self, cf: &Conflict, t_arr: f32, me: usize) -> Option<usize> {
        let other = cf.other as usize;
        let (lo, hi) = (cf.s_other - cf.w, cf.s_other + cf.w);
        let clear = |j: usize, rear: f32| (hi - rear) / self.v[j].max(0.05) > t_arr;
        for &j in &self.edge_veh[other] {
            let j = j as usize;
            let (f, r) = (self.s[j], self.s[j] - self.vlen[j]);
            if j != me && f >= lo && r <= hi && clear(j, r) {
                return Some(j);
            }
        }
        let olen = self.edge_len[other];
        for &j in self.edge_veh[self.conns[other - self.nl].to as usize].iter().take(3) {
            let j = j as usize;
            let r = olen + self.s[j] - self.vlen[j];
            if r > hi {
                break;
            }
            if j != me && olen + self.s[j] >= lo && clear(j, r) {
                return Some(j);
            }
        }
        None
    }

    /// Ponto de conflito à frente ocupado na conversão `cc`, para o veículo `i` cuja frente está em `s_on` na
    /// conversão (negativo = ainda na faixa, antes da linha): obstáculo parado antes da área (folga, 0).
    fn occ_obstacle(&mut self, net: &Net, i: usize, cc: u32, s_on: f32) -> Option<(f32, f32)> {
        // impasse dentro do cruzamento (ex.: quatro "fechando" uns aos outros): segue mesmo assim
        if s_on >= 0.0 && self.wait[i] > OCC_DEADLOCK {
            return None;
        }
        let cfs = self.conflicts_of(net, cc);
        let v = self.v[i].max(1.0);
        let mut best: Option<(f32, f32)> = None;
        for cf in cfs.iter() {
            if cf.merge {
                continue;
            }
            let stop = cf.s_me - cf.w - 0.5;
            if s_on > stop + 0.5 {
                continue; // já está na área: parar ali só pioraria
            }
            if best.map_or(false, |b| stop - s_on >= b.0) {
                continue;
            }
            let t_arr = (cf.s_me - cf.w - s_on).max(0.0) / v;
            if self.occ_by(cf, t_arr, i).is_some() {
                best = Some(((stop - s_on).max(0.0), 0.0));
                continue;
            }
            // outro veículo já na curva concorrente vai chegar à área antes de mim e ocupá-la enquanto eu passo:
            // freio se ainda dá (quem chega depois cede; empate pelo id). Última defesa contra a colisão quando a
            // brecha foi mal avaliada na linha.
            if s_on < 0.0 {
                continue;
            }
            let d_stop = stop - s_on;
            if d_stop < self.v[i] * self.v[i] / (2.0 * 7.0) {
                continue; // não dá mais para parar antes: segue (o outro é quem cede)
            }
            let t_clear_me = (cf.s_me + cf.w + self.vlen[i] - s_on) / v;
            let lo = cf.s_other - cf.w;
            for &j in &self.edge_veh[cf.other as usize] {
                let j = j as usize;
                if self.s[j] >= lo {
                    continue;
                }
                let t_j = (lo - self.s[j]) / self.v[j].max(0.5);
                if t_j < t_clear_me && (t_j < t_arr || (t_j == t_arr && j < i)) {
                    best = Some((d_stop.max(0.0), 0.0));
                    break;
                }
            }
        }
        best
    }

    /// Faixa `l` curta demais para o veículo `i` parar inteiro nela (só a de um nó a outro do mesmo cruzamento).
    fn short_link(&self, l: usize, i: usize) -> bool {
        self.lanes[l].len < self.vlen[i] + 1.0
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

    /// Faixas curtas à frente (comuns no OSM, em cruzamentos de vários nós): quem vai ter de ceder num nó sem semáforo
    /// (ou ao fim de um trecho mais curto que o veículo, mesmo com semáforo) logo depois já decide daqui — senão só "vê" o cruzamento a poucos metros da linha, sem distância para parar.
    /// Devolve a distância até a linha onde terá de parar, ou ∞.
    fn ahead_stop_at(&mut self, net: &Net, i: usize, cc: u32, rest: f32) -> f32 {
        let d_check = (self.v[i] * self.v[i] / (2.0 * self.bcomf[i]) + 20.0).max(25.0);
        let (mut c, mut dist) = (cc, rest);
        // onde parar: na linha de uma faixa que comporte o veículo inteiro (senão a traseira fecha o nó anterior)
        let mut last_ok = rest;
        for _ in 0..3 {
            let cn = &self.conns[c as usize - self.nl];
            let f = cn.to as usize;
            let d2 = dist + cn.len + self.lanes[f].len;
            if self.lanes[f].len >= 30.0 || d2 > d_check || !self.edge_veh[f].is_empty() || !self.edge_veh[c as usize].is_empty() {
                break;
            }
            if self.end_state[self.lanes[f].end_id as usize] != 0 && !self.short_link(f, i) {
                break;
            }
            let c2 = self.stay_cost(self.drv(i), f).1;
            if c2 == NONE || c2 == SINK {
                break;
            }
            let fits = self.lanes[f].len >= self.vlen[i] + 1.0;
            if !self.may_enter_from(net, i, c2, f, d2) {
                return if fits { d2 } else { last_ok };
            }
            if fits {
                last_ok = d2;
            }
            (c, dist) = (c2, d2);
        }
        f32::INFINITY
    }

    /// Pode entrar na conversão `cc` agora? (semáforo, preferência por brecha, espaço de saída)
    fn may_enter(&mut self, net: &Net, id: usize, cc: u32) -> bool {
        self.held_box[id] = false;
        let lane_id = self.edge[id] as usize;
        let d_end = self.lanes[lane_id].len - self.s[id];
        self.may_enter_from(net, id, cc, lane_id, d_end)
    }

    /// `may_enter` como se o veículo estivesse na faixa `lane_id`, a `d_end` do fim dela (também para o nó seguinte,
    /// ao fim de uma faixa curta à frente)
    fn may_enter_from(&mut self, net: &Net, id: usize, cc: u32, lane_id: usize, d_end: f32) -> bool {
        REASON.with(|r| r.set(0));
        BLOCKER.with(|b| b.set(-1));
        if cc == NONE {
            { REASON.with(|r| r.set(1)); return false; }
        }
        if cc == SINK {
            return true;
        }
        let end = self.lanes[lane_id].end_id as usize;
        let st = self.end_state[end];
        let v = self.v[id];
        let b = self.bcomf[id];
        // trecho mais curto que o veículo entre dois nós (canteiro central estreito): quem está nele ainda está dentro do
        // cruzamento (a traseira fica no nó anterior) e termina a travessia sem esperar o semáforo do fim, cedendo a todos
        // — senão, parado ali, fecha o nó anterior e trava quem vem no sentido oposto
        let clearing = st != 0 && self.short_link(lane_id, id);
        let st = if clearing { 0 } else { st };
        let signaled = st != 0;
        if st == 1 {
            { REASON.with(|r| r.set(2)); return false; }
        }
        if st == 2 && v * v / (2.0 * b) < d_end - 1.0 {
            { REASON.with(|r| r.set(3)); return false; } // amarelo e dá para parar
        }
        // placa "Pare": para na linha por ~1,5 s antes de avaliar a brecha
        if !signaled && self.lanes[lane_id].stop && self.stop_ok[id] != lane_id as u32 + 1 {
            if v < 0.3 && d_end < 5.0 && self.wait[id] >= 1.5 {
                self.stop_ok[id] = lane_id as u32 + 1;
            } else {
                REASON.with(|r| r.set(10));
                return false;
            }
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
                        BLOCKER.with(|b| b.set(j as i64));
                        return false;
                    }
                }
            }
        }
        // espaço de saída (keep-clear): fim da faixa de destino e entrada da conversão
        // impasse prolongado (anel/entroncamento travado): aceita "bloquear o cruzamento", reduzindo a folga exigida
        let need = (self.vlen[id] + 2.0) * (1.0 - ((self.wait[id] - 30.0) / 50.0).clamp(0.0, 1.0)).max(0.1);
        // quem "fecha o cruzamento" entra com qualquer brecha na saída e, se a fila parar, fica parado na curva
        let need = if self.box_blocker[id] { need.min(1.0) } else { need };
        // quem já está andando na faixa de saída vai estar mais à frente quando eu chegar lá
        let t_enter = (d_end + self.conns[me_idx].len) / v.max(3.0);
        if let Some(&j) = self.edge_veh[to_lane as usize].first() {
            let j = j as usize;
            let moving = if self.v[j] > 1.5 { self.v[j].min(8.0) * 0.6 * t_enter } else { 0.0 };
            if self.s[j] - self.vlen[j] + moving < need {
                { REASON.with(|r| r.set(4)); BLOCKER.with(|b| b.set(j as i64)); return false; }
            }
        }
        if let Some(&j) = self.edge_veh[cc as usize].first() {
            let j = j as usize;
            if self.s[j] - self.vlen[j] < 2.0 {
                { REASON.with(|r| r.set(5)); BLOCKER.with(|b| b.set(j as i64)); return false; }
            }
        }
        let relaxed = self.wait[id] > 10.0;
        let conflicts = self.conflicts_of(net, cc);
        // ponto de conflito ocupado por quem está dentro do cruzamento (inclusive parado, "fechando" o cruzamento):
        // ninguém entra, com qualquer sinal ou preferência — exceto quem já está dentro dele, num trecho mais curto que
        // o veículo, parado há muito tempo (impasse, como em `occ_obstacle`)
        let inside_stuck = lane_id == self.edge[id] as usize && self.short_link(lane_id, id) && self.wait[id] > OCC_DEADLOCK;
        if !inside_stuck {
            let a = (self.amax[id] * 0.6).max(0.5);
            for cf in conflicts.iter() {
                if cf.merge {
                    continue;
                }
                let dist = (d_end + cf.s_me - cf.w).max(0.0);
                let t_arr = (-v + (v * v + 2.0 * a * dist).sqrt()) / a;
                if let Some(j) = self.occ_by(cf, t_arr, id) {
                    if lane_id == self.edge[id] as usize {
                        self.held_box[id] = true;
                    }
                    REASON.with(|r| r.set(11));
                    BLOCKER.with(|b| b.set(j as i64));
                    return false;
                }
                // quem já está na curva concorrente (andando) e chega à área antes de eu sair dela, mesmo sem ter
                // preferência (ex.: duas esquerdas opostas no mesmo verde): não entro na frente dele
                let lo = cf.s_other - cf.w;
                let t_clear = t_arr + (2.0 * cf.w + self.vlen[id]) / v.max(2.0);
                for &j in &self.edge_veh[cf.other as usize] {
                    let j = j as usize;
                    if self.s[j] >= lo || self.v[j] < 0.5 {
                        continue;
                    }
                    let t_j = (lo - self.s[j]) / self.v[j];
                    if t_j < t_clear && t_j <= t_arr + 1.0 {
                        REASON.with(|r| r.set(7));
                        BLOCKER.with(|b| b.set(j as i64));
                        return false;
                    }
                }
            }
        }
        let t_gap = if relaxed { 1.0 } else { 2.2 };
        let v_me = v.max(2.0);
        // tempo para percorrer `dist` partindo da velocidade atual e acelerando (parado, não se cruza a 2 m/s constantes)
        let a_me = (self.amax[id] * 0.6).max(0.5);
        let t_reach = |dist: f32| (-v + (v * v + 2.0 * a_me * dist.max(0.0)).sqrt()) / a_me;
        // paciência: quem espera muito só cede a quem realmente chega logo
        let patient = self.wait[id] > 30.0;
        for &cf in conflicts.iter() {
            let oi = cf.other as usize - self.nl;
            let (me, o) = (&self.conns[me_idx], &self.conns[oi]);
            // fusão sem placa nem semáforo e de mesma hierarquia: zíper (quem chega primeiro entra), sem prioridade fixa à direita
            let zip = cf.merge && !signaled && me.rank == o.rank;
            if !clearing && (zip || !self.has_priority(me, o, signaled)) {
                // convergência na mesma faixa sem preferência (zíper): em movimento, entra quem chega primeiro ao ponto,
                // com uma folga; quem já está parado esperando entra quando o próximo da outra corrente ainda consegue
                // frear com conforto — esse cede (senão, num fluxo contínuo, quem espera nunca entraria)
                if cf.merge && !signaled_green_streams(signaled, me, o) {
                    let t_me = t_reach(d_end + cf.s_me).min((d_end + cf.s_me) / v_me);
                    let queued = self.wait[id] > MERGE_WAIT;
                    let blocked = self.edge_veh[cf.other as usize].iter().find(|&&j| {
                        let j = j as usize;
                        if self.s[j] - self.vlen[j] > cf.s_other + 1.0 {
                            return false; // já passou do ponto
                        }
                        let t_j = ((cf.s_other - self.s[j]).max(0.0)) / self.v[j].max(2.0);
                        t_j < t_me + MERGE_GAP || (t_j == t_me && cf.other < cc)
                    }).or_else(|| self.edge_veh[o.from as usize].iter().rev().take(2).find(|&&j| {
                        let j = j as usize;
                        if self.v[j] < 0.5 {
                            return false; // parado na linha: a ordem de espera (acima) arbitra
                        }
                        let d_j = self.lanes[o.from as usize].len - self.s[j];
                        let can_stop = self.v[j] * self.v[j] / (2.0 * self.bcomf[j]) + 3.0 < d_j;
                        // cede a quem espera: em fila (corrente lenta) alterna um a um; em fluxo livre só depois de muita espera
                        if can_stop && ((queued && self.v[j] < MERGE_SLOW) || self.wait[id] > MERGE_COURTESY) {
                            return false;
                        }
                        let t_j = (d_j + cf.s_other) / self.v[j].max(2.0);
                        t_j < t_me + MERGE_GAP || (t_j == t_me && cf.other < cc)
                    })).copied();
                    if let Some(j) = blocked {
                        { REASON.with(|r| r.set(6)); BLOCKER.with(|b| b.set(j as i64)); return false; }
                    }
                    // eu ainda posso frear e alguém da outra corrente está parado esperando há mais tempo: cedo a vez
                    if self.v[id] * self.v[id] / (2.0 * self.bcomf[id]) + 3.0 < d_end {
                        let slow = self.v[id] < MERGE_SLOW;
                        if let Some(&k) = self.edge_veh[o.from as usize].last() {
                            let k = k as usize;
                            if self.choice[k] == cf.other && self.v[k] < 0.5 && self.wait[k] > self.wait[id] && ((slow && self.wait[k] > MERGE_WAIT) || self.wait[k] > MERGE_COURTESY) {
                                { REASON.with(|r| r.set(6)); BLOCKER.with(|b| b.set(k as i64)); return false; }
                            }
                        }
                    }
                }
                continue;
            }
            // chegada minha ao ponto de conflito
            let t_me = t_reach(d_end + cf.s_me).min((d_end + cf.s_me) / v_me);
            // veículos já na conversão concorrente
            for &j in &self.edge_veh[cf.other as usize] {
                let j = j as usize;
                let (sj, lj) = (self.s[j], self.vlen[j]);
                if sj - lj > cf.s_other + 1.5 {
                    continue; // já liberou o ponto
                }
                let t_j = if sj >= cf.s_other { 0.0 } else { (cf.s_other - sj) / self.v[j].max(2.0) };
                if !(t_me + t_gap < t_j) {
                    { REASON.with(|r| r.set(7)); BLOCKER.with(|b| b.set(j as i64)); return false; }
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
                // parado na linha e sem poder entrar (fila, saída cheia): não vai ocupar o cruzamento agora
                if self.v[j] < 0.5 && !self.enter_ok[j] {
                    continue;
                }
                // quem está parado na linha há tempo está bloqueado (não vai ocupar o cruzamento já): não prende quem espera muito
                if self.v[j] < 0.5 && self.wait[j] > 3.0 && self.wait[id] > 6.0 {
                    continue;
                }
                let d_j = self.lanes[fl].len - self.s[j];
                let t_j = (d_j + cf.s_other) / self.v[j].max(2.0);
                if patient && t_j > 4.0 {
                    continue;
                }
                if !(t_me + t_gap < t_j) {
                    { REASON.with(|r| r.set(8)); BLOCKER.with(|b| b.set(j as i64)); return false; }
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
            // faixa seguinte curta terminando em nó sem semáforo: quem vai ter de ceder lá já decide daqui (senão só
            // "vê" o cruzamento a poucos metros da linha, sem distância para parar)
            let ahead = if front_decides && cc != SINK { Some((self.ahead_stop_at(net, i, cc, rest), 0.0)).filter(|a| a.0.is_finite()) } else { None };
            // dentro da conversão escolhida
            let to = self.conns[cc as usize - self.nl].to;
            let clen = self.edge_len[cc as usize];
            let other = if let Some(&j) = self.edge_veh[cc as usize].first() {
                let j = j as usize;
                Some((rest + self.s[j] - self.vlen[j], self.v[j]))
            } else if let Some(&j) = self.edge_veh[to as usize].first() {
                let j = j as usize;
                Some((rest + clen + self.s[j] - self.vlen[j], self.v[j]))
            } else {
                None
            };
            // faixa de destino curta que termina em semáforo fechado: a retenção dela também vale para quem ainda está antes
            let tl = &self.lanes[to as usize];
            if tl.len < 30.0 && !self.short_link(to as usize, i) {
                let st = self.end_state[tl.end_id as usize];
                if st == 1 || (st == 2 && self.v[i] * self.v[i] / (2.0 * self.bcomf[i]) < rest + clen + tl.len - 1.0) {
                    let stop = (rest + clen + tl.len - 0.5, 0.0);
                    return Some(match other {
                        Some(o) if o.0 < stop.0 => o,
                        _ => stop,
                    });
                }
            }
            match (other, ahead) {
                (Some(o), Some(a)) if a.0 < o.0 => Some(a),
                (None, a) => a,
                (o, _) => o,
            }
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
        self.route_step();
        lap!(0);
        self.spawn(net);
        lap!(1);

        // pedidos de passagem: quem está com seta ligada para uma troca obrigatória
        self.wants.clear();
        for i in 0..self.alive.len() {
            // seta ligada há muito sem nova tentativa (a cada tentativa ela é renovada) não é mais pedido
            if self.alive[i] && self.mand[i] && self.blink_to[i] != NONE && (self.edge[i] as usize) < self.nl && (self.time as f32 - self.blink_t[i]) < 10.0 {
                self.wants.push((self.blink_to[i], i as u32));
            }
        }
        self.wants.sort_unstable();
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
                let mut lead = self.leader(net, i, e, k, front);
                // dentro do cruzamento (ou chegando à linha já liberado): para antes de um ponto de conflito ocupado
                let occ = if e >= self.nl {
                    self.held_box[i] = false;
                    self.occ_obstacle(net, i, e as u32, self.s[i])
                } else if front && self.enter_ok[i] && self.choice[i] != NONE && self.choice[i] != SINK && self.edge_len[e] - self.s[i] < 40.0 {
                    let rest = self.edge_len[e] - self.s[i];
                    self.occ_obstacle(net, i, self.choice[i], -rest)
                } else {
                    None
                };
                if let Some(o) = occ {
                    self.held_box[i] = true;
                    if lead.map_or(true, |l| o.0 < l.0) {
                        lead = Some(o);
                    }
                }
                if e < self.nl && !self.wants.is_empty() {
                    if let Some(c) = self.courtesy(i, e, k) {
                        if lead.map_or(true, |l| c.0 < l.0) {
                            lead = Some(c);
                        }
                    }
                }
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
                let mut a = (a_max * (free - interact)).clamp(-9.0, a_max);
                // antecipa a curva: chega à linha já na velocidade da conversão escolhida (freada confortável),
                // em vez de entrar rápido e frear forte dentro do cruzamento
                if e < self.nl {
                    let cc = self.choice[i];
                    if cc != NONE && cc != SINK {
                        let vt = self.conns[cc as usize - self.nl].vturn;
                        if v > vt {
                            let rest = (self.edge_len[e] - self.s[i]).max(0.5);
                            let need = (v * v - vt * vt) / (2.0 * rest);
                            if need > 0.6 * b {
                                a = a.min(-need.min(9.0));
                            }
                        }
                    }
                }
                // troca obrigatória sem vão à frente na faixa de destino: ajusta a velocidade para entrar atrás de quem está lá
                if e < self.nl && self.mand[i] && self.lc_why[i] == 4 && self.blink_to[i] != NONE && self.edge_len[e] - self.s[i] < 120.0 {
                    let (ld, _) = self.around(self.blink_to[i] as usize, self.s[i], i);
                    if let Some(j) = ld {
                        let a2 = self.idm(i, Some((self.s[j] - self.vlen[j] - self.s[i], self.v[j])));
                        a = a.min(a2.max(-3.0));
                    }
                }
                if a < -HARSH && self.acc[i] >= -HARSH {
                    self.stats.harsh_brakes += 1;
                }
                self.acc[i] = a;
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
                let w0 = self.wait[i];
                self.wait[i] += dt;
                if self.held_box[i] {
                    self.stats.box_wait_s += dt as f64;
                }
                if self.edge[i] as usize >= self.nl {
                    if w0 < 2.0 && self.wait[i] >= 2.0 {
                        self.stats.box_stops += 1;
                    }
                    if self.held_box[i] && w0 <= OCC_DEADLOCK && self.wait[i] > OCC_DEADLOCK {
                        self.stats.occ_deadlocks += 1;
                    }
                }
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
        if self.overlap_log.is_some() {
            self.hist.resize(self.alive.len(), Default::default());
            for i in 0..self.alive.len() {
                let h = &mut self.hist[i];
                if !self.alive[i] {
                    h.clear();
                    continue;
                }
                if h.len() >= 30 {
                    h.pop_front();
                }
                h.push_back((self.edge[i], self.s[i], self.v[i], self.enter_ok[i]));
            }
        }
        if self.tick % 5 == 0 {
            self.check_overlaps(net);
        }
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

    /// Conta pares de veículos com as carrocerias sobrepostas dentro de um mesmo cruzamento (retângulos orientados,
    /// teste de eixos separados). Só entre conversões que não saem da mesma faixa nem chegam à mesma faixa (essas já
    /// se ordenam como líder/seguidor). Cada par conta uma vez enquanto continuar sobreposto.
    fn check_overlaps(&mut self, net: &Net) {
        let mut inside: Vec<(u32, u32)> = Vec::new();
        for i in 0..self.alive.len() {
            if self.alive[i] && self.edge[i] as usize >= self.nl {
                inside.push((self.conns[self.edge[i] as usize - self.nl].node, i as u32));
            }
        }
        inside.sort_unstable();
        // carroceria = 4 círculos ao longo da trajetória real (a traseira pode ainda estar na faixa de chegada)
        let discs = |i: usize| -> ([[f32; 2]; 4], f32) {
            let e = self.edge[i] as usize;
            let mut p = [[0.0f32; 2]; 4];
            let r = if self.kind[i] == 1 { 1.2 } else { 0.85 };
            let span = (self.vlen[i] - 2.0 * r).max(0.0);
            for k in 0..4 {
                p[k] = self.path_pt(net, e, self.s[i] - r - span * k as f32 / 3.0);
            }
            (p, r - 0.05)
        };
        let hit = |a: &([[f32; 2]; 4], f32), b: &([[f32; 2]; 4], f32)| {
            let rr = (a.1 + b.1) * (a.1 + b.1);
            a.0.iter().any(|p| b.0.iter().any(|q| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) < rr))
        };
        let mut now: std::collections::HashSet<(u32, u32)> = Default::default();
        let mut new_pairs = (0, 0);
        let mut logs: Vec<(usize, usize)> = Vec::new();
        let mut logs_kind: Vec<(usize, usize)> = Vec::new();
        let mut g = 0;
        while g < inside.len() {
            let mut h = g + 1;
            while h < inside.len() && inside[h].0 == inside[g].0 {
                h += 1;
            }
            for a in g..h {
                let ia = inside[a].1 as usize;
                let ca = &self.conns[self.edge[ia] as usize - self.nl];
                let ra = discs(ia);
                for b in a + 1..h {
                    let ib = inside[b].1 as usize;
                    let cb = &self.conns[self.edge[ib] as usize - self.nl];
                    if ca.from == cb.from || ca.to == cb.to {
                        continue;
                    }
                    if hit(&ra, &discs(ib)) {
                        let k = (ia.min(ib) as u32, ia.max(ib) as u32);
                        if !self.overlap_prev.contains(&k) {
                            logs_kind.push((ia, ib));
                            if let Some(log) = &self.overlap_log {
                                let _ = log;
                                logs.push((ia, ib));
                            }
                        }
                        now.insert(k);
                    }
                }
            }
            g = h;
        }
        for &(a, b) in &logs_kind {
            let cfs = self.conflicts_of(net, self.edge[a]);
            if cfs.iter().any(|c| c.other == self.edge[b] && !c.merge) {
                new_pairs.0 += 1;
            } else {
                new_pairs.1 += 1;
            }
        }
        self.stats.junction_overlaps += new_pairs.0;
        self.stats.junction_overlaps_geom += new_pairs.1;
        for (a, b) in logs {
            let (ea, eb) = (self.edge[a], self.edge[b]);
            let cfs = self.conflicts_of(net, ea);
            let cf = cfs.iter().find(|c| c.other == eb).copied();
            let (ca, cb) = (&self.conns[ea as usize - self.nl], &self.conns[eb as usize - self.nl]);
            let st = |c: &ConnRt| self.end_state[self.lanes[c.from as usize].end_id as usize];
            let line = format!(
                "node={} osm={} kinds={}/{} sig={}/{} s={:.1}/{:.1} len={:.1}/{:.1} v={:.1}/{:.1} wait={:.1}/{:.1} box={}/{} conflict={}",
                ca.node, net.nodes[ca.node as usize].osm_id, ca.kind, cb.kind, st(ca), st(cb), self.s[a], self.s[b], ca.len, cb.len,
                self.v[a], self.v[b], self.wait[a], self.wait[b], self.box_blocker[a], self.box_blocker[b],
                cf.map_or("nenhum".to_string(), |c| format!("s_me={:.1} s_o={:.1} w={:.1}", c.s_me, c.s_other, c.w))
            );
            let pa = self.has_priority(ca, cb, false);
            let pb = self.has_priority(cb, ca, false);
            let mut line = line + &format!(" prio(b sobre a)={pa} prio(a sobre b)={pb}");
            for (tag, i) in [("a", a), ("b", b)] {
                line += &format!("\n          {tag}:");
                for (k, &(e, s, v, ok)) in self.hist.get(i).into_iter().flatten().enumerate() {
                    if k % 3 != 0 {
                        continue;
                    }
                    let ed = if (e as usize) < self.nl {
                        format!("L{}({:.0}m)", e, self.lanes[e as usize].len)
                    } else {
                        let c = &self.conns[e as usize - self.nl];
                        format!("C{}[{}>{} n{}]", e, c.from, c.to, c.node)
                    };
                    line += &format!(" {ed} s={s:.1} v={v:.1}{}", if ok { "+" } else { "" });
                }
            }
            line += &format!("\n          a.conn from={} to={}  b.conn from={} to={}", ca.from, ca.to, cb.from, cb.to);
            self.overlap_log.as_mut().unwrap().push(line);
        }
        self.overlap_prev = now;
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
                        self.reroute_if_stuck(net, i);
                    }
                    return;
                }
                let end = self.lanes[e].end_id as usize;
                if self.end_state[end] == 1 && self.red_age[end] > 1.5 && !self.short_link(e, i) {
                    self.stats.red_runs += 1;
                    if self.red_log.len() < 40 { self.red_log.push((e as u32, self.v[i], self.choice[i], self.wait[i], self.red_age[end])); }
                }
                {
                    let to = self.conns[cc as usize - self.nl].to as usize;
                    let room = self.edge_veh[to].first().map_or(self.edge_len[to], |&j| self.s[j as usize] - self.vlen[j as usize]);
                    if room < self.vlen[i] + 1.0 && self.edge_veh[to].first().map_or(false, |&j| self.v[j as usize] < 1.0) {
                        self.stats.box_entries += 1;
                        if self.box_blocker[i] {
                            self.stats.box_entries_blockers += 1;
                        }
                    }
                }
                let over = self.s[i] - self.edge_len[e];
                let p = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
                self.edge_veh[e].remove(p);
                self.edge[i] = cc;
                self.s[i] = over;
                self.insert_sorted(cc, i);
                self.sample_conn_delay(cc as usize - self.nl, self.time as f32 - self.t_due[i]);
                if !self.left_gate[i] {
                    self.left_gate[i] = true;
                    let o = self.origin[i] as usize;
                    let st = &mut self.entry_stats[o];
                    st.0 += (self.t_spawn[i] - self.t_req[i]).max(0.0) as f64;
                    st.1 += (self.time as f32 - self.t_spawn[i]) as f64;
                    st.2 += 1;
                }
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
                self.t_due[i] = self.time as f32 + self.free_left(to as usize, over);
                self.mand[i] = false;
                self.blink_to[i] = NONE;
                self.multi.push(to);
                self.metrics.entered(self.lanes[to as usize].dirseg as usize);
                self.choose_conn(i);
            }
        }
    }

    /// Parado no fim da faixa sem conversão para a rota: aceita a saída que mais aproxima do destino (em linha reta,
    /// retorno só se não houver outra) e deixa o roteador refazer o caminho a partir dela.
    fn reroute_if_stuck(&mut self, net: &Net, i: usize) {
        if self.wait[i] <= 12.0 {
            return;
        }
        let e = self.edge[i] as usize;
        let goal = net.nodes[self.targets[self.tgt[i] as usize].node as usize].pos;
        let mut best: Option<(f64, u32)> = None;
        for &c in &self.lanes[e].conns {
            let cn = &self.conns[c as usize - self.nl];
            if self.kind[i] == 0 && self.lanes[cn.to as usize].bus {
                continue;
            }
            let p = net.nodes[self.lanes[cn.to as usize].to_node as usize].pos;
            let d = len_v(sub(p, goal)) + if cn.kind == K_UTURN { 1e6 } else { 0.0 };
            if best.map_or(true, |b| d < b.0) {
                best = Some((d, c));
            }
        }
        if let Some((_, c)) = best {
            self.choice[i] = c;
        }
    }

    /// Melhor conversão (custo, conversão) entre as faixas vizinhas da mesma via, ignorando se a faixa atual a permite.
    fn best_neighbor_conn(&self, i: usize, drv: Drv) -> Option<(f32, u32)> {
        let e = self.edge[i] as usize;
        let (lo, hi) = self.lanes_of(e);
        let mut best: Option<(f32, u32)> = None;
        for cl in lo..hi {
            if cl == e {
                continue;
            }
            let (c, conn) = self.stay_cost(drv, cl);
            if conn != NONE && conn != SINK && c.is_finite() && best.map_or(true, |b| c < b.0) {
                best = Some((c, conn));
            }
        }
        best
    }

    fn consider_lane_change(&mut self, i: usize) {
        let e = self.edge[i] as usize;
        let rest = self.lanes[e].len - self.s[i];
        // tempo de viagem anormal (voltas sem fim): vira "infrator" e, no limite, desiste
        let elapsed = self.time as f32 - self.t_spawn[i];
        let tf = self.t_free[i];
        if elapsed > (3.0 * tf).max(900.0) {
            self.rule_breaker[i] = true;
        }
        if elapsed > (6.0 * tf).max(1800.0) {
            self.stats.lost += 1;
            self.remove_vehicle(i);
            return;
        }
        let own = self.stay_cost(self.drv(i), e).0;
        // infração: parado no fim da faixa sem saída boa; quem não infringe só o faz depois de muito tempo preso
        if self.choice[i] != SINK && rest < 25.0 && self.wait[i] > 8.0 {
            if self.rule_breaker[i] || self.wait[i] > 45.0 {
                // compara em fluxo livre: a espera do próprio veículo (que entra no atraso medido da conversão dele)
                // não pode, sozinha, justificar a conversão proibida da faixa vizinha
                let free = Drv { app: false, ..self.drv(i) };
                let own_free = self.stay_cost(free, e).0;
                if let Some((cost, c)) = self.best_neighbor_conn(i, free) {
                    if !own_free.is_finite() || own_free - cost > 20.0 {
                        if self.choice[i] != c {
                            self.stats.illegal_moves += 1;
                        }
                        self.choice[i] = c;
                        self.cool[i] = 1.0;
                        // desiste da troca: sem isso a seta ficava ligada para sempre e quem está na faixa vizinha
                        // seguia cedendo a vez (cortesia) a quem nunca entra — e este, pela fila justa, cedia a ele
                        self.mand[i] = false;
                        self.blink_to[i] = NONE;
                        return;
                    }
                }
            }
            if !own.is_finite() && self.wait[i] > 90.0 {
                self.stats.lost += 1;
                self.remove_vehicle(i);
                return;
            }
        }
        // troca de faixa para a faixa de menor custo até o destino (conta o custo de trocar)
        if self.choice[i] != SINK {
            let (lo, hi) = self.lanes_of(e);
            let w = self.lane_w[e];
            let mut best = (own, e);
            for cl in lo..hi {
                if cl == e || !self.compatible(i, cl) {
                    continue;
                }
                let tot = self.stay_cost(self.drv(i), cl).0 + w * (cl as i32 - e as i32).unsigned_abs() as f32 * self.lc_mul(cl);
                // perto do nó só se muda de plano por um ganho grande (a árvore muda com o trânsito)
                if tot + if rest < 50.0 && own.is_finite() { 30.0 } else { 2.0 } < best.0 {
                    best = (tot, cl);
                }
            }
            self.mand[i] = best.1 != e;
            if best.1 != e {
                let target = if best.1 > e { e + 1 } else { e - 1 };
                let forced = rest < 40.0 || self.wait[i] > 6.0;
                self.cool[i] = if self.try_change(i, target, forced) { 1.0 } else { 0.4 };
                return;
            }
        }
        // discricionária (MOBIL): troca se o ganho próprio, descontado o incômodo causado aos outros, supera o limiar;
        // à direita é mais fácil e à esquerda mais difícil (CTB art. 29: a direita é dos mais lentos, ultrapassa-se pela esquerda)
        if self.choice[i] == SINK || rest < 60.0 {
            self.cool[i] = 1.5;
            return;
        }
        let (lo, hi) = self.lanes_of(e);
        let (my_lead, my_foll) = self.around(e, self.s[i], i);
        let a_cur = self.idm(i, self.lead_gap(i, my_lead));
        // quem vem atrás de mim: hoje me segue; depois seguiria o meu líder
        let d_old = my_foll.map_or(0.0, |f| {
            let now = self.idm(f, Some((self.s[i] - self.vlen[i] - self.s[f], self.v[i])));
            let after = self.idm(f, my_lead.map(|j| (self.s[j] - self.vlen[j] - self.s[f], self.v[j])));
            after - now
        });
        let mut pick: Option<(f32, usize)> = None;
        for cl in [e as i32 - 1, e as i32 + 1] {
            if cl < lo as i32 || cl >= hi as i32 {
                continue;
            }
            let cl = cl as usize;
            if !self.compatible(i, cl) {
                continue;
            }
            // não troca para uma faixa que piora a rota
            let rc = self.stay_cost(self.drv(i), cl).0;
            if !rc.is_finite() || (own.is_finite() && rc > own + 5.0) {
                continue;
            }
            let (ld, fl) = self.around(cl, self.s[i], i);
            let a_new = self.idm(i, self.lead_gap(i, ld));
            // novo seguidor: segurança e incômodo
            let d_new = match fl {
                Some(f) => {
                    let gap_f = self.s[i] - self.vlen[i] - self.s[f];
                    let with_me = self.idm(f, Some((gap_f, self.v[i])));
                    if gap_f < 1.0 || with_me < -MOBIL_SAFE {
                        continue;
                    }
                    with_me - self.idm(f, ld.map(|j| (self.s[j] - self.vlen[j] - self.s[f], self.v[j])))
                }
                None => 0.0,
            };
            let bias = if cl > e { -MOBIL_BIAS } else { MOBIL_BIAS };
            let gain = a_new - a_cur + MOBIL_P * (d_new + d_old);
            if gain > MOBIL_TH + bias && pick.map_or(true, |p| gain > p.0) {
                pick = Some((gain, cl));
            }
        }
        if let Some((_, cl)) = pick {
            if self.try_change(i, cl, false) {
                self.cool[i] = 4.0;
                return;
            }
            self.cool[i] = 1.0;
            return;
        }
        self.cool[i] = 2.0;
    }

    /// Aceleração IDM do veículo `i` com o líder `lead` (folga, velocidade); sem líder = só o termo livre.
    fn idm(&self, i: usize, lead: Option<(f32, f32)>) -> f32 {
        let v = self.v[i];
        let v0 = self.desired_speed(i).max(0.5);
        let (a_max, b, t) = (self.amax[i], self.bcomf[i], self.tgap[i]);
        let mut x = 1.0 - (v / v0).powi(4);
        if let Some((gap, vl)) = lead {
            let s_star = 2.0 + (v * t + v * (v - vl) / (2.0 * (a_max * b).sqrt())).max(0.0);
            x -= (s_star / gap.max(0.1)).powi(2);
        }
        (a_max * x).clamp(-9.0, a_max)
    }

    /// Vizinhos na faixa `l` em torno da posição `s` (ignora `skip`): (líder, seguidor).
    fn around(&self, l: usize, s: f32, skip: usize) -> (Option<usize>, Option<usize>) {
        let list = &self.edge_veh[l];
        let p = list.partition_point(|&j| self.s[j as usize] < s);
        let lead = list[p..].iter().map(|&j| j as usize).find(|&j| j != skip);
        let foll = list[..p].iter().rev().map(|&j| j as usize).find(|&j| j != skip);
        (lead, foll)
    }

    /// Folga até o líder de `i` (ou None) na faixa onde ele está.
    fn lead_gap(&self, i: usize, lead: Option<usize>) -> Option<(f32, f32)> {
        lead.map(|j| (self.s[j] - self.vlen[j] - self.s[i], self.v[j]))
    }

    /// Cortesia: o veículo `i` (posição `k` na faixa `e`) é o primeiro atrás de alguém que pede para entrar na faixa
    /// dele (seta, troca obrigatória) e, se for cortês, passa a tratá-lo como líder, abrindo o vão.
    fn courtesy(&self, i: usize, e: usize, k: usize) -> Option<(f32, f32)> {
        let lo = self.wants.partition_point(|w| (w.0 as usize) < e);
        let list = &self.edge_veh[e];
        let mut best: Option<(f32, f32)> = None;
        for &(lane, r) in &self.wants[lo..] {
            if lane as usize != e {
                break;
            }
            let r = r as usize;
            let ds = self.s[r] - self.s[i];
            if ds <= 0.0 || ds > 40.0 {
                continue;
            }
            // só quem está logo atrás do ponto de entrada cede
            if k + 1 < list.len() && self.s[list[k + 1] as usize] < self.s[r] {
                continue;
            }
            if noise(self.pseed[i], r as u32) < -1.0 + 2.0 * (1.0 - COURTESY) {
                continue;
            }
            let gap = self.s[r] - self.vlen[r] - self.s[i];
            if gap > 1.0 && best.map_or(true, |b| gap < b.0) {
                best = Some((gap, self.v[r]));
            }
        }
        best
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

    fn try_change(&mut self, i: usize, to: usize, forced: bool) -> bool {
        let e = self.edge[i] as usize;
        if !self.compatible(i, to) {
            self.lc_why[i] = 1;
            return false;
        }
        // linha contínua de aproximação (retenção/zebrado): não se troca de faixa nos últimos metros antes do nó
        // quem está parado há tempo na faixa errada (fila na retenção) acaba infringindo, como no restante da simulação
        if self.lanes[e].len - self.s[i] < LC_SOLID_ZONE {
            if !(forced && self.wait[i] > 6.0) {
                self.lc_why[i] = 2;
                return false;
            }
            self.stats.solid_crossings += 1;
        }
        // seta: sinaliza antes de mudar; outra intenção recente não é trocada no meio do caminho
        let now = self.time as f32;
        if self.blink_to[i] != to as u32 || now - self.blink_t[i] > 8.0 {
            if self.blink_to[i] != NONE && self.blink_to[i] != to as u32 && now - self.blink_t[i] < 3.0 {
                self.lc_why[i] = 3;
                return false;
            }
            self.blink_to[i] = to as u32;
            self.blink_t[i] = now;
        }
        if now - self.blink_t[i] < if forced { LC_BLINK / 2.0 } else { LC_BLINK } {
            self.lc_why[i] = 3;
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
                self.lc_why[i] = 4;
                return false;
            }
        }
        if p > 0 {
            let f = list[p - 1] as usize;
            let gap = s - self.vlen[i] - self.s[f];
            // preferência de quem já está na faixa de destino: o que vem atrás não pode precisar frear forte
            let need = (3.0 + self.v[f] * 0.9 + 1.5 * (self.v[f] - self.v[i]).max(0.0)) * margin;
            if gap < need {
                self.lc_why[i] = 5;
                return false;
            }
        }
        // executa
        let old_off = self.lanes[e].off;
        let new_off = self.lanes[to].off;
        let pos = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
        self.edge_veh[e].remove(pos);
        self.blink_to[i] = NONE;
        self.mand[i] = false;
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
            while let Some(&(origin, dest, kind, t_req)) = self.pending[g].front() {
                if self.spawn_vehicle(g as u32, origin as u16, dest, kind, t_req) {
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
                    // só nasce onde existe caminho até o destino (sorteia algumas entradas)
                    let mut gate = self.pick_gate(i);
                    for _ in 0..6 {
                        if self.zone_reaches(j, gate as usize) && self.lanes[gate as usize].len >= 15.0 {
                            break;
                        }
                        gate = self.pick_gate(i);
                    }
                    if !self.zone_reaches(j, gate as usize) || self.lanes[gate as usize].len < 15.0 {
                        self.stats.skipped += 1;
                        self.next_spawn[i][j] += self.rng.exp(rate);
                        continue;
                    }
                    let kind = if (self.rng.f() as f64) < self.params.bus_share / 100.0 { 1 } else { 0 };
                    if !self.spawn_vehicle(gate, i as u16, j as u16, kind, self.time as f32) {
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
    /// Posição da frente do veículo e direção unitária (dx, dy).
    /// Ponto da trajetória na aresta `e` a `s` m do começo; negativo numa conversão = ainda na faixa de chegada.
    fn path_pt(&self, net: &Net, e: usize, s: f32) -> [f32; 2] {
        if e >= self.nl {
            let c = &self.conns[e - self.nl];
            if s < 0.0 {
                let l = c.from as usize;
                return self.path_pt(net, l, self.lanes[l].len + s);
            }
            let p = bez8(&c.curve, (s / c.len).clamp(0.0, 1.0));
            return [p[0] as f32, p[1] as f32];
        }
        let l = &self.lanes[e];
        let s = s.clamp(0.0, l.len);
        let a = if l.dir == 0 { l.a0 as f64 + s as f64 } else { l.a0 as f64 - s as f64 };
        let (p, t) = net.segs[l.seg as usize].poly.at(a);
        let q = add(p, mul(right(t), l.off as f64));
        [q[0] as f32, q[1] as f32]
    }

    fn pose_dir(&self, net: &Net, i: usize) -> (f32, f32, f32, f32) {
        let e = self.edge[i] as usize;
        if e < self.nl {
            let l = &self.lanes[e];
            let seg = &net.segs[l.seg as usize];
            let s = self.s[i].clamp(0.0, l.len);
            let a = if l.dir == 0 { l.a0 as f64 + s as f64 } else { l.a0 as f64 - s as f64 };
            let (p, t) = seg.poly.at(a);
            let sign = if l.dir == 0 { 1.0 } else { -1.0 };
            let q = add(p, mul(right(t), (l.off + self.lat[i]) as f64));
            (q[0] as f32, q[1] as f32, (t[0] * sign) as f32, (t[1] * sign) as f32)
        } else {
            let c = &self.conns[e - self.nl];
            let u = (self.s[i] / c.len).clamp(0.0, 1.0);
            let p = bez8(&c.curve, u);
            let q = bez8(&c.curve, (u + 0.02).min(1.0));
            let p0 = bez8(&c.curve, (u - 0.02).max(0.0));
            let d = norm(sub(q, p0));
            (p[0] as f32, p[1] as f32, d[0] as f32, d[1] as f32)
        }
    }

    fn pose(&self, net: &Net, i: usize) -> (f32, f32, f32) {
        let (x, y, dx, dy) = self.pose_dir(net, i);
        (x, y, dy.atan2(dx))
    }

    /// Estado dos veículos indexado pelo slot (o id do veículo é a posição): 6 floats por slot,
    /// [x, y, dx, dy, velocidade, comprimento] da frente do veículo, com comprimento negativo para ônibus.
    /// Slot livre = tudo zero (comprimento 0).
    pub fn snapshot(&self, net: &Net, out: &mut Vec<f32>) {
        out.clear();
        out.resize(self.alive.len() * SNAP_STRIDE, 0.0);
        for i in 0..self.alive.len() {
            if !self.alive[i] {
                continue;
            }
            let (x, y, dx, dy) = self.pose_dir(net, i);
            let len = if self.kind[i] == 1 { -self.vlen[i] } else { self.vlen[i] };
            out[i * SNAP_STRIDE..(i + 1) * SNAP_STRIDE].copy_from_slice(&[x, y, dx, dy, self.v[i], len]);
        }
    }

    /// Rota restante do veículo `i` até a zona de destino: (pontos xy planos, id da zona, tipo, velocidade, espera).
    pub fn track(&self, net: &Net, i: usize) -> Option<(Vec<f32>, i64, u8, f32, f32)> {
        if i >= self.alive.len() || !self.alive[i] {
            return None;
        }
        let dest = self.tgt[i] as usize;
        let zone_dest = self.dest[i] as usize;
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
        // faixa onde o traçado continua depois da parte atual
        let mut cur: Option<usize> = None;
        if e < self.nl {
            let l = &self.lanes[e];
            pts.extend(lane_pts(l, self.s[i]));
            let mut cc = self.choice[i];
            if cc == NONE {
                // sem conversão escolhida: mostra a da melhor faixa vizinha
                let (lo, hi) = self.lanes_of(e);
                let mut best = (f32::INFINITY, NONE);
                for cl in lo..hi {
                    let r = self.stay_cost(self.drv(i), cl);
                    if r.0 < best.0 {
                        best = r;
                    }
                }
                cc = best.1;
            }
            if cc != NONE && cc != SINK {
                let c = &self.conns[cc as usize - self.nl];
                pts.extend(curve_pts(c, 0.0));
                cur = Some(c.to as usize);
            }
        } else {
            let c = &self.conns[e - self.nl];
            pts.extend(curve_pts(c, (self.s[i] / c.len).clamp(0.0, 1.0)));
            cur = Some(c.to as usize);
        }
        let mut guard = 0;
        while let Some(l) = cur {
            guard += 1;
            if guard > 600 {
                break;
            }
            pts.extend(lane_pts(&self.lanes[l], 0.0));
            if self.lanes[l].to_node == self.targets[dest].node {
                break;
            }
            // na faixa de menor custo (considerando o custo de trocar) segue-se a melhor conversão
            let (lo, hi) = self.lanes_of(l);
            let w = self.lane_w[l];
            let mut best = (f32::INFINITY, NONE);
            for cl in lo..hi {
                let r = self.stay_cost(self.drv(i), cl);
                let tot = r.0 + w * (cl as i32 - l as i32).unsigned_abs() as f32;
                if tot < best.0 {
                    best = (tot, r.1);
                }
            }
            if best.1 == NONE || best.1 == SINK {
                break;
            }
            let c = &self.conns[best.1 as usize - self.nl];
            pts.extend(curve_pts(c, 0.0));
            cur = Some(c.to as usize);
        }
        let flat: Vec<f32> = pts.iter().flat_map(|p| [p[0] as f32, p[1] as f32]).collect();
        Some((flat, self.zone_ids[zone_dest], self.kind[i], self.v[i], self.wait[i]))
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
            let lane_ref = if e < self.nl { e } else { self.conns[e - self.nl].from as usize };
            let remaining = self.dist(self.tgt[i] as usize, lane_ref, self.app[i]);
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
        r["boxEntries"] = serde_json::json!(self.stats.box_entries);
        r["boxWaitVehH"] = serde_json::json!(self.stats.box_wait_s / 3600.0);
        r["junctionOverlaps"] = serde_json::json!(self.stats.junction_overlaps + self.stats.junction_overlaps_geom);
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

    /// Texto curto do motivo do último bloqueio em `may_enter` (para depuração).
    pub fn reason_text(code: u32) -> &'static str {
        match code {
            1 => "sem conversão para a rota",
            2 => "semáforo vermelho",
            3 => "semáforo amarelo (consegue parar)",
            4 => "saída ocupada (sem 6–7 m livres no começo da faixa de destino)",
            5 => "conversão ocupada (veículo ainda na curva)",
            6 => "convergência: outro veículo chega antes à mesma faixa",
            7 => "veículo dentro do cruzamento cruza o caminho",
            8 => "cede a quem tem preferência (brecha insuficiente)",
            9 => "fila justa: outro veículo espera há mais tempo pela mesma saída",
            10 => "placa Pare: ainda não parou na linha",
            11 => "cruzamento ocupado: veículo parado ou passando no ponto de conflito",
            _ => "",
        }
    }

    /// Estado detalhado de um veículo (JSON) para depuração.
    pub fn explain(&mut self, net: &Net, i: usize) -> serde_json::Value {
        use serde_json::json;
        if i >= self.alive.len() || !self.alive[i] {
            return serde_json::Value::Null;
        }
        let e = self.edge[i] as usize;
        let on_lane = e < self.nl;
        let kind_name = |k: u8| match k {
            K_LEFT => "esquerda",
            K_STRAIGHT => "em frente",
            K_RIGHT => "direita",
            _ => "retorno",
        };
        let (seg_way, seg_name, lane_idx, lane_len, lane_to, end_state, red_age, lane_rank) = if on_lane {
            let l = &self.lanes[e];
            let sg = &net.segs[l.seg as usize];
            let nm = net.names.get(sg.name as usize).cloned().unwrap_or_default();
            (sg.way_id, nm, l.idx as i64, l.len, l.to_node, self.end_state[l.end_id as usize], self.red_age[l.end_id as usize], l.rank)
        } else {
            let c = &self.conns[e - self.nl];
            let l = &self.lanes[c.from as usize];
            let sg = &net.segs[l.seg as usize];
            let nm = net.names.get(sg.name as usize).cloned().unwrap_or_default();
            (sg.way_id, nm, -1, c.len, c.node, 0, 0.0, c.rank)
        };
        // líder
        let list = self.edge_veh[e].clone();
        let k = list.iter().position(|&x| x as usize == i).unwrap_or(0);
        let front = k + 1 == list.len();
        let leader = if !front {
            let j = list[k + 1] as usize;
            Some(json!({ "id": j, "gap": self.s[j] - self.vlen[j] - self.s[i], "v": self.v[j], "wait": self.wait[j] }))
        } else {
            None
        };
        let rest = self.edge_len[e] - self.s[i];
        // decisão de entrada no cruzamento
        let cc = self.choice[i];
        let mut enter = serde_json::Value::Null;
        if on_lane && front && cc != NONE && cc != SINK {
            let ok = self.may_enter(net, i, cc);
            let code = REASON.with(|r| r.get());
            let blocker = BLOCKER.with(|b| b.get());
            enter = json!({ "ok": ok, "code": code, "reason": Self::reason_text(code), "blocker": if ok { -1 } else { blocker } });
        }
        let choice = if cc == NONE {
            json!("nenhuma")
        } else if cc == SINK {
            json!("destino (sai da rede)")
        } else {
            let c = &self.conns[cc as usize - self.nl];
            json!({ "conn": cc, "kind": kind_name(c.kind), "len": c.len, "pen": c.pen, "livePen": self.live_pen[cc as usize - self.nl], "measuredDelay": self.conn_delay[cc as usize - self.nl], "toLane": c.to, "toIdx": self.lanes[c.to as usize].idx })
        };
        // custo até o destino da faixa atual e das vizinhas
        let tgt = self.tgt[i] as usize;
        let own = if on_lane { self.stay_cost(self.drv(i), e).0 } else { f32::INFINITY };
        let mut lanes_cost = Vec::new();
        if on_lane {
            let (lo, hi) = self.lanes_of(e);
            for cl in lo..hi {
                let (c, conn) = self.stay_cost(self.drv(i), cl);
                lanes_cost.push(json!({ "idx": self.lanes[cl].idx, "here": cl == e, "cost": if c.is_finite() { json!(c) } else { json!(null) }, "conn": if conn == NONE || conn == SINK { json!(null) } else { json!(kind_name(self.conns[conn as usize - self.nl].kind)) } }));
            }
        }
        let elapsed = self.time as f32 - self.t_spawn[i];
        let remaining = {
            let lane_ref = if on_lane { e } else { self.conns[e - self.nl].from as usize };
            self.dist(tgt, lane_ref, self.app[i])
        };
        json!({
            "id": i,
            "kind": if self.kind[i] == 1 { "ônibus" } else { "carro" },
            "origin": self.zone_ids[self.origin[i] as usize],
            "dest": self.zone_ids[self.dest[i] as usize],
            "targetNode": net.nodes[self.targets[tgt].node as usize].osm_id,
            "where": if on_lane { "faixa" } else { "dentro do cruzamento" },
            "way": seg_way,
            "street": seg_name,
            "laneIdx": lane_idx,
            "s": self.s[i],
            "len": lane_len,
            "rest": rest,
            "front": front,
            "v": self.v[i],
            "vDesired": self.desired_speed(i),
            "acc": self.acc[i],
            "wait": self.wait[i],
            "len_m": self.vlen[i],
            "leader": leader,
            "choice": choice,
            "enter": enter,
            "signal": match end_state { 0 => "sem semáforo", 1 => "vermelho", 2 => "amarelo", _ => "verde" },
            "redAge": red_age,
            "rank": lane_rank,
            "toNode": net.nodes[lane_to as usize].osm_id,
            "laneChangeCooldown": self.cool[i].max(0.0),
            "laneChangeBlock": lc_block_text(self.lc_why[i]),
            "costOwn": if own.is_finite() { json!(own) } else { json!(null) },
            "lanes": lanes_cost,
            "ruleBreaker": self.rule_breaker[i],
            "boxBlocker": self.box_blocker[i],
            "heldByBox": self.held_box[i],
            "elapsed": elapsed,
            "freeTime": self.t_free[i],
            "remainingFree": if remaining.is_finite() { json!(remaining) } else { json!(null) },
            "delay": (elapsed - (self.t_free[i] - if remaining.is_finite() { remaining } else { 0.0 })).max(0.0),
        })
    }

    /// Veículos na frente de faixa parados há mais tempo (para achar gargalos): [{id, wait, reason, blocker, street}].
    /// Depuração: estado de troca de faixa do veículo `i`.
    pub fn lc_state(&self, i: usize) -> String {
        let e = self.edge[i] as usize;
        let costs: Vec<String> = if e < self.nl { let (lo, hi) = self.lanes_of(e); (lo..hi).map(|cl| format!("{cl}:{:.0}", self.stay_cost(self.drv(i), cl).0)).collect() } else { vec![] };
        format!("#{i} edge {e} s {:.1} v {:.1} wait {:.0} lc_why {} blink_to {} blink_age {:.1} cool {:.1} mand {} choice {} rb {} custos {costs:?}", self.s[i], self.v[i], self.wait[i], self.lc_why[i], self.blink_to[i] as i64, self.time as f32 - self.blink_t[i], self.cool[i], self.mand[i], self.choice[i], self.rule_breaker[i])
    }

    /// Depuração: veículos parados há mais de `min_wait` s cujo líder efetivo é a cortesia a quem pede para entrar.
    pub fn courtesy_stuck(&mut self, min_wait: f32) -> Vec<String> {
        let mut out = Vec::new();
        for e in 0..self.nl {
            let list = self.edge_veh[e].clone();
            for (k, &i) in list.iter().enumerate() {
                let i = i as usize;
                if self.wait[i] < min_wait {
                    continue;
                }
                if let Some(c) = self.courtesy(i, e, k) {
                    let own = if k + 1 < list.len() { let j = list[k + 1] as usize; self.s[j] - self.vlen[j] - self.s[i] } else { self.edge_len[e] - self.s[i] };
                    if c.0 < own {
                        let r = self.wants.iter().find(|w| w.0 as usize == e && (self.s[w.1 as usize] - self.vlen[w.1 as usize] - self.s[i] - c.0).abs() < 1e-3).map(|w| w.1 as usize);
                        let rinfo = r.map(|r| format!("#{r} faixa {} s {:.1} resto {:.1} wait {:.0} why {} mand {} lc_why {}", self.edge[r], self.s[r], self.edge_len[self.edge[r] as usize] - self.s[r], self.wait[r], lc_block_text(self.lc_why[r]), self.mand[r], self.lc_why[r]));
                        out.push(format!("#{i} faixa {e} s {:.1} resto {:.1} wait {:.0} pos {k}/{} cortesia gap {:.2} (próprio {:.1}) -> {rinfo:?}", self.s[i], self.edge_len[e] - self.s[i], self.wait[i], list.len(), c.0, own));
                    }
                }
            }
        }
        out
    }

    /// Depuração: o que segura o veículo `i` agora (texto, id de quem segura ou -1).
    pub fn who_holds(&mut self, net: &Net, i: usize) -> (String, i64) {
        let e = self.edge[i] as usize;
        let list = self.edge_veh[e].clone();
        let k = list.iter().position(|&x| x as usize == i).unwrap_or(0);
        let where_ = if e < self.nl { format!("faixa {} resto {:.1}", e, self.edge_len[e] - self.s[i]) } else { format!("conv {} s {:.1}/{:.1} node {}", e, self.s[i], self.edge_len[e], self.conns[e - self.nl].node) };
        let base = format!("#{i} {where_} v {:.1} wait {:.0} bb {}", self.v[i], self.wait[i], self.box_blocker[i]);
        if k + 1 < list.len() {
            let j = list[k + 1] as usize;
            return (format!("{base} -> líder gap {:.1}", self.s[j] - self.vlen[j] - self.s[i]), j as i64);
        }
        if e < self.nl {
            let cc = self.choice[i];
            if cc == NONE || cc == SINK {
                return (format!("{base} -> sem conversão"), -1);
            }
            let ok = self.may_enter(net, i, cc);
            let code = REASON.with(|r| r.get());
            let b = BLOCKER.with(|b| b.get());
            if ok {
                // pode entrar mas está parado: o que faz de líder virtual
                let k = self.edge_veh[e].iter().position(|&x| x as usize == i).unwrap();
                let rest = self.edge_len[e] - self.s[i];
                let ahead = self.ahead_stop_at(net, i, cc, rest);
                let occ = if rest < 40.0 { self.occ_obstacle(net, i, cc, -rest) } else { None };
                let ct = self.courtesy(i, e, k);
                let wants: Vec<(u32, u32, f32)> = self.wants.iter().filter(|w| w.0 as usize == e).map(|w| (w.1, self.edge[w.1 as usize], self.s[w.1 as usize] - self.s[i])).collect();
                let lead = self.leader(net, i, e, k, true);
                return (format!("{base} -> entra? sim conv {cc} rest {rest:.1} acc {:.2} | leader {lead:?} ahead {ahead:.1} occ {occ:?} cortesia {ct:?} wants {wants:?}", self.acc[i]), -1);
            }
            return (format!("{base} -> entra? {ok} motivo {code} conv {cc}"), b);
        }
        // na conversão: frente da faixa de saída, ou ponto de conflito ocupado
        let to = self.conns[e - self.nl].to as usize;
        if let Some(&j) = self.edge_veh[to].first() {
            let j = j as usize;
            let gap = self.edge_len[e] - self.s[i] + self.s[j] - self.vlen[j];
            if gap < 8.0 {
                return (format!("{base} -> saída gap {gap:.1}"), j as i64);
            }
        }
        let cfs = self.conflicts_of(net, e as u32);
        for cf in cfs.iter() {
            if cf.merge || self.s[i] > cf.s_me - cf.w {
                continue;
            }
            if let Some(j) = self.occ_by(cf, 0.0, i) {
                return (format!("{base} -> ocupação (conflito conv {})", cf.other), j as i64);
            }
        }
        (format!("{base} -> ?"), -1)
    }

    pub fn stuck_list(&mut self, net: &Net, n: usize) -> Vec<serde_json::Value> {
        use serde_json::json;
        let mut cand: Vec<(f32, usize, usize)> = Vec::new();
        for e in 0..self.nl {
            if let Some(&j) = self.edge_veh[e].last() {
                let j = j as usize;
                if self.wait[j] >= 5.0 {
                    cand.push((self.wait[j], j, e));
                }
            }
        }
        cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        cand.truncate(n);
        let mut out = Vec::new();
        for (w, j, e) in cand {
            let cc = self.choice[j];
            let (ok, code, blocker) = if cc == NONE || cc == SINK {
                (false, if cc == NONE { 1 } else { 0 }, -1)
            } else {
                let ok = self.may_enter(net, j, cc);
                (ok, REASON.with(|r| r.get()), BLOCKER.with(|b| b.get()))
            };
            let sg = &net.segs[self.lanes[e].seg as usize];
            out.push(json!({
                "id": j, "wait": w, "ok": ok, "reason": Self::reason_text(code), "code": code, "blocker": blocker,
                "street": net.names.get(sg.name as usize).cloned().unwrap_or_default(),
                "signal": self.end_state[self.lanes[e].end_id as usize],
            }));
        }
        out
    }

    pub fn lane_info(&self, e: usize) -> String {
        let l = &self.lanes[e];
        format!("lane {} seg {} dir {} idx {} len {:.1} to_node {} end_id {} end_state {} conns {:?}", e, l.seg, l.dir, l.idx, l.len, l.to_node, l.end_id, self.end_state[l.end_id as usize], l.conns)
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
