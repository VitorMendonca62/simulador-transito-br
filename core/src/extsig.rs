//! Semáforos de uma base externa (ex.: CTTU/Recife) no lugar dos `highway=traffic_signals` do OSM.
//!
//! Cada ponto externo é um controlador. Em cruzamento, ele marca os nós de interseção do OSM ao redor
//! (cruzamentos de pista dupla têm vários nós). Em travessia de meio de quadra, ele marca um nó em cada
//! pista paralela próxima, criando o nó quando não há vértice perto.
//!
//! Ajustes conferidos à mão (`scripts/semaforos_recife/ajustes_manuais.json`, mesclados no JSON da CTTU) trocam o
//! encaixe automático por nós OSM e pontos explícitos, e as fases por eixo por fases definidas pelo nome das vias.

use crate::osm::{way_level, RawOsm};
use std::collections::HashMap;

#[derive(serde::Deserialize, Clone, Default)]
pub struct ExtSignal {
    pub id: u32,
    pub lat: f64,
    pub lon: f64,
    /// `true` quando a fonte cita uma via transversal (cruzamento); `false` = travessia em trecho
    #[serde(default)]
    pub cruzamento: bool,
    /// nós OSM do controlador conferidos à mão: substituem o encaixe automático (os que não existem no mapa são ignorados)
    #[serde(default)]
    pub nos: Vec<i64>,
    /// pontos `[lat, lon]` conferidos à mão onde o semáforo fica num trecho (ex.: entrada de uma rua): nó na via mais próxima
    #[serde(default)]
    pub pontos: Vec<[f64; 2]>,
    /// fases conferidas à mão, na ordem do ciclo; vazio = uma fase por eixo de aproximação
    #[serde(default)]
    pub fases: Vec<ExtPhase>,
    /// conversões proibidas `[via de origem, via de destino]` (ids de way do OSM) nos nós do ajuste,
    /// como uma restrição `no_*` do OSM (ex.: retorno por dentro de um cruzamento de mãos separadas)
    #[serde(default)]
    pub proibir: Vec<[i64; 2]>,
    /// setas por faixa conferidas à mão, no lugar do `turn:lanes` do OSM
    #[serde(default)]
    pub setas: Vec<ExtTurns>,
}

/// Setas das faixas de uma via (sintaxe do `turn:lanes` do OSM, da esquerda para a direita: `left|through;right`).
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ExtTurns {
    pub way: i64,
    pub turn: String,
    /// `forward` / `backward` para via de mão dupla; vazio = mão única
    #[serde(default)]
    pub lado: String,
}

/// Fase de um controlador externo. As aproximações entram pelo nome da via de chegada.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ExtPhase {
    /// partes do nome da via (sem diferenciar maiúsculas) ou `"way:<id>"` (way do OSM, para separar mãos de mesmo
    /// nome); `"*"` = aproximações que nenhuma outra fase cita; lista vazia = fase só de pedestres (todos no vermelho)
    #[serde(default)]
    pub vias: Vec<String>,
    #[serde(default = "d_green")]
    pub verde: f32,
    #[serde(default = "d_yellow")]
    pub amarelo: f32,
    #[serde(default = "d_all_red")]
    pub vermelho: f32,
}

fn d_green() -> f32 {
    30.0
}
fn d_yellow() -> f32 {
    3.0
}
fn d_all_red() -> f32 {
    2.0
}

impl ExtPhase {
    /// A via (nome e way do OSM) é citada pela fase (sem contar o `"*"`).
    pub fn cites(&self, name: &str, way: i64) -> bool {
        let name = name.to_lowercase();
        self.vias.iter().any(|v| match v.strip_prefix("way:") {
            Some(id) => id.parse::<i64>().ok() == Some(way),
            None => v != "*" && !v.is_empty() && name.contains(&v.to_lowercase()),
        })
    }
    pub fn rest(&self) -> bool {
        self.vias.iter().any(|v| v == "*")
    }
}

/// Semáforos externos encaixados no mapa.
#[derive(Default)]
pub struct ExtSignals {
    /// nó OSM -> id externo (controlador)
    pub nodes: HashMap<i64, u32>,
    /// fases conferidas à mão por controlador
    pub phases: HashMap<u32, Vec<ExtPhase>>,
}

