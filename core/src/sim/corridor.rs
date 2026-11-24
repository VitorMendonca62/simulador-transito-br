//! Corredor de motos (versão 2, fase 8): faixa virtual entre duas faixas vizinhas do mesmo sentido, só para motos.
//!
//! O modelo continua de fila única: a moto no corredor `c` (entre as faixas `c` e `c + 1`) sai da lista da faixa
//! (`edge_veh`) e entra na lista do corredor (`corr_veh[c]`), ordenada por `s`. `edge[i]` segue sendo uma das duas
//! faixas (a "de referência", a de menor custo até o destino): dá a conversão, a velocidade livre, a retenção e as
//! métricas. Os carros não a veem como líder (ela passa ao lado), mas não trocam de faixa por cima dela
//! (`corr_blocks`), e ela não passa por quem invade o corredor (carro no meio de uma troca, ônibus em faixa estreita).
//!
//! Entra quando o fluxo das duas faixas ao lado está lento (`CORR_VMAX`), sai quando ele anda (`CORR_VOUT`), quando a
//! rota pede outra faixa ou antes de um nó sem semáforo. Em nó com semáforo vai até a linha, à frente do primeiro carro,
//! e entra direto na conversão da faixa de referência quando `may_enter` libera.

use super::*;


impl Sim {
    /// Limites do corredor da moto `i`: a parcela "corredor rápido" (`MB_FAST`, fase 9) entra com o trânsito a até
    /// 40 km/h, só volta acima de 60 km/h e anda até 25 km/h acima do fluxo; as demais seguem os da fase 8.
    fn corr_vmax(&self, i: usize) -> f32 {
        if self.moto_beh[i] & MB_FAST != 0 { CORR_VMAX_FAST } else { CORR_VMAX }
    }
    fn corr_vout(&self, i: usize) -> f32 {
        if self.moto_beh[i] & MB_FAST != 0 { CORR_VOUT_FAST } else { CORR_VOUT }
    }
    fn corr_dv(&self, i: usize) -> f32 {
        if self.moto_beh[i] & MB_FAST != 0 { CORR_DV_FAST } else { CORR_DV }
    }

    /// Eixo do corredor `c` (deslocamento lateral, no sistema do trecho, como `LaneRt::off`).
    pub(super) fn corr_mid(&self, c: usize) -> f32 {
        0.5 * (self.lanes[c].off + self.lanes[c + 1].off)
    }

    pub(super) fn corr_insert(&mut self, c: usize, id: usize) {
        let s = self.s[id];
        let list = &self.corr_veh[c];
        let pos = list.partition_point(|&j| self.s[j as usize] < s);
        self.corr_veh[c].insert(pos, id as u32);
        self.corr_bits[c / 64] |= 1 << (c % 64);
        self.corr[id] = c as u32;
    }

    /// Tira a moto `id` do corredor em que está.
    pub(super) fn corr_unlink(&mut self, id: usize) {
        let c = self.corr[id] as usize;
        let list = &mut self.corr_veh[c];
        let p = list.iter().position(|&x| x as usize == id).unwrap();
        list.remove(p);
        if list.is_empty() {
            self.corr_bits[c / 64] &= !(1 << (c % 64));
        }
        self.corr[id] = NONE;
    }

    /// Corredores ocupados, em ordem.
    pub(super) fn corr_active(&self, out: &mut Vec<u32>) {
        out.clear();
        for (w, &bits) in self.corr_bits.iter().enumerate() {
            let mut b = bits;
            while b != 0 {
                out.push((w * 64) as u32 + b.trailing_zeros());
                b &= b - 1;
            }
        }
    }

    /// O veículo `j` (numa faixa) tem a carroceria dentro do corredor `c`, ou vai passar por ele no resto da troca de
    /// faixa (o deslocamento `lat` vai a zero, do lugar desenhado até o centro da faixa)?
    fn intrudes(&self, j: usize, c: usize) -> bool {
        self.intrudes_at(j, c, 0.0)
    }

