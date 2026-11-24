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
pub mod transit;

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
    ext_signals: Option<extsig::ExtSignals>,
    /// linhas de ônibus (exceções já aplicadas em `raw`), avisos das exceções e encaixe (sob demanda: refeito depois de
    /// cada edição quando alguém pede)
    transit: Option<transit::TransitData>,
    transit_avisos: Vec<String>,
    transit_fit: Option<transit::TransitFit>,
    /// edições das linhas e paradas pelo usuário (não reconstroem a rede, só o encaixe; valem na próxima simulação)
    transit_edits: transit::TransitEdits,
    /// pesos do último lote de rotas tomado (`sim_route_take`), entregues por `sim_route_pen`/`sim_route_cm`
    route_w: (Vec<f32>, Vec<f32>),
    /// edições aplicadas pelo caminho rápido (diagnóstico e testes)
    pub fast_edits: u32,
}

/// Trechos cuja edição mudou, se a mudança cabe no caminho rápido (`Net::update_segs`): vias novas, demolições,
/// semáforos ligados/desligados e inversões de sentido iguais nas duas versões.
fn seg_only_changes(old: &Edits, new: &Edits) -> Option<Vec<SegKey>> {
    if old.roads != new.roads || old.removed != new.removed {
        return None;
    }
    let enabled = |e: &Edits| e.sigs.iter().map(|(k, v)| (*k, v.enabled)).collect::<std::collections::HashMap<i64, bool>>();
    if enabled(old) != enabled(new) {
        return None;
    }
    let mut changed: Vec<SegKey> = Vec::new();
    for k in old.segs.keys().chain(new.segs.keys()) {
        let (a, b) = (old.segs.get(k), new.segs.get(k));
        if a != b {
            if a.is_some_and(|e| e.reverse) || b.is_some_and(|e| e.reverse) {
                return None;
            }
            changed.push(*k);
        }
    }
    changed.sort_unstable();
    changed.dedup();
    Some(changed)
}

fn build_parts(raw: &osm::RawOsm, base: &BaseCache, edits: &Edits, lazy: bool, ext: Option<&extsig::ExtSignals>) -> Result<Net, String> {
    let mut net = Net::build(raw, edits, base)?;
    if !lazy {
        conn::build_connections(&mut net, raw, edits);
    }
    conn::build_signals(&mut net, edits, ext);
    Ok(net)
}

fn build_network_with(data: &[u8], lazy: bool, ext: &[extsig::ExtSignal], transit: Option<transit::TransitData>) -> Result<Network, String> {
    let mut raw = osm::parse(data)?;
    let ext_signals = if ext.is_empty() { None } else { extsig::replace_signals(&mut raw, ext) };
    let transit_avisos = transit.as_ref().map(|t| transit::apply_exceptions(&mut raw, t)).unwrap_or_default();
    let edits = Edits::default();
    let base = BaseCache::new(&raw)?;
    let net = build_parts(&raw, &base, &edits, lazy, ext_signals.as_ref())?;
    let (tiles, delta) = render::update_tiles(&net, None);
    let restr = conn::restriction_index(&raw);
    Ok(Network { raw, base, edits, net, tiles, delta, restr, sim: None, lazy, ext_signals, transit, transit_avisos, transit_fit: None, transit_edits: Default::default(), route_w: Default::default(), fast_edits: 0 })
}

/// Rede com conexões por nó sob demanda (como no navegador).
pub fn build_network_lazy(data: &[u8]) -> Result<Network, String> {
    build_network_with(data, true, &[], None)
}

/// Como `build_network`, com as linhas de ônibus (exceções aplicadas no OSM) e, opcionalmente, semáforos externos.
pub fn build_network_transit(data: &[u8], ext: &[extsig::ExtSignal], transit: transit::TransitData, lazy: bool) -> Result<Network, String> {
    build_network_with(data, lazy, ext, Some(transit))
}

/// Como `build_network`, com os semáforos do OSM trocados pelos da base externa (quando ela cobre o mapa).
pub fn build_network_ext(data: &[u8], ext: &[extsig::ExtSignal]) -> Result<Network, String> {
    build_network_with(data, false, ext, None)
}

