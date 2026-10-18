//! Conexões entre faixas em cada nó (estilo TM:PE), restrições OSM e semáforos.

use crate::geo::*;
use crate::net::*;
use crate::osm::RawOsm;
use std::collections::HashMap;

const LEFT: u8 = 1 << K_LEFT;
const STRAIGHT: u8 = 1 << K_STRAIGHT;
const RIGHT: u8 = 1 << K_RIGHT;
const UTURN: u8 = 1 << K_UTURN;

/// Classifica a manobra entre a direção de chegada e a de saída (coordenadas Y para baixo).
fn angle(in_dir: V2, out_dir: V2) -> f64 {
    cross(in_dir, out_dir).atan2(dot(in_dir, out_dir)).to_degrees()
}

fn classify(in_dir: V2, out_dir: V2) -> u8 {
    let ang = angle(in_dir, out_dir);
    if ang.abs() < 30.0 {
        K_STRAIGHT
    } else if ang > 0.0 {
        K_RIGHT
    } else {
        K_LEFT
    }
}

/// Quando várias saídas têm a mesma manobra (bifurcação em Y: dois ramos "em frente"), cada uma recebe um grupo
/// contíguo de faixas, da esquerda para a direita na mesma ordem dos ramos; a faixa do meio é compartilhada
/// quando a divisão não é exata. Com menos faixas que ramos, cada faixa leva a todos os ramos que lhe cabem.
/// Retorna o intervalo `lo..hi` das `m` faixas que vão para o ramo `j` de `k` (ordenados da esquerda para a direita).
fn lane_group(m: usize, k: usize, j: usize) -> (usize, usize) {
    if k <= 1 || m == 0 {
        return (0, m);
    }
    (j * m / k, ((j + 1) * m + k - 1) / k)
}

/// Divide as `m` faixas que seguem "em frente" entre `counts.len()` ramos (da esquerda para a direita, `counts[j]` =
/// faixas de saída do ramo `j`, `main` = o ramo mais alinhado). Retorna o intervalo `lo..hi` de faixas de cada ramo.
///
/// - Se os ramos juntos têm até `m` faixas, cada um recebe um bloco exclusivo proporcional às suas faixas
///   (ex.: 4 faixas → ramos de 2 e 2).
/// - Senão, o principal fica com todas as faixas que couberem nele, cada uma na sua posição, e cada lado recebe as
///   faixas da sua ponta, compartilhando no máximo uma com o principal (ex.: 4 faixas, avenida de 4 e saída de 1 à
///   direita → as 4 seguem e só a da direita também sai; 2 faixas em Y de 2 e 2 → a da divisa vai aos dois).
fn fork_alloc(m: usize, counts: &[usize], main: usize) -> Vec<(usize, usize)> {
    let k = counts.len();
    if k <= 1 || m == 0 {
        return vec![(0, m); k];
    }
    let c: Vec<usize> = counts.iter().map(|&x| x.clamp(1, m)).collect();
    let total: usize = c.iter().sum();
    if total <= m {
        let mut out = Vec::with_capacity(k);
        let mut cum = 0;
        for &cj in &c {
            let lo = m * cum / total;
            cum += cj;
            let hi = ((m * cum + total - 1) / total).max(lo + 1).min(m);
            out.push((lo, hi));
        }
        return out;
    }
    let left: usize = c[..main].iter().sum();
    let right: usize = c[main + 1..].iter().sum();
    let main_n = c[main].min(m);
    let rem = m - main_n;
    let left_ex = if left + right == 0 { 0 } else { ((rem * left) as f64 / (left + right) as f64).round() as usize };
    let right_ex = rem - left_ex;
    let wl = if left > 0 { left.min(left_ex + 1).max(left_ex) } else { 0 };
    let wr = if right > 0 { right.min(right_ex + 1).max(right_ex) } else { 0 };
    let (kl, kr) = (main, k - main - 1);
    let mut out = Vec::with_capacity(k);
    for j in 0..k {
        out.push(if j == main {
            (left_ex, left_ex + main_n)
        } else if j < main {
            let (lo, hi) = lane_group(wl, kl, j);
            (lo, hi)
        } else {
            let (lo, hi) = lane_group(wr, kr, j - main - 1);
            (m - wr + lo, m - wr + hi)
        });
    }
    out
}

