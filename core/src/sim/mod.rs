//! Simulação microscópica de tráfego (carros e ônibus) sobre a rede.
//!
//! Elementos de via ("arestas"): primeiro todas as faixas, depois cada conversão ligada (a trajetória
//! dentro do cruzamento). Cada veículo está em exatamente uma aresta, ordenado por posição da frente
//! (`s`). Longitudinal: IDM. Lateral: troca de faixa obrigatória (rota) e discricionária (simplificada).
//! Cruzamentos: semáforo (verde/amarelo/vermelho, esquerda permissiva) e preferência (via principal,
//! rotatória, direita) por aceitação de brecha, mais *keep-clear*. Tudo determinístico por semente.

pub mod bus;
pub mod contra;
pub mod corridor;
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
/// rotas em tempo real: um lote de árvores pedido no passo k é instalado exatamente no passo k + `ROUTE_LAG`
/// (3 s), calculado aqui ou em outro worker; o resultado não depende de quem calculou nem da velocidade da máquina
const ROUTE_LAG: u64 = 15;
/// ritmo do recálculo em nós de Dijkstra por passo (o mesmo de quando a árvore era feita em fatias no passo):
/// nz·faixas/900 (volta em ~3 min), entre estes limites; vira tantas árvores por lote ou um lote a cada tantos passos
const ROUTE_BUDGET_MIN: usize = 2_000;
const ROUTE_BUDGET_MAX: usize = 6_000;
const ROUTE_BATCH_MAX: usize = 8;
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
    /// com linhas de ônibus carregadas: os ônibus seguem as linhas (e `bus_share` não vale); desligado, volta ao sorteio
    pub bus_lines: bool,
    /// tempo de parada no ponto (s): mínimo (portas, seta) e máximo; entre eles, 2,5 s por passageiro embarcando
    pub bus_dwell_min: f64,
    pub bus_dwell_max: f64,
    /// passageiros embarcando por parada na hora de pico (média; a hora segue o perfil da OD)
    pub bus_pax_peak: f64,
    /// variação (± s) do horário de partida de cada viagem
    pub bus_jitter: f64,
    /// % de motoristas que usam a faixa exclusiva de ônibus
    pub bus_lane_car_pct: f64,
    /// % de motoristas que param no ponto de ônibus (uma vez por viagem, no primeiro ponto da faixa)
    pub bus_stop_car_pct: f64,
    /// % do fluxo gerado (modelo gravitacional ou `od_hourly`) que nasce como moto; não vale com `od_hourly_moto`
    pub moto_share: f64,
    /// Matriz de motos fixada por hora do dia (24 entradas, veíc./h por "idOrigem>idDestino"), somada aos carros
    /// (importação da Pesquisa OD: motocicleta + mototáxi). Vazia = sem matriz de motos.
    pub od_hourly_moto: Vec<std::collections::HashMap<String, f64>>,
    /// motos andam no corredor entre duas faixas do mesmo sentido quando o trânsito está lento (fase 8)
    pub moto_corridor: bool,
    /// % dos motociclistas que avançam o vermelho com o cruzamento livre (parados na linha, com brecha folgada)
    pub moto_red_pct: f64,
    /// % dos motociclistas que usam a faixa exclusiva de ônibus
    pub moto_bus_lane_pct: f64,
    /// % dos motociclistas que andam na contramão em trechos curtos de mão única para cortar caminho
    pub moto_contra_pct: f64,
    /// % dos motociclistas que usam o corredor também com o trânsito andando (limites acima dos da fase 8)
    pub moto_corr_fast_pct: f64,
}
impl Default for SimParams {
    fn default() -> Self {
        SimParams { seed: 1, hour: 8.0, bus_share: 2.0, beta: 0.08, intrazone: false, illegal_pct: 0.0, od_manual: Default::default(), od_hourly: Vec::new(), reroute: true, app_pct: 60.0, perception: 0.3, box_block_pct: 0.0, bus_lines: true, bus_dwell_min: 8.0, bus_dwell_max: 90.0, bus_pax_peak: 6.0, bus_jitter: 60.0, bus_lane_car_pct: 0.0, bus_stop_car_pct: 0.0, moto_share: 0.0, od_hourly_moto: Vec::new(), moto_corridor: true, moto_red_pct: 0.0, moto_bus_lane_pct: 0.0, moto_contra_pct: 0.0, moto_corr_fast_pct: 0.0 }
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
    /// linhas de ônibus encaixadas na rede (vazio = sem linhas)
    #[serde(skip)]
    pub bus: Vec<bus::BusRoute>,
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
    /// segue o tempo real (árvores recalculadas) ou o de fluxo livre
    app: bool,
    seed: u32,
    /// pode usar faixa exclusiva de ônibus (ônibus, ou carro da parcela `bus_lane_car_pct`)
    bus_ok: bool,
    /// moto da parcela que anda na contramão (pode escolher as conversões `moto_only`)
    contra: bool,
    /// ônibus de linha: plano (`NONE` = carro ou ônibus sem linha), passo atual e passo/faixa da próxima parada
    route: u32,
    step: u32,
    stop_step: u32,
    stop_lane: u32,
}

/// Matriz horária (`od_hourly`/`od_hourly_moto`, 24 entradas "idOrigem>idDestino" → veículos/h) já ligada aos
/// índices das zonas, para interpolar sem `String` nem `HashMap` a cada recálculo (com 253 zonas eram milhares de
/// chaves clonadas e 64 mil `format!` num único passo). Mesma interpolação linear entre horas inteiras do perfil das
/// zonas (`demand::profile_at`); uma célula vale na hora se aparece em alguma das duas horas vizinhas.
#[derive(Default)]
struct OdTable {
    /// (origem, destino, valor por hora, horas em que a chave aparece: bit k = hora k)
    cells: Vec<(u32, u32, [f64; 24], u32)>,
}

impl OdTable {
    /// `canonical`: só chaves escritas exatamente como `"{id}>{id}"` (o que `demand::forced_cells` procura);
    /// senão aceita espaços em volta dos números (como lia a matriz de motos).
    fn build(h: &[std::collections::HashMap<String, f64>], idx: &std::collections::HashMap<i64, usize>, canonical: bool) -> OdTable {
        let mut at: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut cells: Vec<(u32, u32, [f64; 24], u32)> = Vec::new();
        for (hr, m) in h.iter().enumerate().take(24) {
            for (k, &v) in m {
                let pos = match at.get(k.as_str()) {
                    Some(&p) => p,
                    None => {
                        let Some((a, b)) = k.split_once('>') else { continue };
                        let (a, b) = if canonical { (a, b) } else { (a.trim(), b.trim()) };
                        let (Ok(a), Ok(b)) = (a.parse::<i64>(), b.parse::<i64>()) else { continue };
                        if canonical && *k != format!("{a}>{b}") {
                            continue;
                        }
                        let (Some(&i), Some(&j)) = (idx.get(&a), idx.get(&b)) else { continue };
                        cells.push((i as u32, j as u32, [0.0; 24], 0));
                        at.insert(k.as_str(), cells.len() - 1);
                        cells.len() - 1
                    }
                };
                cells[pos].2[hr] = v;
                cells[pos].3 |= 1 << hr;
            }
        }
        OdTable { cells }
    }

    /// Células na hora `hour`: (origem, destino, veículos/h).
    fn at(&self, hour: f64) -> impl Iterator<Item = (usize, usize, f64)> + '_ {
        let hr = hour.rem_euclid(24.0);
        let (i, f) = (hr.floor() as usize % 24, hr - hr.floor());
        let k = (i + 1) % 24;
        self.cells.iter().filter(move |c| c.3 & (1 << i | 1 << k) != 0).map(move |c| {
            let a = if c.3 & (1 << i) != 0 { c.2[i] * (1.0 - f) } else { 0.0 };
            let b = if c.3 & (1 << k) != 0 { c.2[k] * f } else { 0.0 };
            (c.0 as usize, c.1 as usize, a + b)
        })
    }
}

/// Partes da demanda que não mudam durante a simulação, montadas no primeiro recálculo da matriz.
#[derive(Default)]
struct DemandCache {
    zones: Vec<demand::ZoneIn>,
    statics: Vec<demand::ZoneOut>,
    /// tempos entre zonas (min); só guardado quando vem da cópia de fluxo livre (não muda mais)
    t: Option<Vec<Vec<f64>>>,
    /// `od_manual` por índice (sem `od_hourly`)
    manual: Vec<Vec<Option<f64>>>,
    car: Option<OdTable>,
    moto: Option<OdTable>,
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
    /// faixa de contramão só de motos (fase 9, `contra.rs`)
    contra: bool,
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
    /// conversão só de ônibus (`CONN_BUS`)
    bus_only: bool,
    /// entrada numa faixa de contramão: só para a moto da parcela `MB_CONTRA` (fora do roteador)
    moto_only: bool,
}

/// Dados de uma conversão de saída que `stay_cost` lê, contíguos por faixa de origem (`sc_off`): evita carregar a
/// `ConnRt` inteira e a faixa de destino a cada consulta (falhas de cache dominavam o custo).
#[derive(Clone, Copy)]
struct ScConn {
    /// id da conversão (aresta)
    c: u32,
    /// faixa de destino
    to: u32,
    /// `pen + contra_pen`
    pen: f32,
    /// `pen + tempo livre da faixa de destino` (escala do erro de percepção)
    err_base: f32,
    dirseg: u32,
    lane_local: u32,
    zone: u16,
    /// SC_BUS_ONLY | SC_MOTO_ONLY | SC_TO_BUS
    flags: u8,
}
const SC_BUS_ONLY: u8 = 1;
const SC_MOTO_ONLY: u8 = 2;
const SC_TO_BUS: u8 = 4;

#[derive(Clone, Copy)]
struct LaneHot {
    len: f32,
    vfree: f32,
    dirseg: u32,
    to_node: u32,
    contra: bool,
}

/// Conversão que chega a uma faixa (lista achatada `lin`), com o que a fila justa e a fusão leem dela.
#[derive(Clone, Copy)]
struct LinE {
    c: u32,
    node: u32,
    from: u32,
    /// ponta (`end_id`) da faixa de origem, para o estado do semáforo
    from_end: u32,
    rank: i16,
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
    /// ônibus de linha: nascidos, concluídos, paradas atendidas e perdidas, tempo embarcando (s)
    pub bus_spawned: u64,
    pub bus_finished: u64,
    pub bus_stops: u64,
    pub bus_stops_missed: u64,
    pub bus_dwell_s: f64,
    /// carros da parcela que pararam no ponto de ônibus
    pub cars_at_bus_stop: u64,
    /// motos nascidas e concluídas (também contadas em `spawned`/`finished`)
    pub moto_spawned: u64,
    pub moto_finished: u64,
    /// entradas de motos no corredor e moto-segundos andando nele
    pub corridor_entries: u64,
    pub corridor_s: f64,
    /// soma dos tempos de viagem das motos concluídas (s)
    pub moto_travel_sum: f64,
    /// motos da parcela que avançaram o vermelho com o cruzamento livre (não contam em `red_runs`)
    pub moto_red_go: u64,
    /// entradas de motos na contramão e moto-segundos andando nela
    pub contra_entries: u64,
    pub contra_s: f64,
}

/// floats por veículo no quadro enviado à interface (ver `Sim::snapshot`)
pub const SNAP_STRIDE: usize = 7;

/// Tipo de veículo (`Sim::kind`, 7ª posição do snapshot).
pub const KIND_CAR: u8 = 0;
pub const KIND_BUS: u8 = 1;
pub const KIND_MOTO: u8 = 2;
/// comportamentos por parcela dos motociclistas (fase 9), bits de `moto_beh`
pub const MB_RED: u8 = 1;
pub const MB_BUS: u8 = 2;
pub const MB_CONTRA: u8 = 4;
pub const MB_FAST: u8 = 8;
/// estado: a moto da parcela `MB_RED` decidiu avançar o vermelho nesta faixa (limpo ao entrar na conversão)
const MB_REDGO: u8 = 16;
/// parada na linha há tanto tempo (s) antes de avançar o vermelho, e brecha exigida (s) a quem tem o verde
const MOTO_RED_WAIT: f32 = 2.0;
const MOTO_RED_GAP: f32 = 4.0;

fn lc_block_text(c: u8) -> &'static str {
    match c {
        1 => "faixa incompatível",
        2 => "linha contínua perto do nó",
        3 => "seta ligada, aguardando",
        4 => "sem vão à frente na faixa vizinha",
        5 => "sem vão atrás na faixa vizinha",
        6 => "moto no corredor",
        _ => "nenhuma",
    }
}

/// zíper: folga (s) entre chegadas ao ponto de convergência e espera (s) a partir da qual a outra corrente cede
const MERGE_GAP: f32 = 1.0;
const MERGE_WAIT: f32 = 2.0;
/// abaixo desta velocidade (m/s) a corrente está em fila e alterna um a um; acima, só cede após `MERGE_COURTESY` s de espera
const MERGE_SLOW: f32 = 5.0;
const MERGE_COURTESY: f32 = 20.0;
/// Fusão cedendo à via preferencial: freada (m/s²) que o IDM do seguidor da preferencial pode pedir quando quem entra
/// fica à frente dele (critério de segurança do MOBIL, `b_safe`).
const MERGE_BRAKE: f32 = 2.0;

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
/// Freada que o motorista aceita para parar no amarelo (MBST vol. V, CONTRAN 2022: 3 m/s²).
const AMBER_DECEL: f32 = 3.0;
/// Quem seguiu no amarelo mantém a decisão até este tempo de vermelho (dentro do vermelho geral de 2 s).
const AMBER_COMMIT: f32 = 1.5;
/// Faixa concorrente mais curta que isto: a brecha também olha quem vem antes dela (`fast_before`).
const SHORT_APPROACH: f32 = 30.0;
/// Só quem vem rápido antes da faixa concorrente curta conta (os lentos ainda estão à vista dela a tempo).
const FAST_BEFORE: f32 = 9.0;

/// metros finais da faixa, antes do nó, em que não se troca de faixa (linha contínua)
const LC_SOLID_ZONE: f32 = 12.0;
/// tempo mínimo de seta ligada antes de mudar de faixa (s)
const LC_BLINK: f32 = 1.0;

/// Corredor de motos (fase 8): faixa virtual entre duas faixas vizinhas do mesmo sentido. A moto entra quando o fluxo
/// ao lado anda abaixo de `CORR_VMAX` (m/s, 20 km/h), sai acima de `CORR_VOUT` e anda até `CORR_DV` acima dele.
const CORR_VMAX: f32 = 5.6;
const CORR_VOUT: f32 = 11.0;
const CORR_DV: f32 = 4.2;
/// limites da parcela "corredor rápido" (fase 9): entra abaixo de 40 km/h, sai acima de 60 km/h, até +25 km/h
const CORR_VMAX_FAST: f32 = 11.1;
const CORR_VOUT_FAST: f32 = 16.7;
const CORR_DV_FAST: f32 = 6.9;
/// distância mínima (m) entre os centros de duas faixas para haver corredor
const CORR_MIN_W: f32 = 2.8;
/// até onde (m) se olha à frente o fluxo das faixas ao lado e quem invade o corredor
const CORR_LOOK: f32 = 40.0;
/// nó sem semáforo: o corredor acaba a esta distância (m) da linha e a moto volta para uma faixa antes dele
const CORR_END_FREE: f32 = 6.0;
/// custo (s) de cada moto já na fila do corredor, na escolha entre os dois corredores de uma faixa
const CORR_QUEUE_S: f32 = 1.5;
/// tempo (s) que a carroceria de quem troca de faixa leva para atravessar o corredor (`lat` volta a 1,8 m/s)
const CORR_SLIDE: f32 = 2.0;
/// as motos paradas na linha saem na frente do primeiro carro (parado) só nestes primeiros segundos do verde
const CORR_GREEN_HEAD: f32 = 3.0;
/// depois disso, a moto só entra na conversão na frente de carro que freie no máximo isto (m/s²) por ela
const CORR_CUT_BRAKE: f32 = 1.5;
/// meia largura (m) da carroceria por tipo, para quem invade o corredor
fn half_w(kind: u8) -> f32 {
    match kind {
        KIND_BUS => 1.25,
        KIND_MOTO => 0.45,
        _ => 0.9,
    }
}

