use crate::net::*;
use crate::*;

const SAMPLE: &str = include_str!("../../samples/sample.osm");

fn sample() -> Network {
    build_network(SAMPLE.as_bytes()).expect("sample deve importar")
}

#[test]
fn imports_sample() {
    let n = sample();
    let net = &n.net;
    assert!(net.segs.len() > 20, "segmentos: {}", net.segs.len());
    assert_eq!(net.signals.len(), 3, "3 semáforos");
    assert!(!net.conns.is_empty());
    assert_eq!(net.conn_off.len(), net.nodes.len() + 1);
    let tiles = &n.tiles.tiles;
    assert!(tiles.values().any(|t| !t.asphalt.idx.is_empty()));
    assert!(tiles.values().any(|t| !t.white.idx.is_empty()));
    assert!(tiles.values().any(|t| !t.yellow.idx.is_empty()));
    assert!(tiles.values().any(|t| !t.bus.idx.is_empty()), "faixa de ônibus");
}

#[test]
fn conn_indices_valid() {
    let n = sample();
    for c in &n.net.conns {
        let l0 = &n.net.lanes[c.from_lane as usize];
        let l1 = &n.net.lanes[c.to_lane as usize];
        assert_eq!(l0.to, c.node, "faixa de origem termina no nó");
        assert_eq!(l1.from, c.node, "faixa de destino começa no nó");
        assert!(c.state <= CONN_ABSENT);
        assert!(c.curve.iter().all(|v| v.is_finite()));
    }
    assert!(n.net.meshes_ok());
}

#[test]
fn restriction_blocks_left_turn() {
    let n = sample();
    let net = &n.net;
    let blocked: Vec<_> = net.conns.iter().filter(|c| c.state == CONN_BLOCKED).collect();
    assert!(!blocked.is_empty(), "no_left_turn deve bloquear conexões");
    assert!(blocked.iter().all(|c| c.kind == K_LEFT));
    // somente o nó via
    let nodes: std::collections::HashSet<_> = blocked.iter().map(|c| c.node).collect();
    assert_eq!(nodes.len(), 1);
}

#[test]
fn signals_have_phases_per_axis() {
    let n = sample();
    for s in &n.net.signals {
        let node = &n.net.nodes[s.node as usize];
        if node.ends.len() == 4 {
            assert_eq!(s.phases.len(), 2, "cruzamento de 4 ramos tem 2 fases");
        }
        let total: usize = s.phases.iter().map(|p| p.ends.len()).sum();
        assert_eq!(total, s.ends.len());
    }
}

#[test]
fn turn_lanes_tag_is_respected() {
    // Rua Central (sul) chega na avenida com left|through|through;right
    let n = sample();
    let net = &n.net;
    let seg = net
        .segs
        .iter()
        .position(|s| s.turn_f.iter().any(|&t| t != 0) && net.nodes[s.to as usize].ends.len() >= 4)
        .expect("segmento com turn:lanes chegando em cruzamento");
    let s = &net.segs[seg];
    let lanes: Vec<u32> = (0..s.lf as u32).map(|i| s.lane_base + i).collect();
    let kinds_of = |lane: u32| -> Vec<u8> {
        let mut k: Vec<u8> = net.conns.iter().filter(|c| c.from_lane == lane && c.state == CONN_ALLOWED).map(|c| c.kind).collect();
        k.sort();
        k.dedup();
        k
    };
    assert_eq!(kinds_of(lanes[0]), vec![K_LEFT]);
    assert_eq!(kinds_of(lanes[1]), vec![K_STRAIGHT]);
    assert_eq!(kinds_of(lanes[2]), vec![K_STRAIGHT, K_RIGHT]);
}

#[test]
fn roundabout_is_oneway_and_entries_only_turn_right() {
    let n = sample();
    let net = &n.net;
    assert!(net.segs.iter().any(|s| s.roundabout && s.oneway));
    for node in &net.nodes {
        let has_ring = node.ends.iter().any(|&e| net.segs[(e >> 1) as usize].roundabout);
        let has_road = node.ends.iter().any(|&e| !net.segs[(e >> 1) as usize].roundabout);
        if !(has_ring && has_road) {
            continue;
        }
        let ni = net.nodes.iter().position(|x| std::ptr::eq(x, node)).unwrap() as u32;
        for c in net.conns.iter().filter(|c| c.node == ni && c.state == CONN_ALLOWED) {
            let from_ring = net.segs[net.lanes[c.from_lane as usize].seg as usize].roundabout;
            let to_ring = net.segs[net.lanes[c.to_lane as usize].seg as usize].roundabout;
            assert!(from_ring || to_ring, "entrada não pode atravessar a rotatória direto");
        }
    }
}

#[test]
fn rejects_empty() {
    assert!(build_network(b"<osm></osm>").is_err());
}

#[test]
#[ignore]
fn perf_big_map() {
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let t = std::time::Instant::now();
    let n = build_network(&data).unwrap();
    eprintln!(
        "{} bytes → {} nós, {} trechos, {} faixas, {} conexões, {} tri asfalto em {:?}",
        data.len(),
        n.net.nodes.len(),
        n.net.segs.len(),
        n.net.lanes.len(),
        n.net.conns.len(),
        n.tiles.asphalt_tris(),
        t.elapsed()
    );
}

#[test]
fn pbf_matches_xml() {
    let xml = sample();
    let pbf = build_network(include_bytes!("../../samples/sample.pbf")).expect("pbf deve importar");
    assert_eq!(xml.net.nodes.len(), pbf.net.nodes.len());
    assert_eq!(xml.net.segs.len(), pbf.net.segs.len());
    assert_eq!(xml.net.lanes.len(), pbf.net.lanes.len());
    assert_eq!(xml.net.conns.len(), pbf.net.conns.len());
    assert_eq!(xml.net.signals.len(), pbf.net.signals.len());
    assert_eq!(
        xml.net.conns.iter().filter(|c| c.state == CONN_BLOCKED).count(),
        pbf.net.conns.iter().filter(|c| c.state == CONN_BLOCKED).count(),
        "restrições também vêm do PBF"
    );
}

#[test]
fn candidates_cover_every_lane_pair() {
    let n = sample();
    let net = &n.net;
    // no cruzamento central (4 ramos), toda faixa de entrada tem uma conexão (ligada ou candidata) para toda faixa de saída
    let ni = (0..net.nodes.len()).find(|&i| net.nodes[i].signal && net.nodes[i].ends.len() == 4).unwrap();
    let (mut ins, mut outs) = (std::collections::HashSet::new(), std::collections::HashSet::new());
    for &e in &net.nodes[ni].ends {
        let (i, o) = net.approach_lanes(e);
        ins.extend(i);
        outs.extend(o);
    }
    let range = net.conn_off[ni] as usize..net.conn_off[ni + 1] as usize;
    let pairs: std::collections::HashSet<_> = net.conns[range.clone()].iter().map(|c| (c.from_lane, c.to_lane)).collect();
    assert_eq!(pairs.len(), range.len(), "sem pares duplicados");
    assert_eq!(pairs.len(), ins.len() * outs.len());
    assert!(net.conns[range].iter().any(|c| c.state == CONN_ABSENT));
}