    /// Idem, para uma moto do corredor que ainda está chegando ao eixo dele (deslocamento `mlat`).
    fn intrudes_at(&self, j: usize, c: usize, mlat: f32) -> bool {
        let l = self.edge[j] as usize;
        let (a, b) = (self.lanes[l].off + self.lat[j], self.lanes[l].off);
        let (lo, hi) = (a.min(b), a.max(b));
        let m = self.corr_mid(c);
        let (mlo, mhi) = ((m + mlat).min(m), (m + mlat).max(m));
        let d = if mhi < lo { lo - mhi } else if mlo > hi { mlo - hi } else { 0.0 };
        d < half_w(self.kind[j]) + half_w(KIND_MOTO)
    }

    /// Há moto num corredor ao lado da faixa `e`?
    pub(super) fn corr_beside(&self, e: usize) -> bool {
        (e > 0 && self.corr_ok[e - 1] && !self.corr_veh[e - 1].is_empty()) || (self.corr_ok[e] && !self.corr_veh[e].is_empty())
    }

    /// Há moto de um corredor ao lado da faixa `e` ainda saindo dela de lado (`lat`) com a carroceria entre `a` e `b`?
    pub(super) fn corr_sliding_in(&self, e: usize, a: f32, b: f32) -> bool {
        [e.wrapping_sub(1), e].iter().any(|&c| {
            c < self.nl && self.corr_ok[c] && self.corr_veh[c].iter().any(|&m| {
                let m = m as usize;
                self.lat[m] != 0.0 && self.s[m] > a && self.s[m] - self.vlen[m] < b
            })
        })
    }

    /// Moto de um corredor ao lado da faixa `e` que ainda ocupa a faixa (entrando no corredor, com `lat`), à frente do
    /// veículo `i` (com a frente em `s` nas coordenadas da faixa; negativo = ainda na conversão que chega a ela): líder
    /// dele até ela sair de lado.
    pub(super) fn corr_lane_intruder(&self, e: usize, i: usize, s: f32) -> Option<(f32, f32)> {
        let mut best = None;
        for c in [e.wrapping_sub(1), e] {
            if c >= self.nl || !self.corr_ok[c] {
                continue;
            }
            for &m in &self.corr_veh[c] {
                let m = m as usize;
                // o veículo `i` deslizando numa troca de faixa (só se já está na faixa) ou a moto chegando ao corredor
                let li = if s >= 0.0 && self.edge[i] as usize == e { self.lat[i] } else { 0.0 };
                if (self.lat[m] == 0.0 && li == 0.0) || self.s[m] <= s {
                    continue;
                }
                let off = self.lanes[e].off;
                let (lo, hi) = ((off + li).min(off), (off + li).max(off));
                let mid = self.corr_mid(c);
                let (mlo, mhi) = ((mid + self.lat[m]).min(mid), (mid + self.lat[m]).max(mid));
                let d = if mhi < lo { lo - mhi } else if mlo > hi { mlo - hi } else { 0.0 };
                if d < half_w(self.kind[i]) + half_w(KIND_MOTO) {
                    best = min_lead(best, Some(((self.s[m] - self.vlen[m] - s).max(0.1), self.v[m])));
                }
            }
        }
        best
    }

    /// Velocidade do trânsito ao lado do ponto `s` do corredor `c`: a maior entre as duas faixas, cada uma dada pelo
    /// primeiro veículo ao lado ou à frente (até `CORR_LOOK`); faixa vazia = velocidade livre dela.
    pub(super) fn corr_flow(&self, c: usize, s: f32) -> f32 {
        let mut v = 0.0f32;
        for l in [c, c + 1] {
            let list = &self.edge_veh[l];
            let p = list.partition_point(|&j| self.s[j as usize] < s - 3.0);
            let vl = match list.get(p) {
                Some(&j) if self.s[j as usize] - s < CORR_LOOK => self.v[j as usize],
                _ => self.lanes[l].vfree,
            };
            v = v.max(vl);
        }
        v
    }

