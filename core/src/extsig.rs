//! Semáforos de uma base externa (ex.: CTTU/Recife) no lugar dos `highway=traffic_signals` do OSM.
//!
//! Cada ponto externo é um controlador. Em cruzamento, ele marca os nós de interseção do OSM ao redor
//! (cruzamentos de pista dupla têm vários nós). Em travessia de meio de quadra, ele marca um nó em cada
//! pista paralela próxima, criando o nó quando não há vértice perto.

use crate::osm::{way_level, RawOsm};
use std::collections::HashMap;

#[derive(serde::Deserialize, Clone)]
pub struct ExtSignal {
    pub id: u32,
    pub lat: f64,
    pub lon: f64,
    /// `true` quando a fonte cita uma via transversal (cruzamento); `false` = travessia em trecho
    #[serde(default)]
    pub cruzamento: bool,
}

/// Raio em que todos os nós de interseção pertencem ao mesmo cruzamento
const R_JUNCTION: f64 = 30.0;
/// Raio máximo para achar ao menos um nó de interseção
const R_JUNCTION_MAX: f64 = 50.0;
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

/// Troca os semáforos do OSM pelos externos. Se nenhum ponto externo cai na área do mapa, nada muda e
/// devolve `None`. Senão devolve o mapa nó OSM → id externo de cada nó marcado.
pub fn replace_signals(raw: &mut RawOsm, list: &[ExtSignal]) -> Option<HashMap<i64, u32>> {
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
    let pts: Vec<&ExtSignal> = list.iter().filter(|s| inside(s)).collect();
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
    let junctions: Vec<(i64, [f64; 2])> =
        uses.iter().filter(|e| *e.1 >= 2).filter_map(|(&id, _)| raw.coord(id).map(|(la, lo)| (id, xy(la, lo)))).collect();

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
    let mut new_nodes: Vec<(i64, i32, i32)> = Vec::new();
    // (via, posição de inserção, t no trecho, id novo), aplicadas no fim de trás para frente
    let mut inserts: Vec<(usize, usize, f64, i64)> = Vec::new();
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);

    for s in pts {
        let p = [s.lon * kx, s.lat * ky];
        if s.cruzamento {
            let mut near: Vec<(f64, i64)> =
                junctions.iter().map(|(id, q)| (d2(p, *q), *id)).filter(|e| e.0 <= R_JUNCTION_MAX * R_JUNCTION_MAX).collect();
            if !near.is_empty() {
                near.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for (i, (d, id)) in near.iter().enumerate() {
                    if i == 0 || *d <= R_JUNCTION * R_JUNCTION {
                        marked.entry(*id).or_insert(s.id);
                    }
                }
                continue;
            }
            // sem nó de interseção por perto: cai na regra de trecho
        }
        // travessia em trecho: trecho mais próximo de cada via
        let mut per_way: HashMap<usize, (f64, usize, f64, [f64; 2])> = HashMap::new(); // via -> (dist², trecho, t, direção)
        for &(wi, k, a, b) in &segs {
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
        let Some(&(_, _, _, _, dir0)) = best.first() else { continue };
        for (j, &(_, wi, k, t, dir)) in best.iter().enumerate() {
            // só as pistas paralelas à mais próxima (a outra mão da avenida), não as transversais
            // e só com o pé da perpendicular dentro do trecho (senão é a continuação da mesma pista)
            if j > 0 && ((dir[0] * dir0[0] + dir[1] * dir0[1]).abs() < 0.85 || t <= 0.0 || t >= 1.0) {
                continue;
            }
            let refs = &raw.ways[wi].refs;
            let (a, b) = (raw.coord(refs[k]).unwrap(), raw.coord(refs[k + 1]).unwrap());
            let (pa, pb) = (xy(a.0, a.1), xy(b.0, b.1));
            let q = [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t];
            let id = if d2(q, pa) <= R_REUSE * R_REUSE && d2(q, pa) <= d2(q, pb) {
                refs[k]
            } else if d2(q, pb) <= R_REUSE * R_REUSE {
                refs[k + 1]
            } else {
                let nid = SYNTH_BASE + s.id as i64 * 64 + j as i64;
                let la = a.0 as f64 + (b.0 as f64 - a.0 as f64) * t;
                let lo = a.1 as f64 + (b.1 as f64 - a.1 as f64) * t;
                new_nodes.push((nid, la.round() as i32, lo.round() as i32));
                inserts.push((wi, k + 1, t, nid));
                nid
            };
            marked.entry(id).or_insert(s.id);
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
    Some(marked)
}