/// Rede com todas as conexões calculadas (testes, simulação).
pub fn build_network(data: &[u8]) -> Result<Network, String> {
    build_network_with(data, false, &[], None)
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
        // só o perfil de trechos existentes mudou: atualiza a rede atual em vez de reconstruir tudo
        let fast = seg_only_changes(&self.edits, &edits).and_then(|changed| self.net.update_segs(&self.raw, &self.base, &edits, &changed));
        let net = if fast.is_some() {
            self.fast_edits += 1;
            let mut net = std::mem::take(&mut self.net);
            net.conns.clear();
            net.conn_off.clear();
            if !self.lazy {
                conn::build_connections(&mut net, &self.raw, &edits);
            }
            conn::build_signals(&mut net, &edits, self.ext_signals.as_ref());
            net
        } else {
            build_parts(&self.raw, &self.base, &edits, self.lazy, self.ext_signals.as_ref())?
        };
        let prev = std::mem::take(&mut self.tiles);
        let (tiles, delta) = match &fast {
            Some(segs) => render::update_tiles_partial(&net, prev, segs),
            None => render::update_tiles(&net, Some(prev)),
        };
        self.edits = edits;
        self.net = net;
        self.sim = None; // a rede mudou: a simulação precisa recomeçar
        self.transit_fit = None;
        self.tiles = tiles;
        let ts = &self.tiles;
        self.delta.merge(delta, |id| ts.tiles.contains_key(&id));
        Ok(())
    }

    /// Inicia a simulação (JSON: `{"zones":[…],"params":{…}}`).
    pub fn start_sim(&mut self, json: &str) -> Result<(), String> {
        self.start_sim_ext(json, false)
    }

    /// Como `start_sim`; com `route_ext`, as árvores de rotas ficam para outro worker (ver `Sim::new_ext`).
    pub fn start_sim_ext(&mut self, json: &str, route_ext: bool) -> Result<(), String> {
        self.ensure_conns();
        let mut init: sim::SimInit = serde_json::from_str(json).map_err(|e| format!("simulação inválida: {e}"))?;
        init.prios = self.edits.prios.clone();
        if init.params.bus_lines {
            init.bus = self.bus_routes();
        }
        self.sim = Some(sim::Sim::new_ext(&self.net, &init, route_ext)?);
        Ok(())
    }

    /// Encaixe das linhas de ônibus na rede atual (calculado na primeira chamada depois de cada edição).
    pub fn transit_fit(&mut self) -> Option<&transit::TransitFit> {
        let t = self.transit.as_ref()?;
        if self.transit_fit.is_none() {
            self.transit_fit = Some(transit::fit(&self.net, &self.raw, &self.restr, &self.edits, t, &self.transit_edits));
        }
        self.transit_fit.as_ref()
    }

    /// Trechos de itinerário encaixados, no formato da simulação (vazio sem linhas).
    fn bus_routes(&mut self) -> Vec<sim::bus::BusRoute> {
        if self.transit_fit().is_none() {
            return Vec::new();
        }
        let (Some(t), Some(f)) = (&self.transit, &self.transit_fit) else { return Vec::new() };
        let ed = &self.transit_edits;
        let nome = |id: &str| t.paradas.get(id).and_then(|p| p.nome.clone()).or_else(|| ed.added.iter().find(|a| a.id == id).map(|a| a.nome.clone())).unwrap_or_else(|| id.to_string());
        f.trechos
            .iter()
            .filter(|r| r.ok && !ed.line_off(&t.linhas[r.linha].id))
            .map(|r| {
                let (l, p) = (&t.linhas[r.linha], &t.linhas[r.linha].padroes[r.padrao]);
                sim::bus::BusRoute {
                    linha: if l.numero.is_empty() { l.id.clone() } else { l.numero.clone() },
                    padrao: p.id.clone(),
                    caminho: r.caminho.clone(),
                    paradas: r.paradas.iter().map(|q| sim::bus::BusStopIn { passo: q.passo, lane: q.lane, s: q.s, id: q.id.clone(), nome: nome(&q.id), baia: q.baia }).collect(),
                    partidas: transit::scale_departures(&p.trechos[r.trecho].partidas, &|h| ed.factor(&l.id, h)),
                }
            })
            .collect()
    }

    /// Troca as edições das linhas e paradas (JSON de `TransitEdits`; vazio = nenhuma). Refaz só o encaixe, sob demanda.
    pub fn set_transit_edits_str(&mut self, json: &str) -> Result<(), String> {
        let ed = transit::TransitEdits::parse(json)?;
        if ed != self.transit_edits {
            self.transit_edits = ed;
            self.transit_fit = None;
        }
        Ok(())
    }

    /// Polilinhas dos trechos encaixados no eixo da faixa da direita (x, y planos) e o início de cada uma (em pontos,
    /// mais o total no fim).
    fn transit_geom(&mut self) -> (Vec<f32>, Vec<u32>) {
        let (mut pts, mut off) = (Vec::new(), vec![0u32]);
        if self.transit_fit().is_none() {
            return (pts, Vec::new());
        }
        let net = &self.net;
        for r in self.transit_fit.as_ref().unwrap().trechos.iter().filter(|r| r.ok) {
            let mut line: Vec<geo::V2> = Vec::new();
            for &(sg, d) in &r.caminho {
                let s = &net.segs[sg as usize];
                let n = if d == 0 { s.lf } else { s.lb };
                let mut p = geo::offset(&s.poly.pts, s.lane_off(d, n.saturating_sub(1)));
                if d == 1 {
                    p.reverse();
                }
                // pula o ponto repetido na junção dos passos
                let skip = usize::from(line.last().zip(p.first()).is_some_and(|(a, b)| geo::len(geo::sub(*a, *b)) < 0.5));
                line.extend(p.into_iter().skip(skip));
            }
            for q in &line {
                pts.push(q[0] as f32);
                pts.push(q[1] as f32);
            }
            off.push((pts.len() / 2) as u32);
        }
        (pts, off)
    }

    /// Avisos das exceções para ônibus que não couberam no mapa.
    pub fn transit_warnings(&self) -> &[String] {
        &self.transit_avisos
    }

    /// Garante `net.conns` completo (simulação e roteamento).
    pub fn ensure_conns(&mut self) {
        if self.net.conns.is_empty() && !self.net.nodes.is_empty() {
            conn::build_connections(&mut self.net, &self.raw, &self.edits);
        }
    }
}

