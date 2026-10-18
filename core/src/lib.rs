//! Núcleo do simulador de trânsito (Rust → WASM).
//! Fase 1: importação OSM/PBF, grafo viário, faixas, conexões, semáforos e malhas de desenho.

pub mod conn;
pub mod demand;
pub mod extsig;
pub mod geo;
pub mod net;
pub mod osm;
pub mod render;
pub mod sim;

use net::*;
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Rede importada, mantida viva no worker; o JS lê os buffers por getters.
#[wasm_bindgen]
pub struct Network {
    /// OSM bruto, guardado para reconstruir a rede quando há edições de trecho.
    raw: osm::RawOsm,
    /// geometria e atributos do OSM que não dependem das edições (calculados uma vez)
    base: BaseCache,
    edits: Edits,
    net: Net,
    tiles: render::TileSet,
    /// ladrilhos regenerados / esvaziados na última (re)construção
    delta: render::TileDelta,
    restr: std::collections::HashMap<i64, Vec<usize>>,
    sim: Option<sim::Sim>,
    /// conexões por nó só sob demanda (a interface pede ao selecionar); desligado nos testes
    lazy: bool,
    /// semáforos de base externa (nó OSM -> id externo); `None` = semáforos do próprio OSM
    ext_signals: Option<std::collections::HashMap<i64, u32>>,
}

fn build_parts(raw: &osm::RawOsm, base: &BaseCache, edits: &Edits, lazy: bool, ext: Option<&std::collections::HashMap<i64, u32>>) -> Result<Net, String> {
    let mut net = Net::build(raw, edits, base)?;
    if !lazy {
        conn::build_connections(&mut net, raw, edits);
    }
    conn::build_signals(&mut net, edits, ext);
    Ok(net)
}

fn build_network_with(data: &[u8], lazy: bool, ext: &[extsig::ExtSignal]) -> Result<Network, String> {
    let mut raw = osm::parse(data)?;
    let ext_signals = if ext.is_empty() { None } else { extsig::replace_signals(&mut raw, ext) };
    let edits = Edits::default();
    let base = BaseCache::new(&raw)?;
    let net = build_parts(&raw, &base, &edits, lazy, ext_signals.as_ref())?;
    let (tiles, delta) = render::update_tiles(&net, None);
    let restr = conn::restriction_index(&raw);
    Ok(Network { raw, base, edits, net, tiles, delta, restr, sim: None, lazy, ext_signals })
}

/// Rede com conexões por nó sob demanda (como no navegador).
pub fn build_network_lazy(data: &[u8]) -> Result<Network, String> {
    build_network_with(data, true, &[])
}

/// Como `build_network`, com os semáforos do OSM trocados pelos da base externa (quando ela cobre o mapa).
pub fn build_network_ext(data: &[u8], ext: &[extsig::ExtSignal]) -> Result<Network, String> {
    build_network_with(data, false, ext)
}

/// Rede com todas as conexões calculadas (testes, simulação).
pub fn build_network(data: &[u8]) -> Result<Network, String> {
    build_network_with(data, false, &[])
}

impl Network {
    /// Substitui as edições e reconstrói a rede. JSON: `{"segments","signals","roads","removed","connections"}`
    /// (também aceita uma lista simples de `SegEdit`).
    pub fn set_edits(&mut self, json: &str) -> Result<(), String> {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct Payload {
            segments: Vec<SegEdit>,
            signals: Vec<SigEdit>,
            roads: Vec<NewRoad>,
            removed: Vec<SegKey>,
            connections: std::collections::HashMap<String, u8>,
            priorities: std::collections::HashMap<String, u8>,
        }
        let payload: Payload = match serde_json::from_str::<Payload>(json) {
            Ok(p) => p,
            Err(_) => Payload {
                segments: serde_json::from_str(json).map_err(|e| format!("edições inválidas: {e}"))?,
                ..Default::default()
            },
        };
        let mut edits = Edits {
            segs: payload.segments.into_iter().map(|e| (e.key(), e)).collect(),
            sigs: payload.signals.into_iter().map(|e| (e.node, e)).collect(),
            roads: payload.roads,
            removed: payload.removed.into_iter().collect(),
            conns: Default::default(),
            prios: payload.priorities.into_iter().filter(|(_, v)| (1..=3).contains(v)).collect(),
        };
        edits.set_conns(payload.connections);
        let net = build_parts(&self.raw, &self.base, &edits, self.lazy, self.ext_signals.as_ref())?;
        let prev = std::mem::take(&mut self.tiles);
        let (tiles, delta) = render::update_tiles(&net, Some(prev));
        self.edits = edits;
        self.net = net;
        self.sim = None; // a rede mudou: a simulação precisa recomeçar
        self.tiles = tiles;
        let ts = &self.tiles;
        self.delta.merge(delta, |id| ts.tiles.contains_key(&id));
        Ok(())
    }

