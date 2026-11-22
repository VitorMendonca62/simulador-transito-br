//! Ônibus por linha: cada viagem do GTFS nasce no horário dela, segue o caminho encaixado (`transit::TrechoFit`) e
//! para nos pontos para embarque. O caminho é um plano por (passo, faixa) com o custo até o fim, no lugar da árvore
//! por zona dos carros; as regras de troca de faixa, cortesia e cruzamento são as mesmas.

use super::*;

/// Trecho de itinerário encaixado na rede, como a simulação o recebe (ver `Network::start_sim_ext`).
#[derive(Clone, Debug, Default)]
pub struct BusRoute {
    /// número da linha (agrupa as métricas)
    pub linha: String,
    pub padrao: String,
    /// trechos dirigidos (trecho da rede, 0 = from→to, 1 = contrário)
    pub caminho: Vec<(u32, u8)>,
    /// paradas em ordem
    pub paradas: Vec<BusStopIn>,
    /// segundos do dia em que cada viagem entra no trecho
    pub partidas: Vec<u32>,
}

/// Parada de um trecho encaixado: no passo `passo`, na faixa `lane`, a `s` metros do início do trecho da rede (no
/// sentido); `id`/`nome` da parada (a mesma parada aparece em várias linhas).
#[derive(Clone, Debug, Default)]
pub struct BusStopIn {
    pub passo: u32,
    pub lane: u32,
    pub s: f64,
    pub id: String,
    pub nome: String,
    pub baia: bool,
}

pub(super) struct StopRt {
    pub step: u32,
    pub lane: u32,
    /// posição na faixa (m desde o começo dela) onde a frente do ônibus para
    pub s: f32,
    /// índice da parada em `Sim::stop_ids` (a mesma em todas as linhas)
    pub id: u32,
    pub baia: bool,
}

/// Passagens de uma linha por uma parada: última passagem e intervalos (contagem, soma, soma dos quadrados).
#[derive(Default, Clone, Copy)]
pub(super) struct StopLineHw {
    pub last: f32,
    pub n: u32,
    pub sum: f64,
    pub sq: f64,
}

pub(super) struct BusPlan {
    pub line: u32,
    /// trecho dirigido (`seg * 2 + dir`) e primeira faixa (a mais à esquerda) de cada passo
    pub dirseg: Vec<u32>,
    pub lo: Vec<u32>,
    /// por passo e faixa: menor custo (s) até o fim do caminho saindo desta faixa, já com a troca de faixa no passo
    /// (`E` = min sobre as faixas j do passo de direto(j) + troca |l − j|); a faixa em si não conta
    pub after: Vec<Vec<f32>>,
    pub stops: Vec<StopRt>,
    pub length: f32,
    pub t_free: f32,
}

/// Métricas de uma linha (todas as viagens dos padrões dela).
#[derive(Default, Clone, Debug)]
pub struct BusLineStats {
    pub numero: String,
    pub trips: u32,
    pub travel_s: f64,
    pub dist_m: f64,
    /// tempo embarcando nas paradas e tempo parado fora delas (cruzamentos, fila)
    pub dwell_s: f64,
    pub stopped_s: f64,
    pub stops: u32,
    pub missed: u32,
    /// intervalo real entre ônibus do mesmo padrão em cada parada: soma, soma dos quadrados, contagem; comboios
    pub hw_n: u32,
    pub hw_sum: f64,
    pub hw_sq: f64,
    pub bunched: u32,
}