/// Cópia do roteador por faixa num worker auxiliar: calcula as árvores das zonas para a simulação.
#[wasm_bindgen]
pub struct RouterHandle(sim::route::LaneRouter);

#[wasm_bindgen]
impl RouterHandle {
    pub fn from_bytes(bytes: &[u8]) -> Result<RouterHandle, JsError> {
        sim::route::LaneRouter::from_bytes(bytes).map(RouterHandle).ok_or_else(|| JsError::new("roteador inválido"))
    }
    pub fn set_weights(&mut self, pen: &[f32], change_mul: &[f32]) {
        self.0.set_weights(pen, change_mul);
    }
    /// Árvore (tempo de cada faixa até a zona) a partir das faixas-alvo.
    pub fn tree(&self, targets: &[u32]) -> Vec<f32> {
        let mut d = Vec::new();
        self.0.tree(targets, &mut d);
        d
    }
}

#[wasm_bindgen]
pub fn import_osm(data: &[u8]) -> Result<Network, JsError> {
    build_network_with(data, true, &[], None).map_err(|e| JsError::new(&e))
}

/// Importa com semáforos externos e linhas de ônibus (cada JSON vazio = sem). As exceções das linhas mudam o OSM
/// bruto: trocar de linhas pede importar de novo.
#[wasm_bindgen]
pub fn import_osm_full(data: &[u8], signals_json: &str, transit_json: &str) -> Result<Network, JsError> {
    let ext: Vec<extsig::ExtSignal> = if signals_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(signals_json).map_err(|e| JsError::new(&format!("semáforos inválidos: {e}")))?
    };
    let transit = if transit_json.trim().is_empty() { None } else { Some(transit::TransitData::parse(transit_json).map_err(|e| JsError::new(&e))?) };
    build_network_with(data, true, &ext, transit).map_err(|e| JsError::new(&e))
}

