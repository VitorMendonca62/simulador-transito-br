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
        assert!(c.curve.iter().all(|v| v.is_finite()));
    }
    assert!(n.net.meshes_ok());
}

#[test]
fn restriction_blocks_left_turn() {
    let n = sample();
    let net = &n.net;
    let blocked: Vec<_> = net.conns.iter().filter(|c| !c.allowed).collect();
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
        let mut k: Vec<u8> = net.conns.iter().filter(|c| c.from_lane == lane).map(|c| c.kind).collect();
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
        for c in net.conns.iter().filter(|c| c.node == ni) {
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
        xml.net.conns.iter().filter(|c| !c.allowed).count(),
        pbf.net.conns.iter().filter(|c| !c.allowed).count(),
        "restrições também vêm do PBF"
    );
}