/// Manobras de cada faixa. `lanes_lr` = faixas de saída somadas das conversões à esquerda e à direita (para dividir a
/// aproximação num T, onde não há "em frente").
fn lane_masks(tags: &[u8], n: usize, present: u8, lanes_lr: (usize, usize)) -> Vec<u8> {
    let any_tag = tags.iter().any(|&t| t != 0);
    // T sem "em frente" e sem setas: as faixas se dividem entre esquerda e direita na proporção das saídas
    if !any_tag && n >= 3 && present & STRAIGHT == 0 && present & LEFT != 0 && present & RIGHT != 0 {
        let (cl, cr) = (lanes_lr.0.clamp(1, n), lanes_lr.1.clamp(1, n));
        let k = ((n * cl) as f64 / (cl + cr) as f64).round().clamp(1.0, (n - 1) as f64) as usize;
        return (0..n).map(|i| if i < k { LEFT } else { RIGHT }).collect();
    }
    let mut m = vec![0u8; n];
    for i in 0..n {
        let t = tags.get(i).copied().unwrap_or(0);
        m[i] = if t != 0 {
            t
        } else if n == 1 {
            15
        } else if i == 0 {
            // sem seta pintada (OSM sem `turn:lanes`), a faixa da ponta é compartilhada: esquerda e em frente
            LEFT | STRAIGHT
        } else if i == n - 1 {
            RIGHT | STRAIGHT
        } else {
            STRAIGHT
        };
    }
    if !any_tag {
        if present & LEFT != 0 {
            m[0] |= LEFT;
        }
        if present & RIGHT != 0 {
            m[n - 1] |= RIGHT;
        }
        if present & STRAIGHT != 0 && !m.iter().any(|x| x & STRAIGHT != 0) {
            m[n / 2] |= STRAIGHT;
        }
    }
    for i in 0..n {
        m[i] &= present;
        if m[i] == 0 {
            let order: [u8; 4] = if i == 0 {
                [STRAIGHT, LEFT, RIGHT, UTURN]
            } else if i == n - 1 {
                [STRAIGHT, RIGHT, LEFT, UTURN]
            } else {
                [STRAIGHT, RIGHT, LEFT, UTURN]
            };
            if let Some(b) = order.iter().find(|b| present & **b != 0) {
                m[i] = *b;
            }
        }
    }
    m
}

/// Faixa de saída da `t`-ésima de `m` faixas que fazem a manobra. `li0` = índice (na aproximação) da primeira delas.
/// `side` (só "em frente"): 0 = mantém a posição; -1 = entra pela direita (ramal que converge vindo da direita);
/// +1 = entra pela esquerda.
fn target_index(kind: u8, t: usize, m: usize, li0: usize, n_out: usize, side: i8) -> usize {
    let last = n_out - 1;
    let from_left = t.min(last);
    let from_right = last - (m - 1 - t).min(last);
    match kind {
        K_LEFT => from_left,
        K_RIGHT => from_right,
        K_UTURN => 0,
        _ => {
            if side > 0 {
                from_left
            } else if side < 0 {
                from_right
            } else if m <= n_out {
                // cada faixa segue na sua posição (com uma faixa de conversão à esquerda, as demais não "pulam" uma)
                t + li0.min(n_out - m)
            } else if m > 1 {
                // estreitamento: as faixas se juntam
                ((t as f64) * last as f64 / (m - 1) as f64).round() as usize
            } else {
                last / 2
            }
        }
    }
    .min(last)
}

