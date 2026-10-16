//! Núcleo do simulador de trânsito (Rust → WASM).
//! Fase 1: importação OSM/PBF, grafo viário, faixas, conexões, semáforos e malhas de desenho.

pub mod conn;
pub mod geo;
pub mod net;
pub mod osm;
pub mod render;

use net::*;
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Rede importada, mantida viva no worker; o JS lê os buffers por getters.
#[wasm_bindgen]
pub struct Network {
    /// OSM bruto, guardado para reconstruir a rede quando há edições de trecho.
    raw: osm::RawOsm,
    edits: std::collections::HashMap<SegKey, SegEdit>,
    net: Net,
    meshes: render::Meshes,
}

fn build_parts(raw: &osm::RawOsm, edits: &std::collections::HashMap<SegKey, SegEdit>) -> Result<(Net, render::Meshes), String> {
    let mut net = Net::build(raw, edits)?;
    conn::build_connections(&mut net, raw);
    conn::build_signals(&mut net);
    let meshes = render::build_meshes(&net);
    Ok((net, meshes))
}

pub fn build_network(data: &[u8]) -> Result<Network, String> {
    let raw = osm::parse(data)?;
    let edits = std::collections::HashMap::new();
    let (net, meshes) = build_parts(&raw, &edits)?;
    Ok(Network { raw, edits, net, meshes })
}

impl Network {
    /// Substitui o conjunto de edições de trecho e reconstrói a rede (JSON: lista de `SegEdit`).
    pub fn set_edits(&mut self, json: &str) -> Result<(), String> {
        let list: Vec<SegEdit> = serde_json::from_str(json).map_err(|e| format!("edições inválidas: {e}"))?;
        let edits: std::collections::HashMap<SegKey, SegEdit> = list.into_iter().map(|e| (e.key(), e)).collect();
        let (net, meshes) = build_parts(&self.raw, &edits)?;
        self.edits = edits;
        self.net = net;
        self.meshes = meshes;
        Ok(())
    }
}

#[wasm_bindgen]
pub fn import_osm(data: &[u8]) -> Result<Network, JsError> {
    build_network(data).map_err(|e| JsError::new(&e))
}