    /// Quem invade o corredor `c` à frente da moto `i` (ou ao lado dela): líder (folga, velocidade).
    fn corr_intruder(&self, c: usize, i: usize) -> Option<(f32, f32)> {
        let s = self.s[i];
        let mut best = None;
        for l in [c, c + 1] {
            let list = &self.edge_veh[l];
            let p = list.partition_point(|&j| self.s[j as usize] < s);
            for &j in &list[p..] {
                let j = j as usize;
                let gap = self.s[j] - self.vlen[j] - s;
                if gap > CORR_LOOK {
                    break;
                }
                if self.intrudes_at(j, c, self.lat[i]) {
                    best = min_lead(best, Some((gap.max(0.1), self.v[j])));
                    break;
                }
            }
        }
        best
    }

    /// Cabe uma moto de comprimento `len` com a frente em `s` no corredor `c`? (folga para as motos do corredor e
    /// ninguém invadindo o corredor ao lado dela)
    fn corr_room(&self, c: usize, s: f32, len: f32, skip: usize) -> bool {
        let list = &self.corr_veh[c];
        let p = list.partition_point(|&j| self.s[j as usize] < s);
        if let Some(&j) = list[p..].iter().find(|&&j| j as usize != skip) {
            let j = j as usize;
            if self.s[j] - self.vlen[j] - s < 1.0 {
                return false;
            }
        }
        if let Some(&f) = list[..p].iter().rev().find(|&&j| j as usize != skip) {
            let f = f as usize;
            if s - len - self.s[f] < 1.0 + 0.5 * self.v[f] {
                return false;
            }
        }
        for l in [c, c + 1] {
            let list = &self.edge_veh[l];
            let p = list.partition_point(|&j| self.s[j as usize] < s - len - 1.0);
            for &j in &list[p..] {
                let j = j as usize;
                if self.s[j] - self.vlen[j] > s + 1.0 {
                    break;
                }
                if j != skip && self.intrudes(j, c) {
                    return false;
                }
            }
        }
        true
    }

    /// Há moto no corredor `c` na extensão de quem troca de faixa por cima dele (frente em `s`, comprimento `len`,
    /// velocidade `v`)? Conta o quanto cada um avança em relação ao outro enquanto a carroceria atravessa o corredor
    /// (`CORR_SLIDE` s): a moto que vem atrás mais rápido e a que está logo à frente de quem vem mais rápido.
    pub(super) fn corr_blocks(&self, c: usize, s: f32, len: f32, v: f32) -> bool {
        self.corr_veh[c].iter().any(|&m| {
            let m = m as usize;
            let dv = v - self.v[m];
            self.s[m] > s - len - 1.0 - CORR_SLIDE * (-dv).max(0.0) && self.s[m] - self.vlen[m] < s + 1.0 + CORR_SLIDE * dv.max(0.0)
        })
    }

    /// A ponta da faixa `e` deixa a moto chegar à linha pelo corredor (semáforo, ou saída da rede)?
    fn corr_to_line(&self, i: usize, e: usize) -> bool {
        self.end_sig[self.lanes[e].end_id as usize] || self.choice[i] == SINK
    }

