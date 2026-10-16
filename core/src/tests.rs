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
    assert!(!n.meshes.asphalt.idx.is_empty());
    assert!(!n.meshes.white.idx.is_empty());
    assert!(!n.meshes.yellow.idx.is_empty());
    assert!(!n.meshes.bus.idx.is_empty(), "faixa de ônibus");
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
        n.meshes.asphalt.idx.len() / 3,
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
    let before = (n.net.lanes.len(), n.net.conns.len(), n.meshes.asphalt.pos.len());
    let k = key_of(&n, "Rua das Palmeiras", 1);
    n.set_edits(&edit_json(k, "\"oneway\":false,\"lf\":3,\"lb\":2")).unwrap();
    let seg = n.net.segs.iter().find(|s| s.key == k).unwrap();
    assert_eq!((seg.lf, seg.lb), (3, 2));
    assert!(n.net.lanes.len() > before.0, "mais faixas");
    assert!(n.net.conns.len() > before.1, "mais conexões candidatas");
    assert!(n.meshes.asphalt.pos.len() > before.2 / 2);
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
