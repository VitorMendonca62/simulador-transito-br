//! Demanda de viagens por zonas (modelo gravitacional) e resolução de zonas sobre a rede.
//!
//! Zonas são polígonos em metros locais. Cada uma gera `gen` veículos/h (× perfil horário) e atrai
//! viagens proporcionalmente a `attr` e ao decaimento exponencial com o tempo de viagem.

use crate::geo::V2;
use crate::net::Net;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ZoneIn {
    pub id: i64,
    pub poly: Vec<[f64; 2]>,
    /// veículos/h gerados no pico (fator 1 do perfil)
    pub gen: f64,
    /// peso de atração
    pub attr: f64,
    /// 24 fatores relativos (0..); vazio = plano
    pub profile: Vec<f64>,
    /// portal na divisa do mapa (entrada/saída das cidades vizinhas): polígono pequeno em volta do nó de fora,
    /// pode nascer em via expressa
    pub gateway: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct ParamsIn {
    /// sensibilidade ao tempo de viagem (1/min)
    pub beta: f64,
    /// permite viagens dentro da mesma zona
    pub intrazone: bool,
    /// hora do dia (0..24, fracionária)
    pub hour: f64,
    /// sobrescritas absolutas (veículos/h) por par "idOrigem>idDestino"
    pub od_manual: HashMap<String, f64>,
}

impl Default for ParamsIn {
    fn default() -> Self {
        ParamsIn { beta: 0.08, intrazone: false, hour: 8.0, od_manual: HashMap::new() }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Payload {
    pub zones: Vec<ZoneIn>,
    pub params: ParamsIn,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZoneOut {
    pub id: i64,
    /// nós da rede dentro do polígono
    pub nodes: usize,
    /// comprimento de via dentro da zona (m)
    pub road_m: f64,
    pub centroid: [f64; 2],
    /// geração efetiva na hora pedida (veículos/h)
    pub gen_now: f64,
    pub warning: Option<String>,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub zones: Vec<ZoneOut>,
    /// matriz OD em veículos/h na hora pedida: `matrix[i][j]`
    pub matrix: Vec<Vec<f64>>,
}

pub fn point_in_poly(p: V2, poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn centroid(poly: &[[f64; 2]]) -> [f64; 2] {
    if poly.is_empty() {
        return [0.0, 0.0];
    }
    // centróide de área; cai na média dos vértices em polígonos degenerados
    let (mut a2, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let c = p[0] * q[1] - q[0] * p[1];
        a2 += c;
        cx += (p[0] + q[0]) * c;
        cy += (p[1] + q[1]) * c;
    }
    if a2.abs() < 1e-9 {
        let n = poly.len() as f64;
        return [poly.iter().map(|p| p[0]).sum::<f64>() / n, poly.iter().map(|p| p[1]).sum::<f64>() / n];
    }
    [cx / (3.0 * a2), cy / (3.0 * a2)]
}

/// Fator do perfil horário com interpolação linear entre horas.
pub fn profile_at(profile: &[f64], hour: f64) -> f64 {
    if profile.len() != 24 {
        return 1.0;
    }
    let h = hour.rem_euclid(24.0);
    let (i, f) = (h.floor() as usize % 24, h - h.floor());
    profile[i] * (1.0 - f) + profile[(i + 1) % 24] * f
}

/// Distribuição gravitacional: cada linha soma `o[i]` (quando há ao menos um destino possível).
/// `t[i][j]` em minutos. Células `None` de `forced` seguem o modelo; `Some(v)` fixam o valor.
pub fn gravity(o: &[f64], a: &[f64], t: &[Vec<f64>], beta: f64, intrazone: bool, forced: &[Vec<Option<f64>>]) -> Vec<Vec<f64>> {
    let n = o.len();
    let mut out = vec![vec![0.0; n]; n];
    for i in 0..n {
        let mut fixed = 0.0;
        let mut w = vec![0.0; n];
        let mut wsum = 0.0;
        for j in 0..n {
            if let Some(v) = forced[i][j] {
                out[i][j] = v.max(0.0);
                fixed += out[i][j];
            } else if i != j || intrazone {
                w[j] = a[j].max(0.0) * (-beta * t[i][j]).exp();
                wsum += w[j];
            }
        }
        let rest = (o[i] - fixed).max(0.0);
        if wsum > 0.0 {
            for j in 0..n {
                if forced[i][j].is_none() {
                    out[i][j] = rest * w[j] / wsum;
                }
            }
        }
    }
    out
}

/// Resolve zonas sobre a rede e calcula a matriz OD. `travel_min(i, j)` dá o tempo livre entre
/// centróides (M1: distância/velocidade; M3: roteador).
pub fn resolve(net: &Net, p: &Payload, travel_min: &dyn Fn(usize, usize, &[[f64; 2]; 2]) -> f64) -> Resolved {
    let n = p.zones.len();
    let mut zones = Vec::with_capacity(n);
    for z in &p.zones {
        let nodes = net.nodes.iter().filter(|nd| point_in_poly(nd.pos, &z.poly)).count();
        let road_m: f64 = net
            .segs
            .iter()
            .filter(|s| {
                let mid = s.poly.at(s.poly.length() / 2.0).0;
                point_in_poly(mid, &z.poly)
            })
            .map(|s| s.poly.length())
            .sum();
        let g = z.gen * profile_at(&z.profile, p.params.hour);
        let warning = if z.poly.len() < 3 {
            Some("polígono incompleto".to_string())
        } else if road_m < 1.0 && !(z.gateway && nodes > 0) {
            Some("sem vias dentro da zona".to_string())
        } else {
            None
        };
        zones.push(ZoneOut { id: z.id, nodes, road_m, centroid: centroid(&z.poly), gen_now: if warning.is_some() { 0.0 } else { g }, warning });
    }
    let o: Vec<f64> = zones.iter().map(|z| z.gen_now).collect();
    let a: Vec<f64> = p.zones.iter().zip(&zones).map(|(z, zo)| if zo.warning.is_some() { 0.0 } else { z.attr }).collect();
    let mut t = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            t[i][j] = travel_min(i, j, &[zones[i].centroid, zones[j].centroid]);
        }
    }
    let mut forced = vec![vec![None; n]; n];
    for (i, zi) in p.zones.iter().enumerate() {
        for (j, zj) in p.zones.iter().enumerate() {
            if let Some(v) = p.params.od_manual.get(&format!("{}>{}", zi.id, zj.id)) {
                forced[i][j] = Some(*v);
            }
        }
    }
    let matrix = gravity(&o, &a, &t, p.params.beta, p.params.intrazone, &forced);
    Resolved { zones, matrix }
}

/// Estimativa de tempo de viagem sem roteador: distância em linha reta × 1,3 a 30 km/h.
pub fn straight_line_minutes(_i: usize, _j: usize, c: &[[f64; 2]; 2]) -> f64 {
    let d = ((c[0][0] - c[1][0]).powi(2) + (c[0][1] - c[1][1]).powi(2)).sqrt() * 1.3;
    d / (30.0 / 3.6) / 60.0
}