    /// Moto numa faixa: entra num dos corredores dela (`e - 1|e` ou `e|e + 1`) se o trânsito ao lado está lento e uma
    /// das duas faixas serve à rota (`own` = custo de ficar na faixa atual). Sem vão nos carros: passa ao lado.
    pub(super) fn try_corridor(&mut self, i: usize, own: f32) -> bool {
        let e = self.edge[i] as usize;
        let s = self.s[i];
        let rest = self.lanes[e].len - s;
        let need = self.vlen[i] + 3.0 + if self.corr_to_line(i, e) { 0.0 } else { CORR_END_FREE + 40.0 };
        // nos primeiros metros da faixa chegam veículos da conversão e nascem os da zona: entra mais adiante
        if rest < need || s < self.vlen[i] + 8.0 || self.dwell[i] > 0.0 || self.lat[i].abs() > 0.5 {
            return false;
        }
        let drv = self.drv(i);
        let mut best: Option<(f32, usize, usize)> = None;
        for c in [e.wrapping_sub(1), e] {
            if c >= self.nl || !self.corr_ok[c] || self.corr_flow(c, s) >= self.corr_vmax(i) {
                continue;
            }
            // motos já no corredor à frente (fila única até a linha): espalha entre os dois corredores da faixa
            let ahead = {
                let list = &self.corr_veh[c];
                let p = list.partition_point(|&j| self.s[j as usize] < s);
                list[p..].iter().take_while(|&&j| self.s[j as usize] - s < 100.0).count() as f32
            };
            for h in [c, c + 1] {
                let k = if h == e { own } else { self.stay_cost(drv, h).0 };
                let kq = k + CORR_QUEUE_S * ahead;
                if k.is_finite() && (!own.is_finite() || k <= own + 5.0) && best.map_or(true, |b| kq < b.0) {
                    best = Some((kq, c, h));
                }
            }
        }
        let Some((_, c, h)) = best else { return false };
        if !self.corr_room(c, s, self.vlen[i], i) {
            return false;
        }
        let old = self.lanes[e].off;
        self.unlink(e, i);
        self.edge[i] = h as u32;
        self.corr_insert(c, i);
        self.lat[i] += old - self.corr_mid(c);
        self.blink_to[i] = NONE;
        self.mand[i] = false;
        self.stop_at[i] = f32::NAN;
        if h != e {
            self.t_due[i] = self.time as f32 + self.free_left(h, s);
        }
        self.choose_conn(i);
        self.stats.corridor_entries += 1;
        true
    }

    /// Volta do corredor para a faixa `to` (uma das duas), se cabe sem sobrepor e sem freada forte de quem vem atrás.
    /// Voluntária (o trânsito voltou a andar): com o vão de uma troca de faixa comum e sem fazer ninguém frear além do
    /// confortável; obrigatória (rota, fim do corredor): basta caber, com freada de até `MOBIL_SAFE` atrás.
    fn corr_exit(&mut self, i: usize, to: usize, forced: bool) -> bool {
        if self.contra_by[to] != NONE && self.contra_busy(to) {
            return false; // moto na contramão ao lado da faixa (fase 9)
        }
        let s = self.s[i];
        let list = &self.edge_veh[to];
        let p = list.partition_point(|&j| self.s[j as usize] < s);
        if let Some(&j) = list.get(p) {
            let j = j as usize;
            let need = if forced { 1.0 } else { 2.0 + 0.4 * self.v[i] };
            if self.s[j] - self.vlen[j] - s < need {
                return false;
            }
        }
        if p > 0 {
            let f = list[p - 1] as usize;
            let gap = s - self.vlen[i] - self.s[f];
            // obrigatória com quem vem atrás parado: basta caber (ninguém precisa frear)
            if forced && self.v[f] < 0.5 {
                if gap < MIN_GAP {
                    return false;
                }
            } else {
                let (need, brake) = if forced { (1.0, MOBIL_SAFE) } else { (2.0 + 0.9 * self.v[f], 1.0) };
                if gap < need || self.idm(f, Some((gap, self.v[i]))) < -brake {
                    return false;
                }
            }
        }
        let c = self.corr[i] as usize;
        let mid = self.corr_mid(c);
        self.corr_unlink(i);
        self.mand[i] = false;
        self.blink_to[i] = NONE;
        let e = self.edge[i] as usize;
        self.edge[i] = to as u32;
        self.insert_sorted(to as u32, i);
        self.lat[i] += mid - self.lanes[to].off;
        self.stop_at[i] = f32::NAN;
        if to != e {
            self.t_due[i] = self.time as f32 + self.free_left(to, s);
        }
        self.choose_conn(i);
        true
    }