/// Importa trocando os semáforos do OSM pelos da base externa. JSON: lista de `{id, lat, lon, cruzamento}`.
/// Se nenhum ponto cai no mapa, ficam os do OSM.
#[wasm_bindgen]
pub fn import_osm_signals(data: &[u8], signals_json: &str) -> Result<Network, JsError> {
    let ext: Vec<extsig::ExtSignal> = serde_json::from_str(signals_json).map_err(|e| JsError::new(&format!("semáforos inválidos: {e}")))?;
    build_network_with(data, true, &ext, None).map_err(|e| JsError::new(&e))
}

#[wasm_bindgen]
impl Network {
    /// Resumo do encaixe das linhas de ônibus (`{}` sem linhas): trechos, encaixados, paradas e avisos.
    pub fn transit_json(&mut self) -> String {
        if self.transit_fit().is_none() {
            return "{}".into();
        }
        let (Some(t), Some(f)) = (&self.transit, &self.transit_fit) else { return "{}".into() };
        f.summary_json(t, &self.transit_avisos)
    }

    /// Edições das linhas e paradas (JSON de `TransitEdits`). Valem a partir da próxima simulação.
    pub fn set_transit_edits(&mut self, json: &str) -> Result<(), JsError> {
        self.set_transit_edits_str(json).map_err(|e| JsError::new(&e))
    }

    /// Linhas para a interface (`[]` sem linhas): por linha, os padrões e, por trecho, se encaixou, o aviso e quantas
    /// paradas; `pts` indexa `transit_off_u32` (−1 = sem desenho).
    pub fn transit_lines_json(&mut self) -> String {
        if self.transit_fit().is_none() {
            return "[]".into();
        }
        let (Some(t), Some(f)) = (&self.transit, &self.transit_fit) else { return "[]".into() };
        let mut lines: Vec<serde_json::Value> = Vec::with_capacity(t.linhas.len());
        let mut k = 0usize;
        let mut pts_i = 0i64;
        for l in &t.linhas {
            let mut pads = Vec::new();
            for p in &l.padroes {
                let mut trs = Vec::new();
                for _ in &p.trechos {
                    let r = &f.trechos[k];
                    k += 1;
                    let idx = if r.ok { pts_i += 1; pts_i - 1 } else { -1 };
                    trs.push(json!({ "ok": r.ok, "aviso": r.aviso, "paradas": r.paradas.len(), "pts": idx }));
                }
                pads.push(json!({ "id": p.id, "destino": p.destino, "trechos": trs }));
            }
            lines.push(json!({ "id": l.id, "numero": l.numero, "nome": l.nome, "padroes": pads }));
        }
        serde_json::Value::Array(lines).to_string()
    }

    /// Polilinhas dos trechos encaixados (x, y planos em metros locais), na ordem de `transit_lines_json`.
    pub fn transit_pts_f32(&mut self) -> Vec<f32> {
        self.transit_geom().0
    }

    /// Início de cada polilinha em `transit_pts_f32` (em pontos), mais o total no fim.
    pub fn transit_off_u32(&mut self) -> Vec<u32> {
        self.transit_geom().1
    }