fn key_of(n: &Network, name: &str, lanes_f: u8) -> SegKey {
    let net = &n.net;
    let idx = net.names.iter().position(|x| x == name).expect("nome") as u32;
    net.segs.iter().find(|s| s.name == idx && s.lf == lanes_f).expect("trecho").key
}

fn edit_json(k: SegKey, extra: &str) -> String {
    format!("{{\"segments\":[{{\"way\":{},\"from\":{},\"to\":{},{}}}]}}", k[0], k[1], k[2], extra)
}

#[test]
fn edit_lane_count_rebuilds_everything() {
    let mut n = sample();
    let before = (n.net.lanes.len(), n.net.conns.len(), n.tiles.vertex_count(|t| t.asphalt.pos.len()));
    let k = key_of(&n, "Rua das Palmeiras", 1);
    n.set_edits(&edit_json(k, "\"oneway\":false,\"lf\":3,\"lb\":2")).unwrap();
    let seg = n.net.segs.iter().find(|s| s.key == k).unwrap();
    assert_eq!((seg.lf, seg.lb), (3, 2));
    assert!(n.net.lanes.len() > before.0, "mais faixas");
    assert!(n.net.conns.len() > before.1, "mais conexões candidatas");
    assert!(n.tiles.vertex_count(|t| t.asphalt.pos.len()) > before.2 / 2);
    assert!(n.net.meshes_ok());
    // restrições OSM continuam valendo
    assert!(n.net.conns.iter().any(|c| c.state == CONN_BLOCKED));
    // remover edições volta ao original
    n.set_edits(r#"{"segments":[]}"#).unwrap();
    assert_eq!(n.net.lanes.len(), before.0);
    assert_eq!(n.net.conns.len(), before.1);
}

#[test]
fn edit_oneway_and_reverse() {
    let mut n = sample();
    let k = key_of(&n, "Rua do Parque", 1);
    let (from0, to0) = {
        let s = n.net.segs.iter().find(|s| s.key == k).unwrap();
        (s.from, s.to)
    };
    n.set_edits(&edit_json(k, "\"oneway\":true,\"lf\":2,\"reverse\":true")).unwrap();
    let s = n.net.segs.iter().find(|s| s.key == k).unwrap();
    assert!(s.oneway);
    assert_eq!((s.lf, s.lb), (2, 0));
    assert_eq!((s.from, s.to), (to0, from0), "sentido invertido");
    assert_eq!(s.key, k, "a chave continua a original");
    // faixas de ônibus por faixa
    n.set_edits(&edit_json(k, "\"oneway\":false,\"lf\":2,\"lb\":1,\"busF\":[false,true]")).unwrap();
    let s = n.net.segs.iter().find(|s| s.key == k).unwrap();
    assert_eq!(s.bus_f, vec![false, true]);
    assert_eq!(s.bus_b, vec![false]);
}

#[test]
fn edit_clamps_and_rejects_bad_json() {
    let mut n = sample();
    let k = key_of(&n, "Rua do Parque", 1);
    n.set_edits(&edit_json(k, "\"oneway\":false,\"lf\":99,\"lb\":0,\"w\":99")).unwrap();
    let s = n.net.segs.iter().find(|s| s.key == k).unwrap();
    assert_eq!((s.lf, s.lb), (MAX_LANES, 1));
    assert!(s.w < 5.5);
    assert!(n.set_edits("not json").is_err());
}

fn node_idx(n: &Network, osm: i64) -> usize {
    n.net.nodes.iter().position(|x| x.osm_id == osm).expect("nó")
}

#[test]
fn signals_can_be_added_removed_and_configured() {
    let mut n = sample();
    assert_eq!(n.net.signals.len(), 3);
    // junção sem semáforo: Avenida × Rua das Flores (nó OSM 1004 é o semáforo central; pegamos outra junção de grau >= 3)
    let free = n.net.nodes.iter().position(|x| !x.signal && x.ends.len() >= 3).expect("junção livre");
    let osm = n.net.nodes[free].osm_id;
    n.set_edits(&format!(r#"{{"signals":[{{"node":{osm},"enabled":true}}]}}"#)).unwrap();
    assert_eq!(n.net.signals.len(), 4, "semáforo adicionado");
    let sig = n.net.signals.iter().find(|s| n.net.nodes[s.node as usize].osm_id == osm).unwrap();
    assert!(!sig.phases.is_empty() && !sig.ends.is_empty());
    assert!(n.net.nodes[free].signal, "o nó vira semaforizado (faixa de pedestre etc.)");

    // remover um semáforo vindo do OSM
    let existing = *n.raw.signals.iter().next().unwrap();
    n.set_edits(&format!(r#"{{"signals":[{{"node":{existing},"enabled":false}}]}}"#)).unwrap();
    assert_eq!(n.net.signals.len(), 2);
}

#[test]
fn signal_phase_config_is_applied_and_covers_all_approaches() {
    let mut n = sample();
    let s0 = &n.net.signals[0];
    let ni = s0.node as usize;
    let osm = n.net.nodes[ni].osm_id;
    let keys: Vec<String> = s0.ends.iter().map(|e| e.key.clone()).collect();
    assert!(keys.len() >= 3);
    // uma única fase com só a primeira aproximação: as outras ganham uma fase própria automaticamente
    let json = format!(
        r#"{{"signals":[{{"node":{osm},"enabled":true,"offset":7,"phases":[{{"green":42,"yellow":4,"allRed":1,"approaches":["{}"]}}]}}]}}"#,
        keys[0]
    );
    n.set_edits(&json).unwrap();
    let sig = n.net.signals.iter().find(|s| n.net.nodes[s.node as usize].osm_id == osm).unwrap();
    assert_eq!(sig.phases.len(), 2);
    assert_eq!((sig.phases[0].green, sig.phases[0].yellow, sig.phases[0].all_red), (42.0, 4.0, 1.0));
    assert_eq!(sig.phases[0].ends.len(), 1);
    assert_eq!(sig.offset, 7.0);
    let total: usize = sig.phases.iter().map(|p| p.ends.len()).sum();
    assert_eq!(total, sig.ends.len(), "toda aproximação é atendida");
}

fn road_json(extra_roads: &str, removed: &str) -> String {
    format!(r#"{{"roads":[{extra_roads}],"removed":[{removed}]}}"#)
}

#[test]
fn new_road_crossing_creates_intersections() {
    let mut n = sample();
    let (nodes0, segs0) = (n.net.nodes.len(), n.net.segs.len());
    // via vertical em x=-50 (metros locais, Y para baixo): cruza a Avenida (y=0) e a Rua Sentido Único (y=-150)
    let road = r#"{"id":900000000000001,"hw":"secondary","name":"Rua Nova","oneway":false,"lf":2,"lb":2,
        "pts":[{"x":-50,"y":-200},{"x":-50,"y":200}]}"#;
    n.set_edits(&road_json(road, "")).unwrap();
    assert_eq!(n.net.nodes.len(), nodes0 + 4, "2 pontas + 2 cruzamentos");
    // 3 trechos novos + 2 trechos criados pela divisão das vias existentes
    assert_eq!(n.net.segs.len(), segs0 + 3 + 2);
    let crossings = n.net.nodes.iter().filter(|x| x.ends.len() == 4 && x.osm_id < 0).count();
    assert_eq!(crossings, 2);
    assert!(n.net.meshes_ok());
    // conexões entre a via nova e as existentes
    assert!(n.net.conns.iter().any(|c| c.state == CONN_ALLOWED && n.net.segs[n.net.lanes[c.from_lane as usize].seg as usize].way_id == 900000000000001));
    // removendo a via, volta ao original
    n.set_edits(r#"{"roads":[]}"#).unwrap();
    assert_eq!((n.net.nodes.len(), n.net.segs.len()), (nodes0, segs0));
}

#[test]
fn new_road_snaps_to_way_body_and_nodes() {
    let mut n = sample();
    let nodes0 = n.net.nodes.len();
    let road = r#"{"id":900000000000002,"hw":"residential","oneway":true,"lf":1,
        "pts":[{"x":-50,"y":0,"way":5001},{"x":-50,"y":120}]}"#;
    n.set_edits(&road_json(road, "")).unwrap();
    assert_eq!(n.net.nodes.len(), nodes0 + 2, "nó de divisão + ponta");
    let t = n.net.nodes.iter().find(|x| x.osm_id < 0 && x.ends.len() == 3).expect("T na avenida");
    assert!(t.ends.len() == 3);
    // encaixe num nó existente (semáforo central, OSM 1004)
    let road2 = r#"{"id":900000000000003,"hw":"residential","pts":[{"x":0,"y":0,"node":1004},{"x":60,"y":-90}]}"#;
    n.set_edits(&road_json(road2, "")).unwrap();
    let c = n.net.nodes.iter().find(|x| x.osm_id == 1004).unwrap();
    assert_eq!(c.ends.len(), 5, "o nó central ganhou um ramo");
}

#[test]
fn removed_segments_disappear() {
    let mut n = sample();
    let segs0 = n.net.segs.len();
    let k = key_of(&n, "Rua do Parque", 1);
    n.set_edits(&road_json("", &format!("[{},{},{}]", k[0], k[1], k[2]))).unwrap();
    assert_eq!(n.net.segs.len(), segs0 - 1);
    assert!(n.net.segs.iter().all(|s| s.key != k));
    assert!(n.net.meshes_ok());
}

/// Cronometra cada estágio da (re)construção no mapa grande: `PERF_OSM=arquivo cargo test --release perf_stages -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_stages() {
    use std::time::Instant;
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let t = Instant::now();
    let raw = crate::osm::parse(&data).unwrap();
    eprintln!("parse            {:>7.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let edits = Edits::default();
    crate::net::stage_log().lock().unwrap().clear();
    let t = Instant::now();
    let base = crate::net::BaseCache::new(&raw).unwrap();
    let mut net = Net::build(&raw, &edits, &base).unwrap();
    eprintln!("grafo+faixas     {:>7.1} ms  ({} nós, {} trechos)", t.elapsed().as_secs_f64() * 1e3, net.nodes.len(), net.segs.len());
    for (n, ms) in crate::net::stage_log().lock().unwrap().iter() {
        eprintln!("   · {n:<34} {ms:>6.1} ms");
    }
    let t = Instant::now();
    crate::conn::build_connections(&mut net, &raw, &edits);
    eprintln!("conexões         {:>7.1} ms  ({} conexões)", t.elapsed().as_secs_f64() * 1e3, net.conns.len());
    let t = Instant::now();
    crate::conn::build_signals(&mut net, &edits);
    eprintln!("semáforos        {:>7.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let (ts, _) = crate::render::update_tiles(&net, None);
    eprintln!("malhas (ladrilhos){:>6.1} ms  ({} tri asfalto, {} ladrilhos)", t.elapsed().as_secs_f64() * 1e3, ts.asphalt_tris(), ts.tiles.len());
    // reconstrução por edição de faixas (caminho real de set_edits, sem parse)
    let mut n = build_network_lazy_for_test(&data);
    let k = n.net.segs[n.net.segs.len() / 2].key;
    let json = format!(r#"{{"segments":[{{"way":{},"from":{},"to":{},"oneway":false,"lf":2,"lb":2}}]}}"#, k[0], k[1], k[2]);
    let t = Instant::now();
    n.set_edits(&json).unwrap();
    eprintln!("set_edits total  {:>7.1} ms  ({} de {} ladrilhos sujos)", t.elapsed().as_secs_f64() * 1e3, n.delta.dirty.len(), n.tiles.tiles.len());
}

#[test]
fn gravity_conserves_origin_totals_and_respects_forced_cells() {
    let o = vec![100.0, 50.0, 0.0];
    let a = vec![1.0, 2.0, 3.0];
    let t = vec![vec![0.0, 5.0, 10.0], vec![5.0, 0.0, 5.0], vec![10.0, 5.0, 0.0]];
    let none = vec![vec![None; 3]; 3];
    let m = crate::demand::gravity(&o, &a, &t, 0.1, false, &none);
    for i in 0..3 {
        assert!((m[i].iter().sum::<f64>() - o[i]).abs() < 1e-9, "linha {i}");
        assert_eq!(m[i][i], 0.0, "sem viagens intrazona");
    }
    // atração maior e mais perto => mais viagens
    assert!(m[0][1] > m[0][2] * 0.5);
    // célula fixa: valor exato e o resto se redistribui
    let mut forced = none.clone();
    forced[0][1] = Some(70.0);
    let m2 = crate::demand::gravity(&o, &a, &t, 0.1, false, &forced);
    assert_eq!(m2[0][1], 70.0);
    assert!((m2[0].iter().sum::<f64>() - 100.0).abs() < 1e-9);
    // intrazona
    let m3 = crate::demand::gravity(&o, &a, &t, 0.1, true, &none);
    assert!(m3[0][0] > 0.0);
}

#[test]
fn polygon_helpers() {
    let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    assert!(crate::demand::point_in_poly([5.0, 5.0], &sq));
    assert!(!crate::demand::point_in_poly([15.0, 5.0], &sq));
    let c = crate::demand::centroid(&sq);
    assert!((c[0] - 5.0).abs() < 1e-9 && (c[1] - 5.0).abs() < 1e-9);
    let prof: Vec<f64> = (0..24).map(|h| h as f64).collect();
    assert!((crate::demand::profile_at(&prof, 3.5) - 3.5).abs() < 1e-9);
    assert!((crate::demand::profile_at(&prof, 23.5) - 11.5).abs() < 1e-9, "interpola 23->0");
    assert_eq!(crate::demand::profile_at(&[], 5.0), 1.0);
}

#[test]
fn zones_resolve_on_sample() {
    let n = sample();
    // duas zonas: uma sobre a Avenida oeste e outra no leste; uma terceira vazia (longe de tudo)
    let json = r#"{"zones":[
        {"id":1,"gen":600,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
        {"id":2,"gen":300,"attr":2,"poly":[[150,-30],[420,-30],[420,30],[150,30]]},
        {"id":3,"gen":100,"attr":1,"poly":[[5000,5000],[5100,5000],[5100,5100]]}],
        "params":{"hour":8,"beta":0.05}}"#;
    let payload: crate::demand::Payload = serde_json::from_str(json).unwrap();
    let r = crate::demand::resolve(&n.net, &payload, &crate::demand::straight_line_minutes);
    assert!(r.zones[0].nodes > 0 && r.zones[1].nodes > 0);
    assert!(r.zones[2].warning.is_some() && r.zones[2].gen_now == 0.0);
    let row0: f64 = r.matrix[0].iter().sum();
    assert!((row0 - 600.0).abs() < 1e-6, "linha da zona 1 soma a geração (a zona vazia não atrai)");
    assert_eq!(r.matrix[0][2], 0.0);
}

#[test]
fn per_node_conns_match_eager_and_apply_user_edits() {
    let n = sample();
    // por nó == construção completa
    for ni in 0..n.net.nodes.len() {
        let one = crate::conn::node_conns(&n.net, ni, &n.raw, &n.restr, &n.edits);
        let (a, b) = (n.net.conn_off[ni] as usize, n.net.conn_off[ni + 1] as usize);
        assert_eq!(one.len(), b - a, "nó {ni}");
        for (x, y) in one.iter().zip(&n.net.conns[a..b]) {
            assert_eq!((x.from_lane, x.to_lane, x.kind, x.state), (y.from_lane, y.to_lane, y.kind, y.state));
        }
    }
    // edição de conversão por chave estável: liga uma candidata e remove uma padrão
    let ni = (0..n.net.nodes.len()).find(|&i| n.net.nodes[i].signal && n.net.nodes[i].ends.len() == 4).unwrap();
    let list = crate::conn::node_conns(&n.net, ni, &n.raw, &n.restr, &n.edits);
    let absent = list.iter().find(|c| c.state == CONN_ABSENT).unwrap();
    let allowed = list.iter().find(|c| c.state == CONN_ALLOWED).unwrap();
    let k1 = crate::conn::conn_key(&n.net, ni as u32, absent.from_lane, absent.to_lane);
    let k2 = crate::conn::conn_key(&n.net, ni as u32, allowed.from_lane, allowed.to_lane);
    let mut m = n;
    m.set_edits(&format!(r#"{{"connections":{{"{k1}":1,"{k2}":2}}}}"#)).unwrap();
    let ni2 = (0..m.net.nodes.len()).find(|&i| m.net.nodes[i].osm_id == m.net.nodes[ni].osm_id).unwrap();
    let after = crate::conn::node_conns(&m.net, ni2, &m.raw, &m.restr, &m.edits);
    let st = |a: u32, b: u32| after.iter().find(|c| c.from_lane == a && c.to_lane == b).unwrap().state;
    assert_eq!(st(absent.from_lane, absent.to_lane), CONN_ALLOWED);
    assert_eq!(st(allowed.from_lane, allowed.to_lane), CONN_ABSENT);
}

fn same_tiles(a: &crate::render::TileSet, b: &crate::render::TileSet) -> Result<(), String> {
    let mut ka: Vec<_> = a.tiles.keys().copied().collect();
    let mut kb: Vec<_> = b.tiles.keys().copied().collect();
    ka.sort();
    kb.sort();
    if ka != kb {
        return Err(format!("ladrilhos diferentes: {ka:?} vs {kb:?}"));
    }
    for k in ka {
        let (x, y) = (&a.tiles[&k], &b.tiles[&k]);
        let pairs: [(&str, bool); 7] = [
            ("asphalt", x.asphalt.pos == y.asphalt.pos && x.asphalt.idx == y.asphalt.idx),
            ("bus", x.bus.pos == y.bus.pos && x.bus.idx == y.bus.idx),
            ("white", x.white.pos == y.white.pos && x.white.idx == y.white.idx),
            ("yellow", x.yellow.pos == y.yellow.pos && x.yellow.idx == y.yellow.idx),
            ("shape", x.shape == y.shape),
            ("minor", x.minor == y.minor),
            ("major", x.major == y.major),
        ];
        for (name, ok) in pairs {
            if !ok {
                return Err(format!("ladrilho {k:#x}: {name} difere"));
            }
        }
    }
    Ok(())
}

#[test]
fn incremental_tiles_equal_full_rebuild_after_edit_sequence() {
    let mut n = sample();
    let k = key_of(&n, "Rua Central", 3);
    let steps = [
        format!(r#"{{"segments":[{{"way":{},"from":{},"to":{},"oneway":false,"lf":4,"lb":2}}]}}"#, k[0], k[1], k[2]),
        r#"{"roads":[{"id":900000000000001,"hw":"secondary","name":"A","oneway":false,"lf":2,"lb":2,"pts":[{"x":-50,"y":-200},{"x":-50,"y":200}]}]}"#.to_string(),
        r#"{"roads":[{"id":900000000000001,"hw":"secondary","name":"A","oneway":false,"lf":2,"lb":2,"pts":[{"x":-50,"y":-200},{"x":-50,"y":200}]}],"signals":[{"node":1002,"enabled":true}]}"#.to_string(),
        r#"{"removed":[[5000,1000,1013]]}"#.to_string(),
        "{}".to_string(),
    ];
    for (i, json) in steps.iter().enumerate() {
        n.set_edits(json).unwrap_or_else(|e| panic!("passo {i}: {e}"));
        let (full, _) = crate::render::update_tiles(&n.net, None);
        same_tiles(&n.tiles, &full).unwrap_or_else(|e| panic!("passo {i}: {e}"));
    }
}

#[test]
fn local_edit_dirties_only_nearby_tiles() {
    // dois blocos do mesmo mapa a ~3 km: editar um não pode sujar o ladrilho do outro
    let mut n = sample();
    let before = n.delta.dirty.len();
    assert!(before >= 1, "importação gera todos os ladrilhos");
    n.clear_delta();
    let k = key_of(&n, "Rua do Parque", 1);
    n.set_edits(&format!(r#"{{"segments":[{{"way":{},"from":{},"to":{},"oneway":false,"lf":2,"lb":1}}]}}"#, k[0], k[1], k[2])).unwrap();
    assert!(!n.delta.dirty.is_empty() && n.delta.dirty.len() <= n.tiles.tiles.len());
    // sem mudanças, nada fica sujo
    n.clear_delta();
    n.set_edits(&format!(r#"{{"segments":[{{"way":{},"from":{},"to":{},"oneway":false,"lf":2,"lb":1}}]}}"#, k[0], k[1], k[2])).unwrap();
    assert!(n.delta.dirty.is_empty(), "reaplicar as mesmas edições não suja ladrilhos: {:?}", n.delta.dirty);
}

fn build_network_lazy_for_test(data: &[u8]) -> Network {
    crate::build_network_lazy(data).unwrap()
}

// ------------------------------------------------------------------ simulação

const SIM_ZONES: &str = r#"{"zones":[
  {"id":1,"gen":900,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
  {"id":2,"gen":900,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]}],
  "params":{"seed":7,"hour":8,"busShare":5,"beta":0.02}}"#;

fn sim_net() -> Network {
    let mut n = sample();
    n.start_sim(SIM_ZONES).expect("simulação deve iniciar");
    n
}

fn run(n: &mut Network, seconds: f64) {
    let ticks = (seconds / 0.2) as u32;
    for _ in 0..ticks {
        let s = n.sim.as_mut().unwrap();
        s.step(&n.net);
    }
}

#[test]
fn sim_spawns_moves_and_finishes_trips() {
    let mut n = sim_net();
    run(&mut n, 600.0);
    let st = n.sim.as_ref().unwrap().stats.clone();
    assert!(st.spawned > 40, "geração: {st:?}");
    assert!(st.finished > 10, "viagens concluídas: {st:?}");
    assert_eq!(st.red_runs, 0, "ninguém atravessa no vermelho: {st:?}");
    // conservação
    let alive = n.sim.as_ref().unwrap().n_alive as u64;
    assert_eq!(st.spawned, st.finished + st.lost + alive);
    assert!(st.travel_sum / st.finished as f64 > 20.0, "tempo de viagem plausível");
}

#[test]
fn sim_vehicles_never_overlap() {
    let mut n = sim_net();
    for _ in 0..(900.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
        let s = n.sim.as_ref().unwrap();
        for list in s.edge_lists() {
            for w in list.windows(2) {
                let (sa, _) = s.vehicle_s(w[0]);
                let (sb, lb) = s.vehicle_s(w[1]);
                assert!(sa <= sb - lb + 0.05, "sobreposição: traseira de {} em {} / frente de {}", w[1], sb - lb, sa);
            }
        }
        for (e, sv, v, _) in s.debug_vehicles() {
            assert!(v >= 0.0 && sv.is_finite() && v.is_finite(), "estado inválido na aresta {e}");
        }
    }
}

#[test]
fn sim_is_deterministic() {
    let snap = |seed: u64| {
        let mut n = sample();
        n.start_sim(&SIM_ZONES.replace("\"seed\":7", &format!("\"seed\":{seed}"))).unwrap();
        run(&mut n, 300.0);
        let mut v = Vec::new();
        n.sim.as_ref().unwrap().snapshot(&n.net, &mut v);
        (v, n.sim.as_ref().unwrap().stats.clone().spawned)
    };
    let a = snap(7);
    let b = snap(7);
    assert_eq!(a, b, "mesma semente => mesmo resultado");
    let c = snap(8);
    assert_ne!(a.0, c.0, "semente diferente => resultado diferente");
}

#[test]
fn sim_respects_signals_in_red() {
    let mut n = sim_net();
    // mede quantos veículos cruzaram com o sinal vermelho há > 1,5 s: deve ser zero em 15 min
    run(&mut n, 900.0);
    assert_eq!(n.sim.as_ref().unwrap().stats.red_runs, 0);
    // houve interação real com semáforos (veículos esperaram)
    assert!(n.sim.as_ref().unwrap().stats.spawned > 100);
}

#[test]
#[ignore]
fn sim_stress_report() {
    let zones = r#"{"zones":[
      {"id":1,"gen":2500,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
      {"id":2,"gen":2500,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]},
      {"id":3,"gen":1500,"attr":2,"poly":[[-60,-320],[60,-320],[60,-250],[-60,-250]]},
      {"id":4,"gen":1500,"attr":2,"poly":[[-60,250],[60,250],[60,320],[-60,320]]}],
      "params":{"seed":3,"hour":8,"busShare":4,"beta":0.01,"intrazone":false}}"#;
    let mut n = sample();
    n.start_sim(zones).unwrap();
    let mut max_alive = 0;
    let mut min_gap = f32::MAX;
    for t in 0..(1800.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
        let s = n.sim.as_ref().unwrap();
        max_alive = max_alive.max(s.n_alive);
        if t % 25 == 0 {
            for list in s.edge_lists() {
                for w in list.windows(2) {
                    let (sa, _) = s.vehicle_s(w[0]);
                    let (sb, lb) = s.vehicle_s(w[1]);
                    min_gap = min_gap.min(sb - lb - sa);
                }
            }
        }
        if t % 1500 == 0 {
            eprintln!("t={:>5.0}s vivos={:>4} {:?}", s.time, s.n_alive, s.stats);
        }
    }
    eprintln!("max vivos={max_alive}  menor folga entre veículos={min_gap:.2} m");
    assert!(min_gap > -0.2);
}

/// `PERF_OSM=arquivo cargo test --release perf_sim -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_sim() {
    use std::time::Instant;
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    let t = Instant::now();
    n.ensure_conns();
    eprintln!("ensure_conns {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let q = |id: i64, x0: f64, y0: f64, x1: f64, y1: f64, gen: f64| format!(r#"{{"id":{id},"gen":{gen},"attr":1,"poly":[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}]]}}"#);
    let zones = [
        q(1, -6000.0, -6000.0, -300.0, -300.0, 30000.0),
        q(2, 300.0, -6000.0, 6000.0, -300.0, 30000.0),
        q(3, -6000.0, 300.0, -300.0, 6000.0, 30000.0),
        q(4, 300.0, 300.0, 6000.0, 6000.0, 30000.0),
    ]
    .join(",");
    let json = format!(r#"{{"zones":[{zones}],"params":{{"seed":1,"hour":8,"busShare":3,"beta":0.02}}}}"#);
    let t = Instant::now();
    n.start_sim(&json).unwrap();
    eprintln!("Sim::new {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let mut worst = 0.0f64;
    for block in 0..12 {
        if block == 11 {
            n.sim.as_mut().unwrap().prof = [0.0; 7];
        }
        let t = Instant::now();
        let mut peak = 0.0f64;
        for _ in 0..250 {
            let t1 = Instant::now();
            n.sim.as_mut().unwrap().step(&n.net);
            peak = peak.max(t1.elapsed().as_secs_f64());
        }
        let s = n.sim.as_ref().unwrap();
        worst = worst.max(peak);
        eprintln!(
            "min {:>2}: vivos={:>6} gerados={:>6} concl.={:>6} fila={:>6}  passo médio {:>6.2} ms  pior {:>6.2} ms",
            block + 1,
            s.n_alive,
            s.stats.spawned,
            s.stats.finished,
            s.stats.queued,
            t.elapsed().as_secs_f64() * 1e3 / 250.0,
            peak * 1e3
        );
    }
    let p = n.sim.as_ref().unwrap().prof;
    let names = ["sinais", "geração", "aceleração(líder+junção)", "integração", "folgas#1", "mudança de aresta+folgas#2", "troca de faixa"];
    for (k, nme) in names.iter().enumerate() {
        eprintln!("   fase {nme:<28} {:>6.2} ms/passo", p[k] / 250.0);
    }
}


#[test]
fn sim_metrics_report_series_and_heat() {
    let mut n = sim_net();
    run(&mut n, 720.0);
    let s = n.sim.as_ref().unwrap();
    let nd = n.net.segs.len() * 2;
    // série: um ponto por minuto
    assert!(s.metrics.series.len() >= 10, "série: {}", s.metrics.series.len());
    assert_eq!(s.metrics.hist_speed.len(), s.metrics.series.len());
    assert!(s.metrics.hist_speed.iter().all(|r| r.len() == nd));
    // mapa de calor
    let heat = s.metrics.heat(0);
    assert_eq!(heat.len(), nd);
    assert!(heat.iter().any(|&h| h != 255 && h <= 254), "algum trecho com dados");
    // relatório coerente
    let r = s.report(&n.net);
    assert!(r["trips"].as_u64().unwrap() > 10);
    assert!(r["meanTravel"].as_f64().unwrap() > 20.0);
    assert!(r["meanDelay"].as_f64().unwrap() >= 0.0);
    assert!(r["meanDelay"].as_f64().unwrap() <= r["meanTravel"].as_f64().unwrap());
    assert!(r["od"].as_array().unwrap().len() >= 1);
    // com 3 semáforos e fluxo na avenida há atraso medido em cruzamentos
    let total_delay: f64 = r["junctions"].as_array().unwrap().iter().map(|j| j["delayVehH"].as_f64().unwrap()).sum();
    assert!(total_delay >= 0.0);
    let ratio = s.run_ratio(&n.net);
    assert_eq!(ratio.len(), nd);
}

/// Cenário com gargalo conhecido: reduzir a avenida de 2+2 para 1+1 deve piorar atraso/tempo de viagem
/// (mesma semente e demanda). É a base da comparação A/B.
#[test]
fn sim_ab_bottleneck_makes_things_worse() {
    let zones = r#"{"zones":[
      {"id":1,"gen":1800,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
      {"id":2,"gen":1800,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]}],
      "params":{"seed":5,"hour":8,"busShare":0,"beta":0.0}}"#;
    let run_with = |edit: Option<&str>| {
        let mut n = sample();
        if let Some(e) = edit {
            n.set_edits(e).unwrap();
            n.ensure_conns();
        }
        n.start_sim(zones).unwrap();
        run(&mut n, 900.0);
        let r = n.sim.as_ref().unwrap().report(&n.net);
        (r["totalDelayAllVehH"].as_f64().unwrap(), r["meanSpeedKmh"].as_f64().unwrap(), r["trips"].as_u64().unwrap())
    };
    let base = run_with(None);
    // estreita todos os trechos da Avenida Brasil para 1+1
    let n = sample();
    let idx = n.net.names.iter().position(|x| x == "Avenida Brasil").unwrap() as u32;
    let segs: Vec<String> = n
        .net
        .segs
        .iter()
        .filter(|s| s.name == idx)
        .map(|s| format!(r#"{{"way":{},"from":{},"to":{},"oneway":false,"lf":1,"lb":1}}"#, s.key[0], s.key[1], s.key[2]))
        .collect();
    let narrow = run_with(Some(&format!(r#"{{"segments":[{}]}}"#, segs.join(","))));
    eprintln!("base {base:?}  estreita {narrow:?}");
    assert!(narrow.0 > base.0 * 1.2, "atraso total (incluindo quem ficou preso) sobe com o gargalo: {base:?} vs {narrow:?}");
    assert!(narrow.1 < base.1, "velocidade média cai com o gargalo");
    assert!(narrow.2 < base.2, "menos viagens concluídas");
}

#[test]
#[ignore]
fn sim_stuck_diag() {
    let zones = r#"{"zones":[
      {"id":1,"gen":1800,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
      {"id":2,"gen":1800,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]}],
      "params":{"seed":1,"hour":8,"busShare":0,"beta":0,"intrazone":false}}"#;
    let mut n = sample();
    n.start_sim(zones).unwrap();
    for _ in 0..(600.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
    }
    let net = &n.net;
    let s = n.sim.as_mut().unwrap();
    let mut hist = std::collections::BTreeMap::new();
    for _ in 0..300 {
        for _ in 0..5 { s.step(net); }
        for l in s.stuck_report(net, 15.0) {
            if l.starts_with("veh") {
                let st = l.split("end_state=").nth(1).unwrap().chars().next().unwrap();
                let why = l.split("why=").nth(1).unwrap().split(' ').next().unwrap().to_string();
                let sg = l.split(" seg ").nth(1).unwrap().split(' ').next().unwrap().to_string();
                *hist.entry((sg, st, why)).or_insert(0u32) += 1;
            }
        }
    }
    let mut same = 0; let mut total = 0;
    for i in 0..2000 { if let Some((o, d, _, _)) = s.od_of(i) { total += 1; if o == d { same += 1; } } }
    eprintln!("origem==destino: {same}/{total}");
    eprintln!("HIST (seg, estado, motivo) -> amostras: {:?}", hist);
    eprintln!("vivos {} {:?}", s.n_alive, s.stats);
    let mut per = std::collections::BTreeMap::new();
    for e in 0..s.lane_count() {
        for &k in &s.edge_lists()[e] {
            let (_, _) = s.vehicle_s(k);
            let ent = per.entry(e).or_insert((0, 0.0f32));
            ent.0 += 1;
        }
    }
    for (e, (c, _)) in &per { if *c > 6 { eprintln!("lane {} seg {} n={}", e, net.lanes[*e].seg, c); } }
    for l in s.stuck_report(net, 30.0) {
        eprintln!("{l}");
    }
}

#[test]
fn signal_can_be_placed_mid_block() {
    let mut n = sample();
    let longest = (0..n.net.segs.len()).max_by(|&a, &b| n.net.segs[a].poly.length().partial_cmp(&n.net.segs[b].poly.length()).unwrap()).unwrap();
    let (way, mid) = {
        let s = &n.net.segs[longest];
        (s.way_id, s.poly.at(s.poly.length() / 2.0).0)
    };
    let (segs0, sigs0) = (n.net.segs.len(), n.net.signals.len());
    let json = format!(r#"{{"roads":[{{"id":900000000000001,"hw":"mark","pts":[{{"x":{},"y":{},"way":{}}}]}}]}}"#, mid[0], mid[1], way);
    n.set_edits(&json).unwrap();
    assert_eq!(n.net.segs.len(), segs0 + 1, "o trecho deve ser dividido no ponto");
    assert_eq!(n.net.signals.len(), sigs0 + 1, "o nó novo deve ter semáforo");
    let node = n.net.nodes.iter().position(|nd| nd.signal && nd.ends.len() == 2 && ((nd.pos[0] - mid[0]).abs() < 1.0 && (nd.pos[1] - mid[1]).abs() < 1.0)).expect("nó do semáforo");
    let sg = n.net.signals.iter().find(|s| s.node as usize == node).unwrap();
    assert_eq!(sg.phases.len(), 2, "fase de travessia de pedestres");
    // remover o ponto desfaz tudo
    n.set_edits("{}").unwrap();
    assert_eq!(n.net.segs.len(), segs0);
}

#[test]
#[ignore]
fn recife_stuck() {
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let q = |id: i64, x0: f64, y0: f64, x1: f64, y1: f64, gen: f64| format!(r#"{{"id":{id},"gen":{gen},"attr":1,"poly":[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}]]}}"#);
    let zones = [q(1, -1250.0, -150.0, -700.0, 300.0, 900.0), q(2, 700.0, -250.0, 1250.0, 200.0, 900.0), q(3, -250.0, -600.0, 250.0, -250.0, 600.0)].join(",");
    n.start_sim(&format!(r#"{{"zones":[{zones}],"params":{{"seed":1,"hour":8,"busShare":0,"beta":0}}}}"#)).unwrap();
    for _ in 0..(900.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
    }
    let net = &n.net;
    let s = n.sim.as_mut().unwrap();
    eprintln!("vivos {} {:?}", s.n_alive, s.stats);
    let rep = s.stuck_report(net, 120.0);
    let mut by: std::collections::BTreeMap<String, u32> = Default::default();
    for l in &rep {
        if l.starts_with("veh") {
            let ch = if l.contains("choice=NONE") { "SEM_CONVERSAO" } else { "tem_conv" };
            let why = l.split("why=").nth(1).unwrap().split(' ').next().unwrap();
            *by.entry(format!("{ch} why={why}")).or_default() += 1;
        }
    }
    eprintln!("parados>120s: {:?}", by);
    for r in s.red_log.iter().take(3) { eprintln!("INFO {}", s.lane_info(r.0 as usize)); }
    for r in s.red_log.iter().take(15) { eprintln!("RED lane {} v={:.1} choice={} wait={:.1} red_age={:.1}", r.0, r.1, r.2, r.3, r.4); }
    for l in rep.iter().filter(|l| l.contains("choice=NONE")).take(5) {
        eprintln!("{l}");
    }
}

#[test]
fn sim_routes_by_lane_without_illegal_moves() {
    let zones = r#"{"zones":[
      {"id":1,"gen":1500,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
      {"id":2,"gen":1500,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]},
      {"id":3,"gen":900,"attr":2,"poly":[[-60,-320],[60,-320],[60,-250],[-60,-250]]},
      {"id":4,"gen":900,"attr":2,"poly":[[-60,250],[60,250],[60,320],[-60,320]]}],
      "params":{"seed":5,"hour":8,"busShare":3,"beta":0.01}}"#;
    let mut n = sample();
    n.start_sim(zones).unwrap();
    run(&mut n, 900.0);
    let s = n.sim.as_ref().unwrap();
    assert_eq!(s.stats.illegal_moves, 0, "sem infração configurada, ninguém usa conversão proibida");
    assert_eq!(s.stats.lost, 0);
    assert!(s.stats.finished > 100);
}

#[test]
fn sim_illegal_pct_param_is_accepted() {
    let zones = r#"{"zones":[
      {"id":1,"gen":900,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]},
      {"id":2,"gen":900,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]}],
      "params":{"seed":5,"hour":8,"illegalPct":100}}"#;
    let mut n = sample();
    n.start_sim(zones).unwrap();
    run(&mut n, 300.0);
    assert!(n.sim.as_ref().unwrap().stats.finished > 0);
}

#[test]
#[ignore]
fn recife_wait_reasons() {
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let q = |id: i64, x0: f64, y0: f64, x1: f64, y1: f64, gen: f64| format!(r#"{{"id":{id},"gen":{gen},"attr":1,"poly":[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}]]}}"#);
    let zones = [q(1, -1250.0, -150.0, -700.0, 300.0, 900.0), q(2, 700.0, -250.0, 1250.0, 200.0, 900.0), q(3, -250.0, -600.0, 250.0, -250.0, 600.0)].join(",");
    n.start_sim(&format!(r#"{{"zones":[{zones}],"params":{{"seed":1,"hour":8,"busShare":0,"beta":0}}}}"#)).unwrap();
    for _ in 0..(300.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
    }
    let net = &n.net;
    let s = n.sim.as_mut().unwrap();
    // amostra a cada 1 s por 10 min: quem está na frente da faixa, parado, sem semáforo, e por que não entra
    let mut hist: std::collections::BTreeMap<(String, String), u32> = Default::default();
    let mut total = 0u32;
    for _ in 0..600 {
        for _ in 0..5 {
            s.step(net);
        }
        let rep = s.stuck_report(net, 3.0);
        for (idx, l) in rep.iter().enumerate() {
            if l.starts_with("veh") && l.contains("why=4") && l.contains("end_state=0") && idx > 0 && rep[idx - 1].starts_with("   -> exit") {
                let prev = &rep[idx - 1];
                let v: f32 = prev.split(" v=").nth(1).and_then(|x| x.split(' ').next()).and_then(|x| x.parse().ok()).unwrap_or(0.0);
                let nveh: u32 = prev.split(" n=").nth(1).and_then(|x| x.split(' ').next()).and_then(|x| x.parse().ok()).unwrap_or(0);
                let k = (format!("saida_primeiro_v{}", if v > 1.5 { ">1.5 (andando)" } else { "<=1.5 (parado/lento)" }), format!("n_na_saida{}", if nveh <= 3 { "<=3 (livre)" } else { ">3 (fila)" }));
                *hist.entry(k).or_default() += 1;
            }
            if !l.starts_with("veh") || !l.contains("end_state=0") || l.contains("choice=NONE") {
                continue;
            }
            let why = l.split("why=").nth(1).unwrap().split(' ').next().unwrap().to_string();
            let ok = l.split("may_enter=").nth(1).unwrap().split(' ').next().unwrap().to_string();
            total += 1;
            *hist.entry((ok, why)).or_default() += 1;
        }
    }
    eprintln!("AMOSTRAS (parado >3 s na frente, sem semáforo): {total}");
    for (k, v) in &hist {
        eprintln!("  may_enter={} motivo={} -> {}", k.0, k.1, v);
    }
}

#[test]
#[ignore]
fn recife_blockers() {
    let path = std::env::var("PERF_OSM").expect("defina PERF_OSM");
    let data = std::fs::read(path).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let q = |id: i64, x0: f64, y0: f64, x1: f64, y1: f64, gen: f64| format!(r#"{{"id":{id},"gen":{gen},"attr":1,"poly":[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}]]}}"#);
    let zones = [q(1, -1250.0, -150.0, -700.0, 300.0, 900.0), q(2, 700.0, -250.0, 1250.0, 200.0, 900.0), q(3, -250.0, -600.0, 250.0, -250.0, 600.0)].join(",");
    n.start_sim(&format!(r#"{{"zones":[{zones}],"params":{{"seed":1,"hour":8,"busShare":0,"beta":0}}}}"#)).unwrap();
    for _ in 0..(600.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
    }
    let net = &n.net;
    let s = n.sim.as_mut().unwrap();
    let list = s.stuck_list(net, 400);
    let mut by_blocker: std::collections::BTreeMap<String, u32> = Default::default();
    for it in &list {
        if it["code"] == 4 {
            let b = it["blocker"].as_i64().unwrap_or(-1);
            if b >= 0 {
                let info = s.explain(net, b as usize);
                let key = format!(
                    "{} v={:.1} wait={:.0} escolha={} sinal={} front={} resto={:.0}",
                    info["where"], info["v"].as_f64().unwrap_or(0.0), info["wait"].as_f64().unwrap_or(0.0),
                    if info["choice"].is_string() { info["choice"].as_str().unwrap().to_string() } else { info["choice"]["kind"].to_string() },
                    info["signal"], info["front"], info["rest"].as_f64().unwrap_or(0.0).min(999.0).round()
                );
                *by_blocker.entry(key).or_default() += 1;
            }
        }
    }
    eprintln!("parados >=5s: {} ; bloqueadores de 'saída ocupada':", list.len());
    let mut v: Vec<_> = by_blocker.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, c) in v.iter().take(12) {
        eprintln!("  {c:>3}x  {k}");
    }
}

// ------------------------------------------------------------------ placas de preferência

/// Nós de cruzamento sem semáforo (3+ ramos).
fn plain_junctions(n: &Network) -> Vec<u32> {
    (0..n.net.nodes.len() as u32).filter(|&i| !n.net.nodes[i as usize].signal && n.net.nodes[i as usize].ends.len() >= 3).collect()
}

#[test]
fn priority_signs_list_approaches_and_apply_to_the_sim() {
    let mut n = sample();
    let j = *plain_junctions(&n).first().expect("o exemplo tem cruzamento sem semáforo");
    let list: Vec<serde_json::Value> = serde_json::from_str(&n.node_approaches_json(j)).unwrap();
    assert!(list.len() >= 3, "uma aproximação por ramo com faixa de entrada: {list:?}");
    assert!(list.iter().all(|a| a["level"] == 0));
    assert!(list.iter().any(|a| a["auto"] == 1), "alguma via tem preferência pelo mapa");
    // pare em todos os ramos do cruzamento
    let osm = n.net.nodes[j as usize].osm_id;
    let m: std::collections::HashMap<String, u8> = list.iter().map(|a| (format!("{osm}|{}", a["key"].as_str().unwrap()), 3u8)).collect();
    n.set_prio_edits(&serde_json::to_string(&m).unwrap()).unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_str(&n.node_approaches_json(j)).unwrap();
    assert!(list.iter().all(|a| a["level"] == 3), "nível salvo volta na listagem");
    n.start_sim(SIM_ZONES).unwrap();
    for _ in 0..(600.0 / 0.2) as u32 {
        n.sim.as_mut().unwrap().step(&n.net);
        let s = n.sim.as_ref().unwrap();
        for l in s.edge_lists() {
            for w in l.windows(2) {
                let (sa, _) = s.vehicle_s(w[0]);
                let (sb, lb) = s.vehicle_s(w[1]);
                assert!(sa <= sb - lb + 0.05, "sobreposição com placas de pare");
            }
        }
    }
    let st = n.sim.as_ref().unwrap().stats.clone();
    assert_eq!(st.spawned, st.finished + st.lost + n.sim.as_ref().unwrap().n_alive as u64);
    assert!(st.finished > 5, "a simulação continua fluindo com pare: {st:?}");
}

#[test]
fn priority_signs_survive_rebuild_payload() {
    let mut n = sample();
    n.set_edits(r#"{"priorities":{"1|1:2:3:0":2,"9|x":0,"9|y":7}}"#).unwrap();
    assert_eq!(n.edits.prios.len(), 1, "níveis fora de 1..3 são descartados");
}

#[test]
fn vehicle_destination_is_random_and_fixed_for_the_whole_trip() {
    let mut n = sim_net();
    let mut first: std::collections::HashMap<(usize, u32), u32> = std::collections::HashMap::new();
    let mut nodes = std::collections::HashSet::new();
    for _ in 0..60 {
        run(&mut n, 10.0);
        for (id, t0, node) in n.sim.as_ref().unwrap().vehicle_targets() {
            let prev = *first.entry((id, t0)).or_insert(node);
            assert_eq!(prev, node, "veículo {id} trocou de destino no meio da viagem");
            nodes.insert(node);
        }
    }
    let st = n.sim.as_ref().unwrap().stats.clone();
    assert!(st.finished > 10, "viagens concluídas: {st:?}");
    assert!(nodes.len() > 2, "destinos distintos sorteados: {}", nodes.len());
}
