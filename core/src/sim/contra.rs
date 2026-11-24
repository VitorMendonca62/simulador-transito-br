//! Contramão de motos em trecho curto de mão única (versão 2, fase 9).
//!
//! Só existe com `SimParams::moto_contra_pct > 0`: a simulação cria, para cada trecho de mão única curto de via local,
//! uma faixa no sentido contrário só para motos (`LaneRt::contra`), na borda esquerda do trecho (à direita de quem vem
//! na contramão). Ela é anexada depois das faixas da rede (os ids das conversões continuam começando em `nl`).
//! - Conversões de **entrada** (no nó final do trecho, a partir das aproximações) são `moto_only`: ficam fora do
//!   roteador, então nenhuma rota passa por ela, e só a moto da parcela `MB_CONTRA` as escolhe (`stay_cost`). As de
//!   **saída** (no nó inicial) entram no roteador: o custo da faixa de contramão até o destino é conhecido.
//! - Quem vem de frente: a faixa real do lado da borda (`contra_adj`) e a de contramão se excluem. A moto só entra
//!   com ela vazia e sem ninguém entrando nela (`contra_clear`, motivo 12); quem vai entrar nela espera a moto sair
//!   (`contra_busy`, motivo 13).

use super::*;

/// Comprimento útil máximo (m) do trecho de mão única que as motos fazem na contramão, e mínimo (abaixo disso o trecho
/// é parte do cruzamento).
const CONTRA_MAX_LEN: f32 = 80.0;
const CONTRA_MIN_LEN: f32 = 15.0;
/// velocidade de quem anda na contramão (m/s, ~25 km/h)
pub(super) const CONTRA_V: f32 = 6.9;
/// penalidade extra (s) para entrar na contramão: só vale quando encurta mesmo o caminho
const CONTRA_PEN: f32 = 8.0;
/// distância (m) até a linha a partir da qual quem vai entrar na faixa conta como "entrando"
const CONTRA_NEAR: f32 = 40.0;

/// Uma faixa de contramão a criar: a faixa, as faixas reais do lado da borda e as ligações (nó, faixa da rede,
/// manobra, curva) de entrada (a partir de `from`) e de saída (para `to`).
pub(super) struct ContraPlan {
    pub lane: LaneRt,
    pub adj: Vec<u32>,
    pub ins: Vec<(u32, u32, u8, [f32; 8])>,
    pub outs: Vec<(u32, u32, u8, [f32; 8])>,
}

/// Escolhe uma faixa de cada grupo (mesmo trecho e sentido) pela manobra: à direita a mais à direita, à esquerda a
/// mais à esquerda, em frente a mais à direita (a faixa de contramão fica à direita de quem anda nela).
fn pick_by_kind(net: &Net, cands: &[u32], kind: u8) -> u32 {
    let idx = |l: u32| net.lanes[l as usize].idx;
    if kind == K_LEFT { *cands.iter().min_by_key(|&&l| idx(l)).unwrap() } else { *cands.iter().max_by_key(|&&l| idx(l)).unwrap() }
}

/// Faixas de `list` (ids em ordem crescente) agrupadas por trecho e sentido (`lane_base` + sentido), filtradas por
/// `keep`. `list` é a lista do nó (`by_node`): a mesma sequência que filtrar a rede inteira, sem percorrê-la.
fn groups(net: &Net, list: &[u32], keep: impl Fn(&Lane) -> bool) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = Vec::new();
    let mut last = (u32::MAX, u8::MAX);
    for &li in list {
        let l = &net.lanes[li as usize];
        if !keep(l) {
            continue;
        }
        if (l.seg, l.dir) != last {
            out.push(Vec::new());
            last = (l.seg, l.dir);
        }
        out.last_mut().unwrap().push(li);
    }
    out
}

/// Faixas da rede por nó (chegada ou saída), em ordem crescente de id.
fn by_node(net: &Net, node_of: impl Fn(&Lane) -> u32) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); net.nodes.len()];
    for (li, l) in net.lanes.iter().enumerate() {
        out[node_of(l) as usize].push(li as u32);
    }
    out
}