    /// Moto no corredor (no lugar da troca de faixa): troca a faixa de referência entre as duas, ou sai do corredor
    /// quando o trânsito anda, quando a rota pede uma faixa fora do par ou antes de um nó sem semáforo.
    pub(super) fn corr_update(&mut self, i: usize) {
        let elapsed = self.time as f32 - self.t_spawn[i];
        if elapsed > (6.0 * self.t_free[i]).max(1800.0) {
            self.stats.lost += 1;
            self.remove_vehicle(i);
            return;
        }
        let c = self.corr[i] as usize;
        let drv = self.drv(i);
        let (ca, cb) = (self.stay_cost(drv, c).0, self.stay_cost(drv, c + 1).0);
        let e = self.edge[i] as usize;
        let (own, other, oc) = if e == c { (ca, c + 1, cb) } else { (cb, c, ca) };
        if oc + 2.0 < own || (!own.is_finite() && oc.is_finite()) {
            self.edge[i] = other as u32;
            self.t_due[i] = self.time as f32 + self.free_left(other, self.s[i]);
            self.choose_conn(i);
        }
        let home = self.edge[i] as usize;
        let hc = ca.min(cb);
        let s = self.s[i];
        let rest = self.lanes[home].len - s;
        // rota: melhor faixa fora do par (com o custo de trocar até ela)
        let (lo, hi) = self.lanes_of(home);
        let w = self.lane_w[home];
        let mut side: Option<(f32, usize)> = None;
        for l in lo..hi {
            if l == c || l == c + 1 || !self.compatible(i, l) {
                continue;
            }
            let d = if l < c { c - l } else { l - (c + 1) };
            let k = self.stay_cost(drv, l).0 + w * d as f32 * self.lc_mul(l);
            if side.map_or(true, |x| k < x.0) {
                side = Some((k, l));
            }
        }
        let route_out = match side {
            Some((k, _)) => k.is_finite() && (!hc.is_finite() || k + 10.0 < hc),
            None => !hc.is_finite(),
        };
        let st = self.end_state[self.lanes[home].end_id as usize];
        let at_light = self.corr_to_line(i, home) && rest < 40.0 && (st == 1 || st == 2);
        let flowing = !at_light && self.corr_flow(c, s) > self.corr_vout(i);
        let free_end = !self.corr_to_line(i, home) && rest < CORR_END_FREE + 40.0;
        if !(route_out || flowing || free_end) {
            self.mand[i] = false;
            self.blink_to[i] = NONE;
            self.cool[i] = 0.6;
            return;
        }
        let first = match side {
            Some((_, l)) if route_out => if l < c { c } else { c + 1 },
            _ => home,
        };
        let second = if first == c { c + 1 } else { c };
        let forced = route_out || free_end;
        let ok = self.corr_exit(i, first, forced) || self.corr_exit(i, second, forced);
        if !ok && forced {
            // pede passagem (seta): quem vem logo atrás na faixa pode ceder (`courtesy`), como na troca obrigatória
            if self.blink_to[i] != first as u32 || self.time as f32 - self.blink_t[i] > 8.0 {
                self.blink_t[i] = self.time as f32;
            }
            self.blink_to[i] = first as u32;
            self.mand[i] = true;
        }
        self.cool[i] = if ok { 2.0 } else { 0.4 };
    }