/// Raio em que todos os nós de interseção pertencem ao mesmo cruzamento
const R_JUNCTION: f64 = 30.0;
/// Raio máximo para achar ao menos um nó de interseção
const R_JUNCTION_MAX: f64 = 50.0;
/// Pistas auxiliares e mãos afastadas: nó até esta distância que cruza as mesmas duas vias do cruzamento
const R_AUX: f64 = 60.0;
/// Distância máxima até a pista, na travessia em trecho
const R_MIDBLOCK: f64 = 30.0;
/// Vértice já existente a menos disto é reaproveitado em vez de criar nó
const R_REUSE: f64 = 6.0;
/// Base dos ids sintéticos (acima dos usados por `separate_levels`)
const SYNTH_BASE: i64 = 200 << 40;

fn car_way(tags: &[(String, String)]) -> bool {
    let get = |k: &str| tags.iter().find(|t| t.0 == k).map(|t| t.1.as_str());
    get("area") != Some("yes") && get("highway").and_then(crate::net::parse_highway).is_some() && way_level(tags) == 0
}

/// Trechos mais próximos de `p`, um por via (até `R_MIDBLOCK`): (dist², via, trecho, t, direção), do mais perto ao mais longe.
fn near_segs(p: [f64; 2], segs: &[(usize, usize, [f64; 2], [f64; 2])]) -> Vec<(f64, usize, usize, f64, [f64; 2])> {
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let mut per_way: HashMap<usize, (f64, usize, f64, [f64; 2])> = HashMap::new(); // via -> (dist², trecho, t, direção)
    for &(wi, k, a, b) in segs {
        let r = R_MIDBLOCK;
        if a[0].max(b[0]) < p[0] - r || a[0].min(b[0]) > p[0] + r || a[1].max(b[1]) < p[1] - r || a[1].min(b[1]) > p[1] + r {
            continue;
        }
        let ab = [b[0] - a[0], b[1] - a[1]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        if l2 < 1e-6 {
            continue;
        }
        let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0);
        let d = d2(p, [a[0] + ab[0] * t, a[1] + ab[1] * t]);
        if d <= R_MIDBLOCK * R_MIDBLOCK && per_way.get(&wi).map_or(true, |w| d < w.0) {
            let l = l2.sqrt();
            per_way.insert(wi, (d, k, t, [ab[0] / l, ab[1] / l]));
        }
    }
    let mut best: Vec<(f64, usize, usize, f64, [f64; 2])> = per_way.into_iter().map(|(wi, (d, k, t, dir))| (d, wi, k, t, dir)).collect();
    best.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
    best
}

