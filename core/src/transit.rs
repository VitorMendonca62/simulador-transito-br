//! Linhas de ônibus (`public/data/onibus_recife.json`, gerado por `scripts/onibus_recife/`): exceções para ônibus
//! aplicadas no OSM bruto e encaixe de cada itinerário na rede como uma sequência de trechos dirigidos.
//!
//! As exceções mudam o `RawOsm` uma vez, antes do `BaseCache` (como os semáforos externos), e assim valem para toda
//! reconstrução. O encaixe é refeito sob demanda depois de cada edição: o itinerário é uma lista de ways OSM com
//! sentido, e cada way vira os trechos dele na rede atual (way dividido por via nova continua valendo). Itinerário que
//! não encaixa (lacuna, via demolida, conversão desligada) fica desligado com aviso; nada é pulado.

use crate::conn;
use crate::geo::*;
use crate::net::*;
use crate::osm::{RawOsm, RawRestriction, RawWay};
use serde::Deserialize;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::rc::Rc;

/// Ids dos ways sintéticos das ligações só de ônibus (`ligar`): `BUS_BASE + id` (igual ao gerador Python).
pub const BUS_BASE: i64 = 300 << 40;
/// Parada a até essa distância do caminho encaixado entra nele.
const PARADA_RAIO_M: f64 = 30.0;

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Excecao {
    /// vias que o núcleo não carrega (service de corredor ou terminal) entram como vias só de ônibus
    Incluir { ways: Vec<i64> },
    /// ligação de mão única só de ônibus entre dois nós OSM do mapa
    Ligar { id: i64, de_no: i64, para_no: i64 },
    /// ônibus também no sentido contrário de um way de mão única (faixa de ônibus no contrafluxo)
    MaoDupla { way: i64 },
    /// conversão só de ônibus no nó `no`, do way `de` para o way `para`
    Conversao { no: i64, de: i64, para: i64 },
}