/// Perfil de dia útil da demanda de ônibus (fração do pico), usado sem matriz OD horária.
const BUS_HOUR: [f64; 24] = [0.05, 0.03, 0.02, 0.02, 0.1, 0.35, 0.8, 1.0, 0.85, 0.6, 0.55, 0.6, 0.65, 0.6, 0.55, 0.6, 0.75, 0.95, 0.9, 0.65, 0.45, 0.35, 0.2, 0.1];
/// segundos por passageiro embarcando (porta da frente, cartão)
const PAX_S: f32 = 2.5;
/// o ponto comporta dois ônibus: quem para até aqui antes da posição da parada também embarca
const STOP_REACH: f32 = 18.0;
/// carros da parcela que param no ponto: tempo parado (s)
const CAR_STOP: (f32, f32) = (10.0, 30.0);
/// trava de segurança: o caminho de cada ônibus tem no máximo 4 passos à frente olhados de uma vez (`ahead_stop_at`)
const PLAN_LOOK: u32 = 3;
/// peso, em trocas de faixa, de cada faixa entre o ônibus e a faixa da parada pendente
const STOP_LANE_K: f32 = 10.0;
/// ponto numa faixa mais curta que `SHORT_STOP + BUS_LEN` vai para o fim do passo anterior
const SHORT_STOP: f32 = 10.0;
const BUS_LEN: f32 = 12.0;
/// fora da faixa do ponto, o ônibus espera brecha para entrar nela parado antes do ponto até este tempo (s)
const STOP_MERGE_WAIT: f32 = 30.0;
/// ônibus que cabem na baia e deslocamento lateral (m) dela em relação ao centro da faixa
const BAY_CAP: u8 = 2;
const BAY_OFF: f32 = 3.0;