    /// Inicia a simulação (JSON: `{"zones":[…],"params":{…}}`).
    pub fn start_sim(&mut self, json: &str) -> Result<(), String> {
        self.ensure_conns();
        let mut init: sim::SimInit = serde_json::from_str(json).map_err(|e| format!("simulação inválida: {e}"))?;
        init.prios = self.edits.prios.clone();
        self.sim = Some(sim::Sim::new(&self.net, &init)?);
        Ok(())
    }

    /// Garante `net.conns` completo (simulação e roteamento).
    pub fn ensure_conns(&mut self) {
        if self.net.conns.is_empty() && !self.net.nodes.is_empty() {
            conn::build_connections(&mut self.net, &self.raw, &self.edits);
        }
    }
}

#[wasm_bindgen]
pub fn import_osm(data: &[u8]) -> Result<Network, JsError> {
    build_network_with(data, true, &[]).map_err(|e| JsError::new(&e))
}

/// Importa trocando os semáforos do OSM pelos da base externa. JSON: lista de `{id, lat, lon, cruzamento}`.
/// Se nenhum ponto cai no mapa, ficam os do OSM.
#[wasm_bindgen]
pub fn import_osm_signals(data: &[u8], signals_json: &str) -> Result<Network, JsError> {
    let ext: Vec<extsig::ExtSignal> = serde_json::from_str(signals_json).map_err(|e| JsError::new(&format!("semáforos inválidos: {e}")))?;
    build_network_with(data, true, &ext).map_err(|e| JsError::new(&e))
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
                    "osm": n.nodes[s.node as usize].osm_id as f64,
                    "ext": self.ext_signals.as_ref().and_then(|m| m.get(&n.nodes[s.node as usize].osm_id)),
                    "offset": s.offset,
                    "phases": s.phases.iter().map(|p| json!({"green": p.green, "yellow": p.yellow, "allRed": p.all_red, "ends": p.ends})).collect::<Vec<_>>(),
                    "ends": s.ends.iter().map(|e| json!({"end": e.end, "key": e.key, "x": e.pos[0], "y": e.pos[1], "a": e.angle})).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "bounds": n.bounds,
            "geo": n.geo,
            "counts": {"nodes": n.nodes.len(), "segments": n.segs.len(), "lanes": n.lanes.len(), "connections": n.conns.len(), "signals": n.signals.len()},
            "names": n.names,
            "signals": signals,
            "signalSource": if self.ext_signals.is_some() { "ext" } else { "osm" },
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

    /// Resolve zonas sobre a rede e devolve a matriz OD (JSON). Entrada: `{"zones":[…],"params":{…}}`.
    pub fn demand_json(&self, json: &str) -> Result<String, JsError> {
        let payload: demand::Payload = serde_json::from_str(json).map_err(|e| JsError::new(&format!("demanda inválida: {e}")))?;
        let r = demand::resolve(&self.net, &payload, &demand::straight_line_minutes);
        serde_json::to_string(&r).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Reconstrói a rede com as edições de trecho (JSON). Os buffers antigos deixam de valer.
    pub fn apply_edits(&mut self, json: &str) -> Result<(), JsError> {
        self.set_edits(json).map_err(|e| JsError::new(&e))
    }
    pub fn edit_count(&self) -> usize {
        self.edits.segs.len() + self.edits.sigs.len() + self.edits.roads.len() + self.edits.removed.len()
    }
    /// [way, nó_inicial, nó_final] (ids OSM, orientação original) por trecho.
    pub fn seg_keys(&self) -> Vec<f64> {
        self.net.segs.iter().flat_map(|s| s.key.map(|v| v as f64)).collect()
    }
    /// Recuos [do início, do fim] (m) de cada trecho nos cruzamentos.
    pub fn seg_cuts(&self) -> Vec<f32> {
        self.net.segs.iter().flat_map(|s| [s.cut_from as f32, s.cut_to as f32]).collect()
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

    // --- simulação
    pub fn sim_init(&mut self, json: &str) -> Result<(), JsError> {
        self.start_sim(json).map_err(|e| JsError::new(&e))
    }
    pub fn sim_step(&mut self, ticks: u32) {
        if let Some(s) = self.sim.as_mut() {
            for _ in 0..ticks {
                s.step(&self.net);
            }
        }
    }
    pub fn sim_active(&self) -> bool {
        self.sim.is_some()
    }
    pub fn sim_time(&self) -> f64 {
        self.sim.as_ref().map_or(0.0, |s| s.time)
    }
    /// Veículos indexados pelo slot (id = posição): [x, y, dx, dy, velocidade, comprimento (negativo = ônibus)] cada; slot livre = zeros.
    pub fn sim_snapshot(&self) -> Vec<f32> {
        let mut out = Vec::new();
        if let Some(s) = &self.sim {
            s.snapshot(&self.net, &mut out);
        }
        out
    }
    /// Rota restante do veículo `id` (JSON); `{"alive":false}` se já saiu.
    pub fn sim_track(&mut self, id: u32) -> String {
        let net = &self.net;
        match self.sim.as_mut() {
            Some(s) => match s.track(net, id as usize) {
                Some((pts, zone, kind, v, wait)) => {
                    let info = s.explain(net, id as usize);
                    json!({ "alive": true, "id": id, "zone": zone, "kind": kind, "v": v, "wait": wait, "pts": pts, "info": info }).to_string()
                }
                None => json!({ "alive": false, "id": id }).to_string(),
            },
            None => json!({ "alive": false, "id": id }).to_string(),
        }
    }
    /// Os `n` veículos parados há mais tempo na frente de uma faixa, com o motivo (JSON).
    pub fn sim_stuck(&mut self, n: u32) -> String {
        let net = &self.net;
        match self.sim.as_mut() {
            Some(s) => serde_json::Value::Array(s.stuck_list(net, n as usize)).to_string(),
            None => "[]".to_string(),
        }
    }
    pub fn sim_stats(&self) -> String {
        match &self.sim {
            Some(s) => json!({
                "time": s.time,
                "alive": s.n_alive,
                "spawned": s.stats.spawned,
                "finished": s.stats.finished,
                "lost": s.stats.lost,
                "queued": s.stats.queued,
                "meanTravel": if s.stats.finished > 0 { s.stats.travel_sum / s.stats.finished as f64 } else { 0.0 },
                "redRuns": s.stats.red_runs,
                "laneChanges": s.stats.lane_changes,
                "illegalMoves": s.stats.illegal_moves,
                "harshBrakes": s.stats.harsh_brakes,
                "solidCrossings": s.stats.solid_crossings,
                "skipped": s.stats.skipped,
            })
            .to_string(),
            None => "{}".to_string(),
        }
    }
    /// Mapa de calor atual por trecho direcionado (índice = trecho*2 + sentido): 0..254, 255 sem dados.
    /// `kind` 0 = razão de velocidade, 1 = fração parada.
    pub fn sim_heat(&self, kind: u8) -> Vec<u8> {
        self.sim.as_ref().map_or(Vec::new(), |s| s.metrics.heat(kind))
    }
    /// Quantidade de baldes de 60 s já finalizados (para o replay).
    pub fn sim_hist_len(&self) -> u32 {
        self.sim.as_ref().map_or(0, |s| s.metrics.hist_speed.len() as u32)
    }
    /// Razão de velocidade por trecho direcionado no balde `b`.
    pub fn sim_hist(&self, b: u32) -> Vec<u8> {
        self.sim.as_ref().and_then(|s| s.metrics.hist_speed.get(b as usize).cloned()).unwrap_or_default()
    }
    /// Série global: por balde [t, vivos, concluídos, velocidade média (km/h), fila de entrada, 0].
    pub fn sim_series(&self) -> Vec<f32> {
        self.sim.as_ref().map_or(Vec::new(), |s| s.metrics.series_flat())
    }
    /// Relatório consolidado (JSON): viagens, atrasos, vazão, OD, cruzamentos mais atrasados.
    pub fn sim_report(&self) -> String {
        self.sim.as_ref().map_or("{}".to_string(), |s| s.report(&self.net).to_string())
    }
    /// Razão média de velocidade por trecho direcionado em toda a execução.
    pub fn sim_run_ratio(&self) -> Vec<u8> {
        self.sim.as_ref().map_or(Vec::new(), |s| s.run_ratio(&self.net))
    }
    pub fn sim_stop(&mut self) {
        self.sim = None;
    }

    // --- conexões (por nó, sob demanda)
    /// Atualiza só as conversões editadas (sem reconstruir a rede). JSON: mapa chave -> estado.
    pub fn set_conn_edits(&mut self, json: &str) -> Result<(), JsError> {
        let m: std::collections::HashMap<String, u8> = serde_json::from_str(json).map_err(|e| JsError::new(&format!("conversões inválidas: {e}")))?;
        self.edits.set_conns(m);
        self.net.conns.clear();
        self.net.conn_off.clear();
        Ok(())
    }
    /// Atualiza a sinalização de preferência (sem reconstruir; vale a partir da próxima simulação). JSON: chave -> nível.
    pub fn set_prio_edits(&mut self, json: &str) -> Result<(), JsError> {
        let m: std::collections::HashMap<String, u8> = serde_json::from_str(json).map_err(|e| JsError::new(&format!("preferências inválidas: {e}")))?;
        self.edits.prios = m.into_iter().filter(|(_, v)| (1..=3).contains(v)).collect();
        Ok(())
    }
    /// Aproximações do nó (ramos com faixa de entrada) para colocar placas: JSON `[{key,x,y,dx,dy,name,hw,level,auto}]`.
    /// `auto`: 1 = pelo mapa esta via tem preferência, 2 = deve ceder.
    pub fn node_approaches_json(&self, node: u32) -> String {
        let n = &self.net;
        let ni = node as usize;
        if ni >= n.nodes.len() {
            return "[]".into();
        }
        let osm = n.nodes[ni].osm_id;
        let mut ends: Vec<u32> = Vec::new();
        let mut sum: std::collections::HashMap<u32, (f64, f64, f64, f64, f64)> = Default::default();
        for l in n.lanes.iter().filter(|l| l.to == node) {
            let e = l.seg * 2 + if l.dir == 0 { 1 } else { 0 };
            let a = sum.entry(e).or_insert_with(|| {
                ends.push(e);
                (0.0, 0.0, 0.0, 0.0, 0.0)
            });
            a.0 += l.end[0] as f64;
            a.1 += l.end[1] as f64;
            a.2 += l.edir[0] as f64;
            a.3 += l.edir[1] as f64;
            a.4 += 1.0;
        }
        let best = ends.iter().map(|e| sim::approach_rank(n, *e)).min().unwrap_or(0);
        let out: Vec<_> = ends
            .iter()
            .map(|e| {
                let (x, y, dx, dy, c) = sum[e];
                let s = &n.segs[(*e >> 1) as usize];
                let key = approach_key(n, node, *e);
                let level = self.edits.prios.get(&format!("{osm}|{key}")).copied().unwrap_or(0);
                let len = (dx * dx + dy * dy).sqrt().max(1e-9);
                json!({
                    "key": key, "x": x / c, "y": y / c, "dx": dx / len, "dy": dy / len,
                    "name": n.names.get(s.name as usize).cloned().unwrap_or_default(),
                    "level": level,
                    "auto": if sim::approach_rank(n, *e) <= best { 1 } else { 2 },
                })
            })
            .collect();
        serde_json::to_string(&out).unwrap_or_else(|_| "[]".into())
    }
    /// [faixa_origem, faixa_destino, tipo, estado, estado_padrão] por conexão do nó.
    pub fn node_conns_u32(&self, node: u32) -> Vec<u32> {
        conn::node_conns(&self.net, node as usize, &self.raw, &self.restr, &self.edits)
            .iter()
            .flat_map(|c| [c.from_lane, c.to_lane, c.kind as u32, c.state as u32, c.base as u32])
            .collect()
    }
    /// Bézier (8 floats) por conexão do nó, na mesma ordem de `node_conns_u32`.
    pub fn node_conns_f32(&self, node: u32) -> Vec<f32> {
        conn::node_conns(&self.net, node as usize, &self.raw, &self.restr, &self.edits).iter().flat_map(|c| c.curve).collect()
    }

    // --- conexões (todas; só se foram construídas)
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

    // --- malhas por ladrilho (só os que mudaram na última construção)
    /// Marca o delta de ladrilhos como entregue (chamar depois de ler `tiles_*`).
    pub fn clear_delta(&mut self) {
        self.delta = render::TileDelta::default();
    }
    pub fn tiles_dirty(&self) -> Vec<u32> {
        self.delta.dirty.clone()
    }
    pub fn tiles_removed(&self) -> Vec<u32> {
        self.delta.removed.clone()
    }
    /// Por ladrilho sujo: [id, tamanhos de pos×7 (asfalto, ônibus, branco, amarelo, forma, nós menores, cruzamentos),
    /// tamanhos de idx×4, e da camada elevada: tamanhos de pos×4 (asfalto, ônibus, branco, amarelo), de idx×4].
    pub fn tiles_index(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for id in &self.delta.dirty {
            let t = &self.tiles.tiles[id];
            out.push(*id);
            out.extend([t.asphalt.pos.len(), t.bus.pos.len(), t.white.pos.len(), t.yellow.pos.len(), t.shape.len(), t.minor.len(), t.major.len()].map(|v| v as u32));
            out.extend([t.asphalt.idx.len(), t.bus.idx.len(), t.white.idx.len(), t.yellow.idx.len()].map(|v| v as u32));
            match &t.hi {
                Some(h) => {
                    out.extend([h.asphalt.pos.len(), h.bus.pos.len(), h.white.pos.len(), h.yellow.pos.len()].map(|v| v as u32));
                    out.extend([h.asphalt.idx.len(), h.bus.idx.len(), h.white.idx.len(), h.yellow.idx.len()].map(|v| v as u32));
                }
                None => out.extend([0; 8]),
            }
        }
        out
    }
    /// Posições dos ladrilhos sujos, concatenadas na ordem de `tiles_index`.
    pub fn tiles_pos(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for id in &self.delta.dirty {
            let t = &self.tiles.tiles[id];
            for v in [&t.asphalt.pos, &t.bus.pos, &t.white.pos, &t.yellow.pos, &t.shape, &t.minor, &t.major] {
                out.extend_from_slice(v);
            }
            if let Some(h) = &t.hi {
                for v in [&h.asphalt.pos, &h.bus.pos, &h.white.pos, &h.yellow.pos] {
                    out.extend_from_slice(v);
                }
            }
        }
        out
    }
    /// Índices (locais a cada malha) dos ladrilhos sujos, concatenados.
    pub fn tiles_idx(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for id in &self.delta.dirty {
            let t = &self.tiles.tiles[id];
            for v in [&t.asphalt.idx, &t.bus.idx, &t.white.idx, &t.yellow.idx] {
                out.extend_from_slice(v);
            }
            if let Some(h) = &t.hi {
                for v in [&h.asphalt.idx, &h.bus.idx, &h.white.idx, &h.yellow.idx] {
                    out.extend_from_slice(v);
                }
            }
        }
        out
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