fn bezier(p0: [f32; 2], d0: [f32; 2], p3: [f32; 2], d3: [f32; 2], kind: u8) -> [f32; 8] {
    let dx = p3[0] - p0[0];
    let dy = p3[1] - p0[1];
    let dist = (dx * dx + dy * dy).sqrt();
    let k = match kind {
        K_STRAIGHT => dist * 0.33,
        K_UTURN => 5.0,
        _ => dist * 0.45,
    };
    [p0[0], p0[1], p0[0] + d0[0] * k, p0[1] + d0[1] * k, p3[0] - d3[0] * k, p3[1] - d3[1] * k, p3[0], p3[1]]
}

/// Qual das pontas `cands` (índices em `ends`) é a "via principal" em relação à ponta `refp`: a que continua a mesma via
/// (mesmo nome, ou mesmo way), depois a de mais faixas (de saída se `out`, de entrada senão), depois a mais alinhada.
/// Retorna o índice em `cands`.
fn pick_main(net: &Net, ends: &[u32], refp: usize, cands: &[usize], abs_ang: &[f64], ap: &[(Vec<u32>, Vec<u32>)], out: bool) -> usize {
    let seg = |p: usize| &net.segs[(ends[p] >> 1) as usize];
    let r = seg(refp);
    let same = |p: usize| {
        let s = seg(p);
        (s.name != NO_NAME && s.name == r.name) || s.way_id == r.way_id
    };
    let lanes = |p: usize| if out { ap[p].1.len() } else { ap[p].0.len() };
    (0..cands.len())
        .min_by(|&x, &y| {
            let (px, py) = (cands[x], cands[y]);
            same(py).cmp(&same(px)).then(lanes(py).cmp(&lanes(px))).then(abs_ang[x].partial_cmp(&abs_ang[y]).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap_or(0)
}

/// Restrições OSM indexadas pelo nó "via" (id OSM) -> índices em `raw.restrictions`.
pub fn restriction_index(raw: &RawOsm) -> HashMap<i64, Vec<usize>> {
    let mut m: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in raw.restrictions.iter().enumerate() {
        m.entry(r.via).or_default().push(i);
    }
    m
}

/// Chave estável de uma faixa: way:de:para.sentido.posição (orientação original do trecho).
pub fn lane_key(net: &Net, lane: u32) -> String {
    let l = &net.lanes[lane as usize];
    let k = net.segs[l.seg as usize].key;
    format!("{}:{}:{}.{}.{}", k[0], k[1], k[2], l.dir, l.idx)
}

/// Chave estável de uma conversão: nóOsm|faixaOrigem>faixaDestino.
pub fn conn_key(net: &Net, node: u32, from: u32, to: u32) -> String {
    format!("{}|{}>{}", net.nodes[node as usize].osm_id, lane_key(net, from), lane_key(net, to))
}

/// Todas as conexões do nó `ni` (padrão, candidatas e proibidas por restrição), já com as edições do usuário.
pub fn node_conns(net: &Net, ni: usize, raw: &RawOsm, restr: &HashMap<i64, Vec<usize>>, edits: &Edits) -> Vec<Conn> {
    let mut conns: Vec<Conn> = Vec::new();
    let ends = net.nodes[ni].ends.clone();
    let n = ends.len();
    let ap: Vec<(Vec<u32>, Vec<u32>)> = ends.iter().map(|&e| net.approach_lanes(e)).collect();
    let outw: Vec<V2> = ends.iter().map(|&e| net.outward(e)).collect();
    // convergência: para cada saída `b`, a aproximação que segue "em frente" para ela e continua a mesma via (ou a de
    // mais faixas, ou a mais alinhada) é a principal. Quem mais entra em `b` (ramal "em frente" ou conversão) chega por um
    // lado; o ramal "em frente" entra pela ponta desse lado e a principal encosta no lado oposto quando sobra faixa,
    // para cada fluxo ter a sua faixa em vez de dois disputarem as mesmas.
    let mut merge_side = vec![vec![0i8; n]; n];
    for b in 0..n {
        if ap[b].1.is_empty() {
            continue;
        }
        let feeders: Vec<usize> = (0..n).filter(|&a| a != b && !ap[a].0.is_empty()).collect();
        let straight: Vec<usize> = feeders.iter().copied().filter(|&a| classify(mul(outw[a], -1.0), outw[b]) == K_STRAIGHT).collect();
        if feeders.len() < 2 || straight.is_empty() {
            continue;
        }
        let ang: Vec<f64> = straight.iter().map(|&a| angle(mul(outw[a], -1.0), outw[b]).abs()).collect();
        let main = straight[pick_main(net, &ends, b, &straight, &ang, &ap, false)];
        // lado pela posição do braço em relação ao sentido de quem vem pela principal (não pelo sentido da curva:
        // um ramal que desce de cima e entra pela direita faz curva à direita)
        let h = mul(outw[main], -1.0);
        let r = [-h[1], h[0]];
        let mut sides = Vec::new();
        for &a in &feeders {
            if a == main {
                continue;
            }
            let sd: i8 = if dot(outw[a], r) > 0.0 { -1 } else { 1 };
            sides.push(sd);
            if straight.contains(&a) {
                merge_side[a][b] = sd;
            }
        }
        if !sides.is_empty() && sides.iter().all(|&x| x == sides[0]) {
            merge_side[main][b] = -sides[0];
        }
    }

    // restrições do OSM por movimento (aproximação `a` → saída `b`, pelos ways): aplicadas **antes** de distribuir as
    // faixas, para que as faixas se dividam entre as manobras permitidas (com a direita proibida, as duas faixas viram
    // à esquerda) em vez de sobrar faixa sem saída.
    let osm = net.nodes[ni].osm_id;
    let way_of = |p: usize| net.segs[(ends[p] >> 1) as usize].way_id;
    let banned = |a: usize, b: usize| -> bool {
        let Some(list) = restr.get(&osm) else { return false };
        // restrição é para carros: aproximação só de ônibus (corredor exclusivo) não é afetada
        if ap[a].0.iter().all(|&l| net.lanes[l as usize].bus) {
            return false;
        }
        list.iter().any(|&ri| {
            let r = &raw.restrictions[ri];
            // `to` que não chega a este nó (way partido em outro ponto, relação desatualizada): a restrição não se aplica aqui
            if way_of(a) != r.from || !(0..n).any(|p| way_of(p) == r.to) {
                return false;
            }
            if r.kind.starts_with("only_") {
                way_of(b) != r.to
            } else if r.kind.starts_with("no_") {
                way_of(b) == r.to
            } else {
                false
            }
        })
    };
    // distribuição das faixas da aproximação `a` entre as saídas `targets`
    let assign = |a: usize, targets: &[(usize, u8)]| -> Vec<Conn> {
        let ins = &ap[a].0;
        let mut out: Vec<Conn> = Vec::new();
        let present = targets.iter().fold(0u8, |m, t| m | (1 << t.1));
        if present == 0 {
            return out;
        }
        let tags: Vec<u8> = ins.iter().map(|&l| net.lanes[l as usize].tmask).collect();
        let lr = targets.iter().fold((0usize, 0usize), |acc, &(b, k)| match k {
            K_LEFT => (acc.0 + ap[b].1.len(), acc.1),
            K_RIGHT => (acc.0, acc.1 + ap[b].1.len()),
            _ => acc,
        });
        let masks = lane_masks(&tags, ins.len(), present, lr);
        for kind in 0..4u8 {
            let bit = 1u8 << kind;
            let who: Vec<usize> = (0..ins.len()).filter(|&i| masks[i] & bit != 0).collect();
            // ramos desta manobra, da esquerda para a direita
            let mut tg: Vec<(usize, f64)> = targets.iter().filter(|x| x.1 == kind).map(|&(b, _)| (b, if b == a { 0.0 } else { angle(mul(outw[a], -1.0), outw[b]) })).collect();
            tg.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
            // "em frente" com vários ramos: divisão pela capacidade, com o ramo mais alinhado (empate: mais faixas) como principal
            let fork = kind == K_STRAIGHT && tg.len() > 1;
            let main_j = if fork {
                let bs: Vec<usize> = tg.iter().map(|x| x.0).collect();
                pick_main(net, &ends, a, &bs, &tg.iter().map(|x| x.1.abs()).collect::<Vec<_>>(), &ap, true)
            } else {
                0
            };
            let alloc: Vec<(usize, usize)> = if fork {
                let counts: Vec<usize> = tg.iter().map(|&(b, _)| ap[b].1.len()).collect();
                fork_alloc(who.len(), &counts, main_j)
            } else {
                (0..tg.len()).map(|j| lane_group(who.len(), tg.len(), j)).collect()
            };
            for (j, &(b, _)) in tg.iter().enumerate() {
                let (lo, hi) = alloc[j];
                let sub = &who[lo..hi];
                if sub.is_empty() {
                    continue;
                }
                // lado de entrada: ramo à esquerda/direita do principal entra pela sua ponta; um só ramo que recebe
                // também o fluxo "em frente" de outra aproximação mais alinhada (ramal que converge) entra pelo seu lado
                let side: i8 = if kind != K_STRAIGHT {
                    0
                } else if tg.len() > 1 {
                    if j < main_j { 1 } else if j > main_j { -1 } else { 0 }
                } else {
                    merge_side[a][b]
                };
                for (t, &li) in sub.iter().enumerate() {
                    let outs = &ap[b].1;
                    let oi = target_index(kind, t, sub.len(), sub[0], outs.len(), side);
                    let (fl, tl) = (ins[li], outs[oi]);
                    let (l0, l1) = (&net.lanes[fl as usize], &net.lanes[tl as usize]);
                    out.push(Conn { node: ni as u32, from_lane: fl, to_lane: tl, kind, state: CONN_ALLOWED, base: CONN_ALLOWED, curve: bezier(l0.end, l0.edir, l1.start, l1.sdir, kind) });
                }
            }
        }
        out
    };
    for a in 0..n {
        if ap[a].0.is_empty() {
            continue;
        }
        // alvos: outras pontas com faixas de saída
        let mut all: Vec<(usize, u8)> = Vec::new();
        for b in 0..n {
            if b == a || ap[b].1.is_empty() {
                continue;
            }
            all.push((b, classify(mul(outw[a], -1.0), outw[b])));
        }
        // beco sem saída: retorno
        if n == 1 && !ap[a].1.is_empty() {
            all.push((a, K_UTURN));
        }
        let mut allowed: Vec<(usize, u8)> = all.iter().copied().filter(|&(b, _)| !banned(a, b)).collect();
        // todas as saídas proibidas (ex.: rua de mão dupla cuja única saída é uma conversão proibida): retorno na própria via
        if allowed.is_empty() && !ap[a].1.is_empty() && !banned(a, a) {
            allowed.push((a, K_UTURN));
        }
        let real = assign(a, &allowed);
        // o que seria o padrão sem a restrição aparece como "proibida por restrição" (tracejado vermelho)
        if allowed.len() < all.len() {
            let have: std::collections::HashSet<(u32, u32)> = real.iter().map(|c| (c.from_lane, c.to_lane)).collect();
            for mut c in assign(a, &all) {
                let b = (0..n).find(|&b| ap[b].1.contains(&c.to_lane)).unwrap_or(a);
                if banned(a, b) && !have.contains(&(c.from_lane, c.to_lane)) {
                    c.state = CONN_BLOCKED;
                    c.base = CONN_BLOCKED;
                    conns.push(c);
                }
            }
        }
        conns.extend(real);
    }

    // candidatas: todo par entrada -> saída que ainda não existe (estado ausente)
    let existing: std::collections::HashSet<(u32, u32)> = conns.iter().map(|c| (c.from_lane, c.to_lane)).collect();
    for a in 0..n {
        for b in 0..n {
            let kind = if a == b { K_UTURN } else { classify(mul(outw[a], -1.0), outw[b]) };
            for &fl in &ap[a].0 {
                for &tl in &ap[b].1 {
                    if existing.contains(&(fl, tl)) {
                        continue;
                    }
                    let (l0, l1) = (&net.lanes[fl as usize], &net.lanes[tl as usize]);
                    conns.push(Conn { node: ni as u32, from_lane: fl, to_lane: tl, kind, state: CONN_ABSENT, base: CONN_ABSENT, curve: bezier(l0.end, l0.edir, l1.start, l1.sdir, kind) });
                }
            }
        }
    }

    for c in conns.iter_mut() {
        c.base = c.state;
    }

    // edições do usuário (ligar/remover conversões)
    if let Some(list) = edits.conns.get(&osm) {
        for c in conns.iter_mut() {
            let key = conn_key(net, ni as u32, c.from_lane, c.to_lane);
            if let Some((_, st)) = list.iter().find(|(k, _)| *k == key) {
                c.state = *st;
            }
        }
    }
    conns
}

/// Constrói as conexões de todos os nós (usado pelos testes e pela simulação; a interface pede por nó).
pub fn build_connections(net: &mut Net, raw: &RawOsm, edits: &Edits) {
    let restr = restriction_index(raw);
    let mut all: Vec<Conn> = Vec::new();
    let mut off = Vec::with_capacity(net.nodes.len() + 1);
    for ni in 0..net.nodes.len() {
        off.push(all.len() as u32);
        all.extend(node_conns(net, ni, raw, &restr, edits));
    }
    off.push(all.len() as u32);
    net.conns = all;
    net.conn_off = off;
}

/// Semáforos: fases padrão (uma por eixo de aproximação), substituídas pela configuração do usuário quando houver.
/// Índice do eixo (direção módulo 180°) em que cai o ângulo de uma aproximação.
fn axis_of(axes: &[f64], ang: f64) -> Option<usize> {
    axes.iter().position(|a| {
        let d = (a - ang).rem_euclid(std::f64::consts::PI);
        d.min(std::f64::consts::PI - d) < 0.6
    })
}

/// Eixos de um controlador, um ângulo por eixo, na ordem em que aparecem.
fn axes_of<'a>(angles: impl Iterator<Item = &'a f64>) -> Vec<f64> {
    let mut axes = Vec::new();
    for &ang in angles {
        if axis_of(&axes, ang).is_none() {
            axes.push(ang);
        }
    }
    axes
}

/// Monta os semáforos. `ext` (nó OSM -> código externo) junta num só controlador os nós de mesmo código
/// (cruzamento de pista dupla, travessia nas duas mãos): mesmo ciclo, mesma defasagem e fases alinhadas
/// por eixo de aproximação, para que as mãos de um mesmo cruzamento não abram verde conflitante.
pub fn build_signals(net: &mut Net, edits: &Edits, ext: Option<&HashMap<i64, u32>>) {
    let mut signals = Vec::new();
    // aproximações (ponta, ângulo) de cada nó semaforizado
    let mut appr: Vec<(usize, Vec<(u32, f64)>)> = Vec::new();
    for (ni, node) in net.nodes.iter().enumerate() {
        if !node.signal {
            continue;
        }
        let mut approaches: Vec<(u32, f64)> = Vec::new();
        for &e in &node.ends {
            let (ins, _) = net.approach_lanes(e);
            if ins.is_empty() {
                continue;
            }
            let u = net.outward(e);
            approaches.push((e, u[1].atan2(u[0])));
        }
        if !approaches.is_empty() {
            appr.push((ni, approaches));
        }
    }
    // controlador de cada nó: o código externo, ou o próprio nó (chave negativa)
    let ctrl_key = |ni: usize| match ext.and_then(|m| m.get(&net.nodes[ni].osm_id)) {
        Some(&c) => c as i64,
        None => -(ni as i64) - 1,
    };
    let mut members: HashMap<i64, Vec<usize>> = HashMap::new();
    for (k, (ni, _)) in appr.iter().enumerate() {
        members.entry(ctrl_key(*ni)).or_default().push(k);
    }
    // por controlador: eixos, se é travessia em trecho e a defasagem
    let mut ctrls: HashMap<i64, (Vec<f64>, bool, f64)> = HashMap::new();
    for (key, mut ms) in members {
        ms.sort_by_key(|&k| net.nodes[appr[k].0].osm_id);
        let axes = axes_of(ms.iter().flat_map(|&k| appr[k].1.iter().map(|a| &a.1)));
        // travessia em trecho: um eixo só e os dois sentidos (mão dupla, ou as duas mãos de um mesmo controlador)
        let midblock = axes.len() == 1 && ms.iter().map(|&k| appr[k].1.len()).sum::<usize>() >= 2;
        // defasagem do controlador: a do primeiro nó editado (a interface grava a mesma em todos)
        let offset = ms.iter().find_map(|&k| edits.sigs.get(&net.nodes[appr[k].0].osm_id)).map_or(0.0, |e| e.offset);
        ctrls.insert(key, (axes, midblock, offset));
    }
    for (ni, approaches) in &appr {
        let ni = *ni;
        let node = &net.nodes[ni];
        let (axes, midblock, offset) = &ctrls[&ctrl_key(ni)];
        let edit = edits.sigs.get(&node.osm_id);
        let mut phases: Vec<Phase> = match edit.and_then(|e| e.phases.as_ref()) {
            Some(list) => {
                let keys: Vec<(u32, String)> = approaches.iter().map(|(e, _)| (*e, approach_key(net, ni as u32, *e))).collect();
                let mut ps: Vec<Phase> = list
                    .iter()
                    .map(|p| Phase {
                        green: p.green.clamp(0.0, 600.0),
                        yellow: p.yellow.clamp(0.0, 60.0),
                        all_red: p.all_red.clamp(0.0, 60.0),
                        ends: p.approaches.iter().filter_map(|k| keys.iter().find(|x| &x.1 == k).map(|x| x.0)).collect(),
                    })
                    .collect();
                // toda aproximação precisa ser atendida: as não listadas ganham uma fase própria
                let listed: std::collections::HashSet<u32> = ps.iter().flat_map(|p| p.ends.iter().copied()).collect();
                let rest: Vec<u32> = approaches.iter().map(|a| a.0).filter(|e| !listed.contains(e)).collect();
                if !rest.is_empty() {
                    ps.push(Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: rest });
                }
                ps
            }
            // uma fase por eixo do controlador; um nó sem aproximação naquele eixo fica todo vermelho nela
            None => (0..axes.len())
                .map(|k| Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: approaches.iter().filter(|a| axis_of(axes, a.1) == Some(k)).map(|a| a.0).collect() })
                .collect(),
        };
        if phases.is_empty() {
            phases.push(Phase { green: 30.0, yellow: 3.0, all_red: 2.0, ends: approaches.iter().map(|a| a.0).collect() });
        }
        // semáforo em trecho (sem cruzamento): fase própria para a travessia de pedestres, que para os dois sentidos
        if edit.map_or(true, |e| e.phases.is_none()) && *midblock {
            phases.push(Phase { green: 12.0, yellow: 0.0, all_red: 0.0, ends: Vec::new() });
        }
        let mut ends = Vec::new();
        for (e, _) in approaches {
            let s = &net.segs[(*e >> 1) as usize];
            let l = s.poly.length();
            let (sv, lat, sign) = if e & 1 == 1 {
                (l - s.cut_to, s.p0() + s.lf as f64 * s.w + 0.9, 1.0)
            } else {
                (s.cut_from, -(s.lb as f64) * s.w - 0.9, -1.0)
            };
            let (p, t) = s.poly.at(sv);
            let q = add(p, mul(right(t), lat));
            let d = mul(t, sign);
            ends.push(SignalEnd { end: *e, key: approach_key(net, ni as u32, *e), pos: [q[0] as f32, q[1] as f32], angle: d[1].atan2(d[0]) as f32 });
        }
        signals.push(Signal { node: ni as u32, offset: *offset as f32, phases, ends });
    }
    net.signals = signals;
}