impl Sim {
    /// Monta os planos, as partidas e o índice de paradas por faixa. Chamado no fim de `Sim::new_ext`.
    pub(super) fn init_buses(&mut self, net: &Net, routes: &[BusRoute]) {
        // fator da hora: geração total da OD em cada hora ÷ a do pico; sem OD, perfil fixo
        let h = &self.params.od_hourly;
        self.bus_hour = if h.len() == 24 {
            let tot: Vec<f64> = h.iter().map(|m| m.values().sum()).collect();
            let peak = tot.iter().copied().fold(0.0, f64::max);
            if peak > 0.0 { std::array::from_fn(|k| tot[k] / peak) } else { BUS_HOUR }
        } else {
            BUS_HOUR
        };
        let lane_range = |seg: u32, d: u8| {
            let s = &net.segs[seg as usize];
            if d == 0 { (s.lane_base, s.lane_base + s.lf as u32) } else { (s.lane_base + s.lf as u32, s.lane_base + s.lf as u32 + s.lb as u32) }
        };
        let mut lines: Vec<String> = Vec::new();
        let start = self.start_time;
        for r in routes {
            if r.caminho.is_empty() {
                continue;
            }
            let line = match lines.iter().position(|l| *l == r.linha) {
                Some(k) => k,
                None => {
                    lines.push(r.linha.clone());
                    lines.len() - 1
                }
            } as u32;
            let n = r.caminho.len();
            let (mut lo, mut hi, mut dirseg) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
            for &(seg, d) in &r.caminho {
                let (a, b) = lane_range(seg, d);
                lo.push(a);
                hi.push(b);
                dirseg.push(seg * 2 + d as u32);
            }
            // de trás para frente: direto(l) = melhor conversão para o passo seguinte (penalidade + tempo da faixa de
            // chegada + `after` dela); after(l) = min_j direto(j) + troca |l − j|
            let mut after: Vec<Vec<f32>> = vec![Vec::new(); n];
            for k in (0..n).rev() {
                let (a, b) = (lo[k] as usize, hi[k] as usize);
                let direct: Vec<f32> = (a..b)
                    .map(|l| {
                        if k + 1 == n {
                            return 0.0;
                        }
                        let mut best = f32::INFINITY;
                        for &c in &self.lanes[l].conns {
                            let cn = &self.conns[c as usize - self.nl];
                            let m = cn.to as usize;
                            if self.lanes[m].dirseg == dirseg[k + 1] {
                                let off = m - lo[k + 1] as usize;
                                best = best.min(cn.pen + self.lrouter.time[m] + after[k + 1][off]);
                            }
                        }
                        best
                    })
                    .collect();
                after[k] = (a..b).map(|l| (a..b).map(|j| direct[j - a] + self.lane_w[l] * (l as i32 - j as i32).unsigned_abs() as f32).fold(f32::INFINITY, f32::min)).collect();
            }
            let t_free = (lo[0]..hi[0]).map(|l| self.lrouter.time[l as usize] + after[0][(l - lo[0]) as usize]).fold(f32::INFINITY, f32::min);
            if !t_free.is_finite() {
                continue; // o encaixe passa por conversões que a simulação não tem (não deveria acontecer)
            }
            let mut stops: Vec<StopRt> = r
                .paradas
                .iter()
                .filter(|p| (p.passo as usize) < n && p.lane >= lo[p.passo as usize] && p.lane < hi[p.passo as usize])
                .map(|p| {
                    let (step, lane, s, baia) = (p.passo, p.lane, p.s, p.baia);
                    let id = match self.stop_id_of.get(&p.id) {
                        Some(&k) => k,
                        None => {
                            self.stop_ids.push(p.id.clone());
                            self.stop_names.push(p.nome.clone());
                            self.stop_id_of.insert(p.id.clone(), self.stop_ids.len() as u32 - 1);
                            self.stop_ids.len() as u32 - 1
                        }
                    };
                    let (seg, d) = r.caminho[step as usize];
                    let sg = &net.segs[seg as usize];
                    let cut = if d == 0 { sg.cut_from } else { sg.cut_to };
                    let len = self.lanes[lane as usize].len;
                    let s = (s - cut) as f32;
                    // ponto numa faixa curta (o ônibus chega nela já sem distância para parar): passa para o fim do
                    // passo anterior, na faixa da direita, a poucos metros
                    if step > 0 && len < SHORT_STOP + BUS_LEN {
                        let k = step as usize - 1;
                        let pl = hi[k] - 1;
                        let plen = self.lanes[pl as usize].len;
                        return StopRt { step: step - 1, lane: pl, s: (plen - 3.0).max(0.5), id, baia };
                    }
                    // a frente para na posição da parada, sem encostar no nó do fim da faixa
                    let top = (len - 3.0).max(0.5);
                    StopRt { step, lane, s: s.clamp(12f32.min(len * 0.5).min(top), top), id, baia }
                })
                .collect();
            stops.sort_by(|a, b| a.step.cmp(&b.step).then(a.s.total_cmp(&b.s)));
            for st in &stops {
                self.lane_stops.entry(st.lane).or_default().push(st.s);
            }
            let length: f32 = r.caminho.iter().map(|&(s, d)| {
                let (a, _) = lane_range(s, d);
                self.lanes[a as usize].len
            }).sum();
            let pi = self.bus_plans.len() as u32;
            // partidas a partir do início (a rede começa vazia), com uma variação sorteada por viagem; as de antes
            // do início valem no dia seguinte
            let mut deps: Vec<u32> = r.partidas.clone();
            deps.sort_unstable();
            for (k, &p) in deps.iter().enumerate() {
                let sched = if k > 0 { (p - deps[k - 1]) as f32 } else if deps.len() > 1 { (deps[1] - deps[0]) as f32 } else { 0.0 };
                let jit = self.params.bus_jitter as f32 * noise(self.params.seed as u32 ^ 0xB05, pi.wrapping_mul(7919).wrapping_add(k as u32));
                let day = if (p as f64) < start { 86400.0 } else { 0.0 };
                let t = (p as f64 + day + jit as f64).max(start);
                self.bus_deps.push((t, pi, sched));
            }
            self.stop_last.push(vec![f32::NEG_INFINITY; stops.len()]);
            self.bus_plans.push(BusPlan { line, dirseg, lo, after, stops, length, t_free });
        }
        for v in self.lane_stops.values_mut() {
            v.sort_by(f32::total_cmp);
        }
        self.bus_deps.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.bus_lines = lines.into_iter().map(|numero| BusLineStats { numero, ..Default::default() }).collect();
    }