/// Planeja as faixas de contramão da rede (ver o módulo).
pub(super) fn plan(net: &Net, lanes: &[LaneRt]) -> Vec<ContraPlan> {
    let mut out = Vec::new();
    // antes cada trecho candidato varria todas as faixas da rede duas vezes (~0,6 s no Recife)
    let (into, from) = (by_node(net, |l| l.to), by_node(net, |l| l.from));
    for (si, s) in net.segs.iter().enumerate() {
        if !s.oneway || s.roundabout || s.link || s.level != 0 || !(HW_TERTIARY..=HW_LIVING).contains(&s.hw) || s.lf == 0 {
            continue;
        }
        let base = s.lane_base as usize;
        if (0..s.lf as usize).any(|k| net.lanes[base + k].bus) {
            continue;
        }
        let len = lanes[base].len;
        if !(CONTRA_MIN_LEN..=CONTRA_MAX_LEN).contains(&len) {
            continue;
        }
        // geometria: começa no nó final do trecho (b) e termina no inicial (a), na borda esquerda do trecho
        let (a, b) = (s.from, s.to);
        let off = -s.left_ext() + 0.5;
        let total = s.poly.length();
        let at = |sv: f64| -> ([f32; 2], [f32; 2]) {
            let (p, t) = s.poly.at(sv);
            let q = add(p, mul(right(t), off));
            ([q[0] as f32, q[1] as f32], [-t[0] as f32, -t[1] as f32])
        };
        let (start, sdir) = at(total - s.cut_to);
        let (end, edir) = at(s.cut_from);
        let mut ins = Vec::new();
        for g in groups(net, &into[b as usize], |l| l.to == b && l.seg != si as u32 && !l.bus) {
            let l0 = &net.lanes[g[0] as usize];
            if let Some((kind, _)) = crate::conn::turn_curve(l0.end, l0.edir, start, sdir) {
                let fl = pick_by_kind(net, &g, kind);
                let l = &net.lanes[fl as usize];
                let (_, curve) = crate::conn::turn_curve(l.end, l.edir, start, sdir).unwrap();
                ins.push((b, fl, kind, curve));
            }
        }
        let mut outs = Vec::new();
        for g in groups(net, &from[a as usize], |l| l.from == a && l.seg != si as u32 && !l.bus) {
            let l0 = &net.lanes[g[0] as usize];
            if let Some((kind, _)) = crate::conn::turn_curve(end, edir, l0.start, l0.sdir) {
                let tl = pick_by_kind(net, &g, kind);
                let l = &net.lanes[tl as usize];
                let (_, curve) = crate::conn::turn_curve(end, edir, l.start, l.sdir).unwrap();
                outs.push((a, tl, kind, curve));
            }
        }
        if ins.is_empty() || outs.is_empty() {
            continue;
        }
        // a faixa real mais à esquerda (idx 0) é a do lado da borda
        let adj: Vec<u32> = vec![base as u32];
        let end_id = si as u32 * 2;
        out.push(ContraPlan {
            lane: LaneRt {
                seg: si as u32,
                dir: 1,
                idx: 0,
                len,
                off: off as f32,
                a0: (total - s.cut_to) as f32,
                to_node: a,
                end_id,
                bus: false,
                vfree: free_speed(s).min(CONTRA_V),
                rank: 100,
                stop: false,
                ring: false,
                dirseg: si as u32 * 2 + 1,
                conns: Vec::new(),
                contra: true,
            },
            adj,
            ins,
            outs,
        });
    }
    out
}

impl Sim {
    /// A faixa de contramão `cl` pode receber uma moto: a faixa real do lado da borda está vazia e ninguém está entrando
    /// nela (na conversão, ou liberado e perto da linha com ela escolhida).
    pub(super) fn contra_clear(&self, cl: usize) -> bool {
        self.contra_adj[cl].iter().all(|&a| !self.lane_entering(a as usize))
    }

    /// Há moto na contramão ao lado da faixa `a` (ou entrando nela)? Quem vai entrar em `a` espera.
    pub(super) fn contra_busy(&self, a: usize) -> bool {
        let cl = self.contra_by[a];
        cl != NONE && self.lane_entering(cl as usize)
    }

    /// A faixa `l` tem alguém nela, numa conversão que chega a ela, ou liberado perto da linha rumo a ela.
    fn lane_entering(&self, l: usize) -> bool {
        if !self.edge_veh[l].is_empty() {
            return true;
        }
        self.lane_in[l].iter().any(|&p| {
            if !self.edge_veh[p as usize].is_empty() {
                return true;
            }
            let fl = self.conns[p as usize - self.nl].from as usize;
            self.edge_veh[fl].last().map_or(false, |&j| {
                let j = j as usize;
                self.choice[j] == p && self.enter_ok[j] && self.lanes[fl].len - self.s[j] < CONTRA_NEAR
            })
        })
    }

    /// Penalidade extra da conversão `c` para a rota (entrada na contramão).
    pub(super) fn contra_pen(&self, c: &ConnRt) -> f32 {
        if c.moto_only { CONTRA_PEN } else { 0.0 }
    }

    /// Número de faixas de contramão criadas.
    pub fn contra_lanes(&self) -> usize {
        self.lanes.iter().filter(|l| l.contra).count()
    }

    /// Diagnóstico (testes): pares (moto na contramão, veículo na faixa real do lado) com as carrocerias sobrepostas ao
    /// longo do trecho (a contramão conta do nó final; a faixa real, do inicial).
    pub fn contra_conflicts(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for cl in 0..self.nl {
            if !self.lanes[cl].contra {
                continue;
            }
            let len = self.lanes[cl].len;
            for &m in &self.edge_veh[cl] {
                let m = m as usize;
                let (m0, m1) = (self.s[m] - self.vlen[m], self.s[m]);
                for &a in &self.contra_adj[cl] {
                    for &j in &self.edge_veh[a as usize] {
                        let j = j as usize;
                        // posição de j medida a partir do nó final do trecho
                        let (j0, j1) = (len - self.s[j], len - self.s[j] + self.vlen[j]);
                        if m0 < j1 && j0 < m1 {
                            out.push((m as u32, j as u32));
                        }
                    }
                }
            }
        }
        out
    }

    /// Motos andando na contramão agora.
    pub fn motos_in_contra(&self) -> usize {
        (0..self.nl).filter(|&l| self.lanes[l].contra).map(|l| self.edge_veh[l].len()).sum()
    }
}