    /// Paradas para a interface: `[{id, nome, x, y, baia, linhas, editada, nova}]`; posição no caminho encaixado (ou a
    /// do GTFS, se nenhum caminho passa por ela).
    pub fn transit_stops_json(&mut self) -> String {
        if self.transit_fit().is_none() {
            return "[]".into();
        }
        let (Some(t), Some(f)) = (&self.transit, &self.transit_fit) else { return "[]".into() };
        let ed = &self.transit_edits;
        // id -> (posição, baia, linhas)
        let mut at: std::collections::BTreeMap<&str, (geo::V2, bool, std::collections::BTreeSet<&str>)> = Default::default();
        for r in f.trechos.iter().filter(|r| r.ok) {
            let num = t.linhas[r.linha].numero.as_str();
            for p in &r.paradas {
                at.entry(p.id.as_str()).or_insert((p.pos, p.baia, Default::default())).2.insert(num);
            }
        }
        let mut out = Vec::new();
        let mut push = |id: &str, nome: &str, ll: (f64, f64), baia: bool, nova: bool| {
            let e = ed.stops.get(id);
            if e.is_some_and(|e| e.removed) {
                return;
            }
            let (pos, baia, lines) = match at.get(id) {
                Some((p, b, ls)) => (*p, *b, ls.iter().copied().collect::<Vec<_>>()),
                None => (geo::latlon_to_xy(&self.net.geo, ll.0, ll.1), e.and_then(|e| e.baia).unwrap_or(baia), Vec::new()),
            };
            out.push(json!({ "id": id, "nome": nome, "x": pos[0], "y": pos[1], "baia": baia, "linhas": lines, "editada": e.is_some(), "nova": nova }));
        };
        let mut ids: Vec<&String> = t.paradas.keys().collect();
        ids.sort();
        for id in ids {
            let p = &t.paradas[id];
            if p.removida {
                continue;
            }
            let ll = ed.stops.get(id.as_str()).and_then(|e| e.pos).map_or((p.lat, p.lon), |q| (q[0], q[1]));
            // só as do mapa: as que algum trecho encaixado usa, ou dentro dos limites da rede
            let xy = geo::latlon_to_xy(&self.net.geo, ll.0, ll.1);
            let b = self.net.bounds;
            if !at.contains_key(id.as_str()) && !(xy[0] >= b[0] && xy[0] <= b[2] && xy[1] >= b[1] && xy[1] <= b[3]) {
                continue;
            }
            push(id, p.nome.as_deref().unwrap_or(id), ll, p.baia, false);
        }
        for a in &ed.added {
            push(&a.id, &a.nome, (a.pos[0], a.pos[1]), a.baia, true);
        }
        serde_json::Value::Array(out).to_string()
    }