#[derive(Deserialize, Clone, Debug)]
pub struct Parada {
    #[serde(default)]
    pub nome: Option<String>,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub baia: bool,
    #[serde(default)]
    pub removida: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Trecho {
    /// ids de way com sinal = sentido de passagem (positivo na ordem dos nós do way)
    pub ways: Vec<i64>,
    #[serde(default)]
    pub pontos: Vec<[f64; 2]>,
    #[serde(default)]
    pub lacunas: Vec<usize>,
    #[serde(default)]
    pub entra_na_borda: bool,
    #[serde(default)]
    pub sai_na_borda: bool,
    /// [id da parada, metros desde o início do trecho no shape do GTFS]
    #[serde(default)]
    pub paradas: Vec<(String, f64)>,
    /// segundos do dia em que cada viagem entra no trecho
    #[serde(default)]
    pub partidas: Vec<u32>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Padrao {
    pub id: String,
    #[serde(default)]
    pub destino: String,
    #[serde(default)]
    pub desligado: bool,
    pub trechos: Vec<Trecho>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Linha {
    pub id: String,
    #[serde(default)]
    pub numero: String,
    #[serde(default)]
    pub nome: String,
    #[serde(default)]
    pub desligada: bool,
    pub padroes: Vec<Padrao>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct TransitData {
    #[serde(default)]
    pub excecoes: Vec<Excecao>,
    /// vias `service` que algum itinerário atravessa (o gerador as acha): entram como `incluir`
    #[serde(default)]
    pub incluir_auto: Vec<i64>,
    #[serde(default)]
    pub paradas: HashMap<String, Parada>,
    #[serde(default)]
    pub linhas: Vec<Linha>,
}

impl TransitData {
    pub fn parse(json: &str) -> Result<TransitData, String> {
        serde_json::from_str(json).map_err(|e| format!("linhas de ônibus inválidas: {e}"))
    }
}

fn set_tag(tags: &mut Vec<(String, String)>, k: &str, v: &str) {
    match tags.iter_mut().find(|t| t.0 == k) {
        Some(t) => t.1 = v.to_string(),
        None => tags.push((k.to_string(), v.to_string())),
    }
}

/// Aplica as exceções para ônibus no OSM bruto (antes do `BaseCache`). Devolve avisos das que não couberam no mapa.
pub fn apply_exceptions(raw: &mut RawOsm, t: &TransitData) -> Vec<String> {
    let mut avisos = Vec::new();
    let pos: HashMap<i64, usize> = raw.ways.iter().enumerate().map(|(i, w)| (w.id, i)).collect();
    let incluir = t.excecoes.iter().flat_map(|e| match e {
        Excecao::Incluir { ways } => ways.as_slice(),
        _ => &[],
    });
    for &w in incluir.chain(t.incluir_auto.iter()) {
        match pos.get(&w) {
            Some(&i) => set_tag(&mut raw.ways[i].tags, "highway", "busway"),
            None => avisos.push(format!("incluir: via {w} não está no mapa")),
        }
    }
    for e in &t.excecoes {
        match *e {
            Excecao::Incluir { .. } => {}
            Excecao::Ligar { id, de_no, para_no } => {
                if raw.coord(de_no).is_none() || raw.coord(para_no).is_none() {
                    avisos.push(format!("ligar {id}: nó {de_no} ou {para_no} não está no mapa"));
                    continue;
                }
                let tags = vec![("highway".into(), "busway".into()), ("oneway".into(), "yes".into())];
                raw.ways.push(RawWay { id: BUS_BASE + id, refs: vec![de_no, para_no], tags });
            }
            Excecao::MaoDupla { way } => {
                let Some(&i) = pos.get(&way) else {
                    avisos.push(format!("mão dupla: via {way} não está no mapa"));
                    continue;
                };
                mao_dupla(&mut raw.ways[i].tags);
            }
            Excecao::Conversao { no, de, para } => {
                raw.restrictions.push(RawRestriction { kind: "bus_only".into(), from: de, via: no, to: para, bus: true });
            }
        }
    }
    avisos
}

/// Way de mão única passa a ter, no contrafluxo, uma faixa só de ônibus. As listas por faixa do sentido da mão
/// (`turn:lanes`, `bus:lanes`, `psv:lanes`) ganham o sufixo do sentido, como numa via de mão dupla.
fn mao_dupla(tags: &mut Vec<(String, String)>) {
    let get = |tags: &Vec<(String, String)>, k: &str| tags.iter().find(|t| t.0 == k).map(|t| t.1.clone());
    let rev = matches!(get(tags, "oneway").as_deref(), Some("-1") | Some("reverse"));
    let (mao, contra) = if rev { ("backward", "forward") } else { ("forward", "backward") };
    let multi = get(tags, "highway").and_then(|h| parse_highway(&h)).is_some_and(|(hw, _)| hw <= HW_TRUNK);
    let n: u8 = get(tags, &format!("lanes:{mao}"))
        .or_else(|| get(tags, "lanes"))
        .and_then(|v| v.trim().parse().ok())
        .filter(|&n: &u8| n > 0)
        .unwrap_or(if multi { 2 } else { 1 });
    for k in ["turn:lanes", "bus:lanes", "psv:lanes"] {
        if let Some(v) = get(tags, k) {
            tags.retain(|t| t.0 != k);
            set_tag(tags, &format!("{k}:{mao}"), &v);
        }
    }
    set_tag(tags, "oneway", "no");
    set_tag(tags, "lanes", &(n + 1).to_string());
    set_tag(tags, &format!("lanes:{mao}"), &n.to_string());
    set_tag(tags, &format!("lanes:{contra}"), "1");
    set_tag(tags, &format!("bus:lanes:{contra}"), "designated");
}

/// Parada encaixada: no passo `passo` do caminho, na faixa `lane`, a `s` metros do início do trecho (no sentido).
#[derive(Clone, Debug, PartialEq)]
pub struct ParadaFit {
    pub id: String,
    pub passo: u32,
    pub lane: u32,
    pub s: f64,
    pub baia: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrechoFit {
    pub linha: usize,
    pub padrao: usize,
    pub trecho: usize,
    pub ok: bool,
    /// trechos dirigidos (trecho da rede, 0 = from→to, 1 = contrário)
    pub caminho: Vec<(u32, u8)>,
    pub paradas: Vec<ParadaFit>,
    /// paradas que não couberam no caminho (a mais de 30 m ou fora de ordem)
    pub paradas_fora: u32,
    pub aviso: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitFit {
    pub trechos: Vec<TrechoFit>,
}

/// Faixas de um trecho dirigido, da esquerda para a direita.
fn lanes_of(net: &Net, seg: u32, d: u8) -> std::ops::Range<u32> {
    let s = &net.segs[seg as usize];
    if d == 0 {
        s.lane_base..s.lane_base + s.lf as u32
    } else {
        s.lane_base + s.lf as u32..s.lane_base + s.lf as u32 + s.lb as u32
    }
}

fn end_node(net: &Net, seg: u32, d: u8) -> u32 {
    let s = &net.segs[seg as usize];
    if d == 0 { s.to } else { s.from }
}

/// Polilinha do trecho dirigido no sentido de passagem.
fn travel_pts(net: &Net, seg: u32, d: u8) -> Vec<V2> {
    let mut p = net.segs[seg as usize].poly.pts.clone();
    if d == 1 {
        p.reverse();
    }
    p
}

/// Encaixa todos os trechos de todos os padrões na rede atual. Usa `net.conns` quando já existem; senão calcula as
/// conexões dos nós visitados.
pub fn fit(net: &Net, raw: &RawOsm, restr: &HashMap<i64, Vec<usize>>, edits: &Edits, t: &TransitData) -> TransitFit {
    // way -> trechos da rede
    let mut way_segs: HashMap<i64, Vec<u32>> = HashMap::new();
    for (i, s) in net.segs.iter().enumerate() {
        way_segs.entry(s.way_id).or_default().push(i as u32);
    }
    // sentido de cada trecho em relação à ordem dos nós do way bruto (pela geometria: vale para way dividido por via
    // nova, `oneway=-1` e inversão do usuário)
    let raw_pos: HashMap<i64, usize> = raw.ways.iter().enumerate().map(|(i, w)| (w.id, i)).collect();
    let mut way_line: HashMap<i64, (Vec<V2>, Vec<f64>)> = HashMap::new();
    let mut seg_fwd: HashMap<u32, bool> = HashMap::new();
    let mut fwd_of = |seg: u32| -> bool {
        if let Some(&f) = seg_fwd.get(&seg) {
            return f;
        }
        let s = &net.segs[seg as usize];
        let (pts, cum) = way_line.entry(s.way_id).or_insert_with(|| {
            let pts: Vec<V2> = raw_pos
                .get(&s.way_id)
                .map(|&i| raw.ways[i].refs.iter().filter_map(|&r| raw.coord(r)).map(|(la, lo)| latlon_to_xy(&net.geo, la as f64 * 1e-7, lo as f64 * 1e-7)).collect())
                .unwrap_or_default();
            let mut cum = vec![0.0];
            for w in pts.windows(2) {
                cum.push(cum.last().unwrap() + len(sub(w[1], w[0])));
            }
            (pts, cum)
        });
        let ch = |p: V2| {
            let (i, f, _) = nearest_on_poly(pts, p);
            cum.get(i).map_or(0.0, |c| c + f * (cum.get(i + 1).copied().unwrap_or(*c) - c))
        };
        // pontos a 1/4 e 3/4 do trecho (as pontas podem cair no mesmo vértice de um way que dá a volta)
        let l = s.poly.length();
        let f = ch(s.poly.at(l * 0.75).0) >= ch(s.poly.at(l * 0.25).0);
        seg_fwd.insert(seg, f);
        f
    };
    // conexões ligadas (para todos ou só para ônibus) por nó: (faixa de origem, faixa de destino)
    let mut conn_cache: HashMap<u32, Rc<HashSet<(u32, u32)>>> = HashMap::new();
    let mut links = |ni: u32| -> Rc<HashSet<(u32, u32)>> {
        conn_cache
            .entry(ni)
            .or_insert_with(|| {
                Rc::new({
                let on = |c: &Conn| c.state == CONN_ALLOWED || c.state == CONN_BUS;
                if !net.conns.is_empty() {
                    let (a, b) = (net.conn_off[ni as usize] as usize, net.conn_off[ni as usize + 1] as usize);
                    net.conns[a..b].iter().filter(|c| on(c)).map(|c| (c.from_lane, c.to_lane)).collect()
                } else {
                    conn::node_conns(net, ni as usize, raw, restr, edits).iter().filter(|c| on(c)).map(|c| (c.from_lane, c.to_lane)).collect()
                }
                })
            })
            .clone()
    };

    let mut out = TransitFit::default();
    for (li, linha) in t.linhas.iter().enumerate() {
        for (pi, pad) in linha.padroes.iter().enumerate() {
            for (ti, tr) in pad.trechos.iter().enumerate() {
                let mut r = TrechoFit { linha: li, padrao: pi, trecho: ti, ok: false, caminho: Vec::new(), paradas: Vec::new(), paradas_fora: 0, aviso: None };
                if linha.desligada || pad.desligado {
                    r.aviso = Some("desligado nos ajustes".into());
                } else if let Some(&i) = tr.lacunas.first() {
                    r.aviso = Some(format!("lacuna no itinerário: way {} → way {}", tr.ways[i - 1].abs(), tr.ways[i].abs()));
                } else {
                    match search(net, tr, &way_segs, &mut fwd_of, &mut links, &|ni| conn::node_conns(net, ni as usize, raw, restr, edits)) {
                        Ok(c) => {
                            r.ok = true;
                            r.caminho = c;
                            stops(net, t, tr, &mut r);
                        }
                        Err(e) => r.aviso = Some(e),
                    }
                }
                out.trechos.push(r);
            }
        }
    }
    out
}

/// Dijkstra sobre (trecho dirigido, índice em `ways`): segue o way atual ou passa ao seguinte por uma conexão ligada.
fn search(
    net: &Net,
    tr: &Trecho,
    way_segs: &HashMap<i64, Vec<u32>>,
    fwd_of: &mut impl FnMut(u32) -> bool,
    links: &mut impl FnMut(u32) -> Rc<HashSet<(u32, u32)>>,
    all_conns: &dyn Fn(u32) -> Vec<Conn>,
) -> Result<Vec<(u32, u8)>, String> {
    if tr.ways.is_empty() {
        return Err("trecho sem vias".into());
    }
    // trechos dirigidos de cada item, no sentido do itinerário e com faixa nesse sentido
    let mut dirsegs: Vec<Vec<(u32, u8)>> = Vec::with_capacity(tr.ways.len());
    for &w in &tr.ways {
        let Some(segs) = way_segs.get(&w.abs()) else {
            return Err(format!("via {} não existe mais no mapa", w.abs()));
        };
        let want = w > 0;
        let v: Vec<(u32, u8)> = segs
            .iter()
            .map(|&s| (s, if fwd_of(s) == want { 0u8 } else { 1u8 }))
            .filter(|&(s, d)| !lanes_of(net, s, d).is_empty())
            .collect();
        if v.is_empty() {
            return Err(format!("via {} não tem faixa no sentido do itinerário", w.abs()));
        }
        dirsegs.push(v);
    }
    let geo = net.geo;
    let near = |cands: &[(u32, u8)], p: Option<&[f64; 2]>| -> (u32, u8) {
        let Some(p) = p else { return cands[0] };
        let q = latlon_to_xy(&geo, p[0], p[1]);
        *cands.iter().min_by(|a, b| nearest_on_poly(&net.segs[a.0 as usize].poly.pts, q).2.total_cmp(&nearest_on_poly(&net.segs[b.0 as usize].poly.pts, q).2)).unwrap()
    };
    let start = near(&dirsegs[0], tr.pontos.first());
    let last = tr.ways.len() - 1;
    let goal = near(&dirsegs[last], tr.pontos.last());
    let key = |d: (u32, u8), k: usize| (d.0 as u64 * 2 + d.1 as u64) * tr.ways.len() as u64 + k as u64;
    #[derive(PartialEq)]
    struct Item(f64, (u32, u8), usize);
    impl Eq for Item {}
    impl PartialOrd for Item {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Item {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            o.0.total_cmp(&self.0)
        }
    }
    let mut dist: HashMap<u64, f64> = HashMap::new();
    let mut prev: HashMap<u64, ((u32, u8), usize)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(key(start, 0), 0.0);
    heap.push(Item(0.0, start, 0));
    // o começo pode já estar num item adiante (way repetido no começo)
    let mut found = None;
    let mut melhor_k = 0usize;
    while let Some(Item(c, d, k)) = heap.pop() {
        if c > *dist.get(&key(d, k)).unwrap_or(&f64::INFINITY) {
            continue;
        }
        melhor_k = melhor_k.max(k);
        if k == last && d == goal {
            found = Some((d, k));
            break;
        }
        let ni = end_node(net, d.0, d.1);
        let ls = links(ni);
        let from: Vec<u32> = lanes_of(net, d.0, d.1).collect();
        // próximo item: o mesmo way, o seguinte, ou além de itens que o itinerário só toca neste nó (o casamento
        // pega o way em que o ônibus desemboca ou do qual sai um ramo; não há metro de via entre eles)
        let mut kks = vec![k, k + 1];
        let mut j = k + 1;
        while j < last && dirsegs[j].iter().any(|&(s, _)| net.segs[s as usize].from == ni || net.segs[s as usize].to == ni) {
            j += 1;
            kks.push(j);
        }
        for kk in kks {
            if kk > last {
                continue;
            }
            for &d2 in &dirsegs[kk] {
                // sai do nó `ni` por d2
                let s2 = &net.segs[d2.0 as usize];
                let starts_here = if d2.1 == 0 { s2.from == ni } else { s2.to == ni };
                if !starts_here || (d2 == d && kk == k) {
                    continue;
                }
                let to = lanes_of(net, d2.0, d2.1);
                if !from.iter().any(|&fl| to.clone().any(|tl| ls.contains(&(fl, tl)))) {
                    continue;
                }
                let nc = c + s2.poly.length();
                let kd = key(d2, kk);
                if nc < *dist.get(&kd).unwrap_or(&f64::INFINITY) {
                    dist.insert(kd, nc);
                    prev.insert(kd, (d, k));
                    heap.push(Item(nc, d2, kk));
                }
            }
        }
    }
    let Some((mut d, mut k)) = found else {
        let k2 = (melhor_k + 1).min(last);
        let (w1, w2) = (tr.ways[melhor_k].abs(), tr.ways[k2].abs());
        // por que não passa: conversão proibida, ausente, ou as vias não se encontram no sentido do itinerário
        let mut motivo = String::from("as vias não se encontram no sentido do itinerário");
        'busca: for &d1 in &dirsegs[melhor_k] {
            let ni = end_node(net, d1.0, d1.1);
            for &d2 in &dirsegs[k2] {
                let s2 = &net.segs[d2.0 as usize];
                if (if d2.1 == 0 { s2.from } else { s2.to }) != ni {
                    continue;
                }
                let (fl, tl): (Vec<u32>, Vec<u32>) = (lanes_of(net, d1.0, d1.1).collect(), lanes_of(net, d2.0, d2.1).collect());
                let st: Vec<u8> = all_conns(ni).iter().filter(|c| fl.contains(&c.from_lane) && tl.contains(&c.to_lane)).map(|c| c.state).collect();
                let osm = net.nodes[ni as usize].osm_id;
                motivo = if st.contains(&CONN_BLOCKED) {
                    format!("conversão proibida no nó {osm}")
                } else {
                    format!("conversão ausente no nó {osm}")
                };
                break 'busca;
            }
        }
        return Err(format!("sem passagem da via {w1} para a via {w2}: {motivo}"));
    };
    let mut caminho = vec![d];
    while let Some(&(pd, pk)) = prev.get(&key(d, k)) {
        caminho.push(pd);
        d = pd;
        k = pk;
    }
    caminho.reverse();
    Ok(caminho)
}

/// Paradas do trecho projetadas em ordem no caminho encaixado, na faixa mais à direita do sentido (a de ônibus, se
/// houver).
fn stops(net: &Net, t: &TransitData, tr: &Trecho, r: &mut TrechoFit) {
    // pedaços do caminho: (passo, a, b, s no início do pedaço dentro do passo)
    let mut pecas: Vec<(u32, V2, V2, f64)> = Vec::new();
    for (i, &(s, d)) in r.caminho.iter().enumerate() {
        let pts = travel_pts(net, s, d);
        let mut acc = 0.0;
        for w in pts.windows(2) {
            pecas.push((i as u32, w[0], w[1], acc));
            acc += len(sub(w[1], w[0]));
        }
    }
    let mut desde = 0usize;
    for (id, _) in &tr.paradas {
        let Some(p) = t.paradas.get(id) else { continue };
        if p.removida {
            continue;
        }
        let q = latlon_to_xy(&net.geo, p.lat, p.lon);
        let dist = |j: usize| {
            let (_, a, b, _) = pecas[j];
            let v = sub(b, a);
            let l2 = dot(v, v);
            let f = if l2 > 0.0 { (dot(sub(q, a), v) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (len(sub(add(a, mul(v, f)), q)), f)
        };
        // primeiro pedaço a até 30 m, descendo até o mínimo local
        let Some(mut j) = (desde..pecas.len()).find(|&j| dist(j).0 <= PARADA_RAIO_M) else {
            r.paradas_fora += 1;
            continue;
        };
        while j + 1 < pecas.len() && dist(j + 1).0 < dist(j).0 {
            j += 1;
        }
        let (passo, a, b, s0) = pecas[j];
        let f = dist(j).1;
        let (seg, d) = r.caminho[passo as usize];
        let ls: Vec<u32> = lanes_of(net, seg, d).collect();
        let lane = ls.iter().rev().find(|&&l| net.lanes[l as usize].bus).or(ls.last()).copied().unwrap_or(0);
        r.paradas.push(ParadaFit { id: id.clone(), passo, lane, s: s0 + f * len(sub(b, a)), baia: p.baia });
        desde = j;
    }
}

impl TransitFit {
    /// Resumo para a interface: trechos encaixados, desligados e os avisos (até 200).
    pub fn summary_json(&self, t: &TransitData, avisos_exc: &[String]) -> String {
        #[derive(serde::Serialize)]
        struct Falha<'a> {
            linha: &'a str,
            padrao: &'a str,
            trecho: usize,
            aviso: &'a str,
        }
        #[derive(serde::Serialize)]
        struct Resumo<'a> {
            trechos: usize,
            ok: usize,
            paradas: usize,
            paradas_fora: u32,
            falhas: Vec<Falha<'a>>,
            excecoes: &'a [String],
        }
        let falhas = self
            .trechos
            .iter()
            .filter(|r| !r.ok)
            .take(200)
            .map(|r| Falha {
                linha: &t.linhas[r.linha].numero,
                padrao: &t.linhas[r.linha].padroes[r.padrao].id,
                trecho: r.trecho,
                aviso: r.aviso.as_deref().unwrap_or(""),
            })
            .collect();
        serde_json::to_string(&Resumo {
            trechos: self.trechos.len(),
            ok: self.trechos.iter().filter(|r| r.ok).count(),
            paradas: self.trechos.iter().map(|r| r.paradas.len()).sum(),
            paradas_fora: self.trechos.iter().map(|r| r.paradas_fora).sum(),
            falhas,
            excecoes: avisos_exc,
        })
        .unwrap_or_else(|_| "{}".into())
    }
}
