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
    crate::conn::build_signals(&mut net, &edits, None);
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

#[test]
fn lane_changes_use_blinker_and_respect_solid_zone() {
    let mut n = sim_net();
    for _ in 0..60 {
        run(&mut n, 10.0);
    }
    let st = n.sim.as_ref().unwrap().stats.clone();
    assert!(st.finished > 10, "viagens concluídas com seta e zona contínua: {st:?}");
    assert!(st.lane_changes > 0, "ainda há trocas de faixa: {st:?}");
}

/// `PERF_OSM=arquivo cargo test --release recife_behavior -- --ignored --nocapture`: indicadores de comportamento e rota
/// (`CTTU=1` usa os semáforos da CTTU).
#[test]
#[ignore]
fn recife_behavior() {
    let data = std::fs::read(std::env::var("PERF_OSM").expect("defina PERF_OSM")).unwrap();
    let q = |id: i64, x0: f64, y0: f64, x1: f64, y1: f64, gen: f64| format!(r#"{{"id":{id},"gen":{gen},"attr":1,"poly":[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}]]}}"#);
    let gen = 2500.0 * std::env::var("SIM_GEN").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
    let zones = [q(1, -3000.0, -3000.0, -1500.0, -1500.0, gen), q(2, 1500.0, -3000.0, 3000.0, -1500.0, gen), q(3, -3000.0, 1500.0, -1500.0, 3000.0, gen), q(4, 1500.0, 1500.0, 3000.0, 3000.0, gen)].join(",");
    let extra = std::env::var("SIM_PARAMS").unwrap_or_default();
    // CTTU=1: semáforos da base da CTTU (com os ajustes manuais) no lugar dos do OSM
    let mut n = if std::env::var("CTTU").is_ok() {
        let pts: Vec<crate::extsig::ExtSignal> = serde_json::from_str(include_str!("../../public/data/semaforos_cttu.json")).unwrap();
        build_network_ext(&data, &pts).unwrap()
    } else {
        build_network_lazy_for_test(&data)
    };
    n.ensure_conns();
    let mut sum = [0.0f64; 6];
    let seed0 = std::env::var("SIM_SEED0").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    for seed in seed0 + 1..=seed0 + 3 {
        let json = format!(r#"{{"zones":[{zones}],"params":{{"seed":{seed},"hour":8,"busShare":0,"beta":0.0{extra}}}}}"#);
        n.start_sim(&json).unwrap();
        if std::env::var("OVERLAP_LOG").is_ok() {
            n.sim.as_mut().unwrap().overlap_log = Some(Vec::new());
        }
        let t = std::time::Instant::now();
        for _ in 0..(15 * 60 * 5) {
            n.sim.as_mut().unwrap().step(&n.net);
        }
        let s = n.sim.as_ref().unwrap();
        let r = s.report(&n.net);
        let ms = t.elapsed().as_secs_f64() * 1e3 / 4500.0;
        let row = [s.stats.finished as f64, r["totalDelayAllVehH"].as_f64().unwrap(), s.stats.queued as f64, s.stats.illegal_moves as f64 + s.stats.solid_crossings as f64 * 1e-6, s.stats.harsh_brakes as f64, ms];
        eprintln!("semente {seed}: concluídos {:.0}  atraso total {:.1} veíc·h  fila {:.0}  irregulares {:.0}  linha contínua {}  freadas {:.0}  passo {:.2} ms  rotas recalculadas {}", row[0], row[1], row[2], row[3], s.stats.solid_crossings, row[4], row[5], s.reroutes);
        for l in s.overlap_log.iter().flatten() {
            eprintln!("      OVL {l}");
        }
        eprintln!("   cruzamento: sobreposições {} (+{} geometria)  parados dentro {}  espera por ocupação {:.0} veíc·s  impasses {}", s.stats.junction_overlaps, s.stats.junction_overlaps_geom, s.stats.box_stops, s.stats.box_wait_s, s.stats.occ_deadlocks);
        for k in 0..6 {
            sum[k] += row[k] / 3.0;
        }
    }
    eprintln!("MÉDIA: concluídos {:.0}  atraso total {:.1} veíc·h  fila {:.0}  irregulares {:.0}  freadas {:.0}  passo {:.2} ms", sum[0], sum[1], sum[2], sum[3], sum[4], sum[5]);
}

/// Portal na divisa (zona `gateway`): o tráfego das cidades vizinhas nasce na via expressa que entra no mapa.
/// Zona comum nunca nasce em via expressa.
#[test]
fn gateway_zone_spawns_on_expressway() {
    let osm = r#"<osm>
<node id="1" lat="-8.0" lon="-34.910"/><node id="2" lat="-8.0" lon="-34.905"/><node id="3" lat="-8.0" lon="-34.900"/>
<node id="4" lat="-8.0" lon="-34.895"/><node id="5" lat="-8.0" lon="-34.890"/>
<way id="10"><nd ref="1"/><nd ref="2"/><nd ref="3"/><tag k="highway" v="trunk"/><tag k="name" v="BR-101"/></way>
<way id="11"><nd ref="3"/><nd ref="4"/><nd ref="5"/><tag k="highway" v="residential"/><tag k="name" v="Rua A"/></way>
</osm>"#;
    let at = |n: &Network, osm: i64| n.net.nodes.iter().find(|x| x.osm_id == osm).unwrap().pos;
    let square = |p: crate::geo::V2, r: f64| format!("[[{},{}],[{},{}],[{},{}],[{},{}]]", p[0] - r, p[1] - r, p[0] + r, p[1] - r, p[0] + r, p[1] + r, p[0] - r, p[1] + r);
    let spawned = |gateway: bool| {
        let mut n = build_network(osm.as_bytes()).unwrap();
        let (g, d) = (square(at(&n, 1), 30.0), square(at(&n, 5), 30.0));
        let json = format!(
            r#"{{"zones":[{{"id":1001,"gen":600,"attr":1,"gateway":{gateway},"poly":{g}}},{{"id":2,"gen":0,"attr":1,"poly":{d}}}],
              "params":{{"seed":3,"hour":8,"busShare":0,"beta":0,"odManual":{{"1001>2":600}}}}}}"#
        );
        n.start_sim(&json).unwrap();
        run(&mut n, 300.0);
        let st = n.sim.as_ref().unwrap().stats.clone();
        (st.spawned, st.finished)
    };
    let (sp, fin) = spawned(true);
    assert!(sp > 20 && fin > 5, "portal gera e conclui viagens: gerados {sp}, concluídos {fin}");
    assert_eq!(spawned(false).0, 0, "zona comum não nasce em via expressa");
    // o portal pequeno não leva o aviso "sem vias dentro da zona"
    let n = build_network(osm.as_bytes()).unwrap();
    let p: crate::demand::Payload = serde_json::from_str(&format!(r#"{{"zones":[{{"id":1001,"gen":1,"attr":1,"gateway":true,"poly":{}}}]}}"#, square(at(&n, 1), 30.0))).unwrap();
    let r = crate::demand::resolve(&n.net, &p, &crate::demand::straight_line_minutes);
    assert!(r.zones[0].warning.is_none(), "{:?}", r.zones[0].warning);
}



#[test]
fn straight_connections_never_cross_or_skip_lanes() {
    // `PERF_OSM` troca a rede de exemplo por um mapa real (ex.: Recife)
    let mut n = match std::env::var("PERF_OSM") {
        Ok(p) => build_network_lazy_for_test(&std::fs::read(p).unwrap()),
        Err(_) => sample(),
    };
    n.ensure_conns();
    let net = &n.net;
    let (mut same_out, mut skip, mut total) = (0usize, 0usize, 0usize);
    for ni in 0..net.nodes.len() {
        let (lo, hi) = (net.conn_off[ni] as usize, net.conn_off[ni + 1] as usize);
        let cs: Vec<_> = net.conns[lo..hi].iter().filter(|c| c.state == crate::net::CONN_ALLOWED && c.kind == crate::net::K_STRAIGHT).collect();
        total += cs.len();
        for x in &cs {
            for y in &cs {
                let (a, b) = (&net.lanes[x.from_lane as usize], &net.lanes[y.from_lane as usize]);
                let (c, d) = (&net.lanes[x.to_lane as usize], &net.lanes[y.to_lane as usize]);
                if a.seg == b.seg && a.dir == b.dir && c.seg == d.seg && c.dir == d.dir && a.idx < b.idx && c.idx > d.idx {
                    same_out += 1;
                }
            }
        }
        // saltos: faixas vizinhas da mesma aproximação indo à mesma saída com um buraco de 2+
        for x in &cs {
            for y in &cs {
                let (a, b) = (&net.lanes[x.from_lane as usize], &net.lanes[y.from_lane as usize]);
                let (c, d) = (&net.lanes[x.to_lane as usize], &net.lanes[y.to_lane as usize]);
                if a.seg == b.seg && a.dir == b.dir && c.seg == d.seg && c.dir == d.dir && b.idx == a.idx + 1 && d.idx >= c.idx + 2 {
                    skip += 1;
                }
            }
        }
    }
    assert!(total > 0);
    assert_eq!((same_out, skip), (0, 0), "em frente: faixas que se cruzam / pulam uma faixa de saída ({total} conversões)");
}

/// Regras de realismo das conversões padrão na rede inteira. `PERF_OSM=mapa` para um mapa real; `AUDIT_SHOW=n` exemplos.
fn conn_audit(net: &crate::net::Net) -> std::collections::BTreeMap<&'static str, (usize, Vec<String>)> {
    use crate::net::*;
    let show: usize = std::env::var("AUDIT_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut out: std::collections::BTreeMap<&'static str, (usize, Vec<String>)> = Default::default();
    let mut hit = |k: &'static str, msg: String| {
        let e = out.entry(k).or_default();
        e.0 += 1;
        if e.1.len() < show {
            e.1.push(msg);
        }
    };
    for ni in 0..net.nodes.len() {
        let node = &net.nodes[ni];
        let (lo, hi) = (net.conn_off[ni] as usize, net.conn_off[ni + 1] as usize);
        let cs: Vec<&Conn> = net.conns[lo..hi].iter().filter(|c| c.state == CONN_ALLOWED).collect();
        let ends = &node.ends;
        for &a in ends {
            let (ins, _) = net.approach_lanes(a);
            if ins.is_empty() {
                continue;
            }
            let tagged = ins.iter().any(|&l| net.lanes[l as usize].tmask != 0);
            let tag = if tagged { "c/ turn:lanes" } else { "sem tags" };
            let h = crate::geo::mul(net.outward(a), -1.0);
            let ang = |b: u32| {
                let u = net.outward(b);
                crate::geo::cross(h, u).atan2(crate::geo::dot(h, u))
            };
            let exits: Vec<u32> = ends.iter().copied().filter(|&b| b != a && !net.approach_lanes(b).1.is_empty()).collect();
            if exits.is_empty() {
                continue;
            }
            let rank = |b: u32| exits.iter().filter(|&&x| ang(x) < ang(b)).count();
            let where_ = format!("nó osm {} pos [{:.0},{:.0}] ({tag})", node.osm_id, node.pos[0], node.pos[1]);
            let mine: Vec<&&Conn> = cs.iter().filter(|c| ins.contains(&c.from_lane)).collect();
            let key = |c: &Conn| {
                let tl = &net.lanes[c.to_lane as usize];
                let b = exits.iter().copied().find(|&b| net.approach_lanes(b).1.contains(&c.to_lane)).unwrap_or(u32::MAX);
                (if b == u32::MAX { usize::MAX } else { rank(b) }, tl.idx as usize)
            };
            // A: duas faixas da mesma aproximação se cruzam
            let mut crossed = false;
            for x in &mine {
                for y in &mine {
                    let (ix, iy) = (net.lanes[x.from_lane as usize].idx, net.lanes[y.from_lane as usize].idx);
                    if ix < iy && key(x) > key(y) {
                        crossed = true;
                    }
                }
            }
            if crossed {
                hit(if tagged { "A cruzamento entre faixas (c/ tags)" } else { "A cruzamento entre faixas" }, where_.clone());
            }
            // B: faixa sem conversão
            for &l in &ins {
                if !mine.iter().any(|c| c.from_lane == l) {
                    hit("B faixa de entrada sem saída", format!("{where_} faixa {}", net.lanes[l as usize].idx));
                }
            }
            // D: duas faixas da mesma aproximação na mesma faixa de saída, com outra faixa daquela saída livre
            for &b in &exits {
                let outs = net.approach_lanes(b).1;
                let to: Vec<u32> = mine.iter().filter(|c| outs.contains(&c.to_lane)).map(|c| c.to_lane).collect();
                let mut uniq = to.clone();
                uniq.sort();
                uniq.dedup();
                let froms: std::collections::BTreeSet<u32> = mine.iter().filter(|c| outs.contains(&c.to_lane)).map(|c| c.from_lane).collect();
                if uniq.len() < to.len() && uniq.len() < outs.len() && froms.len() <= outs.len() {
                    hit("D duas faixas na mesma saída com faixa livre", where_.clone());
                }
            }
            // F: seta do OSM (turn:lanes) pedindo manobra que a geometria não tem
            if tagged {
                let kinds: u8 = exits.iter().map(|&b| { let d = ang(b).to_degrees(); if d.abs() < 30.0 { 1u8 << K_STRAIGHT } else if d > 0.0 { 1 << K_RIGHT } else { 1 << K_LEFT } }).fold(0, |m, k| m | k);
                if ins.iter().any(|&l| { let t = net.lanes[l as usize].tmask & 7; t != 0 && t & kinds != t }) {
                    hit("F seta do OSM sem manobra correspondente", where_.clone());
                }
            }
            // G: estreitamento (mais faixas em frente que a saída tem)
            for &b in &exits {
                let outs = net.approach_lanes(b).1;
                let n_s = mine.iter().filter(|c| c.kind == K_STRAIGHT && outs.contains(&c.to_lane)).map(|c| c.from_lane).collect::<std::collections::BTreeSet<_>>().len();
                if n_s > outs.len() {
                    hit("G estreitamento em frente", format!("{where_} {} → {}", n_s, outs.len()));
                }
            }
            // E: saída "em frente" que ninguém usa
            for &b in &exits {
                if ang(b).to_degrees().abs() < 30.0 && !mine.iter().any(|c| net.approach_lanes(b).1.contains(&c.to_lane)) {
                    hit("E saída em frente sem conversão", where_.clone());
                }
            }
        }
    }
    out
}

#[test]
fn default_connections_are_realistic() {
    let mut n = match std::env::var("PERF_OSM") {
        Ok(p) => build_network_lazy_for_test(&std::fs::read(p).unwrap()),
        Err(_) => sample(),
    };
    n.ensure_conns();
    let r = conn_audit(&n.net);
    for (k, (c, ex)) in &r {
        eprintln!("{k}: {c}");
        for e in ex {
            eprintln!("    {e}");
        }
    }
    // sem tags do OSM, nenhuma aproximação pode ter faixas que se cruzam
    assert!(!r.contains_key("A cruzamento entre faixas"), "{:?}", r.get("A cruzamento entre faixas"));
    if std::env::var("PERF_OSM").is_err() {
        assert!(!r.contains_key("B faixa de entrada sem saída"), "{:?}", r.get("B faixa de entrada sem saída"));
    }
}

/// Galeria SVG de cruzamentos (conversões padrão). `PERF_OSM=mapa GALLERY=arquivo.html [GALLERY_N=40] [GALLERY_SEED=1]`.
#[test]
#[ignore]
fn conn_gallery() {
    use crate::net::*;
    let data = std::fs::read(std::env::var("PERF_OSM").unwrap()).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let net = &n.net;
    let count: usize = std::env::var("GALLERY_N").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    let mut seed: u64 = std::env::var("GALLERY_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let name = |si: u32| net.names.get(net.segs[si as usize].name as usize).cloned().unwrap_or_default();
    // cruzamentos com alguma aproximação de 2+ faixas e 3+ pontas
    let cands: Vec<usize> = (0..net.nodes.len()).filter(|&ni| {
        let e = &net.nodes[ni].ends;
        e.len() >= 3 && e.iter().any(|&x| net.approach_lanes(x).0.len() >= 2)
    }).collect();
    let mut picked = Vec::new();
    while picked.len() < count.min(cands.len()) {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        let ni = cands[(seed % cands.len() as u64) as usize];
        if !picked.contains(&ni) { picked.push(ni); }
    }
    let pal = ["#ff4d4d", "#7cfc00", "#4f6bff", "#ffd166", "#ff9a3d", "#4cc9f0", "#c77dff", "#f78fb3"];
    let mut html = String::from("<html><body style='background:#0b0f10;color:#ddd;font:12px sans-serif'><div style='display:flex;flex-wrap:wrap'>");
    for &ni in &picked {
        let c = net.nodes[ni].pos;
        let r = 24.0f32;
        let k = 240.0 / (2.0 * r);
        let tx = |p: [f32; 2]| ((p[0] - c[0] as f32 + r) * k, (p[1] - c[1] as f32 + r) * k);
        let mut svg = format!("<svg width='240' height='240' viewBox='0 0 240 240' style='background:#141a1c;margin:2px'>");
        // faixas: linha da faixa até 30 m antes/depois
        for &e in &net.nodes[ni].ends {
            let (ins, outs) = net.approach_lanes(e);
            for &l in &ins {
                let ln = &net.lanes[l as usize];
                let a = [ln.end[0] - ln.edir[0] * 18.0, ln.end[1] - ln.edir[1] * 18.0];
                let (x0, y0) = tx(a); let (x1, y1) = tx(ln.end);
                svg += &format!("<line x1='{x0}' y1='{y0}' x2='{x1}' y2='{y1}' stroke='#3a4446' stroke-width='6'/><line x1='{x0}' y1='{y0}' x2='{x1}' y2='{y1}' stroke='#666' stroke-width='1' marker-end=''/>");
                svg += &format!("<circle cx='{x1}' cy='{y1}' r='3' fill='{}'/><text x='{}' y='{}' fill='#fff' font-size='8'>{}{}</text>", pal[ln.idx as usize % 8], x1 + 4.0, y1 - 4.0, ln.idx, if ln.tmask != 0 { "*" } else { "" });
            }
            for &l in &outs {
                let ln = &net.lanes[l as usize];
                let b = [ln.start[0] + ln.sdir[0] * 18.0, ln.start[1] + ln.sdir[1] * 18.0];
                let (x0, y0) = tx(ln.start); let (x1, y1) = tx(b);
                svg += &format!("<line x1='{x0}' y1='{y0}' x2='{x1}' y2='{y1}' stroke='#2c3436' stroke-width='6'/><circle cx='{x0}' cy='{y0}' r='2.5' fill='#ddd'/><text x='{}' y='{}' fill='#999' font-size='8'>{}</text>", x0 + 3.0, y0 + 9.0, ln.idx);
            }
        }
        let (lo, hi) = (net.conn_off[ni] as usize, net.conn_off[ni + 1] as usize);
        for cn in net.conns[lo..hi].iter().filter(|c| c.state == CONN_ALLOWED) {
            let q = cn.curve;
            let p = |i: usize| tx([q[i * 2], q[i * 2 + 1]]);
            let (a, b, cc, d) = (p(0), p(1), p(2), p(3));
            let col = pal[net.lanes[cn.from_lane as usize].idx as usize % 8];
            svg += &format!("<path d='M{} {} C{} {} {} {} {} {}' stroke='{col}' fill='none' stroke-width='1.6' opacity='0.9'/>", a.0, a.1, b.0, b.1, cc.0, cc.1, d.0, d.1);
        }
        let names: Vec<String> = net.nodes[ni].ends.iter().map(|&e| { let (i, o) = net.approach_lanes(e); format!("{} ({}→ {}←)", name(e >> 1), i.len(), o.len()) }).collect();
        svg += "</svg>";
        html += &format!("<div style='width:244px;font-size:10px'>{svg}<div>#{} osm {} {}</div><div style='color:#888;height:38px;overflow:hidden'>{}</div></div>", picked.iter().position(|&x| x == ni).unwrap(), net.nodes[ni].osm_id, if net.nodes[ni].signal { "🚦" } else { "" }, names.join(" · "));
    }
    html += "</div></body></html>";
    std::fs::write(std::env::var("GALLERY").unwrap(), html).unwrap();
}

/// `PERF_OSM=mapa NODES=osm1,osm2 cargo test --release conn_dump -- --ignored --nocapture`
#[test]
#[ignore]
fn conn_dump() {
    use crate::net::*;
    let data = std::fs::read(std::env::var("PERF_OSM").unwrap()).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let net = &n.net;
    let name = |si: u32| net.names.get(net.segs[si as usize].name as usize).cloned().unwrap_or_default();
    let ids: Vec<i64> = std::env::var("NODES").unwrap().split(',').map(|x| x.trim().parse().unwrap()).collect();
    for ni in (0..net.nodes.len()).filter(|&i| ids.contains(&net.nodes[i].osm_id)) {
        eprintln!("nó osm {}", net.nodes[ni].osm_id);
        for r in n.raw.restrictions.iter().filter(|r| r.via == net.nodes[ni].osm_id) {
            eprintln!("  restrição {} de way {} para way {}", r.kind, r.from, r.to);
        }
        let (lo, hi) = (net.conn_off[ni] as usize, net.conn_off[ni + 1] as usize);
        for &a in &net.nodes[ni].ends {
            let (ins, outs) = net.approach_lanes(a);
            let h = crate::geo::mul(net.outward(a), -1.0);
            eprintln!("  ponta {a} '{}' way {} entra {} sai {} (chega rumo {:.0}°)", name(a >> 1), net.segs[(a >> 1) as usize].way_id, ins.len(), outs.len(), h[1].atan2(h[0]).to_degrees());
            for &b in &net.nodes[ni].ends {
                if b == a || net.approach_lanes(b).1.is_empty() || ins.is_empty() { continue; }
                let u = net.outward(b);
                eprintln!("      -> ponta {b} '{}' ângulo {:.0}° ({} faixas)", name(b >> 1), crate::geo::cross(h, u).atan2(crate::geo::dot(h, u)).to_degrees(), net.approach_lanes(b).1.len());
            }
            for &l in &ins {
                let blocked: Vec<String> = net.conns[lo..hi].iter().filter(|c| c.state == CONN_BLOCKED && c.from_lane == l).map(|c| { let b = net.nodes[ni].ends.iter().find(|&&b| net.approach_lanes(b).1.contains(&c.to_lane)).unwrap(); format!("p{}#{}", b, net.lanes[c.to_lane as usize].idx) }).collect();
                if !blocked.is_empty() { eprintln!("      faixa {} BLOQUEADAS por restrição: {}", net.lanes[l as usize].idx, blocked.join(" ")); }
                let v: Vec<String> = net.conns[lo..hi].iter().filter(|c| c.state == CONN_ALLOWED && c.from_lane == l).map(|c| { let tl = &net.lanes[c.to_lane as usize]; let b = net.nodes[ni].ends.iter().find(|&&b| net.approach_lanes(b).1.contains(&c.to_lane)).unwrap(); format!("{}→p{}#{}", ["L","S","R","U"][c.kind as usize], b, tl.idx) }).collect();
                eprintln!("      faixa {} (tag {}): {}", net.lanes[l as usize].idx, net.lanes[l as usize].tmask, v.join(" "));
            }
        }
    }
}

/// Vazão de entrada pelos portais da divisa num mapa real. `PERF_OSM=mapa cargo test --release gateway_entry_flow -- --ignored --nocapture`
#[test]
#[ignore]
fn gateway_entry_flow() {
    use crate::net::*;
    let data = std::fs::read(std::env::var("PERF_OSM").unwrap()).unwrap();
    let mut n = build_network_lazy_for_test(&data);
    n.ensure_conns();
    let rate: f64 = std::env::var("GATE_RATE").ok().and_then(|v| v.parse().ok()).unwrap_or(1800.0);
    // pontas da rede em vias principais com 2+ faixas entrando no mapa
    let mut ends: Vec<(usize, usize)> = Vec::new();
    for (ni, nd) in n.net.nodes.iter().enumerate() {
        if nd.ends.len() != 1 {
            continue;
        }
        let e = nd.ends[0];
        let s = &n.net.segs[(e >> 1) as usize];
        let (_, outs) = n.net.approach_lanes(e);
        if s.hw <= HW_PRIMARY && outs.len() >= 2 && s.poly.length() > 60.0 {
            ends.push((ni, outs.len()));
        }
    }
    ends.sort_by_key(|x| std::cmp::Reverse(x.1));
    ends.truncate(4);
    // portais escolhidos à mão: GATE_NODES=osm1,osm2 (raio GATE_R m)
    if let Ok(v) = std::env::var("GATE_NODES") {
        let ids: Vec<i64> = v.split(',').map(|x| x.trim().parse().unwrap()).collect();
        ends = (0..n.net.nodes.len()).filter(|&i| ids.contains(&n.net.nodes[i].osm_id)).map(|i| (i, 0)).collect();
    }
    let gate_r: f64 = std::env::var("GATE_R").ok().and_then(|v| v.parse().ok()).unwrap_or(30.0);
    let sq = |p: crate::geo::V2, r: f64| format!("[[{},{}],[{},{}],[{},{}],[{},{}]]", p[0] - r, p[1] - r, p[0] + r, p[1] - r, p[0] + r, p[1] + r, p[0] - r, p[1] + r);
    let mut zones = Vec::new();
    let mut od = Vec::new();
    for (k, &(ni, lanes)) in ends.iter().enumerate() {
        let id = 1001 + k;
        eprintln!("portal {id}: nó osm {} ({} faixas entrando) '{}'", n.net.nodes[ni].osm_id, lanes, n.net.nodes[ni].ends.first().map(|&e| n.net.names.get(n.net.segs[(e >> 1) as usize].name as usize).cloned().unwrap_or_default()).unwrap_or_default());
        let out = std::env::var("GATE_DIR").map_or(false, |v| v == "out");
        zones.push(format!(r#"{{"id":{id},"gen":{rate},"attr":1,"gateway":true,"poly":{}}}"#, sq(n.net.nodes[ni].pos, gate_r)));
        od.push(if out { format!(r#""2>{id}":{rate}"#) } else { format!(r#""{id}>2":{rate}"#) });
    }
    let b = n.net.bounds;
    let c = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    zones.push(format!(r#"{{"id":2,"gen":{},"attr":1,"poly":{}}}"#, rate * 4.0, sq(c, 2500.0)));
    let json = format!(r#"{{"zones":[{}],"params":{{"seed":1,"hour":8,"busShare":0,"beta":0,"odManual":{{{}}}}}}}"#, zones.join(","), od.join(","));
    n.start_sim(&json).unwrap();
    let mins: usize = std::env::var("GATE_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    for m in 1..=mins {
        for _ in 0..300 {
            n.sim.as_mut().unwrap().step(&n.net);
        }
        if m % 5 == 0 {
            let s = n.sim.as_ref().unwrap();
            eprintln!("min {m:>2}: gerados {} (esperado {:.0}) fila de entrada {} vivos {} concluídos {}", s.stats.spawned, rate * ends.len() as f64 * m as f64 / 60.0, s.stats.queued, s.n_alive, s.stats.finished);
        }
    }
    eprintln!("{}", n.sim.as_mut().unwrap().gate_report(&n.net));
    let s = n.sim.as_mut().unwrap();
    let r = s.report(&n.net);
    eprintln!("atraso médio (todos) {} s; perdidos {}", r["meanDelayAll"], s.stats.lost);
    for l in s.stuck_report(&n.net, std::env::var("STUCK_S").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0)).iter().take(25) {
        eprintln!("PRESO {l}");
    }
}

/// Viaduto (layer=1) que no OSM compartilha nós com a rotatória do térreo: os níveis não se conectam,
/// mas a rampa que termina no viaduto continua ligada a ele.
#[test]
fn bridge_sharing_nodes_with_ground_road_is_separated() {
    let xml = r#"<osm>
      <node id="1" lat="0.0000" lon="0.0000"/><node id="2" lat="0.0000" lon="0.0010"/>
      <node id="3" lat="0.0000" lon="0.0020"/><node id="4" lat="0.0000" lon="0.0030"/>
      <node id="5" lat="-0.0010" lon="0.0010"/><node id="6" lat="0.0010" lon="0.0010"/>
      <node id="7" lat="0.0010" lon="0.0030"/>
      <way id="10"><nd ref="1"/><nd ref="2"/><nd ref="3"/><nd ref="4"/>
        <tag k="highway" v="trunk"/><tag k="bridge" v="viaduct"/><tag k="layer" v="1"/></way>
      <way id="11"><nd ref="5"/><nd ref="2"/><nd ref="6"/><tag k="highway" v="secondary"/></way>
      <way id="12"><nd ref="7"/><nd ref="3"/><tag k="highway" v="trunk_link"/><tag k="layer" v="1"/></way>
    </osm>"#;
    let n = build_network(xml.as_bytes()).expect("importa");
    let ends = |osm: i64| n.net.nodes.iter().filter(|x| x.osm_id == osm).map(|x| x.ends.len()).collect::<Vec<_>>();
    // sem o nó compartilhado, cada via só passa por ali: não há cruzamento algum
    assert!(ends(2).is_empty() && ends(2 + (65 << 40)).is_empty(), "viaduto e térreo não se cruzam");
    assert!(n.net.nodes.iter().all(|x| x.ends.len() < 4));
    assert_eq!(ends(3), vec![3], "rampa no mesmo nível segue conectada ao viaduto");
    assert!(n.net.meshes_ok());
}

/// Avenida de pista dupla (duas mãos a ~15 m) cortada por uma rua; semáforo OSM na rua, longe de tudo.
const DUAL: &str = r#"<?xml version="1.0"?><osm version="0.6">
 <node id="1" lat="-8.0600000" lon="-34.9100000"/><node id="2" lat="-8.0600000" lon="-34.9050000"/><node id="3" lat="-8.0600000" lon="-34.9000000"/>
 <node id="11" lat="-8.0601350" lon="-34.9000000"/><node id="12" lat="-8.0601350" lon="-34.9050000"/><node id="13" lat="-8.0601350" lon="-34.9100000"/>
 <node id="20" lat="-8.0580000" lon="-34.9050000"/><node id="21" lat="-8.0570000" lon="-34.9050000"><tag k="highway" v="traffic_signals"/></node>
 <node id="22" lat="-8.0620000" lon="-34.9050000"/>
 <way id="100"><nd ref="1"/><nd ref="2"/><nd ref="3"/><tag k="highway" v="primary"/><tag k="oneway" v="yes"/><tag k="name" v="Av. A"/></way>
 <way id="101"><nd ref="11"/><nd ref="12"/><nd ref="13"/><tag k="highway" v="primary"/><tag k="oneway" v="yes"/><tag k="name" v="Av. A"/></way>
 <way id="102"><nd ref="21"/><nd ref="20"/><nd ref="2"/><nd ref="12"/><nd ref="22"/><tag k="highway" v="residential"/><tag k="name" v="Rua B"/></way>
</osm>"#;

fn ext(id: u32, lat: f64, lon: f64, cruzamento: bool) -> crate::extsig::ExtSignal {
    crate::extsig::ExtSignal { id, lat, lon, cruzamento, ..Default::default() }
}

#[test]
fn external_signals_replace_osm_ones() {
    let osm = build_network(DUAL.as_bytes()).unwrap();
    assert_eq!(osm.net.signals.len(), 1, "sem base externa fica o semáforo do OSM");

    let pts = [
        ext(7, -8.0600700, -34.9050600, true), // cruzamento: os dois nós da pista dupla (2 e 12)
        ext(8, -8.0600675, -34.9080000, false), // travessia entre as mãos, longe de vértices
    ];
    let n = build_network_ext(DUAL.as_bytes(), &pts).unwrap();
    let sig: Vec<&Node> = n.net.signals.iter().map(|s| &n.net.nodes[s.node as usize]).collect();
    let ids: std::collections::HashSet<i64> = sig.iter().map(|n| n.osm_id).collect();
    assert!(!ids.contains(&21), "semáforo do OSM removido");
    assert!(ids.contains(&2) && ids.contains(&12), "os dois nós do cruzamento: {ids:?}");
    let novos: Vec<&&Node> = sig.iter().filter(|n| n.osm_id >= 200 << 40).collect();
    assert_eq!(novos.len(), 2, "um nó novo em cada mão da avenida");
    assert!(novos.iter().all(|n| n.ends.len() == 2), "nós de trecho");
    let m = &n.ext_signals.as_ref().unwrap().nodes;
    assert_eq!(m.get(&2), Some(&7));
    assert!(novos.iter().all(|x| m.get(&x.osm_id) == Some(&8)));
    let meta: serde_json::Value = serde_json::from_str(&n.meta_json()).unwrap();
    assert_eq!(meta["signalSource"], "ext");
}

/// Pontas em verde de um semáforo no instante `t`, com a mesma conta da simulação.
fn green_ends(s: &crate::net::Signal, t: f64) -> Vec<u32> {
    let cycle: f64 = s.phases.iter().map(|p| (p.green + p.yellow + p.all_red) as f64).sum();
    let mut r = (t + s.offset as f64).rem_euclid(cycle);
    for p in &s.phases {
        if r < p.green as f64 {
            return p.ends.clone();
        }
        r -= (p.green + p.yellow + p.all_red) as f64;
    }
    Vec::new()
}

#[test]
fn external_signal_nodes_share_one_controller() {
    let pts = [ext(7, -8.0600700, -34.9050600, true), ext(8, -8.0600675, -34.9080000, false)];
    let mut n = build_network_ext(DUAL.as_bytes(), &pts).unwrap();
    let check = |n: &crate::Network, offset: f32| {
        let by_osm = |id: i64| n.net.signals.iter().find(|s| n.net.nodes[s.node as usize].osm_id == id).unwrap();
        let (a, b) = (by_osm(2), by_osm(12));
        let cycle = |s: &crate::net::Signal| s.phases.iter().map(|p| p.green + p.yellow + p.all_red).sum::<f32>();
        assert_eq!(cycle(a), cycle(b), "mesmo ciclo");
        assert_eq!((a.offset, b.offset), (offset, offset), "mesma defasagem");
        // eixo (direção módulo 180°, em graus) de uma aproximação
        let axis = |e: u32| {
            let u = n.net.outward(e);
            (u[1].atan2(u[0]).to_degrees().rem_euclid(180.0) / 90.0).round() as i32 % 2
        };
        let mut seen = std::collections::HashSet::new();
        for t in 0..cycle(a) as i32 {
            let (ga, gb) = (green_ends(a, t as f64), green_ends(b, t as f64));
            assert_eq!(ga.is_empty(), gb.is_empty(), "t={t}: os dois nós abrem e fecham juntos");
            let axes: std::collections::HashSet<i32> = ga.iter().chain(&gb).map(|&e| axis(e)).collect();
            assert!(axes.len() <= 1, "t={t}: verde em eixos conflitantes {axes:?}");
            seen.extend(axes);
        }
        assert_eq!(seen.len(), 2, "avenida e rua têm verde em algum momento");
    };
    check(&n, 0.0);

    // travessia nas duas mãos: um controlador só, com a fase de pedestres
    let mid: Vec<&crate::net::Signal> = n.net.signals.iter().filter(|s| n.net.nodes[s.node as usize].osm_id >= 200 << 40).collect();
    assert!(mid.iter().all(|s| s.phases.len() == 2 && s.phases[1].ends.is_empty()));

    // edição de defasagem num nó vale para o controlador; fases editadas continuam respeitadas
    n.set_edits(r#"{"signals":[{"node":12,"enabled":true,"offset":10}]}"#).unwrap();
    check(&n, 10.0);
    let key = n.net.signals.iter().find(|s| n.net.nodes[s.node as usize].osm_id == 2).unwrap().ends[0].key.clone();
    n.set_edits(&format!(
        r#"{{"signals":[{{"node":12,"enabled":true,"offset":10}},{{"node":2,"enabled":true,"offset":10,"phases":[{{"green":20,"yellow":3,"allRed":2,"approaches":["{key}"]}}]}}]}}"#
    ))
    .unwrap();
    let a = n.net.signals.iter().find(|s| n.net.nodes[s.node as usize].osm_id == 2).unwrap();
    assert_eq!((a.phases[0].green, a.offset), (20.0, 10.0));
}

#[test]
fn external_signal_manual_adjustment() {
    let fases = vec![
        crate::extsig::ExtPhase { vias: vec!["av. a".into()], verde: 44.0, amarelo: 3.0, vermelho: 2.0 },
        crate::extsig::ExtPhase { vias: vec!["*".into()], verde: 17.0, amarelo: 3.0, vermelho: 2.0 },
    ];
    // só o nó 2 (o encaixe automático pegaria também o 12) e um nó novo na Rua B, ao norte do cruzamento
    let adj = crate::extsig::ExtSignal { nos: vec![2], pontos: vec![[-8.0590, -34.9050]], fases: fases.clone(), ..ext(9, -8.0600700, -34.9050600, true) };
    let n = build_network_ext(DUAL.as_bytes(), &[adj]).unwrap();
    let net = &n.net;
    let osm: std::collections::HashSet<i64> = net.signals.iter().map(|s| net.nodes[s.node as usize].osm_id).collect();
    let novo = (200i64 << 40) + 9 * 64 + 32;
    assert_eq!(osm, [2, novo].into_iter().collect(), "nós do ajuste, sem o encaixe automático");
    let name = |e: u32| net.names[net.segs[(e >> 1) as usize].name as usize].as_str();
    let s2 = net.signals.iter().find(|s| net.nodes[s.node as usize].osm_id == 2).unwrap();
    assert_eq!(s2.phases.iter().map(|p| p.green).collect::<Vec<_>>(), vec![44.0, 17.0]);
    assert!(!s2.phases[0].ends.is_empty() && s2.phases[0].ends.iter().all(|&e| name(e) == "Av. A"));
    assert!(!s2.phases[1].ends.is_empty() && s2.phases[1].ends.iter().all(|&e| name(e) == "Rua B"));
    // no nó novo só chega a Rua B: verde na fase "*", vermelho na da avenida (mesmo ciclo do controlador)
    let sn = net.signals.iter().find(|s| net.nodes[s.node as usize].osm_id == novo).unwrap();
    assert_eq!(sn.phases.len(), 2);
    assert!(sn.phases[0].ends.is_empty() && !sn.phases[1].ends.is_empty());
    assert_eq!(n.ext_signals.as_ref().unwrap().nodes.get(&novo), Some(&9));

    // nós do ajuste que não existem neste mapa: volta o encaixe automático (com as fases automáticas, não as do ajuste)
    let adj = crate::extsig::ExtSignal { nos: vec![999_999], fases, ..ext(9, -8.0600700, -34.9050600, true) };
    let n = build_network_ext(DUAL.as_bytes(), &[adj]).unwrap();
    let osm: std::collections::HashSet<i64> = n.net.signals.iter().map(|s| n.net.nodes[s.node as usize].osm_id).collect();
    assert!(osm.contains(&2) && osm.contains(&12));
    assert!(n.net.signals.iter().all(|s| s.phases.iter().all(|p| p.green != 44.0)));
}

#[test]
fn external_signal_adjustment_turns_and_bans() {
    use crate::net::{CONN_ALLOWED, K_RIGHT};
    let base = || crate::extsig::ExtSignal { nos: vec![2, 12], ..ext(9, -8.0600700, -34.9050600, true) };
    // conversões permitidas no nó OSM `osm` saindo do way `from`: (way de destino, manobra)
    let allowed = |n: &crate::Network, osm: i64, from: i64| -> Vec<(i64, u8)> {
        let net = &n.net;
        let ni = net.nodes.iter().position(|x| x.osm_id == osm).unwrap();
        let way = |l: u32| net.segs[net.lanes[l as usize].seg as usize].way_id;
        net.conns[net.conn_off[ni] as usize..net.conn_off[ni + 1] as usize]
            .iter()
            .filter(|c| c.state == CONN_ALLOWED && way(c.from_lane) == from)
            .map(|c| (way(c.to_lane), c.kind))
            .collect()
    };
    let n = build_network_ext(DUAL.as_bytes(), &[base()]).unwrap();
    assert!(allowed(&n, 2, 100).iter().any(|c| c.0 == 102), "sem ajuste a Av. A entra na Rua B");

    // proibir: a Av. A não entra mais na Rua B no nó 2 (segue em frente)
    let n = build_network_ext(DUAL.as_bytes(), &[crate::extsig::ExtSignal { proibir: vec![[100, 102]], ..base() }]).unwrap();
    let a = allowed(&n, 2, 100);
    assert!(!a.is_empty() && a.iter().all(|c| c.0 == 100), "{a:?}");

    // setas: as faixas da Av. A no nó 2 só viram à direita
    let setas = vec![crate::extsig::ExtTurns { way: 100, turn: "right|right".into(), lado: String::new() }];
    let n = build_network_ext(DUAL.as_bytes(), &[crate::extsig::ExtSignal { setas, ..base() }]).unwrap();
    let a = allowed(&n, 2, 100);
    assert!(!a.is_empty() && a.iter().all(|c| c.1 == K_RIGHT), "{a:?}");

    // fase por way: só a mão 101 da avenida na primeira fase
    let fases = vec![
        crate::extsig::ExtPhase { vias: vec!["way:101".into()], verde: 40.0, amarelo: 3.0, vermelho: 2.0 },
        crate::extsig::ExtPhase { vias: vec!["*".into()], verde: 20.0, amarelo: 3.0, vermelho: 2.0 },
    ];
    let n = build_network_ext(DUAL.as_bytes(), &[crate::extsig::ExtSignal { fases, ..base() }]).unwrap();
    let net = &n.net;
    let s12 = net.signals.iter().find(|s| net.nodes[s.node as usize].osm_id == 12).unwrap();
    let s2 = net.signals.iter().find(|s| net.nodes[s.node as usize].osm_id == 2).unwrap();
    let way = |e: u32| net.segs[(e >> 1) as usize].way_id;
    assert!(!s12.phases[0].ends.is_empty() && s12.phases[0].ends.iter().all(|&e| way(e) == 101));
    assert!(s2.phases[0].ends.is_empty(), "no nó 2 a mão 100 não está na fase do way 101");
}

/// Encaixe automático de um cruzamento de pista dupla com um nó de passagem e uma alça: o nó de passagem não vira
/// semáforo, a alça sem nome vai para a fase da transversal pelo eixo e a avenida (primária) leva mais verde que a rua.
const DUAL_LINK: &str = r#"<?xml version="1.0"?><osm version="0.6">
 <node id="1" lat="-8.0600000" lon="-34.9100000"/><node id="2" lat="-8.0600000" lon="-34.9050000"/><node id="3" lat="-8.0600000" lon="-34.9000000"/>
 <node id="11" lat="-8.0601350" lon="-34.9000000"/><node id="12" lat="-8.0601350" lon="-34.9050000"/><node id="13" lat="-8.0601350" lon="-34.9100000"/>
 <node id="20" lat="-8.0580000" lon="-34.9050000"/><node id="22" lat="-8.0620000" lon="-34.9050000"/>
 <node id="30" lat="-8.0600000" lon="-34.9052000"/>
 <node id="40" lat="-8.0601350" lon="-34.9049000"/><node id="41" lat="-8.0605000" lon="-34.9046000"/><node id="42" lat="-8.0640000" lon="-34.9046000"/>
 <way id="100"><nd ref="1"/><nd ref="30"/><tag k="highway" v="primary"/><tag k="oneway" v="yes"/><tag k="lanes" v="3"/><tag k="name" v="Av. A"/></way>
 <way id="104"><nd ref="30"/><nd ref="2"/><nd ref="3"/><tag k="highway" v="primary"/><tag k="oneway" v="yes"/><tag k="lanes" v="3"/><tag k="name" v="Av. A"/></way>
 <way id="101"><nd ref="11"/><nd ref="40"/><nd ref="12"/><nd ref="13"/><tag k="highway" v="primary"/><tag k="oneway" v="yes"/><tag k="lanes" v="3"/><tag k="name" v="Av. A"/></way>
 <way id="102"><nd ref="20"/><nd ref="2"/><nd ref="12"/><nd ref="22"/><tag k="highway" v="residential"/><tag k="name" v="Rua B"/></way>
 <way id="103"><nd ref="42"/><nd ref="41"/><nd ref="40"/><tag k="highway" v="residential"/><tag k="oneway" v="yes"/></way>
</osm>"#;

#[test]
fn external_signal_auto_main_vs_cross() {
    let n = build_network_ext(DUAL_LINK.as_bytes(), &[ext(7, -8.0600700, -34.9050600, true)]).unwrap();
    let net = &n.net;
    let osm: std::collections::HashSet<i64> = net.signals.iter().map(|s| net.nodes[s.node as usize].osm_id).collect();
    assert!(osm.contains(&2) && osm.contains(&12) && osm.contains(&40), "{osm:?}");
    assert!(!osm.contains(&30), "nó de passagem (uma entrada só) não vira semáforo: {osm:?}");
    let name = |e: u32| net.names.get(net.segs[(e >> 1) as usize].name as usize).map_or("", |s| s.as_str());
    for s in &net.signals {
        assert_eq!(s.phases.len(), 2, "principal × transversal");
        assert!(s.phases[0].ends.iter().all(|&e| name(e) == "Av. A"));
        assert!(s.phases[1].ends.iter().all(|&e| name(e) != "Av. A"));
        assert!(s.phases[0].green > s.phases[1].green, "a avenida leva mais verde: {:?}", s.phases.iter().map(|p| p.green).collect::<Vec<_>>());
    }
    // a alça sem nome (sobe do sul, eixo da Rua B) entra na fase da transversal
    let s40 = net.signals.iter().find(|s| net.nodes[s.node as usize].osm_id == 40).unwrap();
    assert!(s40.phases[1].ends.iter().any(|&e| name(e).is_empty()));
}

#[test]
fn external_signals_outside_map_keep_osm() {
    let n = build_network_ext(DUAL.as_bytes(), &[ext(1, -23.56, -46.65, true)]).unwrap();
    assert!(n.ext_signals.is_none());
    assert_eq!(n.net.signals.len(), 1);
}

/// Mapa real com a base da CTTU: `RECIFE_OSM=~/Downloads/mapa-recife.osm cargo test --release cttu_recife -- --ignored --nocapture`
#[test]
#[ignore]
fn cttu_recife() {
    let Ok(path) = std::env::var("RECIFE_OSM") else { return };
    let data = std::fs::read(path).unwrap();
    let pts: Vec<crate::extsig::ExtSignal> = serde_json::from_str(include_str!("../../public/data/semaforos_cttu.json")).unwrap();
    let osm = build_network(&data).unwrap();
    let t = std::time::Instant::now();
    let n = build_network_ext(&data, &pts).unwrap();
    let m = &n.ext_signals.as_ref().unwrap().nodes;
    let [x0, y0, x1, y1] = n.net.bounds;
    let geo = n.net.geo;
    let inside: Vec<u32> = pts
        .iter()
        .filter(|p| {
            let (x, y) = ((p.lon - geo[1]) * geo[2], -(p.lat - geo[0]) * 111_319.490_793_273_57);
            x >= x0 && x <= x1 && y >= y0 && y <= y1
        })
        .map(|p| p.id)
        .collect();
    let placed: std::collections::HashSet<u32> = n.net.signals.iter().filter_map(|s| m.get(&n.net.nodes[s.node as usize].osm_id).copied()).collect();
    let missing: Vec<u32> = inside.iter().copied().filter(|i| !placed.contains(i)).collect();
    // por controlador: nº de fases (do primeiro nó), nós, fases vazias em algum nó e nós de uma entrada só
    let mut by: std::collections::BTreeMap<u32, Vec<&crate::net::Signal>> = Default::default();
    for s in &n.net.signals {
        if let Some(&c) = m.get(&n.net.nodes[s.node as usize].osm_id) {
            by.entry(c).or_default().push(s);
        }
    }
    let mut hist: std::collections::BTreeMap<usize, usize> = Default::default();
    let (mut empty, mut single, mut nodes) = (0, 0, 0);
    for ss in by.values() {
        *hist.entry(ss[0].phases.len()).or_default() += 1;
        nodes += ss.len();
        empty += ss.iter().any(|s| s.phases.iter().any(|p| p.ends.is_empty())) as usize;
        single += ss.iter().filter(|s| s.ends.len() == 1).count();
    }
    println!("controladores por nº de fases {hist:?} | {nodes} nós | controladores com fase vazia em algum nó {empty} | nós de uma entrada só {single}");
    println!(
        "OSM: {} semáforos | CTTU: {} nós sinalizados, {} de {} pontos no mapa encaixados ({:.0} ms); sem encaixe: {:?}",
        osm.net.signals.len(),
        n.net.signals.len(),
        placed.len(),
        inside.len(),
        t.elapsed().as_secs_f64() * 1e3,
        missing
    );
}

/// Controladores CTTU conferidos à mão: nós, fases e quem tem verde em cada uma, no mapa real.
/// `RECIFE_OSM=~/Downloads/mapa-recife-novo.osm cargo test --release cttu_ajustes -- --ignored --nocapture`
/// Com `CTTU_IDS=66,28` lista também esses controladores (sem conferir nada).
#[test]
#[ignore]
fn cttu_ajustes_recife() {
    let Ok(path) = std::env::var("RECIFE_OSM") else { return };
    let data = std::fs::read(path).unwrap();
    let mut pts: Vec<crate::extsig::ExtSignal> = serde_json::from_str(include_str!("../../public/data/semaforos_cttu.json")).unwrap();
    // CTTU_AUTO=1: ignora os ajustes manuais (compara o encaixe automático com eles)
    let auto = std::env::var("CTTU_AUTO").is_ok();
    let manual: Vec<u32> = pts.iter().filter(|p| !p.nos.is_empty() || !p.pontos.is_empty()).map(|p| p.id).collect();
    if auto {
        for p in pts.iter_mut() {
            *p = crate::extsig::ExtSignal { id: p.id, lat: p.lat, lon: p.lon, cruzamento: p.cruzamento, ..Default::default() };
        }
    }
    let n = build_network_ext(&data, &pts).unwrap();
    let ext = n.ext_signals.as_ref().unwrap();
    let net = &n.net;
    let name = |e: u32| net.names.get(net.segs[(e >> 1) as usize].name as usize).map_or("(sem nome)", |s| s.as_str());
    let extra: Vec<u32> = std::env::var("CTTU_IDS").unwrap_or_default().split(',').filter_map(|x| x.trim().parse().ok()).collect();
    for p in pts.iter().filter(|p| manual.contains(&p.id) || extra.contains(&p.id)) {
        let adjusted = !p.nos.is_empty() || !p.pontos.is_empty();
        println!("== CTTU {}", p.id);
        let mut cycle = None;
        for s in net.signals.iter().filter(|s| ext.nodes.get(&net.nodes[s.node as usize].osm_id) == Some(&p.id)) {
            let node = &net.nodes[s.node as usize];
            let c: f32 = s.phases.iter().map(|f| f.green + f.yellow + f.all_red).sum();
            assert!(cycle.map_or(true, |x| x == c), "mesmo ciclo no controlador");
            cycle = Some(c);
            println!("  nó {} ({} ramos)", node.osm_id, node.ends.len());
            for (k, f) in s.phases.iter().enumerate() {
                let who: Vec<&str> = f.ends.iter().map(|&e| name(e)).collect();
                println!("    fase {} {:>3}s: {:?}", k + 1, f.green, who);
            }
            assert!(!adjusted || s.phases.len() == p.fases.len(), "nó {}: toda aproximação coberta pelas fases do ajuste", node.osm_id);
            // CTTU_CONNS=1: conversões ligadas do nó, agrupadas por rua de origem -> destino
            if std::env::var("CTTU_CONNS").is_ok() {
                let ni = s.node as usize;
                let mut g: std::collections::BTreeMap<(String, String, u8), (Vec<u8>, Vec<u8>)> = Default::default();
                for c in &net.conns[net.conn_off[ni] as usize..net.conn_off[ni + 1] as usize] {
                    if c.state != crate::net::CONN_ALLOWED {
                        continue;
                    }
                    let (l0, l1) = (&net.lanes[c.from_lane as usize], &net.lanes[c.to_lane as usize]);
                    let sn = |l: &crate::net::Lane| format!("{}#{}{}", net.names.get(net.segs[l.seg as usize].name as usize).map_or("?", |s| s.as_str()), l.seg, if l.dir == 0 { '+' } else { '-' });
                    let e = g.entry((sn(l0), sn(l1), c.kind)).or_default();
                    e.0.push(l0.idx);
                    e.1.push(l1.idx);
                }
                for ((a, b, k), (fi, ti)) in g {
                    println!("      {a} -> {b} [{}] faixas {fi:?} -> {ti:?}", ["esq", "frente", "dir", "retorno"][k as usize]);
                }
            }
        }
        if !adjusted {
            continue;
        }
        let placed = p.nos.iter().filter(|id| net.signals.iter().any(|s| net.nodes[s.node as usize].osm_id == **id)).count();
        assert_eq!(placed, p.nos.len(), "CTTU {}: todos os nós do ajuste viram semáforo", p.id);
    }
}

/// "Fechar o cruzamento" (CTB art. 45) vale só para a parcela `boxBlockPct` dos motoristas: com 0 % quase ninguém
/// entra sem espaço para sair; com 100 % isso acontece bem mais, e quem está dentro do cruzamento segura a transversal.
#[test]
fn box_blocking_applies_only_to_share() {
    let zones = |pct: f64| format!(r#"{{"zones":[
      {{"id":1,"gen":2500,"attr":1,"poly":[[-420,-30],[-150,-30],[-150,30],[-420,30]]}},
      {{"id":2,"gen":2500,"attr":1,"poly":[[150,-30],[420,-30],[420,30],[150,30]]}},
      {{"id":3,"gen":1500,"attr":2,"poly":[[-60,-320],[60,-320],[60,-250],[-60,-250]]}},
      {{"id":4,"gen":1500,"attr":2,"poly":[[-60,250],[60,250],[60,320],[-60,320]]}}],
      "params":{{"seed":3,"hour":8,"busShare":0,"beta":0.01,"boxBlockPct":{pct}}}}}"#);
    let go = |pct: f64| {
        let mut n = sample();
        n.start_sim(&zones(pct)).unwrap();
        n.sim.as_mut().unwrap().overlap_log = Some(Vec::new());
        run(&mut n, 1200.0);
        for l in n.sim.as_ref().unwrap().overlap_log.iter().flatten() {
            eprintln!("OVL {pct}: {l}");
        }
        n.sim.as_ref().unwrap().stats.clone()
    };
    let (none, all, part) = (go(0.0), go(100.0), go(30.0));
    eprintln!("0 %: {none:?}\n100 %: {all:?}\n30 %: {part:?}");
    assert_eq!(none.box_entries_blockers, 0);
    assert_eq!(all.box_entries_blockers, all.box_entries);
    assert!(all.box_entries > (none.box_entries + 5) * 3, "100 % fecha o cruzamento bem mais que 0 %: {} vs {}", all.box_entries, none.box_entries);
    assert!(part.box_entries_blockers > 0 && part.box_entries_blockers < part.box_entries.max(1) + 1);
    assert!(all.box_wait_s > none.box_wait_s, "transversal espera mais por cruzamento ocupado");
    assert_eq!(all.junction_overlaps, 0, "ocupação do ponto de conflito impede colisão com quem está parado no cruzamento");
}