    /// Líder de quem é a primeira moto do corredor (faixa de referência `e`).
    fn corr_front_lead(&mut self, net: &Net, i: usize, e: usize) -> Option<(f32, f32)> {
        let rest = self.edge_len[e] - self.s[i];
        if !self.corr_to_line(i, e) {
            // nó sem semáforo: o corredor acaba antes dele e a moto volta para uma faixa (`corr_update`). Com a saída
            // pedida (seta) e sem vão ao lado (carros parados a 2 m, menos que uma moto), avança até a linha: passa à
            // frente de quem está ao lado e entra na faixa quando o primeiro sai; parada antes, ficava presa
            self.enter_ok[i] = false;
            let end = if self.mand[i] { 0.5 } else { CORR_END_FREE };
            return Some(((rest - end).max(0.0), 0.0));
        }
        let mut lead = self.lane_end_lead(net, i, e, true);
        // retida na linha: a moto para com a frente nela (o carro para `s0` = 2 m antes), à frente do primeiro carro
        if let Some(l) = lead {
            if l.1 == 0.0 && (l.0 - rest.max(0.0)).abs() < 0.01 {
                lead = Some((l.0 + 0.8, 0.0));
            }
        }
        // ainda não dá para passar a linha (conversão cheia, carro arrancando ao lado): para nela
        if rest < 12.0 && self.enter_ok[i] && self.corr_pick(i, false).is_none() {
            lead = min_lead(lead, Some((rest.max(0.0) + 0.8, 0.0)));
        }
        // carro da faixa de referência andando à frente dela, perto da linha: ele entra primeiro
        if rest < 15.0 {
            if let Some(&j) = self.edge_veh[e].last() {
                let j = j as usize;
                if self.v[j] > 1.0 && self.s[j] > self.s[i] {
                    // atrás dele, segue-o; ao lado dele, espera na linha que ele passe
                    let g = self.s[j] - self.vlen[j] - self.s[i];
                    lead = min_lead(lead, Some(if g > 0.0 { (g, self.v[j]) } else { (rest.max(0.0) + 0.8, 0.0) }));
                }
            }
        }
        lead
    }

    /// Acelerações das motos no corredor (passo 1, depois das faixas).
    pub(super) fn corridor_accels(&mut self, net: &Net, act: &[u32]) {
        let dt = self.dt as f64;
        for &c in act {
            let c = c as usize;
            let len = self.corr_veh[c].len();
            for k in 0..len {
                let i = self.corr_veh[c][k] as usize;
                let e = self.edge[i] as usize;
                self.stats.corridor_s += dt;
                let front = k + 1 == len;
                let mut lead = if !front {
                    let j = self.corr_veh[c][k + 1] as usize;
                    Some((self.s[j] - self.vlen[j] - self.s[i], self.v[j]))
                } else {
                    self.corr_front_lead(net, i, e)
                };
                lead = min_lead(lead, self.corr_intruder(c, i));
                let rest = self.edge_len[e] - self.s[i];
                self.held_box[i] = false;
                let cc = self.choice[i];
                if front && self.enter_ok[i] && cc != NONE && cc != SINK && rest < 40.0 {
                    let o = self.occ_obstacle(net, i, cc, -rest);
                    if o.is_none() && self.occ_skipped {
                        self.occ_go[i] = cc + 1;
                    }
                    if let Some(o) = o {
                        self.held_box[i] = true;
                        lead = min_lead(lead, Some((o.0, o.1)));
                    }
                }
                let v = self.v[i];
                let v0 = self.desired_speed(i).min(self.corr_flow(c, self.s[i]) + self.corr_dv(i)).max(0.5);
                let (a_max, b, t) = (self.amax[i], self.bcomf[i], self.tgap[i]);
                let mut x = 1.0 - (v / v0).powi(4);
                if let Some((gap, vl)) = lead {
                    let s_star = 1.0 + (v * t + v * (v - vl) / (2.0 * (a_max * b).sqrt())).max(0.0);
                    x -= (s_star / gap.max(0.1)).powi(2);
                }
                let mut a = (a_max * x).clamp(-9.0, a_max);
                if cc != NONE && cc != SINK {
                    let vt = self.conns[cc as usize - self.nl].vturn;
                    if v > vt {
                        let need = (v * v - vt * vt) / (2.0 * rest.max(0.5));
                        if need > 0.6 * b {
                            a = a.min(-need.min(9.0));
                        }
                    }
                }
                if a < -HARSH && self.acc[i] >= -HARSH {
                    self.stats.harsh_brakes += 1;
                }
                self.acc[i] = a;
            }
        }
    }