struct RouteBatch {
    zones: Vec<u32>,
    trees: Vec<Option<Vec<f32>>>,
    /// passo em que o lote é instalado
    due: u64,
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
    /// 1 bit por aresta: ligada se `edge_veh` dela não está vazia (o passo só percorre as ocupadas, em ordem)
    occ_bits: Vec<u64>,
    /// arestas ocupadas no passo atual (buffer reaproveitado)
    active: Vec<u32>,
    node_conns: Vec<Vec<u32>>,
    /// conversões que chegam a cada faixa
    lane_in: Vec<Vec<u32>>,
    /// conflitos de cada conversão, calculados na primeira consulta: `conf_flat[conf_rng[c].0..][..conf_rng[c].1]`
    /// (`u32::MAX` = ainda não calculados). Arena contígua: sem `Rc` (o contador de referências custava uma falha de
    /// cache por consulta)
    conf_flat: Vec<Conflict>,
    conf_rng: Vec<(u32, u32)>,
    /// tempo até a zona de destino por faixa (até o primeiro nó dela): vale enquanto o veículo está fora da zona
    zone_dist: Vec<Vec<f32>>,
    /// cópia de fluxo livre de `zone_dist` (meio segundo), para quem não usa app e para o tempo livre da viagem
    zone_free: Vec<Vec<u16>>,
    /// faixas-alvo de cada zona (as que terminam num nó dela) e árvore em recálculo
    zone_tl: Vec<Vec<u32>>,
    /// lote de árvores pedido e ainda não instalado
    route_batch: Option<RouteBatch>,
    /// árvores calculadas fora da simulação (outro worker): o lote pedido espera a ponte buscar (`route_take`);
    /// sem isso (ou se a resposta não chegar a tempo), são calculadas aqui no passo de instalação
    route_ext: bool,
    route_out: bool,
    /// instante em que a árvore de cada zona foi recalculada pela última vez
    tree_at: Vec<f64>,
    /// veículos em circulação rumo a cada zona (prioridade do recálculo)
    to_zone: Vec<u32>,
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
    /// conversões de saída de cada faixa em formato compacto para `stay_cost` (faixa l: `sc[sc_off[l]..sc_off[l + 1]]`)
    sc: Vec<ScConn>,
    sc_off: Vec<u32>,
    /// `lane_in` achatado (faixa l: `lin[lin_off[l]..lin_off[l + 1]]`)
    lin: Vec<LinE>,
    lin_off: Vec<u32>,
    /// destino, origem e nó de cada conversão (índice = aresta − nl), fora da `ConnRt` para caber no cache
    conn_to: Vec<u32>,
    conn_from: Vec<u32>,
    conn_node: Vec<u32>,
    /// faixas ocupadas no último `refresh_live_pen` (multiplicador de troca diferente de 1) e área de trabalho
    lc_prev: Vec<u32>,
    lc_occ: Vec<u32>,
    /// conversões com `conn_seen` finito (a ordem não importa: cada uma só escreve a sua penalidade)
    seen_conns: Vec<u32>,
    /// campos das faixas lidos por veículo a cada passo, fora da `LaneRt` (que carrega a lista de conversões)
    lane_hot: Vec<LaneHot>,
    conn_vturn: Vec<f32>,
    lrouter: LaneRouter,
    local_used: usize,
    /// destino de cada veículo (índice em `targets`), fixo da origem ao fim da viagem
    tgt: Vec<u32>,
    /// custo de trocar de faixa em cada faixa (s)
    lane_w: Vec<f32>,
    rule_breaker: Vec<bool>,
    /// motorista que "fecha o cruzamento" (ver `SimParams::box_block_pct`)
    box_blocker: Vec<bool>,
    /// comportamentos por parcela da moto (`MB_*`); 0 para carros e ônibus
    moto_beh: Vec<u8>,
    /// parado neste passo por um ponto de conflito ocupado (para `Stats::box_wait_s`)
    held_box: Vec<bool>,
    /// primeiro da faixa retido na linha por um nó logo adiante (`ahead_stop_at`), mesmo com `enter_ok`
    ahead_held: Vec<bool>,
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
    /// faixas com fila de entrada não vazia (para não varrer todas as faixas a cada passo)
    pend_gates: Vec<u32>,
    /// menor `next_spawn[i][j]` de cada zona de origem (linha sem nascimento devido é pulada)
    row_next: Vec<f64>,
    rng: Rng,
    /// matriz de motos (`od_hourly_moto`): taxas, próximo nascimento e menor por origem, com PRNG próprio para que,
    /// sem a matriz, a simulação seja exatamente a de antes
    rate_m: Vec<Vec<f64>>,
    next_spawn_m: Vec<Vec<f64>>,
    row_next_m: Vec<f64>,
    rng_moto: Rng,
    next_matrix_at: f64,
    /// recálculos da matriz OD feitos (diagnóstico de passos longos)
    pub matrix_refreshes: u32,
    dem: Option<DemandCache>,
    sigs: Vec<SigRt>,
    end_state: Vec<u8>,
    red_age: Vec<f32>,
    /// tempo (s) desde que a ponta abriu (verde), para a saída das motos na frente (`corr_pick`)
    green_age: Vec<f32>,
    /// pontas controladas por algum semáforo (as demais ficam sempre em 0)
    sig_ends: Vec<u32>,
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
    /// faixa com algum pedido de passagem em `wants`
    want_lane: Vec<bool>,
    /// buffers reaproveitados entre passos
    movers: Vec<(u32, u32)>,
    inside: Vec<(u32, u32)>,
    overlap_now: std::collections::HashSet<(u32, u32)>,
    acc: Vec<f32>,
    /// decisão de entrada em cruzamento reaproveitada por 1 passo (reavaliada a cada 2)
    enter_ok: Vec<bool>,
    /// faixa (+1) em que o veículo decidiu seguir no amarelo por não conseguir parar; vale no início do vermelho
    amber_go: Vec<u32>,
    /// conversão (+1) em que o veículo saiu de um impasse por ocupação: mantém a decisão depois de arrancar
    occ_go: Vec<u32>,
    /// a última chamada de `occ_obstacle` deixou de respeitar alguma ocupação por impasse
    occ_skipped: bool,
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
    /// diagnóstico (só com `overlap_log`): últimos estados (aresta, s, v, pode entrar, sinal no fim da faixa: 9 = na conversão) de cada veículo
    hist: Vec<std::collections::VecDeque<(u32, f32, f32, bool, u8)>>,
    /// conversões que entram no roteador dos carros (as só de ônibus vêm depois delas)
    n_car_conns: usize,
    // ônibus de linha (ver `bus.rs`)
    bus_plans: Vec<bus::BusPlan>,
    bus_lines: Vec<bus::BusLineStats>,
    /// (instante, plano, intervalo programado) de cada partida, em ordem; a próxima a sair; fila de quem não coube
    bus_deps: Vec<(f64, u32, f32)>,
    bus_next: usize,
    bus_queue: std::collections::VecDeque<(u32, f32, f32)>,
    bus_hour: [f64; 24],
    /// última passagem (instante) por parada de cada plano
    stop_last: Vec<Vec<f32>>,
    /// pontos de ônibus de cada faixa (posição na faixa), para os carros da parcela que param no ponto
    lane_stops: std::collections::HashMap<u32, Vec<f32>>,
    /// paradas das linhas (id do GTFS ou da edição, nome) e índice por id; passagens por (parada, linha)
    stop_ids: Vec<String>,
    stop_names: Vec<String>,
    stop_id_of: std::collections::HashMap<String, u32>,
    stop_hw: std::collections::HashMap<(u32, u32), bus::StopLineHw>,
    /// ônibus em cada baia (pela parada)
    bay_occ: std::collections::HashMap<u32, u8>,
    last_spawn: usize,
    /// por veículo: plano, passo, próxima parada, embarque restante (s), ponto escolhido (carro), acumulados da viagem
    /// (embarcando, parado fora de parada, intervalo programado), parcelas de carro
    route: Vec<u32>,
    bstep: Vec<u32>,
    bstop: Vec<u32>,
    dwell: Vec<f32>,
    stop_at: Vec<f32>,
    bacc: Vec<[f32; 3]>,
    bus_lane_ok: Vec<bool>,
    /// carro da parcela que para no ponto: 0 não, 1 vai parar, 2 já parou
    stop_car: Vec<u8>,
    /// ônibus fora da faixa, numa baia (parada + 1; 0 = não): não está na lista da faixa, não anda, não troca de faixa
    in_bay: Vec<u32>,
    /// corredor de motos: `corr_ok[l]` = há corredor entre as faixas `l` e `l + 1`; motos em cada corredor (ordenadas
    /// por `s`, fora de `edge_veh`), bits dos corredores ocupados, ponta de faixa com semáforo e, por veículo, o
    /// corredor em que está (`NONE` = numa faixa; `edge` segue sendo a faixa de referência)
    corr_ok: Vec<bool>,
    /// contramão (fase 9): para cada faixa de contramão, as faixas reais do lado da borda; para cada uma destas, a de
    /// contramão ao lado (`NONE` se não há)
    contra_adj: Vec<Vec<u32>>,
    contra_by: Vec<u32>,
    corr_veh: Vec<Vec<u32>>,
    corr_bits: Vec<u64>,
    end_sig: Vec<bool>,
    corr: Vec<u32>,
    corr_act: Vec<u32>,
}

impl Sim {
    pub fn new(net: &Net, init: &SimInit) -> Result<Sim, String> {
        Sim::new_ext(net, init, false)
    }

    /// Com `route_ext`, as árvores iniciais das zonas ficam vazias para outro worker calcular (`init_put`); o que
    /// faltar no 1º passo é calculado aqui. Sem, é igual a `new`.
    pub fn new_ext(net: &Net, init: &SimInit, route_ext: bool) -> Result<Sim, String> {
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
                contra: false,
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
        // ---- contramão de motos (fase 9): faixas só da simulação, depois das da rede
        let contra = if init.params.moto_contra_pct > 0.0 { contra::plan(net, &lanes) } else { Vec::new() };
        let mut contra_adj: Vec<(u32, Vec<u32>)> = Vec::with_capacity(contra.len());
        let mut contra_links = Vec::with_capacity(contra.len());
        for c in contra {
            contra_adj.push((lanes.len() as u32, c.adj));
            contra_links.push((lanes.len() as u32, c.ins, c.outs));
            lanes.push(c.lane);
        }
        let nl = lanes.len();
        // ---- conversões ligadas
        let mut conns: Vec<ConnRt> = Vec::new();
        let mut node_conns: Vec<Vec<u32>> = vec![Vec::new(); net.nodes.len()];
        let mut lane_in: Vec<Vec<u32>> = vec![Vec::new(); lanes.len()];
        // as só de ônibus (com linhas) vêm depois das ligadas, para os ids das ligadas não mudarem
        let with_bus = init.params.bus_lines && !init.bus.is_empty();
        let mut n_car_conns = 0;
        for c in net.conns.iter().filter(|c| c.state == CONN_ALLOWED).chain(net.conns.iter().filter(|c| with_bus && c.state == CONN_BUS)) {
            if c.state == CONN_ALLOWED {
                n_car_conns += 1;
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
            lane_in[c.to_lane as usize].push(eid);
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
                bus_only: c.state == CONN_BUS,
                moto_only: false,
            });
        }
        // contramão: entradas (só da parcela, fora do roteador) e saídas
        for (cl, ins, outs) in contra_links {
            let links = ins.into_iter().map(|(n, f, k, c)| (n, f, cl, k, c, true)).chain(outs.into_iter().map(|(n, t, k, c)| (n, cl, t, k, c, false)));
            for (node, from, to, kind, curve, entry) in links {
                let fl = &lanes[from as usize];
                let pts: Vec<V2> = (0..=8).map(|i| bez8(&curve, i as f32 / 8.0)).collect();
                let len = pts.windows(2).map(|w| len_v(sub(w[1], w[0]))).sum::<f64>() as f32;
                let vturn = match kind {
                    K_STRAIGHT => fl.vfree.min(13.0),
                    K_RIGHT => 6.5,
                    K_LEFT => 5.5,
                    _ => 3.5,
                }
                .min(contra::CONTRA_V);
                let eid = (nl + conns.len()) as u32;
                let (ring, fl_end) = (fl.ring, fl.end_id);
                lanes[from as usize].conns.push(eid);
                node_conns[node as usize].push(eid);
                lane_in[to as usize].push(eid);
                conns.push(ConnRt {
                    node,
                    from,
                    to,
                    kind,
                    len: len.max(0.5),
                    curve,
                    pen: route::turn_penalty(net, kind, node),
                    jpen: route::junction_penalty(net, node),
                    vturn,
                    // na contramão cede a todos, na entrada e na saída
                    rank: 100,
                    ring,
                    u_me: net.outward(fl_end),
                    bus_only: false,
                    moto_only: entry,
                });
            }
        }
        let mut edge_len: Vec<f32> = lanes.iter().map(|l| l.len).collect();
        edge_len.extend(conns.iter().map(|c| c.len));
        let ne = edge_len.len();