#[cfg(test)]
mod tests {
    use super::lane_group;

    #[test]
    fn fork_splits_lanes_left_to_right() {
        // 4 faixas, 2 ramos: 2 + 2
        assert_eq!((lane_group(4, 2, 0), lane_group(4, 2, 1)), ((0, 2), (2, 4)));
        // 3 faixas, 2 ramos: a do meio é compartilhada, as das pontas não cruzam
        assert_eq!((lane_group(3, 2, 0), lane_group(3, 2, 1)), ((0, 2), (1, 3)));
        // 1 faixa: leva a todos os ramos; 2 faixas, 3 ramos: ninguém fica sem faixa
        assert_eq!((lane_group(1, 2, 0), lane_group(1, 2, 1)), ((0, 1), (0, 1)));
        assert_eq!((lane_group(2, 3, 0), lane_group(2, 3, 1), lane_group(2, 3, 2)), ((0, 1), (0, 2), (1, 2)));
        // um só ramo: todas as faixas
        assert_eq!(lane_group(5, 1, 0), (0, 5));
    }

    #[test]
    fn fork_alloc_by_capacity() {
        use super::fork_alloc;
        // avenida de 4 + saída de 1 à direita: as 4 seguem, só a da direita também sai
        assert_eq!(fork_alloc(4, &[4, 1], 0), vec![(0, 4), (3, 4)]);
        // saída de 2 à direita: ainda só a da direita (uma faixa compartilhada)
        assert_eq!(fork_alloc(4, &[4, 2], 0), vec![(0, 4), (3, 4)]);
        // Y de 2 faixas em ramos de 2 e 2, principal à direita: as duas seguem pelo principal, a da esquerda também pelo outro
        assert_eq!(fork_alloc(2, &[2, 2], 1), vec![(0, 1), (0, 2)]);
        // 4 faixas que se dividem em 2 + 2: blocos exclusivos
        assert_eq!(fork_alloc(4, &[2, 2], 0), vec![(0, 2), (2, 4)]);
        assert_eq!(fork_alloc(4, &[1, 1], 0), vec![(0, 2), (2, 4)]);
        // 3 faixas em ramos de 2 e 2: a do meio é compartilhada
        assert_eq!(fork_alloc(3, &[2, 2], 0), vec![(0, 2), (1, 3)]);
    }

    #[test]
    fn straight_lanes_keep_their_position() {
        use super::target_index;
        use crate::net::K_STRAIGHT;
        // 4 faixas, a da esquerda converte: as 3 que seguem vão para 1, 2, 3 (não 0, 2, 3)
        let v: Vec<usize> = (0..3).map(|t| target_index(K_STRAIGHT, t, 3, 1, 4, 0)).collect();
        assert_eq!(v, vec![1, 2, 3]);
        // ramal de 1 faixa que converge pela direita entra na faixa da direita; pela esquerda, na da esquerda
        assert_eq!(target_index(K_STRAIGHT, 0, 1, 0, 4, -1), 3);
        assert_eq!(target_index(K_STRAIGHT, 0, 1, 0, 4, 1), 0);
    }
}