    pub fn meta_json(&self) -> String {
        // structs com Serialize em vez de `json!`: no Recife (1.200 semáforos) o `Value` montado nó a nó custava ~20 ms
        // por edição
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PhaseJ<'a> {
            green: f32,
            yellow: f32,
            all_red: f32,
            ends: &'a [u32],
        }
        #[derive(serde::Serialize)]
        struct EndJ<'a> {
            end: u32,
            key: &'a str,
            x: f32,
            y: f32,
            a: f32,
        }
        #[derive(serde::Serialize)]
        struct SignalJ<'a, X: serde::Serialize> {
            node: u32,
            osm: f64,
            ext: Option<X>,
            offset: f32,
            phases: Vec<PhaseJ<'a>>,
            ends: Vec<EndJ<'a>>,
        }
        #[derive(serde::Serialize)]
        struct Counts {
            nodes: usize,
            segments: usize,
            lanes: usize,
            connections: usize,
            signals: usize,
        }
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct MetaJ<'a, X: serde::Serialize> {
            bounds: [f64; 4],
            geo: [f64; 3],
            counts: Counts,
            names: &'a [String],
            signals: Vec<SignalJ<'a, X>>,
            signal_source: &'static str,
        }
        let n = &self.net;
        let signals = n
            .signals
            .iter()
            .map(|s| SignalJ {
                node: s.node,
                osm: n.nodes[s.node as usize].osm_id as f64,
                ext: self.ext_signals.as_ref().and_then(|m| m.nodes.get(&n.nodes[s.node as usize].osm_id)),
                offset: s.offset,
                phases: s.phases.iter().map(|p| PhaseJ { green: p.green, yellow: p.yellow, all_red: p.all_red, ends: &p.ends }).collect(),
                ends: s.ends.iter().map(|e| EndJ { end: e.end, key: &e.key, x: e.pos[0], y: e.pos[1], a: e.angle }).collect(),
            })
            .collect();
        let meta = MetaJ {
            bounds: n.bounds,
            geo: n.geo,
            counts: Counts { nodes: n.nodes.len(), segments: n.segs.len(), lanes: n.lanes.len(), connections: n.conns.len(), signals: n.signals.len() },
            names: &n.names,
            signals,
            signal_source: if self.ext_signals.is_some() { "ext" } else { "osm" },
        };
        serde_json::to_string(&meta).unwrap_or_default()
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
    /// Inicia com as árvores de rotas calculadas por outros workers (`RouterHandle`): as iniciais chegam por
    /// `sim_init_put`, os lotes em tempo real saem por `sim_route_take` e voltam por `sim_route_put`.
    pub fn sim_init_ext(&mut self, json: &str) -> Result<(), JsError> {
        self.start_sim_ext(json, true).map_err(|e| JsError::new(&e))
    }
    /// Grafo do roteador por faixa (bytes para `RouterHandle::from_bytes`).
    pub fn sim_router_bytes(&self) -> Vec<u8> {
        self.sim.as_ref().map_or(Vec::new(), |s| s.router_bytes())
    }
    pub fn sim_zone_count(&self) -> u32 {
        self.sim.as_ref().map_or(0, |s| s.zone_count() as u32)
    }
    /// Faixas-alvo da zona `z` (início da árvore dela).
    pub fn sim_zone_targets(&self, z: u32) -> Vec<u32> {
        self.sim.as_ref().map_or(Vec::new(), |s| s.zone_targets(z as usize).to_vec())
    }
    pub fn sim_init_put(&mut self, z: u32, tree: Vec<f32>) {
        if let Some(s) = self.sim.as_mut() {
            s.init_put(&self.net, z as usize, tree);
        }
    }
    pub fn sim_init_missing(&self) -> u32 {
        self.sim.as_ref().map_or(0, |s| s.init_missing() as u32)
    }
    /// Zonas do lote recém-pedido (vazio se nenhum); os pesos vêm em seguida por `sim_route_pen`/`sim_route_cm`.
    pub fn sim_route_take(&mut self) -> Vec<u32> {
        let Some(s) = self.sim.as_mut() else { return Vec::new() };
        match s.route_take() {
            Some((zones, pen, cm)) => {
                self.route_w = (pen.to_vec(), cm.to_vec());
                zones
            }
            None => Vec::new(),
        }
    }
    /// Penalidade de cada conversão no lote tomado por último.
    pub fn sim_route_pen(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.route_w.0)
    }
    /// Multiplicador da troca de faixa no lote tomado por último.
    pub fn sim_route_cm(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.route_w.1)
    }
    pub fn sim_route_put(&mut self, z: u32, tree: Vec<f32>) {
        if let Some(s) = self.sim.as_mut() {
            s.route_put(z, tree);
        }
    }
    /// O próximo passo depende de árvores que ainda não chegaram (sem elas, `sim_step` as calcula aqui).
    pub fn sim_route_wait(&self) -> bool {
        self.sim.as_ref().is_some_and(|s| s.route_wait())
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
    /// Recálculos da matriz OD feitos até agora (diagnóstico de passos longos no worker).
    pub fn sim_matrix_refreshes(&self) -> u32 {
        self.sim.as_ref().map_or(0, |s| s.matrix_refreshes)
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
                "boxEntries": s.stats.box_entries,
                "boxEntriesBlockers": s.stats.box_entries_blockers,
                "boxStops": s.stats.box_stops,
                "boxWait": s.stats.box_wait_s,
                "junctionOverlaps": s.stats.junction_overlaps,
                "junctionOverlapsGeom": s.stats.junction_overlaps_geom,
                "occDeadlocks": s.stats.occ_deadlocks,
                "busesActive": s.bus_summary().0,
                "busTripsDone": s.bus_summary().1,
                "busSpeedKmh": s.bus_summary().2,
                "busStops": s.stats.bus_stops,
                "busStopsMissed": s.stats.bus_stops_missed,
                "carsAtBusStop": s.stats.cars_at_bus_stop,
                "motosActive": s.motos_alive(),
                "motoSpawned": s.stats.moto_spawned,
                "motoFinished": s.stats.moto_finished,
                "motosInCorridor": s.motos_in_corridor(),
                "corridorEntries": s.stats.corridor_entries,
                "motoRedGo": s.stats.moto_red_go,
                "motosContra": s.motos_in_contra(),
                "contraEntries": s.stats.contra_entries,
            })
            .to_string(),
            None => "{}".to_string(),
        }
    }
    /// Métricas dos ônibus por linha (JSON; `{}` sem simulação ou sem linhas).
    /// Parada `id` na simulação (JSON: linhas, próximos ônibus, espera média; `null` sem simulação ou fora das linhas).
    pub fn sim_stop_info(&self, id: &str) -> String {
        match &self.sim {
            Some(s) => s.stop_info(id).to_string(),
            None => "null".to_string(),
        }
    }
    pub fn sim_bus_report(&self) -> String {
        match &self.sim {
            Some(s) => s.bus_report().to_string(),
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