        // ---- zonas: nós de destino e portões de origem
        let nz = init.zones.len();
        let mut sink = Vec::with_capacity(nz);
        let mut gates = Vec::with_capacity(nz);
        for z in &init.zones {
            let ip = demand::InPoly::new(&z.poly);
            let inside: Vec<bool> = net.nodes.iter().map(|n| z.poly.len() >= 3 && ip.contains(n.pos)).collect();
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

        // ---- roteamento por faixa: conversões ligadas + trocas de faixa
        let lane_len: Vec<f32> = lanes.iter().map(|l| l.len).collect();
        let lane_speed: Vec<f32> = lanes.iter().map(|l| l.vfree).collect();
        // as ligadas primeiro (os índices delas no roteador são os ids, para a penalidade viva), depois as saídas da contramão
        let conn_list: Vec<(u32, u32, f32)> =
            conns[..n_car_conns].iter().chain(conns[n_car_conns..].iter().filter(|c| !c.bus_only && !c.moto_only)).map(|c| (c.from, c.to, c.pen)).collect();
        let lane_key: Vec<(u32, u8)> = lanes.iter().map(|l| (l.seg, if l.contra { 2 } else { l.dir })).collect();
        let lrouter = LaneRouter::new(&lane_key, &lane_len, &lane_speed, &conn_list);
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
            if !route_ext {
                lrouter.tree(&targets, &mut dd);
            }
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
            occ_bits: vec![0; ne.div_ceil(64)],
            active: Vec::new(),
            node_conns,
            lane_in,
            sc: Vec::new(),
            sc_off: Vec::new(),
            lin: Vec::new(),
            lin_off: Vec::new(),
            conn_to: Vec::new(),
            conn_from: Vec::new(),
            conn_node: Vec::new(),
            lc_prev: Vec::new(),
            lc_occ: Vec::new(),
            seen_conns: Vec::new(),
            lane_hot: Vec::new(),
            conn_vturn: Vec::new(),
            conf_flat: Vec::new(),
            conf_rng: vec![(u32::MAX, 0); ne],
            zone_free: Vec::new(),
            zone_dist,
            zone_tl,
            route_batch: None,
            route_ext,
            route_out: false,
            tree_at: Vec::new(),
            to_zone: vec![0; nz],
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
            moto_beh: Vec::new(),
            held_box: Vec::new(),
            ahead_held: Vec::new(),
            overlap_prev: Default::default(),
            overlap_log: None,
            hist: Vec::new(),
            app: Vec::new(),
            pseed: Vec::new(),
            t_due: Vec::new(),
            t_req: Vec::new(),
            left_gate: Vec::new(),
            entry_stats: vec![(0.0, 0.0, 0); nz],
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
            pending: vec![Default::default(); nl],
            pend_gates: Vec::new(),
            row_next: vec![f64::INFINITY; nz],
            rng: Rng::new(init.params.seed),
            rate_m: vec![vec![0.0; nz]; nz],
            next_spawn_m: vec![vec![f64::INFINITY; nz]; nz],
            row_next_m: vec![f64::INFINITY; nz],
            rng_moto: Rng::new(init.params.seed ^ 0x4D07_0B1C_E5EE_D000),
            next_matrix_at: 0.0,
            matrix_refreshes: 0,
            dem: None,
            sigs,
            end_state: vec![0; net.segs.len() * 2],
            green_age: vec![0.0; net.segs.len() * 2],
            red_age: vec![0.0; net.segs.len() * 2],
            sig_ends: Vec::new(),
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
            want_lane: vec![false; nl],
            movers: Vec::new(),
            inside: Vec::new(),
            overlap_now: Default::default(),
            acc: Vec::new(),
            enter_ok: Vec::new(),
            amber_go: Vec::new(),
            occ_go: Vec::new(),
            occ_skipped: false,
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
            n_car_conns,
            bus_plans: Vec::new(),
            bus_lines: Vec::new(),
            bus_deps: Vec::new(),
            bus_next: 0,
            bus_queue: Default::default(),
            bus_hour: [0.0; 24],
            stop_last: Vec::new(),
            lane_stops: Default::default(),
            stop_ids: Vec::new(),
            stop_names: Vec::new(),
            stop_id_of: Default::default(),
            stop_hw: Default::default(),
            bay_occ: Default::default(),
            in_bay: Vec::new(),
            last_spawn: 0,
            route: Vec::new(),
            bstep: Vec::new(),
            bstop: Vec::new(),
            dwell: Vec::new(),
            stop_at: Vec::new(),
            bacc: Vec::new(),
            bus_lane_ok: Vec::new(),
            stop_car: Vec::new(),
            corr_ok: Vec::new(),
            contra_adj: {
                let mut v = vec![Vec::new(); nl];
                for (cl, adj) in &contra_adj {
                    v[*cl as usize] = adj.clone();
                }
                v
            },
            contra_by: {
                let mut v = vec![NONE; nl];
                for (cl, adj) in &contra_adj {
                    for &a in adj {
                        v[a as usize] = *cl;
                    }
                }
                v
            },
            corr_veh: vec![Vec::new(); nl],
            corr_bits: vec![0; nl.div_ceil(64)],
            end_sig: vec![false; net.segs.len() * 2],
            corr: Vec::new(),
            corr_act: Vec::new(),
        };
        sim.start_time = sim.time;
        sim.tree_at = vec![sim.time; nz];
        let nc = sim.conns.len();
        sim.conn_delay = vec![0.0; nc];
        sim.conn_seen = vec![f32::NEG_INFINITY; nc];
        sim.live_pen = sim.conns.iter().map(|c| c.pen).collect();
        let mut ends: Vec<u32> = sim.sigs.iter().flat_map(|sg| sg.phases.iter().flat_map(|p| p.3.iter().copied())).collect();
        ends.sort_unstable();
        ends.dedup();
        for &e in &ends {
            sim.end_sig[e as usize] = true;
        }
        sim.sig_ends = ends;
        sim.corr_ok = (0..nl)
            .map(|l| {
                let a = &sim.lanes[l];
                l + 1 < nl && {
                    let b = &sim.lanes[l + 1];
                    b.seg == a.seg && b.dir == a.dir && !a.bus && !b.bus && (a.off - b.off).abs() >= CORR_MIN_W
                }
            })
            .collect();
        sim.build_sc();
        if with_bus {
            sim.init_buses(net, &init.bus);
        }
        if !route_ext || sim.init_missing() == 0 {
            sim.finish_init(net);
        }
        Ok(sim)
    }

    /// Árvores iniciais prontas: cópia de fluxo livre e matriz OD (que usa o tempo livre entre zonas).
    fn finish_init(&mut self, net: &Net) {
        if self.params.reroute {
            self.zone_free = self.zone_dist.iter().map(|d| d.iter().map(|&x| enc_t(x)).collect()).collect();
        }
        self.refresh_matrix(net);
    }

    /// Zonas cuja árvore inicial ainda falta (só com `route_ext`).
    pub fn init_missing(&self) -> usize {
        self.zone_dist.iter().filter(|d| d.is_empty()).count()
    }

    pub fn zone_count(&self) -> usize {
        self.zone_ids.len()
    }

    /// Faixas-alvo da zona `z` (onde a árvore começa).
    pub fn zone_targets(&self, z: usize) -> &[u32] {
        &self.zone_tl[z]
    }

    /// Grafo do roteador por faixa, para uma cópia em outro worker (`LaneRouter::from_bytes`).
    pub fn router_bytes(&self) -> Vec<u8> {
        self.lrouter.to_bytes()
    }

    /// Árvore inicial da zona `z` calculada fora. A última que chega fecha a inicialização.
    pub fn init_put(&mut self, net: &Net, z: usize, tree: Vec<f32>) {
        if z >= self.zone_dist.len() || !self.zone_dist[z].is_empty() || tree.len() != self.nl {
            return;
        }
        self.zone_dist[z] = tree;
        if self.init_missing() == 0 {
            self.finish_init(net);
        }
    }

    /// Calcula aqui as árvores iniciais que não chegaram.
    fn init_fill(&mut self, net: &Net) {
        for z in 0..self.zone_dist.len() {
            if self.zone_dist[z].is_empty() {
                let mut d = Vec::new();
                self.lrouter.tree(&self.zone_tl[z], &mut d);
                self.zone_dist[z] = d;
            }
        }
        self.finish_init(net);
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
            // tempo livre (a árvore inicial, antes do trânsito medido)
            let d = if self.zone_free.is_empty() { self.zone_dist[j][l as usize] } else { dec_t(self.zone_free[j][l as usize]) };
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

    /// Monta (uma vez) o que a demanda reaproveita entre recálculos: zonas, geometria resolvida sobre a rede e as
    /// matrizes fixadas por índice.
    fn demand_cache(&mut self, net: &Net) {
        if self.dem.is_some() {
            return;
        }
        let nz = self.zone_ids.len();
        let zones: Vec<demand::ZoneIn> = (0..nz)
            .map(|i| demand::ZoneIn { id: self.zone_ids[i], poly: self.zone_poly[i].clone(), gen: self.zone_gen[i], attr: self.zone_attr[i], profile: self.zone_profile[i].clone(), gateway: self.zone_gateway[i] })
            .collect();
        let mids = demand::seg_mids(net);
        let statics = zones.iter().map(|z| demand::zone_static(net, &mids, z)).collect();
        let idx: std::collections::HashMap<i64, usize> = self.zone_ids.iter().enumerate().map(|(i, &z)| (z, i)).collect();
        let h = &self.params.od_hourly;
        let car = (h.len() == 24).then(|| OdTable::build(h, &idx, true));
        let manual = if car.is_none() { demand::forced_cells(&zones, &self.params.od_manual) } else { Vec::new() };
        let hm = &self.params.od_hourly_moto;
        let moto = (hm.len() == 24).then(|| OdTable::build(hm, &idx, false));
        self.dem = Some(DemandCache { zones, statics, t: None, manual, car, moto });
    }

    /// Recalcula as taxas da matriz de motos (células fixas, sem modelo gravitacional).
    fn refresh_moto(&mut self) {
        let Some(table) = self.dem.as_ref().and_then(|d| d.moto.as_ref()) else { return };
        let hour = (self.time / 3600.0).rem_euclid(24.0);
        let nz = self.zone_ids.len();
        let mut m = vec![vec![0.0f64; nz]; nz];
        for (i, j, v) in table.at(hour) {
            if i == j && !self.params.intrazone {
                continue;
            }
            m[i][j] += v.max(0.0);
        }
        for i in 0..nz {
            for j in 0..nz {
                let new_rate = if self.gates[i].is_empty() { 0.0 } else { m[i][j] / 3600.0 };
                let old = self.rate_m[i][j];
                self.rate_m[i][j] = new_rate;
                if new_rate <= 0.0 {
                    self.next_spawn_m[i][j] = f64::INFINITY;
                } else if old <= 0.0 || !self.next_spawn_m[i][j].is_finite() {
                    self.next_spawn_m[i][j] = self.time + self.rng_moto.exp(new_rate);
                } else if (new_rate - old).abs() > 1e-12 {
                    let remaining = (self.next_spawn_m[i][j] - self.time).max(0.0);
                    self.next_spawn_m[i][j] = self.time + remaining * old / new_rate;
                }
            }
        }
        for i in 0..nz {
            self.row_next_m[i] = self.next_spawn_m[i].iter().copied().fold(f64::INFINITY, f64::min);
        }
    }

    /// Recalcula a matriz OD para a hora atual e as taxas de chegada.
    fn refresh_matrix(&mut self, net: &Net) {
        self.matrix_refreshes += 1;
        self.demand_cache(net);
        let hour = (self.time / 3600.0).rem_euclid(24.0);
        let mut dem = self.dem.take().unwrap();
        let t = match dem.t.take() {
            Some(t) => t,
            None => {
                let me: &Sim = self;
                demand::travel_matrix(&dem.statics, &|i, j, _c| me.zone_minutes(i, j))
            }
        };
        // células fixadas na hora: fatia horária (interpolada como o perfil das zonas) ou `od_manual`
        let hourly;
        let forced = match &dem.car {
            Some(table) => {
                let nz = dem.zones.len();
                let mut f = vec![vec![None; nz]; nz];
                for (i, j, v) in table.at(hour) {
                    f[i][j] = Some(v);
                }
                hourly = f;
                &hourly
            }
            None => &dem.manual,
        };
        let (_, matrix) = demand::resolve_with(&dem.statics, &dem.zones, hour, self.params.beta, self.params.intrazone, forced, &t);
        // com a cópia de fluxo livre os tempos não mudam mais
        if !self.zone_free.is_empty() {
            dem.t = Some(t);
        }
        self.dem = Some(dem);
        let nz = self.zone_ids.len();
        for i in 0..nz {
            for j in 0..nz {
                let veh_h = if self.gates[i].is_empty() { 0.0 } else { matrix[i][j] };
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
        for i in 0..nz {
            self.row_next[i] = self.next_spawn[i].iter().copied().fold(f64::INFINITY, f64::min);
        }
        self.refresh_moto();
    }

    // ------------------------------------------------------------------ rotas em tempo real

    /// Recalcula, em lotes, as árvores das zonas com o tempo medido em cada trecho (atraso por conversão e ocupação
    /// das faixas). Como o veículo escolhe a conversão a cada faixa olhando a árvore atual, quem usa app desvia no
    /// caminho. O lote pedido num passo é instalado `ROUTE_LAG` passos depois; enquanto isso pode ser calculado em
    /// outro worker (`route_take`/`route_put`).
    fn route_step(&mut self) {
        let nz = self.zone_dist.len();
        if !self.params.reroute || nz == 0 {
            return;
        }
        if self.route_batch.as_ref().is_some_and(|b| self.tick >= b.due) {
            let mut b = self.route_batch.take().unwrap();
            for (k, &z) in b.zones.iter().enumerate() {
                // sem resposta de fora: calcula aqui, com os mesmos pesos (só mudam no próximo pedido)
                let tree = b.trees[k].take().unwrap_or_else(|| {
                    let mut d = Vec::new();
                    self.lrouter.tree(&self.zone_tl[z as usize], &mut d);
                    d
                });
                self.zone_dist[z as usize] = tree;
                self.tree_at[z as usize] = self.time;
                self.reroutes += 1;
            }
            self.route_out = false;
        }
        // árvores por janela de `ROUTE_LAG` passos (cada árvore percorre ~todas as faixas)
        let budget = (nz * self.nl / 900).clamp(ROUTE_BUDGET_MIN, ROUTE_BUDGET_MAX) as f64;
        let per_lag = ROUTE_LAG as f64 * budget / self.nl.max(1) as f64;
        let (every, n) = if per_lag >= 1.0 {
            (ROUTE_LAG, (per_lag.round() as usize).clamp(1, nz.min(ROUTE_BATCH_MAX)))
        } else {
            ((ROUTE_LAG as f64 / per_lag).round() as u64, 1)
        };
        if self.route_batch.is_none() && self.tick > 0 && self.tick % every == 0 {
            // pesos do momento: faixa em fluxo livre + atraso medido de cada conversão (o atraso já inclui a
            // lentidão no trecho de quem fez aquela manobra, então não se soma também a velocidade do trecho)
            self.refresh_live_pen();
            // as zonas com a árvore mais velha, pesadas pela raiz de quem está indo para lá: minimiza a idade média
            // da árvore que os veículos usam (zona vazia também é recalculada, só com menos frequência)
            let score = |z: usize| (self.time - self.tree_at[z]) * (1.0 + self.to_zone[z] as f64).sqrt();
            let mut order: Vec<usize> = (0..nz).collect();
            order.sort_by(|&a, &b| score(b).total_cmp(&score(a)).then(a.cmp(&b)));
            let zones: Vec<u32> = order[..n].iter().map(|&z| z as u32).collect();
            let trees = if self.route_ext {
                vec![None; n]
            } else {
                zones
                    .iter()
                    .map(|&z| {
                        let mut d = Vec::new();
                        self.lrouter.tree(&self.zone_tl[z as usize], &mut d);
                        Some(d)
                    })
                    .collect()
            };
            self.route_out = self.route_ext;
            self.route_batch = Some(RouteBatch { zones, trees, due: self.tick + ROUTE_LAG });
        }
    }

    /// Lote recém-pedido para calcular fora (uma vez): zonas, penalidade de cada conversão e multiplicador de troca
    /// de faixa. Vazio se não há pedido novo.
    pub fn route_take(&mut self) -> Option<(Vec<u32>, &[f32], &[f32])> {
        if !self.route_out {
            return None;
        }
        self.route_out = false;
        let zones = self.route_batch.as_ref()?.zones.clone();
        Some((zones, &self.live_pen[..self.n_car_conns], &self.lrouter.change_mul))
    }

    /// Árvore de uma zona do lote pendente, calculada fora.
    pub fn route_put(&mut self, z: u32, tree: Vec<f32>) {
        if tree.len() != self.nl {
            return;
        }
        if let Some(b) = self.route_batch.as_mut() {
            if let Some(k) = b.zones.iter().position(|&x| x == z) {
                b.trees[k] = Some(tree);
            }
        }
    }

    /// O próximo passo instala um lote que ainda não chegou inteiro (quem calcula fora deve esperar antes de chamar
    /// `step`, senão as que faltam são calculadas aqui) ou ainda faltam árvores iniciais.
    pub fn route_wait(&self) -> bool {
        if self.zone_dist.iter().any(|d| d.is_empty()) {
            return true;
        }
        self.route_batch.as_ref().is_some_and(|b| self.tick >= b.due && b.trees.iter().any(|t| t.is_none()))
    }

    /// Idade média (s) da árvore de rotas usada pelos veículos em circulação (cada um pesa a da sua zona de destino).
    pub fn route_age(&self) -> f64 {
        let n: u64 = self.to_zone.iter().map(|&k| k as u64).sum();
        if n == 0 {
            return 0.0;
        }
        self.to_zone.iter().zip(&self.tree_at).map(|(&k, &t)| k as f64 * (self.time - t)).sum::<f64>() / n as f64
    }

    /// Motos na rede agora.
    pub fn motos_alive(&self) -> usize {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.kind[i] == KIND_MOTO).count()
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
        if self.conn_seen[ci].is_finite() {
            self.conn_delay[ci] = 0.75 * self.conn_delay[ci] + 0.25 * d;
        } else {
            self.conn_delay[ci] = d;
            self.seen_conns.push(ci as u32);
        }
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
        // só as faixas ocupadas (pelo espelho `occ_bits`): nas vazias não há fila e o multiplicador de troca é 1
        let mut occ = std::mem::take(&mut self.lc_occ);
        occ.clear();
        for (w, &bits) in self.occ_bits.iter().enumerate() {
            let mut b = bits;
            while b != 0 {
                let l = w * 64 + b.trailing_zeros() as usize;
                if l >= self.nl {
                    break;
                }
                occ.push(l as u32);
                b &= b - 1;
            }
            if (w + 1) * 64 >= self.nl {
                break;
            }
        }
        for &l in &occ {
            let j = *self.edge_veh[l as usize].last().unwrap() as usize;
            let cc = self.choice[j];
            if cc != NONE && cc != SINK {
                let ci = cc as usize - self.nl;
                let ongoing = now - self.t_due[j];
                if ongoing > self.conn_delay[ci] {
                    self.conn_delay[ci] = ongoing.min(600.0);
                    if !self.conn_seen[ci].is_finite() {
                        self.conn_seen[ci] = now;
                        self.seen_conns.push(ci as u32);
                    }
                }
            }
        }
        // vazias desde a última vez voltam a 1 (= `lc_mul` de faixa vazia); as ocupadas recebem o valor atual
        for &l in &self.lc_prev {
            self.lrouter.change_mul[l as usize] = 1.0;
        }
        for &l in &occ {
            let m = self.lc_mul(l as usize);
            self.lrouter.change_mul[l as usize] = m;
        }
        self.lc_occ = std::mem::replace(&mut self.lc_prev, occ);
        // só as conversões já medidas: nas demais a penalidade continua a fixa (`c.pen`, valor inicial de `live_pen`
        // e do roteador)
        for k in 0..self.seen_conns.len() {
            let ci = self.seen_conns[k] as usize;
            if ci >= self.n_car_conns {
                continue;
            }
            let c = &self.conns[ci];
            let age = now - self.conn_seen[ci];
            // sem ninguém passando há tempo, a medição perde valor e volta ao típico
            let w = (-(age - 60.0).max(0.0) / 300.0).exp();
            let pen = (c.pen - c.jpen) + w * self.conn_delay[ci] + (1.0 - w) * c.jpen;
            self.live_pen[ci] = pen;
            self.lrouter.set_pen(ci, pen);
        }
    }

    // ------------------------------------------------------------------ semáforos

    fn update_signals(&mut self, net: &Net) {
        let _ = net;
        for &e in &self.sig_ends {
            self.end_state[e as usize] = 0;
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
        for &e in &self.sig_ends {
            let e = e as usize;
            if self.end_state[e] == 1 {
                self.red_age[e] += dt;
            } else {
                self.red_age[e] = 0.0;
            }
            if self.end_state[e] == 3 {
                self.green_age[e] += dt;
            } else {
                self.green_age[e] = 0.0;
            }
        }
    }

    // ------------------------------------------------------------------ veículos

    /// Sorteio das parcelas de comportamento da moto (fase 9), fora do PRNG: com 0 % nada muda.
    fn moto_parcels(&self, seed: u32) -> u8 {
        let p = &self.params;
        let mut b = 0;
        for (bit, key, pct) in [(MB_RED, 0x3ED0, p.moto_red_pct), (MB_BUS, 0xB05B, p.moto_bus_lane_pct), (MB_CONTRA, 0xC047, p.moto_contra_pct), (MB_FAST, 0xFA57, p.moto_corr_fast_pct)] {
            if pct > 0.0 && ((noise(seed, key) + 1.0) * 50.0) < pct as f32 {
                b |= bit;
            }
        }
        b
    }

    /// Nasce um veículo na faixa `lane` (ou numa vizinha do mesmo trecho). Ônibus de linha: `route` = plano, `dest`
    /// = `u16::MAX` (sem zona de destino).
    fn spawn_vehicle(&mut self, lane: u32, origin: u16, dest: u16, kind: u8, t_req: f32, route: u32) -> bool {
        let tree = if route != NONE {
            0
        } else {
            let Some(t) = self.pick_target(dest as usize, lane as usize) else { return false };
            t
        };
        let mut drv = Drv { tgt: tree as usize, app: (self.rng.f() as f64) < self.params.app_pct / 100.0, seed: self.rng.next() as u32, bus_ok: kind == KIND_BUS, contra: false, route, step: 0, stop_step: NONE, stop_lane: NONE };
        // sorteios por parcela fora do PRNG da simulação: com 0 % o resto da simulação não muda
        if kind == KIND_CAR && ((noise(drv.seed, 0xB05A) + 1.0) * 50.0) < self.params.bus_lane_car_pct as f32 {
            drv.bus_ok = true;
        }
        let mbeh = if kind == KIND_MOTO { self.moto_parcels(drv.seed) } else { 0 };
        if mbeh & MB_BUS != 0 {
            drv.bus_ok = true;
        }
        drv.contra = mbeh & MB_CONTRA != 0;
        if route != NONE {
            if let Some(st) = self.bus_plans[route as usize].stops.first().filter(|st| st.step == 0) {
                (drv.stop_step, drv.stop_lane) = (0, st.lane);
            }
        }
        // nasce na faixa do mesmo trecho que melhor leva ao destino escolhido
        let lane = {
            let (lo, hi) = self.lanes_of(lane as usize);
            let mut best = (f32::INFINITY, lane as usize);
            for cl in lo..hi {
                if !drv.bus_ok && self.lanes[cl].bus {
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
        if !drv.bus_ok && l.bus {
            return false;
        }
        // moto na contramão ao lado (fase 9): espera ela sair
        if self.contra_by[lane as usize] != NONE && self.contra_busy(lane as usize) {
            return false;
        }
        let (vlen, tgap, amax, bcomf, vfac) = if kind == KIND_BUS {
            (12.0, self.rng.range(1.4, 1.8), self.rng.range(0.9, 1.2), self.rng.range(1.8, 2.4), self.rng.range(0.8, 0.95))
        } else if kind == KIND_MOTO {
            // moto (estimativas): curta, arranca mais forte, segue mais perto e anda um pouco acima do limite
            (self.rng.range(2.0, 2.3), self.rng.range(0.6, 1.0), self.rng.range(2.8, 3.8), self.rng.range(2.5, 3.5), self.rng.range(1.0, 1.2))
        } else {
            (self.rng.range(4.1, 4.9), self.rng.range(0.9, 1.5), self.rng.range(1.6, 2.6), self.rng.range(2.2, 3.4), self.rng.range(0.9, 1.15))
        };
        // posição de nascimento: em faixas longas, um ponto qualquer ao longo dela (não só logo após o cruzamento)
        let llen = self.lanes[lane as usize].len;
        let mut s0 = vlen + 0.5;
        let mut placed = false;
        // ônibus de linha nasce no começo do caminho (antes das paradas dele)
        let attempts = if llen >= 45.0 && route == NONE { 4 } else { 1 };
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
            let lead_ok = lead_ok && !(self.corr_beside(lane as usize) && self.corr_sliding_in(lane as usize, cand - vlen - 4.0, cand + 6.0));
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
                self.amber_go.push(0);
                self.occ_go.push(0);
                self.stop_ok.push(0);
                self.t_free.push(0.0);
                self.origin.push(0);
                self.rule_breaker.push(false);
                self.box_blocker.push(false);
                self.moto_beh.push(0);
                self.held_box.push(false);
                self.ahead_held.push(false);
                self.app.push(false);
                self.pseed.push(0);
                self.t_due.push(0.0);
                self.t_req.push(0.0);
                self.left_gate.push(false);
                self.tgt.push(0);
                self.route.push(NONE);
                self.bstep.push(0);
                self.bstop.push(0);
                self.dwell.push(0.0);
                self.stop_at.push(f32::NAN);
                self.bacc.push([0.0; 3]);
                self.bus_lane_ok.push(false);
                self.stop_car.push(0);
                self.in_bay.push(0);
                self.corr.push(NONE);
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
        self.amber_go[id] = 0;
        self.occ_go[id] = 0;
        self.dest[id] = dest;
        if route == NONE {
            self.to_zone[dest as usize] += 1;
        }
        self.route[id] = route;
        self.bstep[id] = 0;
        self.bstop[id] = 0;
        self.dwell[id] = 0.0;
        self.stop_at[id] = f32::NAN;
        self.bacc[id] = [0.0; 3];
        self.in_bay[id] = 0;
        self.corr[id] = NONE;
        self.bus_lane_ok[id] = drv.bus_ok && kind != KIND_BUS;
        self.moto_beh[id] = mbeh;
        self.stop_car[id] = if kind == KIND_CAR && ((noise(drv.seed, 0x570B) + 1.0) * 50.0) < self.params.bus_stop_car_pct as f32 { 1 } else { 0 };
        self.origin[id] = origin;
        self.rule_breaker[id] = (self.rng.f() as f64) < self.params.illegal_pct / 100.0;
        // sorteio fora do PRNG da simulação: com 0 % o resto da simulação não muda
        self.box_blocker[id] = ((noise(drv.seed, 0xB0C5) + 1.0) * 50.0) < self.params.box_block_pct as f32;
        self.held_box[id] = false;
        self.ahead_held[id] = false;
        self.tgt[id] = tree;
        self.app[id] = drv.app;
        self.pseed[id] = drv.seed;
        self.t_free[id] = if route != NONE {
            self.bus_plans[route as usize].t_free
        } else {
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
        if route == NONE {
            self.stats.spawned += 1;
            if kind == KIND_MOTO {
                self.stats.moto_spawned += 1;
            }
        }
        self.last_spawn = id;
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
        let (route, step) = (self.route[i], self.bstep[i]);
        let (mut stop_step, mut stop_lane) = (NONE, NONE);
        if route != NONE {
            if let Some(st) = self.bus_plans[route as usize].stops.get(self.bstop[i] as usize) {
                (stop_step, stop_lane) = (st.step, st.lane);
            }
        }
        Drv { tgt: self.tgt[i] as usize, app: self.app[i], seed: self.pseed[i], bus_ok: self.kind[i] == KIND_BUS || self.bus_lane_ok[i], contra: self.moto_beh[i] & MB_CONTRA != 0, route, step, stop_step, stop_lane }
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
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] == NONE).map(|i| (i, self.t_spawn[i].to_bits(), self.targets[self.tgt[i] as usize].node)).collect()
    }

    fn remove_vehicle(&mut self, id: usize) {
        if self.corr[id] != NONE {
            self.corr_unlink(id);
        }
        let e = self.edge[id] as usize;
        if self.edge_veh[e].contains(&(id as u32)) {
            self.unlink(e, id);
        }
        self.alive[id] = false;
        if self.route[id] == NONE {
            self.to_zone[self.dest[id] as usize] -= 1;
        }
        self.free.push(id as u32);
        self.n_alive -= 1;
    }

    /// A aresta `e` tem algum veículo (espelho de `!edge_veh[e].is_empty()`, sem tocar na lista).
    #[inline]
    fn occ(&self, e: usize) -> bool {
        self.occ_bits[e / 64] & (1 << (e % 64)) != 0
    }

    fn insert_sorted(&mut self, edge: u32, id: usize) {
        let s = self.s[id];
        let list = &self.edge_veh[edge as usize];
        let pos = list.partition_point(|&j| self.s[j as usize] < s);
        self.edge_veh[edge as usize].insert(pos, id as u32);
        self.occ_bits[edge as usize / 64] |= 1 << (edge % 64);
    }

    /// Tira o veículo `id` da lista da aresta `e` (ele precisa estar nela).
    fn unlink(&mut self, e: usize, id: usize) {
        let list = &mut self.edge_veh[e];
        let p = list.iter().position(|&x| x as usize == id).unwrap();
        list.remove(p);
        if list.is_empty() {
            self.occ_bits[e / 64] &= !(1 << (e % 64));
        }
    }

    /// Monta a tabela compacta das conversões de saída por faixa (`sc`), na ordem de `lanes[l].conns`.
    fn build_sc(&mut self) {
        let mut sc = Vec::new();
        let mut off = Vec::with_capacity(self.nl + 1);
        for l in 0..self.nl {
            off.push(sc.len() as u32);
            for &c in &self.lanes[l].conns {
                let cn = &self.conns[c as usize - self.nl];
                let nx = cn.to as usize;
                let flags = if cn.bus_only { SC_BUS_ONLY } else { 0 } | if cn.moto_only { SC_MOTO_ONLY } else { 0 } | if self.lanes[nx].bus { SC_TO_BUS } else { 0 };
                sc.push(ScConn {
                    c,
                    to: cn.to,
                    pen: cn.pen + self.contra_pen(cn),
                    err_base: cn.pen + self.lrouter.time[nx],
                    dirseg: self.lanes[nx].dirseg,
                    lane_local: self.lane_local[nx],
                    zone: self.lane_zone[nx],
                    flags,
                });
            }
        }
        off.push(sc.len() as u32);
        self.sc = sc;
        self.sc_off = off;
        let mut lin = Vec::new();
        let mut lin_off = Vec::with_capacity(self.lane_in.len() + 1);
        for l in 0..self.lane_in.len() {
            lin_off.push(lin.len() as u32);
            lin.extend(self.lane_in[l].iter().map(|&o| {
                let oc = &self.conns[o as usize - self.nl];
                LinE { c: o, node: oc.node, from: oc.from, from_end: self.lanes[oc.from as usize].end_id, rank: oc.rank }
            }));
        }
        lin_off.push(lin.len() as u32);
        self.lin = lin;
        self.lin_off = lin_off;
        self.conn_to = self.conns.iter().map(|c| c.to).collect();
        self.conn_from = self.conns.iter().map(|c| c.from).collect();
        self.conn_node = self.conns.iter().map(|c| c.node).collect();
        self.conn_vturn = self.conns.iter().map(|c| c.vturn).collect();
        self.lane_hot = self.lanes.iter().map(|l| LaneHot { len: l.len, vfree: l.vfree, dirseg: l.dirseg, to_node: l.to_node, contra: l.contra }).collect();
    }

    /// Melhor conversão da faixa `l` para o destino: (custo até a zona, conversão). Carros ignoram faixas de ônibus.
    fn stay_cost(&self, drv: Drv, l: usize) -> (f32, u32) {
        if drv.route != NONE {
            return self.bus_stay(drv, l);
        }
        let tree = drv.tgt;
        let t = &self.targets[tree];
        let lane = &self.lanes[l];
        if lane.to_node == t.node {
            return (0.0, SINK);
        }
        if !drv.bus_ok && lane.bus {
            return (f32::INFINITY, NONE);
        }
        let z = t.zone as usize;
        let sigma = self.params.perception as f32;
        // quem pode seguir por dentro da zona até o ponto sempre prefere isso; senão vai rumo à zona
        let (mut local, mut other) = ((f32::INFINITY, NONE), (f32::INFINITY, NONE));
        let (lo, hi) = (self.sc_off[l] as usize, self.sc_off[l + 1] as usize);
        for k in &self.sc[lo..hi] {
            if k.flags & SC_BUS_ONLY != 0 || (k.flags & SC_MOTO_ONLY != 0 && !drv.contra) || (!drv.bus_ok && k.flags & SC_TO_BUS != 0) {
                continue;
            }
            let (c, nx) = (k.c, k.to as usize);
            // percepção: a próxima via parece um pouco mais rápida ou lenta para cada motorista (mesmo erro em
            // todas as faixas dela, para não inventar trocas de faixa entre faixas equivalentes)
            // a conversão imediata usa a penalidade fixa: o atraso medido nela inclui a espera do próprio veículo
            // (quem está na frente da fila); o atraso dos cruzamentos seguintes já está na árvore (`zone_d`)
            let pen = k.pen;
            let err = if sigma > 0.0 { sigma * noise(drv.seed, k.dirseg) * k.err_base } else { 0.0 };
            if k.zone == t.zone {
                let d = t.local[k.lane_local as usize];
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

    fn conflicts_of(&mut self, net: &Net, c: u32) -> std::ops::Range<usize> {
        let (a, n) = self.conf_rng[c as usize];
        if a != u32::MAX {
            return a as usize..(a + n) as usize;
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
            // todos os cruzamentos, não só o primeiro: duas curvas podem se cruzar duas vezes (ex.: esquerdas opostas,
            // em "S"). Viram uma área só, que cobre os dois pontos — com um ponto só, um lado freava num e batia no
            // outro; com dois separados, cada um podia ter a vez num deles e os dois ficavam se esperando
            let mut xs: Vec<(f32, f32, f32)> = Vec::new();
            for a in 0..pts_me.len() - 1 {
                for b in 0..pts_o.len() - 1 {
                    if let Some((t, u)) = seg_cross_t(pts_me[a], pts_me[a + 1], pts_o[b], pts_o[b + 1]) {
                        let s_me = ((cum_me[a] + t * (cum_me[a + 1] - cum_me[a])) * scale_me) as f32;
                        let s_o = ((cum_o[b] + u * (cum_o[b + 1] - cum_o[b])) * scale_o) as f32;
                        // quanto mais rasante o ângulo, mais longa a área comum às duas trajetórias
                        let (da, db) = (sub(pts_me[a + 1], pts_me[a]), sub(pts_o[b + 1], pts_o[b]));
                        let sin = (cross(da, db).abs() / (len_v(da) * len_v(db)).max(1e-9)) as f32;
                        let w = (OCC_HALF_W / sin.max(0.2)).clamp(OCC_HALF_W, 5.5);
                        xs.push((s_me, s_o, w));
                    }
                }
            }
            let found = !xs.is_empty();
            if found {
                let lo_me = xs.iter().map(|x| x.0).fold(f32::MAX, f32::min);
                let hi_me = xs.iter().map(|x| x.0).fold(f32::MIN, f32::max);
                let lo_o = xs.iter().map(|x| x.1).fold(f32::MAX, f32::min);
                let hi_o = xs.iter().map(|x| x.1).fold(f32::MIN, f32::max);
                let w = xs.iter().map(|x| x.2).fold(0.0, f32::max) + ((hi_me - lo_me).max(hi_o - lo_o)) / 2.0;
                out.push(Conflict { other: o, s_me: (lo_me + hi_me) / 2.0, s_other: (lo_o + hi_o) / 2.0, merge: false, w });
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
        let a = self.conf_flat.len();
        self.conf_flat.extend_from_slice(&out);
        self.conf_rng[c as usize] = (a as u32, out.len() as u32);
        a..a + out.len()
    }

    /// Veículo da conversão `cf.other` com a carroceria sobre a área do ponto de conflito que ainda não a terá
    /// liberado daqui a `t_arr` s (quando quem pergunta chegaria lá). Inclui quem já passou à faixa de saída com a
    /// traseira ainda na curva.
    ///
    /// `strict` (decisão antecipada, para o nó seguinte a uma faixa curta): quem segue para uma faixa de saída mais
    /// curta que ele não conta como "vai liberar a tempo" — parado nela, a traseira fica na área até ele passar pelo
    /// nó seguinte, e passar pode depender justamente de eu não entrar (ex.: retorno curto de mão dupla, cada um
    /// esperando na ponta da sua faixa com a traseira no caminho do outro). Conta também quem vai entrar na conversão
    /// concorrente rumo a essa faixa curta e já não consegue parar antes da linha.
    fn occ_by(&self, cf: &Conflict, t_arr: f32, me: usize, strict: bool) -> Option<usize> {
        let other = cf.other as usize;
        let (lo, hi) = (cf.s_other - cf.w, cf.s_other + cf.w);
        let to = self.conns[other - self.nl].to as usize;
        let clear = |j: usize, rear: f32| !(strict && self.short_link(to, j)) && (hi - rear) / self.v[j].max(0.05) > t_arr;
        for &j in &self.edge_veh[other] {
            let j = j as usize;
            let (f, r) = (self.s[j], self.s[j] - self.vlen[j]);
            if j != me && f >= lo && r <= hi && clear(j, r) {
                return Some(j);
            }
        }
        let olen = self.edge_len[other];
        for &j in self.edge_veh[to].iter().take(3) {
            let j = j as usize;
            let r = olen + self.s[j] - self.vlen[j];
            if r > hi {
                break;
            }
            if j != me && olen + self.s[j] >= lo && clear(j, r) {
                return Some(j);
            }
        }
        if strict {
            let fl = self.conns[other - self.nl].from as usize;
            if let Some(&j) = self.edge_veh[fl].last() {
                let j = j as usize;
                let d = self.lanes[fl].len - self.s[j];
                if j != me && self.choice[j] == other as u32 && self.enter_ok[j] && !self.ahead_held[j] && self.short_link(to, j) && self.v[j] * self.v[j] / (2.0 * self.bcomf[j]) + 1.0 > d {
                    return Some(j);
                }
            }
        }
        None
    }

    /// Ponto de conflito à frente ocupado na conversão `cc`, para o veículo `i` cuja frente está em `s_on` na
    /// conversão (negativo = ainda na faixa, antes da linha): obstáculo parado antes da área (folga, 0, quem ocupa).
    fn occ_obstacle(&mut self, net: &Net, i: usize, cc: u32, s_on: f32) -> Option<(f32, f32, usize)> {
        // impasse dentro do cruzamento (ex.: quatro "fechando" uns aos outros), na curva ou num trecho mais curto que o
        // veículo entre dois nós (como `inside_stuck` em `may_enter`): segue mesmo assim, e mantém a decisão depois de
        // arrancar (`occ_go`) — menos diante de quem já está saindo do mesmo jeito, para sair um de cada vez
        let stuck = self.occ_go[i] == cc + 1 || self.wait[i] > OCC_DEADLOCK && {
            let e = self.edge[i] as usize;
            e >= self.nl || self.short_link(e, i)
        };
        self.occ_skipped = false;
        let r = self.conflicts_of(net, cc);
        let cfs = &self.conf_flat[r];
        let v = self.v[i].max(1.0);
        let mut best: Option<(f32, f32, usize)> = None;
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
            if let Some(j) = self.occ_by(cf, t_arr, i, false) {
                if !stuck || self.occ_going(j) {
                    best = Some(((stop - s_on).max(0.0), 0.0, j));
                } else {
                    self.occ_skipped = true;
                }
                continue;
            }
            // outro veículo já na curva concorrente vai chegar à área antes de mim e ocupá-la enquanto eu passo:
            // freio se ainda dá (quem chega depois cede; empate pelo id). Última defesa contra a colisão quando a
            // brecha foi mal avaliada na linha.
            if s_on < 0.0 || stuck {
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
                    best = Some((d_stop.max(0.0), 0.0, j));
                    break;
                }
            }
        }
        best
    }

    /// `j` já está saindo de um impasse por ocupação pela conversão em que está (ou que vai pegar)?
    fn occ_going(&self, j: usize) -> bool {
        let e = self.edge[j] as usize;
        let cj = if e >= self.nl { e as u32 } else { self.choice[j] };
        self.occ_go[j] != 0 && self.occ_go[j] - 1 == cj
    }

    /// Faixa `l` curta demais para o veículo `i` parar inteiro nela (só a de um nó a outro do mesmo cruzamento).
    fn short_link(&self, l: usize, i: usize) -> bool {
        // a moto usa o comprimento de um carro: o trecho do canteiro continua sendo parte do cruzamento para ela
        let vlen = if self.kind[i] == KIND_MOTO { 4.5 } else { self.vlen[i] };
        self.lanes[l].len < vlen + 1.0
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
            let f = self.conn_to[c as usize - self.nl] as usize;
            let flen = self.edge_len[f];
            let d2 = dist + self.edge_len[c as usize] + flen;
            if flen >= 30.0 || d2 > d_check || self.occ(f) || self.occ(c as usize) {
                break;
            }
            let fits = flen >= self.vlen[i] + 1.0;
            let st = self.end_state[self.lanes[f].end_id as usize];
            if st != 0 && !self.short_link(f, i) {
                // semáforo ao fim de uma faixa curta mais adiante: fechado (vermelho, ou amarelo com espaço para parar) já
                // segura daqui; aberto, o nó dele não é avaliado de longe
                let v = self.v[i];
                if st == 1 || (st == 2 && v * v / (2.0 * self.bcomf[i].max(AMBER_DECEL)) < d2 - 1.0) {
                    return if fits { d2 } else { last_ok };
                }
                break;
            }
            let c2 = self.stay_cost(self.drv(i), f).1;
            if c2 == NONE || c2 == SINK {
                break;
            }
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
        // quem decidiu seguir no amarelo (sem distância para parar) mantém a decisão no começo do vermelho (vermelho geral)
        let committed = st == 1 && self.amber_go[id] == lane_id as u32 + 1 && self.red_age[end] + d_end.max(0.0) / v.max(1.0) < AMBER_COMMIT;
        // parcela de motos que avança o vermelho (fase 9): parada na linha há `MOTO_RED_WAIT` s, avalia o nó como se
        // não tivesse preferência sobre ninguém, com brecha folgada; depois de arrancar mantém a decisão (`MB_REDGO`)
        let red_go = st == 1 && !committed && !clearing && self.moto_beh[id] & MB_RED != 0 && lane_id == self.edge[id] as usize
            && (self.moto_beh[id] & MB_REDGO != 0 || (d_end < 5.0 && v < 0.5 && self.wait[id] >= MOTO_RED_WAIT));
        if st == 1 && !committed && !red_go {
            { REASON.with(|r| r.set(2)); return false; }
        }
        if st == 2 {
            // dá para parar freando a até 3 m/s² (taxa usual do MBST vol. V para o amarelo)?
            if v * v / (2.0 * b.max(AMBER_DECEL)) < d_end - 1.0 {
                { REASON.with(|r| r.set(3)); return false; }
            }
            if lane_id == self.edge[id] as usize {
                self.amber_go[id] = lane_id as u32 + 1;
            }
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
        let (me_idx, to_lane) = (cc as usize - self.nl, self.conn_to[cc as usize - self.nl]);
        // fila justa na saída: quem espera há mais tempo, vindo de outra faixa para a mesma faixa de saída, passa antes
        {
            let node = self.conns[me_idx].node;
            let my_wait = self.wait[id];
            // as conversões que chegam a `to_lane` (mesma ordem de `node_conns[node]`, filtrada por `to`)
            let (lo, hi) = (self.lin_off[to_lane as usize] as usize, self.lin_off[to_lane as usize + 1] as usize);
            let my_rank = self.conns[me_idx].rank;
            for oc in &self.lin[lo..hi] {
                let o = oc.c;
                if oc.node != node || oc.from as usize == lane_id {
                    continue;
                }
                if self.end_state[oc.from_end as usize] == 1 {
                    continue;
                }
                // a ordem de espera só desempata entre iguais: quem tem preferência (o anel da rotatória sobre quem
                // entra, a via principal sobre a secundária) não cede a vez a quem não tem
                if !signaled && my_rank < oc.rank {
                    continue;
                }
                if !self.occ(oc.from as usize) {
                    continue;
                }
                if let Some(&j) = self.edge_veh[oc.from as usize].last() {
                    let j = j as usize;
                    // só cede a quem vai mesmo entrar: liberado no nó (`enter_ok`) e não retido por um nó logo adiante —
                    // senão dois que se cedem a vez (direto ou em roda) ficam parados para sempre
                    if self.choice[j] == o && self.v[j] < 0.5 && self.enter_ok[j] && !self.ahead_held[j] && (self.wait[j] > my_wait + 2.0 || (self.wait[j] >= my_wait - 2.0 && j < id)) {
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
            // ainda no começo da curva e quase parado (fila); andando, é só o líder de quem vem atrás (IDM)
            if self.s[j] - self.vlen[j] < 2.0 && self.v[j] < 3.0 {
                { REASON.with(|r| r.set(5)); BLOCKER.with(|b| b.set(j as i64)); return false; }
            }
        }
        // contramão (fase 9): a moto só entra com a faixa real do lado vazia e ninguém entrando nela; quem vai entrar
        // nessa faixa espera a moto sair da contramão
        if self.lanes[to_lane as usize].contra && !self.contra_clear(to_lane as usize) {
            REASON.with(|r| r.set(12));
            return false;
        }
        if self.contra_by[to_lane as usize] != NONE && self.contra_busy(to_lane as usize) {
            REASON.with(|r| r.set(13));
            return false;
        }
        let relaxed = self.wait[id] > 10.0;
        let r = self.conflicts_of(net, cc);
        let conflicts = &self.conf_flat[r];
        // ponto de conflito ocupado por quem está dentro do cruzamento (inclusive parado, "fechando" o cruzamento):
        // ninguém entra, com qualquer sinal ou preferência — exceto quem já está dentro dele, num trecho mais curto que
        // o veículo, parado há muito tempo (impasse, como em `occ_obstacle`)
        let inside_stuck = lane_id == self.edge[id] as usize && self.short_link(lane_id, id) && (self.wait[id] > OCC_DEADLOCK || self.occ_go[id] == cc + 1);
        // avaliando de longe o nó seguinte a uma faixa curta (`ahead_stop_at`): ocupação sem crédito de "vai liberar"
        let ahead = lane_id != self.edge[id] as usize;
        if !inside_stuck {
            let a = (self.amax[id] * 0.6).max(0.5);
            for cf in conflicts.iter() {
                if cf.merge {
                    continue;
                }
                let dist = (d_end + cf.s_me - cf.w).max(0.0);
                let t_arr = (-v + (v * v + 2.0 * a * dist).sqrt()) / a;
                if let Some(j) = self.occ_by(cf, t_arr, id, ahead) {
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
        // saída da contramão parada há muito tempo: segue (quem ela segura não anda até ela sair)
        if self.lanes[lane_id].contra && self.wait[id] > OCC_DEADLOCK {
            return true;
        }
        let t_gap = if red_go { MOTO_RED_GAP } else if relaxed { 1.0 } else { 2.2 };
        let v_me = v.max(2.0);
        // tempo para percorrer `dist` partindo da velocidade atual e acelerando (parado, não se cruza a 2 m/s constantes)
        let a_me = (self.amax[id] * 0.6).max(0.5);
        let t_reach = |dist: f32| (-v + (v * v + 2.0 * a_me * dist.max(0.0)).sqrt()) / a_me;
        // paciência: quem espera muito só cede a quem realmente chega logo
        let patient = self.wait[id] > 30.0 && !red_go;
        for &cf in conflicts.iter() {
            let oi = cf.other as usize - self.nl;
            let (me, o) = (&self.conns[me_idx], &self.conns[oi]);
            // fusão sem placa nem semáforo e de mesma hierarquia: zíper (quem chega primeiro entra), sem prioridade fixa à direita
            let zip = cf.merge && !signaled && me.rank == o.rank;
            if !clearing && !red_go && (zip || !self.has_priority(me, o, signaled)) {
                // fusão sem semáforo em que eu tenho a preferência: não paro na linha por quem vem da via secundária
                // (ele é que cede); quem já está na conversão dele e chega antes ao ponto vira meu líder (`merge_ghost`)
                if cf.merge && !signaled && !zip {
                    continue;
                }
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
                        // quem saiu parado da linha agora há pouco acelera como eu (a 2 m/s constantes parecia longe)
                        let (vj, aj) = (self.v[j], (self.amax[j] * 0.6).max(0.5));
                        let t_j = (-vj + (vj * vj + 2.0 * aj * (cf.s_other - self.s[j]).max(0.0)).sqrt()) / aj;
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
                            // como na fila justa: quem tem preferência (anel, via principal) não cede, e ninguém cede a
                            // quem está retido por um nó logo adiante (não vai entrar)
                            let yields = signaled || me.rank >= o.rank;
                            if yields && !self.ahead_held[k] && self.choice[k] == cf.other && self.v[k] < 0.5 && self.wait[k] > self.wait[id] && ((slow && self.wait[k] > MERGE_WAIT) || self.wait[k] > MERGE_COURTESY) {
                                { REASON.with(|r| r.set(6)); BLOCKER.with(|b| b.set(k as i64)); return false; }
                            }
                        }
                    }
                }
                continue;
            }
            // chegada minha ao ponto de conflito
            let t_me = t_reach(d_end + cf.s_me).min((d_end + cf.s_me) / v_me);
            // fusão cedendo à preferencial (alça, rua que desemboca numa avenida): não é cruzamento. Quem entra acelera de
            // verdade pela conversão e só precisa que, quando chegar ao ponto, o seguidor da preferencial esteja longe o
            // bastante para o IDM dele não pedir mais que `MERGE_BRAKE` (como o `b_safe` do MOBIL); quem passa antes,
            // com folga, fica à frente
            let ins = if cf.merge && !red_go { Some(self.merge_arrival(id, me_idx, d_end + cf.s_me)) } else { None };
            let vlen_me = self.vlen[id];
            let blocks = |t_j: f32, j: usize| -> bool {
                let Some((t_m, v_m)) = ins else { return !(t_me + t_gap < t_j) };
                let vj = self.v[j].max(2.0);
                if t_j + (self.vlen[j] + 2.0) / vj + 1.0 < t_m {
                    return false;
                }
                let gap = vj * (t_j - t_m) - vlen_me;
                if gap <= 0.5 {
                    return true;
                }
                let (aj, bj) = (self.amax[j], self.bcomf[j]);
                let s_star = 2.0 + (vj * self.tgap[j] + vj * (vj - v_m) / (2.0 * (aj * bj).sqrt())).max(0.0);
                let free = 1.0 - (vj / self.desired_speed(j).max(0.5)).powi(4);
                let need = if relaxed { MERGE_BRAKE * 1.25 } else { MERGE_BRAKE };
                aj * (free - (s_star / gap).powi(2)) < -need
            };
            // veículos já na conversão concorrente
            for &j in &self.edge_veh[cf.other as usize] {
                let j = j as usize;
                let (sj, lj) = (self.s[j], self.vlen[j]);
                if sj - lj > cf.s_other + 1.5 {
                    continue; // já liberou o ponto
                }
                let t_j = if sj >= cf.s_other { 0.0 } else { (cf.s_other - sj) / self.v[j].max(2.0) };
                if blocks(t_j, j) {
                    { REASON.with(|r| r.set(7)); BLOCKER.with(|b| b.set(j as i64)); return false; }
                }
            }
            // os que estão chegando pela faixa que alimenta a conversão
            let fl = o.from as usize;
            // aproximação concorrente com sinal vermelho não disputa a brecha (só quem já está na conversão) — pelo sinal
            // dela, não o meu: quem atravessa um trecho curto entre dois nós avalia o segundo sem semáforo (`clearing`)
            if self.end_state[self.lanes[fl].end_id as usize] == 1 {
                continue;
            }
            for &j in self.edge_veh[fl].iter().rev().take(2) {
                let j = j as usize;
                // desempate anti-impasse (ex.: quatro aproximações cedendo à direita): quem espera há mais tempo passa
                if !signaled && self.v[j] < 0.5 && self.wait[j] + 3.0 < self.wait[id] {
                    continue;
                }
                // parado na linha e sem poder entrar (fila, saída cheia, nó logo adiante): não vai ocupar o cruzamento agora
                if self.v[j] < 0.5 && (!self.enter_ok[j] || self.ahead_held[j]) {
                    continue;
                }
                // quem está parado na linha há tempo está bloqueado (não vai ocupar o cruzamento já): não prende quem espera muito
                if !red_go && self.v[j] < 0.5 && self.wait[j] > 3.0 && self.wait[id] > 6.0 {
                    continue;
                }
                let d_j = self.lanes[fl].len - self.s[j];
                let t_j = (d_j + cf.s_other) / self.v[j].max(2.0);
                if patient && t_j > 4.0 {
                    continue;
                }
                if blocks(t_j, j) {
                    { REASON.with(|r| r.set(8)); BLOCKER.with(|b| b.set(j as i64)); return false; }
                }
            }
            // faixa concorrente curta (comum no OSM: cruzamentos de vários nós): quem vem rápido antes dela, na conversão
            // ou na faixa anterior rumo a ela, chega ao ponto de conflito em poucos segundos sem estar à vista
            let fl_len = self.lanes[fl].len;
            if fl_len < SHORT_APPROACH && !patient {
                let horizon = ins.map_or(t_me + t_gap, |(t_m, _)| t_m + 3.0);
                if let Some(j) = self.fast_before(fl, fl_len + cf.s_other, horizon) {
                    { REASON.with(|r| r.set(8)); BLOCKER.with(|b| b.set(j as i64)); return false; }
                }
            }
        }
        if red_go {
            self.moto_beh[id] |= MB_REDGO;
        }
        true
    }

    /// Chegada de `id` ao ponto de fusão a `dist` m, acelerando firme (0,8 × a máx.) até a velocidade da conversão
    /// `me_idx`: (tempo, velocidade ao chegar).
    fn merge_arrival(&self, id: usize, me_idx: usize, dist: f32) -> (f32, f32) {
        let v = self.v[id];
        let a = (self.amax[id] * 0.8).max(0.5);
        let vc = self.conns[me_idx].vturn.max(v).max(1.0);
        let dist = dist.max(0.0);
        let d_acc = (vc * vc - v * v) / (2.0 * a);
        if dist <= d_acc {
            let vm = (v * v + 2.0 * a * dist).sqrt();
            ((vm - v) / a, vm)
        } else {
            ((vc - v) / a + (dist - d_acc) / vc, vc)
        }
    }

    /// Veículo rápido (≥ `FAST_BEFORE` m/s) que vai entrar na faixa `fl` vindo de uma conversão que chega a ela (já na
    /// conversão ou na faixa anterior, com essa conversão escolhida e liberado) e percorre `extra` metros a partir do
    /// início de `fl` em menos de `t_max` s.
    fn fast_before(&self, fl: usize, extra: f32, t_max: f32) -> Option<usize> {
        for &p in &self.lane_in[fl] {
            let pc = &self.conns[p as usize - self.nl];
            let plen = self.edge_len[p as usize];
            for &j in &self.edge_veh[p as usize] {
                let j = j as usize;
                if self.v[j] >= FAST_BEFORE && (plen - self.s[j] + extra) / self.v[j] < t_max {
                    return Some(j);
                }
            }
            let pl = pc.from as usize;
            // aproximação anterior fechada: quem está nela vai parar
            if self.end_state[self.lanes[pl].end_id as usize] == 1 {
                continue;
            }
            if let Some(&j) = self.edge_veh[pl].last() {
                let j = j as usize;
                if self.choice[j] == p && self.enter_ok[j] && self.v[j] >= FAST_BEFORE && (self.lanes[pl].len - self.s[j] + plen + extra) / self.v[j] < t_max {
                    return Some(j);
                }
            }
        }
        None
    }

    // ---- passo

    /// Líder (folga até a traseira dele, velocidade) para o veículo `i`, ou None.
    fn leader(&mut self, net: &Net, i: usize, e: usize, k: usize, front_decides: bool) -> Option<(f32, f32)> {
        let list = &self.edge_veh[e];
        if k + 1 < list.len() {
            let j = list[k + 1] as usize;
            return Some((self.s[j] - self.vlen[j] - self.s[i], self.v[j]));
        }
        if e < self.nl {
            return self.lane_end_lead(net, i, e, front_decides);
        }
        self.conn_lead(net, i, e)
    }

    /// Líder virtual de quem é o primeiro da faixa `e` (ou a primeira moto do corredor, com `e` = faixa de referência):
    /// retenção na linha, conversão escolhida, fusão e faixa de destino curta com semáforo fechado.
    fn lane_end_lead(&mut self, net: &Net, i: usize, e: usize, front_decides: bool) -> Option<(f32, f32)> {
        let elen = self.edge_len[e];
        let rest = elen - self.s[i];
        {
            let cc = self.choice[i];
            if cc == SINK {
                return None; // sai da rede ao fim da faixa; sem líder virtual
            }
            if front_decides {
                self.ahead_held[i] = false;
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
            // parar na própria linha (e não mais adiante, ao fim de uma faixa curta que o comporte) = não vai entrar agora
            if front_decides {
                self.ahead_held[i] = ahead.map_or(false, |a| a.0 <= rest + 0.5);
            }
            // dentro da conversão escolhida
            let to = self.conn_to[cc as usize - self.nl];
            let clen = self.edge_len[cc as usize];
            let other = if self.occ(cc as usize) {
                let j = self.edge_veh[cc as usize][0] as usize;
                Some((rest + self.s[j] - self.vlen[j], self.v[j]))
            } else if let Some(&j) = self.edge_veh[to as usize].first() {
                let j = j as usize;
                Some((rest + clen + self.s[j] - self.vlen[j], self.v[j]))
            } else {
                None
            };
            let other = if rest < LOOKAHEAD { min_lead(other, self.merge_ghost(net, i, cc, rest)) } else { other };
            // faixa de destino curta que termina em semáforo fechado: a retenção dela também vale para quem ainda está antes
            let tl = &self.lanes[to as usize];
            if tl.len < 30.0 && !self.short_link(to as usize, i) {
                let st = self.end_state[tl.end_id as usize];
                if st == 1 || (st == 2 && self.v[i] * self.v[i] / (2.0 * self.bcomf[i].max(AMBER_DECEL)) < rest + clen + tl.len - 1.0) {
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
        }
    }

    /// Líder de quem está à frente na conversão `e`.
    fn conn_lead(&mut self, net: &Net, i: usize, e: usize) -> Option<(f32, f32)> {
        let rest = self.edge_len[e] - self.s[i];
        {
            let to = self.conns[e - self.nl].to;
            let other = self.edge_veh[to as usize].first().map(|&j| {
                let j = j as usize;
                (rest + self.s[j] - self.vlen[j], self.v[j])
            });
            let other = min_lead(other, self.merge_ghost(net, i, e as u32, -self.s[i]));
            let other = if self.corr_beside(to as usize) { min_lead(other, self.corr_lane_intruder(to as usize, i, -rest)) } else { other };
            // já dentro da conversão: a retenção da faixa de destino curta com semáforo fechado continua valendo
            // (senão quem freou antes da conversão volta a acelerar nela e chega à faixa curta sem espaço para parar)
            let tl = &self.lanes[to as usize];
            if tl.len < 30.0 && !self.short_link(to as usize, i) {
                let st = self.end_state[tl.end_id as usize];
                if st == 1 || (st == 2 && self.v[i] * self.v[i] / (2.0 * self.bcomf[i].max(AMBER_DECEL)) < rest + tl.len - 1.0) {
                    let stop = (rest + tl.len - 0.5, 0.0);
                    return Some(match other {
                        Some(o) if o.0 < stop.0 => o,
                        _ => stop,
                    });
                }
            }
            other
        }
    }

    /// Líder projetado numa fusão: quem está numa conversão que converge com a minha (`cc`, cuja linha está a `d0` m da
    /// minha frente; negativo = já estou nela) e chega antes ao ponto de fusão é seguido como se já estivesse à frente,
    /// a (minha distância ao ponto − a dele − o comprimento dele). Antes ele só virava líder ao chegar à faixa de saída.
    fn merge_ghost(&mut self, _net: &Net, i: usize, cc: u32, d0: f32) -> Option<(f32, f32)> {
        // as fusões de `conflicts_of(cc)`, na mesma ordem: conversões do mesmo nó para a mesma faixa de saída, vindas
        // de outra faixa (a lista achatada `lin` evita varrer todos os conflitos e tocar nas `ConnRt`)
        let ci = cc as usize - self.nl;
        let (to, node, from) = (self.conn_to[ci] as usize, self.conn_node[ci], self.conn_from[ci]);
        let d_me = d0 + self.edge_len[cc as usize];
        let mut best: Option<(f32, f32)> = None;
        for le in &self.lin[self.lin_off[to] as usize..self.lin_off[to + 1] as usize] {
            let o = le.c;
            if o == cc || le.node != node || le.from == from || !self.occ(o as usize) {
                continue;
            }
            let s_other = self.edge_len[o as usize];
            for &j in &self.edge_veh[o as usize] {
                let j = j as usize;
                let dj = s_other - self.s[j];
                if j == i || dj >= d_me {
                    continue;
                }
                let gap = d_me - dj - self.vlen[j];
                // ainda antes da linha, lado a lado com ele na projeção: quem decide é a entrada (`may_enter`), não o líder
                if d0 > 0.0 && gap < 1.0 {
                    continue;
                }
                // ele (mais rápido) libera o ponto antes de eu chegar: vai estar à frente e se afastando, não freio já
                if self.v[j] > self.v[i] && (dj + self.vlen[j]) / self.v[j].max(0.5) + 0.5 < d_me / self.v[i].max(2.0) {
                    continue;
                }
                best = min_lead(best, Some((gap, self.v[j])));
            }
        }
        best
    }

    fn desired_speed(&self, i: usize) -> f32 {
        let e = self.edge[i] as usize;
        if e < self.nl {
            self.lane_hot[e].vfree * self.vfac[i]
        } else {
            let ci = e - self.nl;
            (self.lane_hot[self.conn_from[ci] as usize].vfree * self.vfac[i]).min(self.conn_vturn[ci])
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
        if self.init_missing() > 0 {
            self.init_fill(net);
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
        for &(l, _) in &self.wants {
            self.want_lane[l as usize] = false;
        }
        self.wants.clear();
        for i in 0..self.alive.len() {
            // seta ligada há muito sem nova tentativa (a cada tentativa ela é renovada) não é mais pedido
            if self.alive[i] && self.mand[i] && self.blink_to[i] != NONE && (self.edge[i] as usize) < self.nl && (self.time as f32 - self.blink_t[i]) < 10.0 {
                self.wants.push((self.blink_to[i], i as u32));
            }
        }
        self.wants.sort_unstable();
        for &(l, _) in &self.wants {
            self.want_lane[l as usize] = true;
        }
        // 1) acelerações (com o estado do início do passo), percorrendo as arestas ocupadas em ordem crescente
        let n = self.alive.len();
        self.multi.clear();
        self.active.clear();
        for (w, &bits) in self.occ_bits.iter().enumerate() {
            let mut b = bits;
            while b != 0 {
                self.active.push((w * 64) as u32 + b.trailing_zeros());
                b &= b - 1;
            }
        }
        for ai in 0..self.active.len() {
            let e = self.active[ai] as usize;
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
                    let o = self.occ_obstacle(net, i, e as u32, self.s[i]);
                    if o.is_none() && self.occ_skipped {
                        self.occ_go[i] = e as u32 + 1;
                    }
                    o
                } else if front && self.enter_ok[i] && self.choice[i] != NONE && self.choice[i] != SINK && self.edge_len[e] - self.s[i] < 40.0 {
                    let rest = self.edge_len[e] - self.s[i];
                    let o = self.occ_obstacle(net, i, self.choice[i], -rest);
                    if o.is_none() && self.occ_skipped {
                        self.occ_go[i] = self.choice[i] + 1;
                    }
                    o
                } else {
                    None
                };
                if let Some(o) = occ.map(|o| (o.0, o.1)) {
                    self.held_box[i] = true;
                    if lead.map_or(true, |l| o.0 < l.0) {
                        lead = Some(o);
                    }
                }
                // moto entrando no corredor ao lado, ainda com a carroceria nesta faixa
                if e < self.nl && self.corr_beside(e) {
                    lead = min_lead(lead, self.corr_lane_intruder(e, i, self.s[i]));
                }
                if e < self.nl && self.want_lane[e] {
                    if let Some(c) = self.courtesy(i, e, k) {
                        if lead.map_or(true, |l| c.0 < l.0) {
                            lead = Some((c.0, c.1));
                        }
                    }
                }
                // parada no ponto (ônibus de linha, ou carro da parcela que para no ponto)
                if e < self.nl && (self.route[i] != NONE || self.stop_car[i] == 1) {
                    if let Some(o) = self.stop_lead(i, e) {
                        lead = min_lead(lead, Some(o));
                    }
                }
                let v = self.v[i];
                let v0 = self.desired_speed(i).max(0.5);
                let (a_max, b, t) = (self.amax[i], self.bcomf[i], self.tgap[i]);
                let free = 1.0 - (v / v0).powi(4);
                let mut interact = 0.0;
                if let Some((gap, vl)) = lead {
                    let gap = gap.max(0.1);
                    let s0 = self.s0_of(i);
            let s_star = s0 + (v * t + v * (v - vl) / (2.0 * (a_max * b).sqrt())).max(0.0);
                    interact = (s_star / gap).powi(2);
                }
                let mut a = (a_max * (free - interact)).clamp(-9.0, a_max);
                // antecipa a curva: chega à linha já na velocidade da conversão escolhida (freada confortável),
                // em vez de entrar rápido e frear forte dentro do cruzamento
                if e < self.nl {
                    let cc = self.choice[i];
                    if cc != NONE && cc != SINK {
                        let vt = self.conn_vturn[cc as usize - self.nl];
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
        let mut corr_act = std::mem::take(&mut self.corr_act);
        self.corr_active(&mut corr_act);
        if !corr_act.is_empty() {
            self.corridor_accels(net, &corr_act);
        }
        lap!(2);
        // 2) integração
        for i in 0..n {
            if !self.alive[i] {
                continue;
            }
            if self.in_bay[i] != 0 {
                // na baia: parado fora da faixa, embarcando ou esperando brecha para voltar
                self.v[i] = 0.0;
                self.metrics.bus_sample(0.0, dt);
                self.stop_update(i, dt);
                if self.dwell[i] <= 0.0 && self.in_bay[i] != 0 {
                    self.wait[i] += dt;
                }
                continue;
            }
            let v = self.v[i];
            let nv = (v + self.acc[i] * dt).max(0.0);
            self.s[i] += 0.5 * (v + nv) * dt;
            self.v[i] = nv;
            if (self.edge[i] as usize) < self.nl {
                let l = self.lane_hot[self.edge[i] as usize];
                if l.contra {
                    self.stats.contra_s += dt as f64;
                } else {
                    self.metrics.sample(l.dirseg as usize, nv, dt, l.to_node as usize, l.len - self.s[i]);
                }
            }
            if self.route[i] != NONE {
                self.metrics.bus_sample(nv, dt);
            } else if self.kind[i] == KIND_MOTO {
                self.metrics.moto_sample(nv, dt);
            }
            if self.route[i] != NONE || self.stop_car[i] == 1 {
                self.stop_update(i, dt);
            }
            if nv < 0.3 && self.dwell[i] > 0.0 {
                // embarcando: não é espera (a fila justa e o zíper dos cruzamentos olham `wait`)
                self.wait[i] = 0.0;
            } else if nv < 0.3 {
                let w0 = self.wait[i];
                self.wait[i] += dt;
                if self.held_box[i] {
                    self.stats.box_wait_s += dt as f64;
                }
                if self.edge[i] as usize >= self.nl {
                    if w0 < 2.0 && self.wait[i] >= 2.0 {
                        self.stats.box_stops += 1;
                        if self.overlap_log.is_some() {
                            let c = &self.conns[self.edge[i] as usize - self.nl];
                            let tl = &self.lanes[c.to as usize];
                            let line = format!("BOX veh={i} node={} osm={} kind={} s={:.1}/{:.1} to_len={:.1} to_sig={} to_n={} held_box={}", c.node, net.nodes[c.node as usize].osm_id, c.kind, self.s[i], c.len, tl.len, self.end_state[tl.end_id as usize], self.edge_veh[c.to as usize].len(), self.held_box[i]);
                            let mut line = line + &format!(" wait={:.0}\n          hist:", self.wait[i]);
                            for &(he, hs, hv, ok, sig) in self.hist.get(i).into_iter().flatten() {
                                let ed = if (he as usize) < self.nl { format!("L{he}") } else { format!("C{he}") };
                                line += &format!(" {ed} s={hs:.1} v={hv:.1} sig={sig}{}", if ok { "+" } else { "" });
                            }
                            self.overlap_log.as_mut().unwrap().push(line);
                        }
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
        self.corridor_gaps(&corr_act);
        lap!(4);

        // 4) mudanças de aresta
        let mut movers = std::mem::take(&mut self.movers);
        movers.clear();
        for i in 0..n {
            if self.alive[i] && self.s[i] >= self.edge_len[self.edge[i] as usize] {
                movers.push((self.edge[i], i as u32));
            }
        }
        movers.sort_by(|a, b| a.0.cmp(&b.0).then(self.s[b.1 as usize].partial_cmp(&self.s[a.1 as usize]).unwrap()));
        for &(_, vid) in &movers {
            self.advance(net, vid as usize);
        }
        self.movers = movers;

        self.enforce_gaps();
        self.corridor_gaps(&corr_act);
        self.corr_act = corr_act;
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
                let e = self.edge[i] as usize;
                let sig = if e < self.nl { self.end_state[self.lanes[e].end_id as usize] } else { 9 };
                h.push_back((self.edge[i], self.s[i], self.v[i], self.enter_ok[i], sig));
            }
        }
        if self.tick % 5 == 0 {
            self.check_overlaps(net);
        }
        lap!(5);

        // 5) trocas de faixa
        for i in 0..n {
            if self.alive[i] && (self.edge[i] as usize) < self.nl && self.cool[i] <= 0.0 && self.in_bay[i] == 0 {
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
            let queued: u64 = self.pend_gates.iter().map(|&g| self.pending[g as usize].len() as u64).sum();
            self.metrics.end_tick(self.time, &fs, self.n_alive, queued);
        }
    }

    /// Conta pares de veículos com as carrocerias sobrepostas dentro de um mesmo cruzamento (retângulos orientados,
    /// teste de eixos separados). Só entre conversões que não saem da mesma faixa nem chegam à mesma faixa (essas já
    /// se ordenam como líder/seguidor). Cada par conta uma vez enquanto continuar sobreposto.
    fn check_overlaps(&mut self, net: &Net) {
        let mut inside = std::mem::take(&mut self.inside);
        inside.clear();
        let mut now = std::mem::take(&mut self.overlap_now);
        now.clear();
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
            let r = match self.kind[i] { KIND_BUS => 1.2, KIND_MOTO => 0.5, _ => 0.85 };
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
            let r = self.conflicts_of(net, self.edge[a]);
            if self.conf_flat[r].iter().any(|c| c.other == self.edge[b] && !c.merge) {
                new_pairs.0 += 1;
            } else {
                new_pairs.1 += 1;
            }
        }
        self.stats.junction_overlaps += new_pairs.0;
        self.stats.junction_overlaps_geom += new_pairs.1;
        for (a, b) in logs {
            let (ea, eb) = (self.edge[a], self.edge[b]);
            let r = self.conflicts_of(net, ea);
            let cf = self.conf_flat[r].iter().find(|c| c.other == eb).copied();
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
                for (k, &(e, s, v, ok, _)) in self.hist.get(i).into_iter().flatten().enumerate() {
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
        self.overlap_now = std::mem::replace(&mut self.overlap_prev, now);
        self.inside = inside;
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
                if self.corr[i] != NONE {
                    if !self.corr_to_conn(i) {
                        return;
                    }
                    continue; // a faixa pode ter mudado: refaz com ela
                }
                let cc = self.choice[i];
                if cc == SINK {
                    if self.route[i] != NONE {
                        self.bus_finish(i);
                    } else {
                        self.stats.finished += 1;
                        let travel = self.time - self.t_spawn[i] as f64;
                        if self.kind[i] == KIND_MOTO {
                            self.stats.moto_finished += 1;
                            self.stats.moto_travel_sum += travel;
                        }
                        self.stats.travel_sum += travel;
                        self.metrics.trip_done(self.origin[i] as usize, self.dest[i] as usize, travel, self.t_free[i] as f64);
                    }
                    self.remove_vehicle(i);
                    return;
                }
                if cc == NONE && self.route[i] != NONE {
                    // ônibus sem conversão para o passo seguinte nesta faixa: espera no fim dela (a troca obrigatória
                    // e, depois de muito tempo, a conversão da faixa vizinha o levam adiante); nunca sai da rota
                    self.s[i] = self.edge_len[e] - 0.01;
                    self.v[i] = 0.0;
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
                if self.moto_beh[i] & MB_REDGO != 0 {
                    // avanço do vermelho de propósito (parcela da fase 9): não é furo por defeito
                    self.moto_beh[i] &= !MB_REDGO;
                    if self.end_state[end] == 1 {
                        self.stats.moto_red_go += 1;
                    }
                } else if self.end_state[end] == 1 && self.red_age[end] > 1.5 && !self.short_link(e, i) {
                    self.stats.red_runs += 1;
                    if self.red_log.len() < 40 { self.red_log.push((e as u32, self.v[i], self.choice[i], self.wait[i], self.red_age[end])); }
                    if self.overlap_log.is_some() {
                        let mut line = format!(
                            "RED veh={i} lane={e} len={:.1} node={} osm={} v={:.1} wait={:.1} red_age={:.1} enter_ok={} breaker={} box={} kind={}",
                            self.lanes[e].len, self.lanes[e].to_node, net.nodes[self.lanes[e].to_node as usize].osm_id, self.v[i], self.wait[i],
                            self.red_age[end], self.enter_ok[i], self.rule_breaker[i], self.box_blocker[i], self.conns[cc as usize - self.nl].kind
                        );
                        line += "\n          hist:";
                        for &(he, hs, hv, ok, sig) in self.hist.get(i).into_iter().flatten() {
                            let ed = if (he as usize) < self.nl { format!("L{he}") } else { format!("C{he}") };
                            line += &format!(" {ed} s={hs:.1} v={hv:.1} sig={sig}{}", if ok { "+" } else { "" });
                        }
                        self.overlap_log.as_mut().unwrap().push(line);
                    }
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
                self.unlink(e, i);
                self.edge[i] = cc;
                self.s[i] = over;
                self.insert_sorted(cc, i);
                if self.kind[i] == KIND_MOTO && self.lanes[e].dir == 1 {
                    // na conversão o deslocamento lateral da moto fica no sistema da viagem (ver `pose_dir`)
                    self.lat[i] = -self.lat[i];
                }
                self.sample_conn_delay(cc as usize - self.nl, self.time as f32 - self.t_due[i]);
                if !self.left_gate[i] {
                    self.left_gate[i] = true;
                    if self.route[i] == NONE {
                        let o = self.origin[i] as usize;
                        let st = &mut self.entry_stats[o];
                        st.0 += (self.t_spawn[i] - self.t_req[i]).max(0.0) as f64;
                        st.1 += (self.time as f32 - self.t_spawn[i]) as f64;
                        st.2 += 1;
                    }
                }
                self.multi.push(cc);
            } else {
                let c = &self.conns[e - self.nl];
                let to = c.to;
                let over = self.s[i] - self.edge_len[e];
                self.unlink(e, i);
                self.edge[i] = to;
                self.s[i] = over;
                self.insert_sorted(to, i);
                if self.kind[i] == KIND_MOTO && self.lanes[to as usize].dir == 1 {
                    self.lat[i] = -self.lat[i];
                }
                self.t_due[i] = self.time as f32 + self.free_left(to as usize, over);
                self.mand[i] = false;
                self.blink_to[i] = NONE;
                self.multi.push(to);
                if self.lanes[to as usize].contra {
                    self.stats.contra_entries += 1;
                } else {
                    self.metrics.entered(self.lanes[to as usize].dirseg as usize);
                }
                if self.route[i] != NONE {
                    self.bus_enter(i, to as usize);
                }
                self.stop_at[i] = f32::NAN;
                self.choose_conn(i);
            }
        }
    }

    /// Parado no fim da faixa sem conversão para a rota: aceita a saída que mais aproxima do destino (em linha reta,
    /// retorno só se não houver outra) e deixa o roteador refazer o caminho a partir dela.
    fn reroute_if_stuck(&mut self, net: &Net, i: usize) {
        if self.wait[i] <= 12.0 || self.route[i] != NONE {
            return;
        }
        let e = self.edge[i] as usize;
        let goal = net.nodes[self.targets[self.tgt[i] as usize].node as usize].pos;
        let mut best: Option<(f64, u32)> = None;
        for &c in &self.lanes[e].conns {
            let cn = &self.conns[c as usize - self.nl];
            if cn.bus_only || cn.moto_only || (!self.drv(i).bus_ok && self.lanes[cn.to as usize].bus) {
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
        if self.corr[i] != NONE {
            self.corr_update(i);
            return;
        }
        let e = self.edge[i] as usize;
        let rest = self.lanes[e].len - self.s[i];
        // tempo de viagem anormal (voltas sem fim): vira "infrator" e, no limite, desiste
        let elapsed = self.time as f32 - self.t_spawn[i];
        let tf = self.t_free[i];
        let bus = self.route[i] != NONE;
        if self.dwell[i] > 0.0 {
            self.cool[i] = 0.5;
            return;
        }
        // ônibus de linha não viram infratores nem desistem (as paradas alongam a viagem; travado, aparece nos gargalos)
        if !bus && elapsed > (3.0 * tf).max(900.0) {
            self.rule_breaker[i] = true;
        }
        if !bus && elapsed > (6.0 * tf).max(1800.0) {
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
            if !bus && !own.is_finite() && self.wait[i] > 90.0 {
                self.stats.lost += 1;
                self.remove_vehicle(i);
                return;
            }
        }
        // moto: corredor entre as faixas quando o trânsito ao lado está lento
        if self.kind[i] == KIND_MOTO && self.params.moto_corridor && self.try_corridor(i, own) {
            self.cool[i] = 1.0;
            return;
        }
        // ônibus com parada pendente neste passo: vai para a faixa dela (também no último passo, que termina em `SINK`)
        let stop_here = bus && self.drv(i).stop_step == self.bstep[i];
        // troca de faixa para a faixa de menor custo até o destino (conta o custo de trocar)
        if self.choice[i] != SINK || stop_here {
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
        let (my_lead, my_foll) = self.around(e, self.s[i], i);
        // atrás de quem está parado no ponto, ultrapassa também perto do fim da faixa (fora da linha contínua)
        let behind_stop = my_lead.is_some_and(|j| self.dwell[j] > 0.0) && rest > LC_SOLID_ZONE + 5.0;
        // e fica na faixa dela (sem ultrapassagem por conforto)
        if stop_here || ((self.choice[i] == SINK || rest < 60.0) && !behind_stop) {
            self.cool[i] = 1.5;
            return;
        }
        let (lo, hi) = self.lanes_of(e);
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
    /// Vão parado da IDM (m): a moto para mais perto (1 m) que carro e ônibus (2 m). O passo e as checagens de troca de
    /// faixa usam o mesmo valor; com valores diferentes, a moto parada a 1 m de quem quer entrar na frente dela parecia
    /// precisar de uma freada de ~9 m/s² e a entrada era recusada para sempre (impasse no fim do corredor).
    fn s0_of(&self, i: usize) -> f32 {
        if self.kind[i] == KIND_MOTO { 1.0 } else { 2.0 }
    }

    fn idm(&self, i: usize, lead: Option<(f32, f32)>) -> f32 {
        let v = self.v[i];
        let v0 = self.desired_speed(i).max(0.5);
        let (a_max, b, t) = (self.amax[i], self.bcomf[i], self.tgap[i]);
        let mut x = 1.0 - (v / v0).powi(4);
        if let Some((gap, vl)) = lead {
            let s_star = self.s0_of(i) + (v * t + v * (v - vl) / (2.0 * (a_max * b).sqrt())).max(0.0);
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
    fn courtesy(&self, i: usize, e: usize, k: usize) -> Option<(f32, f32, usize)> {
        let lo = self.wants.partition_point(|w| (w.0 as usize) < e);
        let list = &self.edge_veh[e];
        let mut best: Option<(f32, f32, usize)> = None;
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
            // a moto saindo do corredor cabe num vão menor (ela entra de lado, sem vir de trás)
            let min = if self.corr[r] != NONE { MIN_GAP } else { 1.0 };
            if gap > min && best.map_or(true, |b| gap < b.0) {
                best = Some((gap, self.v[r], r));
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
        !(self.lanes[cl].bus && self.kind[i] != KIND_BUS && !self.bus_lane_ok[i])
    }

    fn try_change(&mut self, i: usize, to: usize, forced: bool) -> bool {
        let e = self.edge[i] as usize;
        if !self.compatible(i, to) {
            self.lc_why[i] = 1;
            return false;
        }
        // moto na contramão ao lado da faixa de destino (fase 9)
        if self.contra_by[to] != NONE && self.contra_busy(to) {
            self.lc_why[i] = 7;
            return false;
        }
        // não atravessa por cima de moto no corredor entre as duas faixas
        let c = e.min(to);
        if self.corr_ok[c] && !self.corr_veh[c].is_empty() && self.corr_blocks(c, self.s[i], self.vlen[i], self.v[i]) {
            self.lc_why[i] = 6;
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
        self.unlink(e, i);
        self.blink_to[i] = NONE;
        self.mand[i] = false;
        self.edge[i] = to as u32;
        self.insert_sorted(to as u32, i);
        self.lat[i] += old_off - new_off;
        self.stop_at[i] = f32::NAN;
        self.choose_conn(i);
        self.stats.lane_changes += 1;
        true
    }

    // ---- geração de viagens

    /// Nascimentos devidos do par `i`→`j`: da matriz de carros (com ônibus e motos por `bus_share`/`moto_share`) ou,
    /// com `moto`, da matriz de motos (PRNG próprio no relógio de chegada).
    fn spawn_due(&mut self, i: usize, j: usize, moto: bool) {
        loop {
            let (due, rate) = if moto { (self.next_spawn_m[i][j], self.rate_m[i][j]) } else { (self.next_spawn[i][j], self.rate[i][j]) };
            if due > self.time {
                break;
            }
            if rate <= 0.0 || self.gates[i].is_empty() {
                if moto { self.next_spawn_m[i][j] = f64::INFINITY } else { self.next_spawn[i][j] = f64::INFINITY }
                break;
            }
            let gap = if moto { self.rng_moto.exp(rate) } else { 0.0 };
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
                if moto { self.next_spawn_m[i][j] += gap } else { self.next_spawn[i][j] += self.rng.exp(rate) }
                continue;
            }
            let kind = if moto {
                KIND_MOTO
            } else {
                // com linhas carregadas, os ônibus vêm só delas; com a matriz de motos, as motos vêm só dela
                let bus = if self.bus_plans.is_empty() { self.params.bus_share } else { 0.0 };
                let mot = if self.params.od_hourly_moto.is_empty() { self.params.moto_share } else { 0.0 };
                let u = self.rng.f() as f64;
                if u < bus / 100.0 { KIND_BUS } else if u < (bus + mot) / 100.0 { KIND_MOTO } else { KIND_CAR }
            };
            if !self.spawn_vehicle(gate, i as u16, j as u16, kind, self.time as f32, NONE) {
                if self.pending[gate as usize].len() < 200 {
                    if self.pending[gate as usize].is_empty() {
                        self.pend_gates.push(gate);
                    }
                    self.pending[gate as usize].push_back((i as u32, j as u16, kind, self.time as f32));
                    self.stats.queued += 1;
                }
            }
            if moto { self.next_spawn_m[i][j] += gap } else { self.next_spawn[i][j] += self.rng.exp(rate) }
        }
    }

    fn spawn(&mut self, net: &Net) {
        let _ = net;
        let nz = self.zone_ids.len();
        // filas de entrada (primeiro as pendentes)
        self.pend_gates.sort_unstable();
        for gi in 0..self.pend_gates.len() {
            let g = self.pend_gates[gi] as usize;
            while let Some(&(origin, dest, kind, t_req)) = self.pending[g].front() {
                if self.spawn_vehicle(g as u32, origin as u16, dest, kind, t_req, NONE) {
                    self.pending[g].pop_front();
                    self.stats.queued = self.stats.queued.saturating_sub(1);
                } else {
                    break;
                }
            }
        }
        let pending = &self.pending;
        self.pend_gates.retain(|&g| !pending[g as usize].is_empty());
        for i in 0..nz {
            if self.row_next[i] > self.time {
                continue; // nenhum par desta origem tem nascimento devido
            }
            for j in 0..nz {
                self.spawn_due(i, j, false);
            }
            self.row_next[i] = self.next_spawn[i].iter().copied().fold(f64::INFINITY, f64::min);
        }
        if !self.params.od_hourly_moto.is_empty() {
            for i in 0..nz {
                if self.row_next_m[i] > self.time {
                    continue;
                }
                for j in 0..nz {
                    self.spawn_due(i, j, true);
                }
                self.row_next_m[i] = self.next_spawn_m[i].iter().copied().fold(f64::INFINITY, f64::min);
            }
        }
        if !self.bus_plans.is_empty() {
            self.spawn_buses();
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
            let off = if self.corr[i] != NONE { self.corr_mid(self.corr[i] as usize) } else { l.off };
            let q = add(p, mul(right(t), (off + self.lat[i]) as f64));
            (q[0] as f32, q[1] as f32, (t[0] * sign) as f32, (t[1] * sign) as f32)
        } else {
            let c = &self.conns[e - self.nl];
            let u = (self.s[i] / c.len).clamp(0.0, 1.0);
            let p = bez8(&c.curve, u);
            let q = bez8(&c.curve, (u + 0.02).min(1.0));
            let p0 = bez8(&c.curve, (u - 0.02).max(0.0));
            let d = norm(sub(q, p0));
            // moto: deslocamento lateral que sobrou da faixa (ou do corredor) de chegada, no sistema da viagem
            let p = if self.kind[i] == KIND_MOTO && self.lat[i] != 0.0 { add(p, mul(right(d), self.lat[i] as f64)) } else { p };
            (p[0] as f32, p[1] as f32, d[0] as f32, d[1] as f32)
        }
    }

    fn pose(&self, net: &Net, i: usize) -> (f32, f32, f32) {
        let (x, y, dx, dy) = self.pose_dir(net, i);
        (x, y, dy.atan2(dx))
    }

    /// Estado dos veículos indexado pelo slot (o id do veículo é a posição): 6 floats por slot,
    /// [x, y, dx, dy, velocidade, comprimento, tipo] da frente do veículo (tipo: `KIND_CAR`, `KIND_BUS`, `KIND_MOTO`).
    /// Slot livre = tudo zero (comprimento 0).
    pub fn snapshot(&self, net: &Net, out: &mut Vec<f32>) {
        out.clear();
        out.resize(self.alive.len() * SNAP_STRIDE, 0.0);
        for i in 0..self.alive.len() {
            if !self.alive[i] {
                continue;
            }
            let (x, y, dx, dy) = self.pose_dir(net, i);
            out[i * SNAP_STRIDE..(i + 1) * SNAP_STRIDE].copy_from_slice(&[x, y, dx, dy, self.v[i], self.vlen[i], self.kind[i] as f32]);
        }
    }

    /// Rota restante do veículo `i` até a zona de destino: (pontos xy planos, id da zona, tipo, velocidade, espera).
    pub fn track(&self, net: &Net, i: usize) -> Option<(Vec<f32>, i64, u8, f32, f32)> {
        if i >= self.alive.len() || !self.alive[i] {
            return None;
        }
        let bus = self.route[i] != NONE;
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
        // motorista da projeção: para o ônibus, o passo do plano e a parada pendente avançam junto com o traçado
        // (o `bus_stay` só enxerga `PLAN_LOOK` passos à frente do passo dado)
        let mut d = self.drv(i);
        let mut d_stop = self.bstop[i] as usize;
        let advance = |d: &mut Drv, d_stop: &mut usize, l: usize| {
            if !bus {
                return;
            }
            let p = &self.bus_plans[d.route as usize];
            let k = d.step as usize;
            if k + 1 < p.dirseg.len() && p.dirseg[k + 1] == self.lanes[l].dirseg {
                d.step += 1;
            }
            while d.stop_step != NONE && d.stop_step < d.step {
                *d_stop += 1;
                (d.stop_step, d.stop_lane) = p.stops.get(*d_stop).map_or((NONE, NONE), |st| (st.step, st.lane));
            }
        };
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
                    let r = self.stay_cost(d, cl);
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
        // o ônibus percorre o plano inteiro (até o fim da linha); o carro, até o destino
        let max_guard = if bus { self.bus_plans[d.route as usize].dirseg.len() * 2 + 10 } else { 600 };
        let mut guard = 0;
        while let Some(l) = cur {
            guard += 1;
            if guard > max_guard {
                break;
            }
            advance(&mut d, &mut d_stop, l);
            pts.extend(lane_pts(&self.lanes[l], 0.0));
            if !bus && self.lanes[l].to_node == self.targets[dest].node {
                break;
            }
            // na faixa de menor custo (considerando o custo de trocar) segue-se a melhor conversão
            let (lo, hi) = self.lanes_of(l);
            let w = self.lane_w[l];
            let mut best = (f32::INFINITY, NONE);
            for cl in lo..hi {
                let r = self.stay_cost(d, cl);
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
        let zone = if bus { -1 } else { self.zone_ids[zone_dest] };
        Some((flat, zone, self.kind[i], self.v[i], self.wait[i]))
    }

    /// Relatório consolidado (JSON) da execução até agora.
    pub fn report(&self, net: &Net) -> serde_json::Value {
        let queued: u64 = self.pending.iter().map(|q| q.len() as u64).sum();
        let mut r = self.metrics.report(&self.zone_ids, &|n| net.nodes[n].osm_id, self.time - self.start_time, self.n_alive, queued, self.stats.spawned);
        // Atraso dos que ainda circulam (tempo decorrido menos o que levariam em fluxo livre até aqui) e dos
        // que esperam para entrar na rede: sem isso, um gargalo "melhora" a média só dos que conseguiram chegar.
        let mut alive_delay = 0.0f64;
        for i in 0..self.alive.len() {
            // os ônibus de linha têm as métricas próprias (`bus_report`)
            if !self.alive[i] || self.route[i] != NONE {
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
        if !self.bus_plans.is_empty() {
            r["bus"] = self.bus_report();
        }
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
            12 => "contramão: a faixa do lado tem quem vem de frente",
            13 => "moto na contramão ao lado da faixa de saída",
            _ => "",
        }
    }

    /// Primeiro da faixa, liberado para entrar (`may_enter`) mas parado: o que faz de líder virtual dele (o mais perto
    /// entre os obstáculos de `leader`, ocupação à frente e cortesia). (texto, quem segura ou -1)
    fn hold_of(&mut self, net: &Net, i: usize) -> Option<(String, i64)> {
        let e = self.edge[i] as usize;
        let cc = self.choice[i];
        if e >= self.nl || cc == NONE || cc == SINK || self.v[i] >= 0.5 {
            return None;
        }
        let k = self.edge_veh[e].iter().position(|&x| x as usize == i)?;
        let rest = self.edge_len[e] - self.s[i];
        let mut best: Option<(f32, String, i64)> = None;
        let mut offer = |gap: f32, txt: String, who: i64| {
            if best.as_ref().map_or(true, |b| gap < b.0) {
                best = Some((gap, txt, who));
            }
        };
        let ahead = self.ahead_stop_at(net, i, cc, rest);
        if ahead.is_finite() {
            let (code, who) = (REASON.with(|r| r.get()), BLOCKER.with(|b| b.get()));
            offer(ahead, format!("no nó seguinte, logo depois da faixa curta à frente: {}", Self::reason_text(code)), who);
        }
        let to = self.conns[cc as usize - self.nl].to as usize;
        let clen = self.edge_len[cc as usize];
        if let Some(&j) = self.edge_veh[cc as usize].first() {
            let j = j as usize;
            offer(rest + self.s[j] - self.vlen[j], "veículo à frente ainda na curva".into(), j as i64);
        } else if let Some(&j) = self.edge_veh[to].first() {
            let j = j as usize;
            offer(rest + clen + self.s[j] - self.vlen[j], "fila logo depois do cruzamento".into(), j as i64);
        }
        let tl = &self.lanes[to];
        if tl.len < 30.0 && !self.short_link(to, i) && self.end_state[tl.end_id as usize] == 1 {
            offer(rest + clen + tl.len - 0.5, "faixa curta à frente com semáforo fechado".into(), -1);
        }
        if rest < 40.0 {
            if let Some(o) = self.occ_obstacle(net, i, cc, -rest) {
                offer(o.0, "ponto de conflito à frente ocupado".into(), o.2 as i64);
            }
        }
        if let Some(c) = self.courtesy(i, e, k) {
            offer(c.0, "dá passagem a quem sinaliza para entrar na faixa".into(), c.2 as i64);
        }
        best.map(|b| (b.1, b.2))
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
        // líder (a moto no corredor não está na lista da faixa: o líder é a moto seguinte do corredor)
        let in_corr = self.corr[i] != NONE;
        let list = if in_corr { self.corr_veh[self.corr[i] as usize].clone() } else { self.edge_veh[e].clone() };
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
            let held = if ok { self.hold_of(net, i) } else { None };
            enter = json!({
                "ok": ok, "code": code, "reason": Self::reason_text(code), "blocker": if ok { -1 } else { blocker },
                "held": held.map(|(reason, by)| json!({ "reason": reason, "blocker": by })),
            });
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
        let bus = self.route[i] != NONE;
        let remaining = {
            let lane_ref = if on_lane { e } else { self.conns[e - self.nl].from as usize };
            if bus { self.bus_remaining(i, lane_ref) } else { self.dist(tgt, lane_ref, self.app[i]) }
        };
        let line = if bus {
            let p = &self.bus_plans[self.route[i] as usize];
            json!({
                "linha": self.bus_lines[p.line as usize].numero,
                "passo": self.bstep[i],
                "passos": p.dirseg.len(),
                "proximaParada": self.bstop[i],
                "paradas": p.stops.len(),
                "embarcando": self.dwell[i].max(0.0),
                "proximaParadaNome": self.next_stop_name(i),
                "tempoEmbarcando": self.bacc[i][0],
                "naBaia": self.in_bay[i] != 0,
            })
        } else {
            serde_json::Value::Null
        };
        // parcelas de comportamento da moto (fase 9), na ordem de `MB_*`
        let moto_parcels: Vec<&str> = [(MB_RED, "vermelho"), (MB_BUS, "faixaOnibus"), (MB_CONTRA, "contramao"), (MB_FAST, "corredorRapido")]
            .iter()
            .filter(|(b, _)| self.moto_beh[i] & b != 0)
            .map(|&(_, n)| n)
            .collect();
        let in_contra = (self.edge[i] as usize) < self.nl && self.lanes[self.edge[i] as usize].contra;
        json!({
            "id": i,
            "kind": match self.kind[i] { KIND_BUS => "ônibus", KIND_MOTO => "moto", _ => "carro" },
            "bus": line,
            "origin": if bus { json!(null) } else { json!(self.zone_ids[self.origin[i] as usize]) },
            "dest": if bus { json!(null) } else { json!(self.zone_ids[self.dest[i] as usize]) },
            "targetNode": if bus { json!(null) } else { json!(net.nodes[self.targets[tgt].node as usize].osm_id) },
            "where": if in_corr { "corredor" } else if on_lane { "faixa" } else { "dentro do cruzamento" },
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
            "motoParcels": moto_parcels,
            "inContra": in_contra,
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
        if self.corr[i] != NONE {
            return (format!("#{i} moto no corredor {} (faixa {e}) resto {:.1} v {:.1}", self.corr[i], self.edge_len[e] - self.s[i], self.v[i]), -1);
        }
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
        let r = self.conflicts_of(net, e as u32);
        for cf in self.conf_flat[r].iter() {
            if cf.merge || self.s[i] > cf.s_me - cf.w {
                continue;
            }
            if let Some(j) = self.occ_by(cf, 0.0, i, false) {
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
            // liberado mas parado: o que de fato o segura
            let held = if ok { self.hold_of(net, j) } else { None };
            let sg = &net.segs[self.lanes[e].seg as usize];
            out.push(json!({
                "id": j, "wait": w, "ok": ok, "reason": Self::reason_text(code), "code": code, "blocker": blocker,
                "held": held.map(|(reason, by)| json!({ "reason": reason, "blocker": by })),
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
/// O mais próximo de dois líderes (folga, velocidade).
fn min_lead(a: Option<(f32, f32)>, b: Option<(f32, f32)>) -> Option<(f32, f32)> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if y.0 < x.0 { y } else { x }),
        (x, None) => x,
        (None, y) => y,
    }
}

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