/// Troca os semáforos do OSM pelos externos. Se nenhum ponto externo cai na área do mapa, nada muda e
/// devolve `None`. Senão devolve o nó OSM → id externo de cada nó marcado e as fases conferidas à mão.
pub fn replace_signals(raw: &mut RawOsm, list: &[ExtSignal]) -> Option<ExtSignals> {
    if raw.nodes.is_empty() {
        return None;
    }
    let (mut la0, mut la1, mut lo0, mut lo1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    let ways: Vec<usize> = (0..raw.ways.len()).filter(|&i| car_way(&raw.ways[i].tags)).collect();
    for &wi in &ways {
        for &r in &raw.ways[wi].refs {
            if let Some((la, lo)) = raw.coord(r) {
                la0 = la0.min(la);
                la1 = la1.max(la);
                lo0 = lo0.min(lo);
                lo1 = lo1.max(lo);
            }
        }
    }
    if ways.is_empty() {
        return None;
    }
    let inside = |s: &ExtSignal| {
        let (la, lo) = ((s.lat * 1e7) as i32, (s.lon * 1e7) as i32);
        la >= la0 && la <= la1 && lo >= lo0 && lo <= lo1
    };
    let mut pts: Vec<&ExtSignal> = list.iter().filter(|s| inside(s)).collect();
    // os conferidos à mão primeiro: um nó disputado fica com eles
    pts.sort_by_key(|s| s.nos.is_empty() && s.pontos.is_empty());
    if pts.is_empty() {
        return None;
    }

    // projeção local em metros
    let lat_c = (la0 as f64 + la1 as f64) / 2.0 * 1e-7;
    let kx = 111_319.49 * lat_c.to_radians().cos();
    let ky = 111_319.49;
    let xy = |la: i32, lo: i32| [lo as f64 * 1e-7 * kx, la as f64 * 1e-7 * ky];

    // nós de interseção: usados 2+ vezes pelas vias de carro
    let mut uses: HashMap<i64, u32> = HashMap::new();
    for &wi in &ways {
        let refs = &raw.ways[wi].refs;
        for (k, &r) in refs.iter().enumerate() {
            // via fechada repete o primeiro nó no fim; não conta como interseção
            if k + 1 == refs.len() && refs.len() > 2 && refs[0] == r {
                continue;
            }
            *uses.entry(r).or_default() += 1;
        }
    }
    // entradas (via, sentido) de cada nó: nó de passagem ou de bifurcação (uma entrada só) não tem conflito para o
    // semáforo resolver e, marcado, só fecha para quem acabou de passar no verde
    let mut ins: HashMap<i64, u8> = HashMap::new();
    for &wi in &ways {
        let w = &raw.ways[wi];
        let get = |k: &str| w.tags.iter().find(|t| t.0 == k).map(|t| t.1.as_str());
        let roundabout = matches!(get("junction"), Some("roundabout") | Some("circular"));
        let (fwd, bwd) = match get("oneway") {
            Some("yes") | Some("true") | Some("1") => (true, false),
            Some("-1") | Some("reverse") => (false, true),
            Some("no") | Some("false") | Some("0") => (true, true),
            _ => (true, !roundabout && get("highway") != Some("motorway")),
        };
        for k in 0..w.refs.len() {
            *ins.entry(w.refs[k]).or_default() += (fwd && k > 0) as u8 + (bwd && k + 1 < w.refs.len()) as u8;
        }
    }
    // nomes (ids) das vias de carro de cada nó
    let mut name_id: HashMap<String, u32> = HashMap::new();
    let mut names: HashMap<i64, Vec<u32>> = HashMap::new();
    for &wi in &ways {
        let w = &raw.ways[wi];
        if let Some(n) = w.tags.iter().find(|t| t.0 == "name").map(|t| t.1.clone()) {
            let k = name_id.len() as u32;
            let n = *name_id.entry(n).or_insert(k);
            for &r in &w.refs {
                let v = names.entry(r).or_default();
                if !v.contains(&n) {
                    v.push(n);
                }
            }
        }
    }
    let junctions: Vec<(i64, [f64; 2])> = uses
        .iter()
        .filter(|e| *e.1 >= 2 && ins.get(e.0).copied().unwrap_or(0) >= 2)
        .filter_map(|(&id, _)| raw.coord(id).map(|(la, lo)| (id, xy(la, lo))))
        .collect();

    // trechos das vias de carro já projetados: (via, índice do trecho, a, b)
    let mut segs: Vec<(usize, usize, [f64; 2], [f64; 2])> = Vec::new();
    for &wi in &ways {
        let refs = &raw.ways[wi].refs;
        for k in 0..refs.len().saturating_sub(1) {
            if let (Some(a), Some(b)) = (raw.coord(refs[k]), raw.coord(refs[k + 1])) {
                segs.push((wi, k, xy(a.0, a.1), xy(b.0, b.1)));
            }
        }
    }

    let mut marked: HashMap<i64, u32> = HashMap::new();
    let mut phases: HashMap<u32, Vec<ExtPhase>> = HashMap::new();
    // cruzamentos encaixados automaticamente: (id, posição, nós do núcleo), para achar as pistas auxiliares no fim
    let mut aux: Vec<(u32, [f64; 2], Vec<i64>)> = Vec::new();
    let mut new_nodes: Vec<(i64, i32, i32)> = Vec::new();
    // (via, posição de inserção, t no trecho, id novo), aplicadas no fim de trás para frente
    let mut inserts: Vec<(usize, usize, f64, i64)> = Vec::new();
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);

    // nó do ponto `t` do trecho `k` da via `wi`: o vértice a menos de `R_REUSE`, ou um nó novo de id `nid`
    let mut place = |raw: &RawOsm, wi: usize, k: usize, t: f64, nid: i64| -> i64 {
        let refs = &raw.ways[wi].refs;
        let (a, b) = (raw.coord(refs[k]).unwrap(), raw.coord(refs[k + 1]).unwrap());
        let (pa, pb) = (xy(a.0, a.1), xy(b.0, b.1));
        let q = [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t];
        if d2(q, pa) <= R_REUSE * R_REUSE && d2(q, pa) <= d2(q, pb) {
            refs[k]
        } else if d2(q, pb) <= R_REUSE * R_REUSE {
            refs[k + 1]
        } else {
            let la = a.0 as f64 + (b.0 as f64 - a.0 as f64) * t;
            let lo = a.1 as f64 + (b.1 as f64 - a.1 as f64) * t;
            new_nodes.push((nid, la.round() as i32, lo.round() as i32));
            inserts.push((wi, k + 1, t, nid));
            nid
        }
    };

    for s in pts {
        let p = [s.lon * kx, s.lat * ky];
        // conferido à mão: os nós listados e um nó na via mais próxima de cada ponto
        if !s.nos.is_empty() || !s.pontos.is_empty() {
            let mut any = false;
            for &id in &s.nos {
                if raw.coord(id).is_some() {
                    marked.entry(id).or_insert(s.id);
                    any = true;
                    for &[from, to] in &s.proibir {
                        raw.restrictions.push(crate::osm::RawRestriction { kind: "no_turn".into(), from, via: id, to });
                    }
                }
            }
            for (j, q) in s.pontos.iter().enumerate() {
                if let Some(&(_, wi, k, t, _)) = near_segs([q[1] * kx, q[0] * ky], &segs).first() {
                    let id = place(raw, wi, k, t, SYNTH_BASE + s.id as i64 * 64 + 32 + j as i64);
                    marked.entry(id).or_insert(s.id);
                    any = true;
                }
            }
            if any {
                for t in &s.setas {
                    let key = if t.lado.is_empty() { "turn:lanes".to_string() } else { format!("turn:lanes:{}", t.lado) };
                    if let Some(w) = raw.ways.iter_mut().find(|w| w.id == t.way) {
                        // a seta conferida vale sozinha naquele sentido
                        w.tags.retain(|(k, _)| k != &key && !(t.lado.is_empty() && k == "turn:lanes:forward"));
                        w.tags.push((key, t.turn.clone()));
                    }
                }
                if !s.fases.is_empty() {
                    phases.insert(s.id, s.fases.clone());
                }
                continue;
            }
            // nada do ajuste existe neste mapa (outra versão do OSM): encaixe automático
        }
        if s.cruzamento {
            let mut near: Vec<(f64, i64)> =
                junctions.iter().map(|(id, q)| (d2(p, *q), *id)).filter(|e| e.0 <= R_JUNCTION_MAX * R_JUNCTION_MAX).collect();
            if !near.is_empty() {
                near.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let mut core: Vec<i64> = Vec::new();
                for (i, (d, id)) in near.iter().enumerate() {
                    if i == 0 || *d <= R_JUNCTION * R_JUNCTION {
                        marked.entry(*id).or_insert(s.id);
                        core.push(*id);
                    }
                }
                aux.push((s.id, p, core));
                continue;
            }
            // sem nó de interseção por perto: cai na regra de trecho
        }
        // travessia em trecho: trecho mais próximo de cada via
        let best = near_segs(p, &segs);
        let Some(&(_, _, _, _, dir0)) = best.first() else { continue };
        for (j, &(_, wi, k, t, dir)) in best.iter().enumerate() {
            // só as pistas paralelas à mais próxima (a outra mão da avenida), não as transversais
            // e só com o pé da perpendicular dentro do trecho (senão é a continuação da mesma pista)
            if j > 0 && ((dir[0] * dir0[0] + dir[1] * dir0[1]).abs() < 0.85 || t <= 0.0 || t >= 1.0) {
                continue;
            }
            let id = place(raw, wi, k, t, SYNTH_BASE + s.id as i64 * 64 + j as i64);
            marked.entry(id).or_insert(s.id);
        }
    }

    // pistas auxiliares, depois que todo ponto pegou os nós dele: nó mais afastado que cruza duas das vias do
    // cruzamento (a principal e a transversal)
    for (id, p, core) in aux {
        let core_names: Vec<u32> = core.iter().flat_map(|c| names.get(c).into_iter().flatten().copied()).collect();
        for (j, q) in &junctions {
            if marked.contains_key(j) || d2(p, *q) > R_AUX * R_AUX {
                continue;
            }
            if names.get(j).map_or(0, |v| v.iter().filter(|n| core_names.contains(n)).count()) >= 2 {
                marked.insert(*j, id);
            }
        }
    }

    // mesma posição: o de maior t entra primeiro e fica depois
    inserts.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)).then(b.2.partial_cmp(&a.2).unwrap()));
    for (wi, pos, _, nid) in inserts {
        raw.ways[wi].refs.insert(pos, nid);
    }
    if !new_nodes.is_empty() {
        raw.nodes.extend(new_nodes);
        raw.nodes.sort_unstable_by_key(|n| n.0);
        raw.nodes.dedup_by_key(|n| n.0);
    }
    raw.signals = marked.keys().copied().collect();
    Some(ExtSignals { nodes: marked, phases })
}