    /// Folga mínima entre motos consecutivas de cada corredor.
    pub(super) fn corridor_gaps(&mut self, act: &[u32]) {
        for &c in act {
            let c = c as usize;
            let len = self.corr_veh[c].len();
            if len < 2 {
                continue;
            }
            for k in (0..len - 1).rev() {
                let (a, b) = (self.corr_veh[c][k] as usize, self.corr_veh[c][k + 1] as usize);
                let limit = self.s[b] - self.vlen[b] - MIN_GAP;
                if self.s[a] > limit {
                    self.s[a] = limit.max(self.s[a] - 5.0);
                    self.v[a] = self.v[a].min(self.v[b]);
                }
            }
        }
    }

    /// Por qual das duas faixas do corredor a moto `i` pode passar a linha agora: (faixa, conversão). A conversão
    /// precisa ter lugar e o primeiro carro da faixa estar parado (a moto sai na frente dele) ou atrás da moto com
    /// distância para frear sem susto (`MOBIL_SAFE`).
    /// `rolling`: a moto já arrancou para a linha (decidiu com o carro parado); basta que ele ainda esteja devagar e
    /// atrás da frente dela.
    pub(super) fn corr_pick(&self, i: usize, rolling: bool) -> Option<(usize, u32)> {
        let home = self.edge[i] as usize;
        if !self.corr_to_line(i, home) || self.choice[i] == NONE {
            return None;
        }
        if self.choice[i] == SINK {
            return Some((home, SINK));
        }
        let c = self.corr[i] as usize;
        let other = if home == c { c + 1 } else { c };
        let over = (self.s[i] - self.edge_len[home]).max(0.0);
        let drv = self.drv(i);
        let hc = self.stay_cost(drv, home).0;
        // começo do verde (ou ponta sem semáforo, saída da rede): quem estava parado na linha sai na frente do carro
        let head = self.green_age[self.lanes[home].end_id as usize] < CORR_GREEN_HEAD;
        // das duas faixas, a de conversão mais livre (as motos da fila do corredor saem alternando entre elas)
        let mut best: Option<(f32, usize, u32)> = None;
        for l in [home, other] {
            let cc = if l == home {
                self.choice[i]
            } else {
                let (k, cc) = self.stay_cost(drv, l);
                if !k.is_finite() || k > hc + 5.0 || cc == NONE || cc == SINK {
                    continue;
                }
                cc
            };
            let room = self.edge_veh[cc as usize].first().map_or(f32::INFINITY, |&j| self.s[j as usize] - self.vlen[j as usize]);
            let car = self.edge_veh[l].last().map_or(true, |&j| {
                let j = j as usize;
                let gap = self.s[i] - self.vlen[i] - self.s[j];
                // parado, ou arrancando devagar com a moto já à frente dele (ele cede), ou atrás com distância para frear
                (head && self.v[j] < 0.5)
                    || (rolling && self.v[j] < 2.0 && self.s[j] < self.s[i] - 0.5)
                    || (gap > MIN_GAP && self.idm(j, Some((gap, self.v[i].max(self.v[j])))) >= -CORR_CUT_BRAKE)
            });
            if room >= over + MIN_GAP && car && best.map_or(true, |b| room > b.0 + 0.5) {
                best = Some((room, l, cc));
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// Moto no corredor chegou ao fim da faixa de referência: entra direto na conversão de uma das duas faixas
    /// (`corr_pick`; de volta à lista da faixa, só para a passagem de `advance`); senão espera na linha.
    pub(super) fn corr_to_conn(&mut self, i: usize) -> bool {
        let Some((l, cc)) = self.corr_pick(i, true) else {
            let len = self.edge_len[self.edge[i] as usize];
            self.s[i] = len - 0.01;
            self.v[i] = 0.0;
            return false;
        };
        let c = self.corr[i] as usize;
        // a moto segue desenhada onde está (`advance` passa o deslocamento ao sistema da conversão)
        self.lat[i] += self.corr_mid(c) - self.lanes[l].off;
        self.corr_unlink(i);
        if l != self.edge[i] as usize {
            self.s[i] += self.edge_len[l] - self.edge_len[self.edge[i] as usize];
            self.edge[i] = l as u32;
        }
        self.choice[i] = cc;
        self.insert_sorted(l as u32, i);
        true
    }

    /// Diagnóstico (testes): pares (moto no corredor, veículo numa faixa ao lado) com as carrocerias sobrepostas.
    pub fn corridor_conflicts(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut act = Vec::new();
        self.corr_active(&mut act);
        for &c in &act {
            let c = c as usize;
            for &m in &self.corr_veh[c] {
                let m = m as usize;
                for l in [c, c + 1] {
                    for &j in &self.edge_veh[l] {
                        let j = j as usize;
                        let lon = self.s[j] > self.s[m] - self.vlen[m] && self.s[j] - self.vlen[j] < self.s[m];
                        let lat = (self.lanes[l].off + self.lat[j] - (self.corr_mid(c) + self.lat[m])).abs() < half_w(self.kind[j]) + half_w(KIND_MOTO) - 0.05;
                        if lon && lat {
                            out.push((m as u32, j as u32));
                        }
                    }
                }
            }
        }
        out
    }

    /// Diagnóstico (testes): motos no corredor agora e listas de cada corredor ocupado.
    pub fn corridor_lists(&self) -> Vec<Vec<u32>> {
        let mut act = Vec::new();
        self.corr_active(&mut act);
        act.iter().map(|&c| self.corr_veh[c as usize].clone()).collect()
    }

    pub fn in_corridor(&self, i: usize) -> bool {
        i < self.corr.len() && self.alive[i] && self.corr[i] != NONE
    }

    /// Diagnóstico (testes): motos do corredor paradas na linha à frente do primeiro carro (parado) da faixa de referência.
    pub fn corridor_ahead_at_line(&self) -> usize {
        let mut act = Vec::new();
        self.corr_active(&mut act);
        act.iter()
            .filter_map(|&c| self.corr_veh[c as usize].last().map(|&m| m as usize))
            .filter(|&m| {
                let e = self.edge[m] as usize;
                self.edge_len[e] - self.s[m] < 1.0
                    && self.edge_veh[e].last().is_some_and(|&j| self.v[j as usize] < 0.1 && self.s[j as usize] < self.s[m])
            })
            .count()
    }

    pub fn dbg_veh(&self, i: usize) -> String {
        format!("#{i}k{} e{} lat{:.2} off{:.2} s{:.1}/{:.0} v{:.1} c{} ch{} ok{} w{:.0}", self.kind[i], self.edge[i], self.lat[i], self.lanes.get(self.edge[i] as usize).map_or(f32::NAN, |l| l.off), self.s[i], self.edge_len[self.edge[i] as usize], self.v[i], self.corr[i] as i64, self.choice[i] as i64, self.enter_ok[i] as u8, self.wait[i])
    }

    pub fn debug_slots(&self) -> usize {
        self.alive.len()
    }

    /// Diagnóstico (testes): tempo parado (s) de cada moto na rede.
    /// Diagnóstico para testes: espera (s) de cada moto no corredor agora.
    pub fn corridor_waits(&self) -> Vec<f32> {
        self.corr_veh.iter().flatten().map(|&i| self.wait[i as usize]).collect()
    }

    /// Diagnóstico para testes: motos paradas há entre `a` e `b` s.
    pub fn moto_ids_waiting(&self, a: f32, b: f32) -> Vec<usize> {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.kind[i] == KIND_MOTO && self.wait[i] > a && self.wait[i] <= b).collect()
    }

    pub fn moto_waits(&self) -> Vec<f32> {
        (0..self.alive.len()).filter(|&i| self.alive[i] && self.kind[i] == KIND_MOTO).map(|i| self.wait[i]).collect()
    }

    /// Motos no corredor agora.
    pub fn motos_in_corridor(&self) -> usize {
        let mut act = Vec::new();
        self.corr_active(&mut act);
        act.iter().map(|&c| self.corr_veh[c as usize].len()).sum()
    }
}