#[wasm_bindgen]
impl Network {
    pub fn meta_json(&self) -> String {
        let n = &self.net;
        let signals: Vec<_> = n
            .signals
            .iter()
            .map(|s| {
                json!({
                    "node": s.node,
                    "phases": s.phases.iter().map(|p| json!({"green": p.green, "yellow": p.yellow, "allRed": p.all_red, "ends": p.ends})).collect::<Vec<_>>(),
                    "ends": s.ends.iter().map(|e| json!({"end": e.end, "x": e.pos[0], "y": e.pos[1], "a": e.angle})).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "bounds": n.bounds,
            "counts": {"nodes": n.nodes.len(), "segments": n.segs.len(), "lanes": n.lanes.len(), "connections": n.conns.len(), "signals": n.signals.len()},
            "names": n.names,
            "signals": signals,
        })
        .to_string()
    }

    // --- nós
    pub fn node_xy(&self) -> Vec<f32> {
        self.net.nodes.iter().flat_map(|n| [n.pos[0] as f32, n.pos[1] as f32]).collect()
    }
    /// [grau, flags(bit0 semáforo, bit1 travessia)] por nó.
    pub fn node_info(&self) -> Vec<u32> {
        self.net.nodes.iter().flat_map(|n| [n.ends.len() as u32, n.signal as u32 | (n.crossing as u32) << 1]).collect()
    }
    pub fn node_osm_ids(&self) -> Vec<f64> {
        self.net.nodes.iter().map(|n| n.osm_id as f64).collect()
    }
    /// Pontas de cada nó, em ordem horária: CSR (offsets em node_end_off).
    pub fn node_ends(&self) -> Vec<u32> {
        self.net.nodes.iter().flat_map(|n| n.ends.iter().copied()).collect()
    }
    pub fn node_end_off(&self) -> Vec<u32> {
        let mut o = vec![0u32];
        for n in &self.net.nodes {
            o.push(o.last().unwrap() + n.ends.len() as u32);
        }
        o
    }
    /// Vértices intermediários das vias (nós de forma).
    pub fn shape_xy(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for s in &self.net.segs {
            let n = s.poly.pts.len();
            for p in s.poly.pts.iter().take(n.saturating_sub(1)).skip(1) {
                out.push(p[0] as f32);
                out.push(p[1] as f32);
            }
        }
        out
    }

    /// Reconstrói a rede com as edições de trecho (JSON). Os buffers antigos deixam de valer.
    pub fn apply_edits(&mut self, json: &str) -> Result<(), JsError> {
        self.set_edits(json).map_err(|e| JsError::new(&e))
    }
    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }
    /// [way, nó_inicial, nó_final] (ids OSM, orientação original) por trecho.
    pub fn seg_keys(&self) -> Vec<f64> {
        self.net.segs.iter().flat_map(|s| s.key.map(|v| v as f64)).collect()
    }
    /// Largura da faixa (m) por trecho.
    pub fn seg_w(&self) -> Vec<f32> {
        self.net.segs.iter().map(|s| s.w as f32).collect()
    }
    /// Eixo de cada trecho: pontos xy em sequência, com offsets em `seg_pts_off`.
    pub fn seg_pts(&self) -> Vec<f32> {
        self.net.segs.iter().flat_map(|s| s.poly.pts.iter().flat_map(|p| [p[0] as f32, p[1] as f32])).collect()
    }
    pub fn seg_pts_off(&self) -> Vec<u32> {
        let mut o = vec![0u32];
        for s in &self.net.segs {
            o.push(o.last().unwrap() + s.poly.pts.len() as u32);
        }
        o
    }

    // --- segmentos e faixas
    /// [from, to, lf, lb, hw, name, flags(bit0 oneway, bit1 rotatória, bit2 link), maxspeed, lane_base] por segmento.
    pub fn seg_u32(&self) -> Vec<u32> {
        self.net
            .segs
            .iter()
            .flat_map(|s| {
                [
                    s.from,
                    s.to,
                    s.lf as u32,
                    s.lb as u32,
                    s.hw as u32,
                    s.name,
                    s.oneway as u32 | (s.roundabout as u32) << 1 | (s.link as u32) << 2,
                    s.maxspeed as u32,
                    s.lane_base,
                ]
            })
            .collect()
    }
    /// [seg, dir, idx, from, to, bus] por faixa.
    pub fn lane_u32(&self) -> Vec<u32> {
        self.net.lanes.iter().flat_map(|l| [l.seg, l.dir as u32, l.idx as u32, l.from, l.to, l.bus as u32]).collect()
    }
    /// [sx, sy, sdx, sdy, ex, ey, edx, edy] por faixa.
    pub fn lane_f32(&self) -> Vec<f32> {
        self.net
            .lanes
            .iter()
            .flat_map(|l| [l.start[0], l.start[1], l.sdir[0], l.sdir[1], l.end[0], l.end[1], l.edir[0], l.edir[1]])
            .collect()
    }

    // --- conexões
    /// [nó, faixa_origem, faixa_destino, tipo(0 esq,1 frente,2 dir,3 retorno), estado(1 ligada, 0 proibida, 2 ausente)]
    pub fn conn_u32(&self) -> Vec<u32> {
        self.net.conns.iter().flat_map(|c| [c.node, c.from_lane, c.to_lane, c.kind as u32, c.state as u32]).collect()
    }
    /// Bézier cúbica (p0,p1,p2,p3) por conexão.
    pub fn conn_f32(&self) -> Vec<f32> {
        self.net.conns.iter().flat_map(|c| c.curve).collect()
    }
    pub fn conn_off(&self) -> Vec<u32> {
        self.net.conn_off.clone()
    }

    // --- malhas
    pub fn asphalt_pos(&self) -> Vec<f32> {
        self.meshes.asphalt.pos.clone()
    }
    pub fn asphalt_idx(&self) -> Vec<u32> {
        self.meshes.asphalt.idx.clone()
    }
    pub fn bus_pos(&self) -> Vec<f32> {
        self.meshes.bus.pos.clone()
    }
    pub fn bus_idx(&self) -> Vec<u32> {
        self.meshes.bus.idx.clone()
    }
    pub fn white_pos(&self) -> Vec<f32> {
        self.meshes.white.pos.clone()
    }
    pub fn white_idx(&self) -> Vec<u32> {
        self.meshes.white.idx.clone()
    }
    pub fn yellow_pos(&self) -> Vec<f32> {
        self.meshes.yellow.pos.clone()
    }
    pub fn yellow_idx(&self) -> Vec<u32> {
        self.meshes.yellow.idx.clone()
    }

    /// Candidatos a rótulo de rua: [x, y, ângulo, comprimento, rank, nome, largura]
    pub fn labels_f32(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for s in &self.net.segs {
            if s.name == NO_NAME {
                continue;
            }
            let l = s.poly.length();
            let inner = l - s.cut_from - s.cut_to;
            if inner < 10.0 {
                continue;
            }
            let count = ((inner / 90.0).floor() as usize).max(1);
            for k in 0..count {
                let sv = s.cut_from + inner * (k as f64 + 0.5) / count as f64;
                let (p, t) = s.poly.at(sv);
                let mut a = t[1].atan2(t[0]);
                if a > std::f64::consts::FRAC_PI_2 {
                    a -= std::f64::consts::PI;
                } else if a <= -std::f64::consts::FRAC_PI_2 {
                    a += std::f64::consts::PI;
                }
                let width = (s.left_ext() + s.right_ext()) as f32;
                out.extend_from_slice(&[p[0] as f32, p[1] as f32, a as f32, (inner / count as f64) as f32, s.hw as f32, s.name as f32, width]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