    /// Ônibus cuja partida chegou: nasce no começo do 1º passo do caminho, ou espera numa fila própria.
    pub(super) fn spawn_buses(&mut self) {
        while self.bus_next < self.bus_deps.len() && self.bus_deps[self.bus_next].0 <= self.time {
            let (_, p, h) = self.bus_deps[self.bus_next];
            self.bus_queue.push_back((p, self.time as f32, h));
            self.bus_next += 1;
        }
        let mut k = 0;
        while k < self.bus_queue.len() {
            let (p, t_req, h) = self.bus_queue[k];
            let lane = self.bus_plans[p as usize].lo[0];
            if self.spawn_vehicle(lane, 0, u16::MAX, KIND_BUS, t_req, p) {
                let id = self.last_spawn;
                self.bacc[id] = [0.0, 0.0, h];
                // faixa inicial curta: o ônibus já nasce sobre o primeiro ponto (o terminal) e embarca ali
                if let Some(st) = self.bus_plans[p as usize].stops.first() {
                    if st.step == 0 && st.lane == self.edge[id] && st.s < self.s[id] + 3.0 {
                        self.v[id] = 0.0;
                        self.start_boarding(id);
                    }
                }
                self.stats.bus_spawned += 1;
                self.bus_queue.remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// Passo do plano em que está a faixa `l` (a partir do passo atual do motorista), se estiver no caminho.
    fn bus_step_of(&self, drv: Drv, l: usize) -> Option<usize> {
        let p = &self.bus_plans[drv.route as usize];
        let d = self.lanes[l].dirseg;
        let k0 = drv.step as usize;
        (k0..(k0 + PLAN_LOOK as usize + 1).min(p.dirseg.len())).find(|&k| p.dirseg[k] == d)
    }

    /// Custo do ônibus saindo da faixa `l` do passo `k` (sem o tempo da própria faixa). Com parada pendente no passo,
    /// vale o custo a partir da faixa da parada mais um peso grande por faixa de distância até ela: o ônibus vai para
    /// a faixa da parada antes de seguir (não é custo de tempo, é a restrição de parar).
    fn bus_after(&self, drv: Drv, k: usize, l: usize) -> f32 {
        let p = &self.bus_plans[drv.route as usize];
        if drv.stop_step as usize == k {
            let ls = drv.stop_lane as usize;
            STOP_LANE_K * self.lane_w[l] * (l as i32 - ls as i32).unsigned_abs() as f32 + p.after[k][ls - p.lo[k] as usize]
        } else {
            p.after[k][l - p.lo[k] as usize]
        }
    }

    /// `stay_cost` do ônibus: só conversões para o passo seguinte do caminho; no último passo, sai ao fim da faixa.
    pub(super) fn bus_stay(&self, drv: Drv, l: usize) -> (f32, u32) {
        let Some(k) = self.bus_step_of(drv, l) else { return (f32::INFINITY, NONE) };
        let p = &self.bus_plans[drv.route as usize];
        if k + 1 == p.dirseg.len() {
            // fim do caminho; com parada pendente, a faixa vale pela distância até a da parada
            return (if drv.stop_step as usize == k { self.bus_after(drv, k, l) } else { 0.0 }, SINK);
        }
        let mut best = (f32::INFINITY, NONE);
        for &c in &self.lanes[l].conns {
            let cn = &self.conns[c as usize - self.nl];
            let m = cn.to as usize;
            if self.lanes[m].dirseg != p.dirseg[k + 1] {
                continue;
            }
            let d = cn.pen + self.lrouter.time[m] + self.bus_after(drv, k + 1, m);
            if d < best.0 {
                best = (d, c);
            }
        }
        // parada pendente neste passo: a faixa vale pela distância até a da parada (a conversão segue a desta faixa)
        if drv.stop_step as usize == k {
            best.0 = self.bus_after(drv, k, l);
        }
        best
    }

    /// Ao entrar na faixa `to` vindo de uma conversão: avança o passo do caminho.
    pub(super) fn bus_enter(&mut self, i: usize, to: usize) {
        let p = &self.bus_plans[self.route[i] as usize];
        let k = self.bstep[i] as usize;
        if k + 1 < p.dirseg.len() && p.dirseg[k + 1] == self.lanes[to].dirseg {
            self.bstep[i] += 1;
        }
    }

    /// Ponto em que o veículo `i`, na faixa `e`, vai parar: a próxima parada do ônibus (se ele está na faixa dela) ou o
    /// ponto de ônibus escolhido pelo carro da parcela que para no ponto.
    fn stop_target(&mut self, i: usize, e: usize) -> Option<f32> {
        let r = self.route[i];
        if r != NONE {
            let p = &self.bus_plans[r as usize];
            let st = p.stops.get(self.bstop[i] as usize)?;
            // fora da faixa do ponto (sem conversão direta para ela): para antes dele e espera brecha para entrar
            // (antes da linha contínua do fim da faixa, onde não se troca de faixa)
            if st.step != self.bstep[i] {
                return None;
            }
            if st.lane as usize == e {
                return Some(st.s);
            }
            return (self.wait[i] < STOP_MERGE_WAIT).then(|| st.s.min(self.lanes[e].len - LC_SOLID_ZONE - 2.0).max(0.5));
        }
        if self.stop_car[i] != 1 {
            return None;
        }
        if self.stop_at[i].is_nan() {
            // o primeiro ponto da faixa que ainda dá para alcançar freando com conforto
            let need = self.v[i] * self.v[i] / (2.0 * self.bcomf[i]) + 5.0;
            let s = self.s[i];
            let at = self.lane_stops.get(&(e as u32)).and_then(|v| v.iter().copied().find(|&x| x - s > need))?;
            self.stop_at[i] = at;
        }
        Some(self.stop_at[i])
    }

    /// Obstáculo virtual (parado) na parada: o IDM freia até ali; a frente fica na posição da parada.
    pub(super) fn stop_lead(&mut self, i: usize, e: usize) -> Option<(f32, f32)> {
        let at = self.stop_target(i, e)?;
        let gap = at + 2.0 - self.s[i];
        (gap > -1.0).then_some((gap.max(0.1), 0.0))
    }

    /// Depois da integração: começa, conta e encerra o embarque; parada que ficou para trás é contada como perdida.
    pub(super) fn stop_update(&mut self, i: usize, dt: f32) {
        let e = self.edge[i] as usize;
        let bus = self.route[i] != NONE;
        if self.in_bay[i] != 0 && self.dwell[i] <= 0.0 {
            self.bay_merge(i);
            return;
        }
        if self.dwell[i] > 0.0 {
            self.dwell[i] -= dt;
            if self.dwell[i] <= 0.0 {
                self.dwell[i] = 0.0;
                if bus {
                    self.bstop[i] += 1;
                } else {
                    self.stop_car[i] = 2;
                    self.stop_at[i] = f32::NAN;
                }
            }
            return;
        }
        if bus {
            if self.v[i] < 0.3 {
                self.bacc[i][1] += dt;
            }
            // paradas que ficaram para trás: passo já percorrido, ou a frente além do ponto (em outra faixa ou não)
            loop {
                let p = &self.bus_plans[self.route[i] as usize];
                let Some(st) = p.stops.get(self.bstop[i] as usize) else { break };
                let passed = st.step < self.bstep[i] || (st.step == self.bstep[i] && (e >= self.nl || self.s[i] > st.s + 3.0));
                if !passed {
                    break;
                }
                let line = p.line as usize;
                self.bstop[i] += 1;
                self.stats.bus_stops_missed += 1;
                self.bus_lines[line].missed += 1;
            }
        }
        if e >= self.nl || self.v[i] >= 0.3 {
            return;
        }
        let Some(at) = self.stop_target(i, e) else { return };
        let d = at - self.s[i];
        if d > STOP_REACH || d < -3.0 {
            return;
        }
        if bus {
            if self.bus_plans[self.route[i] as usize].stops[self.bstop[i] as usize].lane as usize == e {
                self.start_boarding(i);
            }
        } else {
            self.dwell[i] = self.rng.range(CAR_STOP.0, CAR_STOP.1);
            self.stats.cars_at_bus_stop += 1;
        }
    }

    /// Começa o embarque do ônibus `i` na próxima parada dele.
    fn start_boarding(&mut self, i: usize) {
        {
            let r = self.route[i] as usize;
            let hour = (self.time / 3600.0).rem_euclid(24.0);
            let (h0, f) = (hour.floor() as usize % 24, hour - hour.floor());
            let fac = self.bus_hour[h0] * (1.0 - f) + self.bus_hour[(h0 + 1) % 24] * f;
            let pax = self.poisson(self.params.bus_pax_peak * fac);
            let (lo, hi) = (self.params.bus_dwell_min as f32, self.params.bus_dwell_max.max(self.params.bus_dwell_min) as f32);
            let t = (lo + PAX_S * pax as f32).clamp(lo, hi);
            // tempo de embarque + 1 s de seta antes de voltar ao fluxo
            self.dwell[i] = t + LC_BLINK;
            self.bacc[i][0] += t + LC_BLINK;
            self.stats.bus_stops += 1;
            self.stats.bus_dwell_s += t as f64;
            let k = self.bstop[i] as usize;
            let line = self.bus_plans[r].line as usize;
            let now = self.time as f32;
            let last = self.stop_last[r][k];
            let ls = &mut self.bus_lines[line];
            ls.stops += 1;
            if last.is_finite() {
                let hw = (now - last) as f64;
                ls.hw_n += 1;
                ls.hw_sum += hw;
                ls.hw_sq += hw * hw;
                if self.bacc[i][2] > 0.0 && (hw as f32) < 0.25 * self.bacc[i][2] {
                    ls.bunched += 1;
                }
            }
            self.stop_last[r][k] = now;
            let (sid, baia) = (self.bus_plans[r].stops[k].id, self.bus_plans[r].stops[k].baia);
            let h = self.stop_hw.entry((sid, line as u32)).or_insert(StopLineHw { last: f32::NEG_INFINITY, ..Default::default() });
            if h.last.is_finite() {
                let hw = (now - h.last) as f64;
                h.n += 1;
                h.sum += hw;
                h.sq += hw * hw;
            }
            h.last = now;
            // baia: sai da faixa para embarcar (quem vem atrás passa); cheia, embarca na faixa
            let e = self.edge[i] as usize;
            if baia && e < self.nl && *self.bay_occ.get(&sid).unwrap_or(&0) < BAY_CAP {
                *self.bay_occ.entry(sid).or_insert(0) += 1;
                self.unlink(e, i);
                self.in_bay[i] = sid + 1;
                self.v[i] = 0.0;
                self.acc[i] = 0.0;
                let l = &self.lanes[e];
                self.lat[i] = if l.dir == 0 { BAY_OFF } else { -BAY_OFF };
            }
        }
    }

    /// Ônibus na baia, embarque terminado: volta à faixa quando há brecha (regra da fusão: o seguidor não precisa frear
    /// mais que `MERGE_BRAKE`; vão à frente para arrancar).
    fn bay_merge(&mut self, i: usize) {
        let e = self.edge[i] as usize;
        let (ld, fl) = self.around(e, self.s[i], i);
        if ld.is_some_and(|j| self.s[j] - self.vlen[j] - self.s[i] < 3.0) {
            return;
        }
        if let Some(f) = fl {
            let gap = self.s[i] - self.vlen[i] - self.s[f];
            if gap < 1.0 || self.idm(f, Some((gap, 0.0))) < -MERGE_BRAKE {
                return;
            }
        }
        let sid = self.in_bay[i] - 1;
        if let Some(o) = self.bay_occ.get_mut(&sid) {
            *o = o.saturating_sub(1);
        }
        self.in_bay[i] = 0;
        self.insert_sorted(e as u32, i);
        // o `lat` volta a 0 aos poucos (saída da baia desenhada como troca de faixa)
    }

    /// Ônibus chegou ao fim do caminho.
    pub(super) fn bus_finish(&mut self, i: usize) {
        let p = &self.bus_plans[self.route[i] as usize];
        let ls = &mut self.bus_lines[p.line as usize];
        ls.trips += 1;
        ls.travel_s += self.time - self.t_spawn[i] as f64;
        ls.dist_m += p.length as f64;
        ls.dwell_s += self.bacc[i][0] as f64;
        ls.stopped_s += self.bacc[i][1].max(0.0) as f64;
        // paradas que sobraram (não alcançadas até o fim)
        let left = p.stops.len().saturating_sub(self.bstop[i] as usize) as u32;
        ls.missed += left;
        self.stats.bus_stops_missed += left as u64;
        self.stats.bus_finished += 1;
    }

    /// Tempo livre restante (s) do ônibus a partir da faixa `l` (para o atraso).
    pub(super) fn bus_remaining(&self, i: usize, l: usize) -> f32 {
        let drv = self.drv(i);
        match self.bus_step_of(drv, l) {
            Some(k) => self.lrouter.time[l] + self.bus_plans[drv.route as usize].after[k][l - self.bus_plans[drv.route as usize].lo[k] as usize],
            None => 0.0,
        }
    }

    fn poisson(&mut self, lambda: f64) -> u32 {
        if lambda <= 0.0 {
            return 0;
        }
        let l = (-lambda.min(60.0)).exp();
        let (mut k, mut p) = (0u32, 1.0f64);
        loop {
            p *= self.rng.f() as f64;
            if p <= l {
                return k;
            }
            k += 1;
        }
    }

    /// Ônibus em circulação, viagens concluídas e velocidade comercial média (km/h) das concluídas.
    pub fn bus_summary(&self) -> (usize, u64, f64) {
        let active = (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] != NONE).count();
        let (d, t) = self.bus_lines.iter().fold((0.0, 0.0), |a, l| (a.0 + l.dist_m, a.1 + l.travel_s));
        (active, self.stats.bus_finished, if t > 0.0 { d / t * 3.6 } else { 0.0 })
    }

    /// Relatório por linha (JSON): viagens, tempo de viagem, velocidade comercial, atraso em paradas e fora delas,
    /// regularidade (intervalo real nas paradas, coeficiente de variação, comboios).
    pub fn bus_report(&self) -> serde_json::Value {
        let lines: Vec<serde_json::Value> = self
            .bus_lines
            .iter()
            .filter(|l| l.trips > 0 || l.stops > 0)
            .map(|l| {
                let n = l.trips.max(1) as f64;
                let hw_mean = if l.hw_n > 0 { l.hw_sum / l.hw_n as f64 } else { 0.0 };
                let hw_cv = if l.hw_n > 1 && hw_mean > 0.0 { ((l.hw_sq / l.hw_n as f64 - hw_mean * hw_mean).max(0.0)).sqrt() / hw_mean } else { 0.0 };
                serde_json::json!({
                    "linha": l.numero,
                    "viagens": l.trips,
                    "tempoMedio": l.travel_s / n,
                    "velocidadeKmh": if l.travel_s > 0.0 { l.dist_m / l.travel_s * 3.6 } else { 0.0 },
                    "paradoNasParadas": l.dwell_s / n,
                    "paradoForaDasParadas": l.stopped_s / n,
                    "paradas": l.stops,
                    "paradasPerdidas": l.missed,
                    "intervaloMedio": hw_mean,
                    "intervaloCV": hw_cv,
                    "comboios": l.bunched,
                })
            })
            .collect();
        let (active, done, kmh) = self.bus_summary();
        serde_json::json!({
            "ativos": active,
            "partidas": self.bus_next,
            "naFila": self.bus_queue.len(),
            "concluidas": done,
            "velocidadeKmh": kmh,
            "paradas": self.stats.bus_stops,
            "paradasPerdidas": self.stats.bus_stops_missed,
            "linhas": lines,
        })
    }

    /// Diagnóstico para testes: (rota, passo, faixa ou conversão, embarcando) de cada ônibus em circulação.
    pub fn bus_states(&self) -> Vec<(u32, u32, u32, bool)> {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] != NONE).map(|i| (self.route[i], self.bstep[i], self.edge[i], self.dwell[i] > 0.0)).collect()
    }

    /// Trecho dirigido de cada passo do plano `p` e (de, para) de cada conversão: para conferir que o ônibus não sai do caminho.
    pub fn bus_plan_dirsegs(&self, p: usize) -> Vec<u32> {
        self.bus_plans[p].dirseg.clone()
    }

    /// Parada `id` na simulação: linhas que passam, próximos ônibus (até 8, com a chegada estimada em fluxo livre) e
    /// espera média por linha (E[h²] / 2E[h] dos intervalos medidos ali). `null` se a parada não está em nenhuma linha.
    pub fn stop_info(&self, id: &str) -> serde_json::Value {
        use serde_json::json;
        let Some(&sid) = self.stop_id_of.get(id) else { return serde_json::Value::Null };
        let mut lines: Vec<u32> = self.bus_plans.iter().filter(|p| p.stops.iter().any(|s| s.id == sid)).map(|p| p.line).collect();
        lines.sort_unstable();
        lines.dedup();
        let mut next: Vec<(f32, usize, f32)> = Vec::new();
        for i in (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] != NONE) {
            let p = &self.bus_plans[self.route[i] as usize];
            let Some(st) = p.stops.iter().skip(self.bstop[i] as usize).find(|s| s.id == sid) else { continue };
            // distância e tempo livre: resto da faixa atual, passos até o da parada, posição na faixa dela
            let e = self.edge[i] as usize;
            let k0 = self.bstep[i] as usize;
            let (mut d, mut t) = if e < self.nl {
                let l = &self.lanes[e];
                if st.step as usize == k0 {
                    let x = (st.s - self.s[i]).max(0.0);
                    (x, x / (l.vfree * 0.85))
                } else {
                    let x = (l.len - self.s[i]).max(0.0);
                    (x, x / (l.vfree * 0.85))
                }
            } else {
                (0.0, 0.0)
            };
            for k in (k0 + 1)..=(st.step as usize) {
                let l = p.lo[k] as usize;
                let part = if k == st.step as usize { st.s } else { self.lanes[l].len };
                d += part;
                t += self.lrouter.time[l] * part / self.lanes[l].len.max(1.0);
            }
            next.push((t, i, d));
        }
        next.sort_by(|a, b| a.0.total_cmp(&b.0));
        let next: Vec<serde_json::Value> = next
            .iter()
            .take(8)
            .map(|&(t, i, d)| json!({ "veh": i, "linha": self.bus_lines[self.bus_plans[self.route[i] as usize].line as usize].numero, "dist": d, "eta": t, "embarcando": self.dwell[i] > 0.0 }))
            .collect();
        let wait: Vec<serde_json::Value> = lines
            .iter()
            .map(|&l| {
                let h = self.stop_hw.get(&(sid, l)).copied().unwrap_or_default();
                let w = if h.n > 0 && h.sum > 0.0 { Some(h.sq / (2.0 * h.sum)) } else { None };
                json!({ "linha": self.bus_lines[l as usize].numero, "intervalos": h.n, "intervaloMedio": if h.n > 0 { Some(h.sum / h.n as f64) } else { None }, "esperaMedia": w })
            })
            .collect();
        json!({
            "id": id,
            "nome": self.stop_names[sid as usize],
            "linhas": lines.iter().map(|&l| self.bus_lines[l as usize].numero.clone()).collect::<Vec<_>>(),
            "proximos": next,
            "espera": wait,
            "naBaia": self.bay_occ.get(&sid).copied().unwrap_or(0),
        })
    }

    /// Nome da próxima parada do ônibus `i` (vazio se não há).
    pub(super) fn next_stop_name(&self, i: usize) -> String {
        let p = &self.bus_plans[self.route[i] as usize];
        p.stops.get(self.bstop[i] as usize).map_or(String::new(), |st| self.stop_names[st.id as usize].clone())
    }

    /// Diagnóstico para testes: ônibus agora numa baia.
    pub fn buses_in_bay(&self) -> usize {
        self.in_bay.iter().zip(&self.alive).filter(|(b, a)| **a && **b != 0).count()
    }

    /// Diagnóstico para testes: tempo parado (`wait`, s) de cada ônibus de linha em circulação.
    pub fn bus_waits(&self) -> Vec<f32> {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] != NONE).map(|i| self.wait[i]).collect()
    }

    /// Diagnóstico para testes: ônibus de linha parados há entre `a` e `b` s.
    pub fn bus_ids_waiting(&self, a: f32, b: f32) -> Vec<usize> {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.route[i] != NONE && self.wait[i] > a && self.wait[i] <= b).collect()
    }

    /// Diagnóstico para testes: carros (não ônibus de linha) agora numa conversão só de ônibus e numa faixa de ônibus.
    pub fn cars_on_bus_edges(&self) -> (usize, usize) {
        let (mut conn, mut lane) = (0, 0);
        for i in (0..self.alive.len()).filter(|&i| self.alive[i] && self.kind[i] != super::KIND_BUS) {
            let e = self.edge[i] as usize;
            if e >= self.nl && self.conns[e - self.nl].bus_only {
                conn += 1;
            }
            if e < self.nl && self.lanes[e].bus {
                lane += 1;
            }
        }
        (conn, lane)
    }

    pub fn edge_dirseg(&self, e: usize) -> (u32, Option<u32>) {
        if e < self.nl {
            (self.lanes[e].dirseg, None)
        } else {
            let c = &self.conns[e - self.nl];
            (self.lanes[c.from as usize].dirseg, Some(self.lanes[c.to as usize].dirseg))
        }
    }
}
