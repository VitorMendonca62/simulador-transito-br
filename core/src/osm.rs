//! Leitura de .osm (XML) e .pbf para uma estrutura bruta mínima.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashSet;

pub struct RawWay {
    pub id: i64,
    pub refs: Vec<i64>,
    pub tags: Vec<(String, String)>,
}

pub struct RawRestriction {
    pub kind: String,
    pub from: i64,
    pub via: i64,
    pub to: i64,
}

#[derive(Default)]
pub struct RawOsm {
    /// (id, lat, lon) com lat/lon em 1e-7 grau. Ordenado por id após `finish`.
    pub nodes: Vec<(i64, i32, i32)>,
    pub signals: HashSet<i64>,
    pub crossings: HashSet<i64>,
    pub ways: Vec<RawWay>,
    pub restrictions: Vec<RawRestriction>,
}

impl RawOsm {
    pub fn finish(&mut self) {
        if !self.nodes.windows(2).all(|w| w[0].0 < w[1].0) {
            self.nodes.sort_unstable_by_key(|n| n.0);
            self.nodes.dedup_by_key(|n| n.0);
        }
        self.drop_far_ways();
        self.separate_levels();
    }

    /// Desfaz nós que o OSM compartilha (por erro de mapeamento) entre vias de níveis diferentes que
    /// passam por ele — ex.: viaduto atravessando uma rotatória no térreo. Transição legítima de nível
    /// acontece na ponta de uma via (rampa chegando à ponte); quando o nó é miolo de vias em 2+ níveis
    /// distintos, cada nível ganha a sua cópia do nó. O nível 0 (ou o mais próximo dele) mantém o id
    /// original, com semáforos e travessias; as cópias recebem ids sintéticos acima do espaço do OSM.
    fn separate_levels(&mut self) {
        let levels: Vec<i32> = self.ways.iter().map(|w| way_level(&w.tags)).collect();
        if levels.iter().all(|&l| l == 0) {
            return;
        }
        // nó -> [(via, nível, é miolo)]
        let mut uses: std::collections::HashMap<i64, Vec<(usize, i32, bool)>> = Default::default();
        for (wi, w) in self.ways.iter().enumerate() {
            let last = w.refs.len().saturating_sub(1);
            for (k, &r) in w.refs.iter().enumerate() {
                uses.entry(r).or_default().push((wi, levels[wi], k != 0 && k != last));
            }
        }
        let mut remap: Vec<(usize, i64, i64)> = Vec::new(); // (via, id antigo, id novo)
        let mut new_nodes: Vec<(i64, i32, i32)> = Vec::new();
        let mut sorted: Vec<(&i64, &Vec<(usize, i32, bool)>)> = uses.iter().collect();
        sorted.sort_unstable_by_key(|e| *e.0);
        for (&id, us) in sorted {
            let mut inner: Vec<i32> = us.iter().filter(|u| u.2).map(|u| u.1).collect();
            inner.sort_unstable();
            inner.dedup();
            if inner.len() < 2 {
                continue;
            }
            let Some((la, lo)) = self.coord(id) else { continue };
            let keep = *inner.iter().min_by_key(|&&l| (l.abs(), l)).unwrap();
            for &(wi, lv, _) in us {
                // ponta de via vai para o nível de miolo mais próximo do seu
                let g = *inner.iter().min_by_key(|&&l| ((l - lv).abs(), l)).unwrap();
                if g == keep {
                    continue;
                }
                let nid = id + (((g + 64) as i64) << 40);
                remap.push((wi, id, nid));
                new_nodes.push((nid, la, lo));
            }
        }
        if remap.is_empty() {
            return;
        }
        for (wi, old, new) in remap {
            for r in self.ways[wi].refs.iter_mut().filter(|r| **r == old) {
                *r = new;
            }
        }
        self.nodes.extend(new_nodes);
        self.nodes.sort_unstable_by_key(|n| n.0);
        self.nodes.dedup_by_key(|n| n.0);
    }

    /// Remove vias a mais de ~110 km da mediana das vias (sobras de conversões/consultas mal filtradas,
    /// que esticariam a caixa do mapa e deixariam a cidade invisível).
    fn drop_far_ways(&mut self) {
        let firsts: Vec<(i32, i32)> = self.ways.iter().filter_map(|w| w.refs.first().and_then(|&r| self.coord(r))).collect();
        if firsts.len() < 2 {
            return;
        }
        let med = |mut v: Vec<i32>| {
            let m = v.len() / 2;
            *v.select_nth_unstable(m).1
        };
        let (mlat, mlon) = (med(firsts.iter().map(|c| c.0).collect()), med(firsts.iter().map(|c| c.1).collect()));
        let k = (mlat as f64 * 1e-7).to_radians().cos();
        let far = |c: (i32, i32)| {
            let (dy, dx) = ((c.0 - mlat) as f64 * 1e-7, (c.1 - mlon) as f64 * 1e-7 * k);
            dy * dy + dx * dx > 1.0
        };
        let nodes = std::mem::take(&mut self.nodes);
        let coord = |id: i64| nodes.binary_search_by_key(&id, |n| n.0).ok().map(|i| (nodes[i].1, nodes[i].2));
        self.ways.retain(|w| !w.refs.iter().any(|&r| coord(r).is_some_and(far)));
        self.nodes = nodes;
    }

    pub fn coord(&self, id: i64) -> Option<(i32, i32)> {
        self.nodes.binary_search_by_key(&id, |n| n.0).ok().map(|i| (self.nodes[i].1, self.nodes[i].2))
    }

    fn node_tags<'a>(&mut self, id: i64, tags: impl Iterator<Item = (&'a str, &'a str)>) {
        for (k, v) in tags {
            match (k, v) {
                ("highway", "traffic_signals") => {
                    self.signals.insert(id);
                }
                ("highway", "crossing") => {
                    self.crossings.insert(id);
                }
                ("crossing", v) if v != "no" => {
                    self.crossings.insert(id);
                }
                _ => {}
            }
        }
    }

    fn finish_relation(&mut self, tags: &[(String, String)], members: &[(u8, i64, String)]) {
        let get = |k: &str| tags.iter().find(|t| t.0 == k).map(|t| t.1.as_str());
        if get("type") != Some("restriction") {
            return;
        }
        // `restriction:motorcar` vale para carros como a geral; `restriction:hgv`, `:bus` etc. não se aplicam ao carro
        let Some(kind) = get("restriction").or_else(|| get("restriction:motorcar")) else { return };
        // exceção que inclui carros (ex.: except=motorcar) faz a restrição não valer para o tráfego simulado
        if get("except").map_or(false, |e| e.split(';').any(|x| matches!(x.trim(), "motorcar" | "motor_vehicle" | "vehicle"))) {
            return;
        }
        let from = members.iter().find(|m| m.0 == 1 && m.2 == "from").map(|m| m.1);
        let to = members.iter().find(|m| m.0 == 1 && m.2 == "to").map(|m| m.1);
        let via = members.iter().find(|m| m.0 == 0 && m.2 == "via").map(|m| m.1);
        if let (Some(from), Some(to), Some(via)) = (from, to, via) {
            self.restrictions.push(RawRestriction { kind: kind.to_string(), from, via, to });
        }
    }
}

/// Nível vertical da via: `layer`, ou ±1 implícito para ponte/túnel sem `layer`.
pub fn way_level(tags: &[(String, String)]) -> i32 {
    let get = |k: &str| tags.iter().find(|t| t.0 == k).map(|t| t.1.as_str());
    if let Some(l) = get("layer").and_then(|v| v.trim().parse::<f64>().ok()) {
        return l.round() as i32;
    }
    match (get("bridge"), get("tunnel")) {
        (Some(b), _) if b != "no" => 1,
        (_, Some(t)) if t != "no" => -1,
        _ => 0,
    }
}

fn keep_key(k: &str) -> bool {
    matches!(k, "highway" | "name" | "oneway" | "junction" | "layer" | "bridge" | "tunnel" | "maxspeed" | "area" | "lanes" | "busway" | "psv" | "bus" | "motorcar" | "motor_vehicle" | "access")
        || matches!(k, "motorcar:conditional" | "motor_vehicle:conditional" | "access:conditional")
        || k.starts_with("lanes:")
        || k.starts_with("turn:lanes")
        || k.starts_with("bus:lanes")
        || k.starts_with("psv:lanes")
        || k.starts_with("busway:")
}

pub fn parse(data: &[u8]) -> Result<RawOsm, String> {
    let head = data.iter().copied().skip_while(|b| b.is_ascii_whitespace() || *b == 0xEF || *b == 0xBB || *b == 0xBF).next();
    let mut osm = if head == Some(b'<') { parse_xml(data)? } else { parse_pbf(data)? };
    osm.finish();
    Ok(osm)
}

// ---------------------------------------------------------------- PBF

fn parse_pbf(data: &[u8]) -> Result<RawOsm, String> {
    use osmpbf::{Element, ElementReader, RelMemberType};
    let reader = ElementReader::new(std::io::Cursor::new(data));
    let mut osm = RawOsm::default();
    reader
        .for_each(|el| match el {
            Element::Node(n) => {
                osm.nodes.push((n.id(), n.decimicro_lat(), n.decimicro_lon()));
                osm.node_tags(n.id(), n.tags());
            }
            Element::DenseNode(n) => {
                osm.nodes.push((n.id(), n.decimicro_lat(), n.decimicro_lon()));
                osm.node_tags(n.id(), n.tags());
            }
            Element::Way(w) => {
                let tags: Vec<(String, String)> =
                    w.tags().filter(|(k, _)| keep_key(k)).map(|(k, v)| (k.to_string(), v.to_string())).collect();
                if tags.iter().any(|t| t.0 == "highway") {
                    osm.ways.push(RawWay { id: w.id(), refs: w.refs().collect(), tags });
                }
            }
            Element::Relation(r) => {
                if r.tags().any(|(k, v)| k == "type" && v == "restriction") {
                    let tags: Vec<(String, String)> = r.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                    let members: Vec<(u8, i64, String)> = r
                        .members()
                        .map(|m| {
                            let t = match m.member_type {
                                RelMemberType::Node => 0,
                                RelMemberType::Way => 1,
                                RelMemberType::Relation => 2,
                            };
                            (t, m.member_id, m.role().unwrap_or("").to_string())
                        })
                        .collect();
                    osm.finish_relation(&tags, &members);
                }
            }
        })
        .map_err(|e| format!("PBF inválido: {e}"))?;
    Ok(osm)
}

// ---------------------------------------------------------------- XML

enum Cur {
    None,
    Node { id: i64, lat: i32, lon: i32, tags: Vec<(String, String)> },
    Way(RawWay),
    Rel { tags: Vec<(String, String)>, members: Vec<(u8, i64, String)> },
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == name {
            return a.unescape_value().ok().map(|v| v.into_owned());
        }
    }
    None
}

/// Aceita ids numéricos e no formato `way/123` (exports GeoJSON→OSM).
fn parse_id(s: &str) -> Option<i64> {
    s.rsplit('/').next().and_then(|t| t.parse().ok())
}

fn deg7(s: &str) -> i32 {
    (s.parse::<f64>().unwrap_or(0.0) * 1e7).round() as i32
}

fn parse_xml(data: &[u8]) -> Result<RawOsm, String> {
    let mut reader = Reader::from_reader(data);
    let mut buf = Vec::new();
    let mut osm = RawOsm::default();
    let mut cur = Cur::None;

    fn close(osm: &mut RawOsm, cur: Cur) {
        match cur {
            Cur::Node { id, lat, lon, tags } => {
                osm.nodes.push((id, lat, lon));
                osm.node_tags(id, tags.iter().map(|t| (t.0.as_str(), t.1.as_str())));
            }
            Cur::Way(w) => {
                if w.tags.iter().any(|t| t.0 == "highway") {
                    osm.ways.push(w);
                }
            }
            Cur::Rel { tags, members } => osm.finish_relation(&tags, &members),
            Cur::None => {}
        }
    }

    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|e| format!("XML inválido: {e}"))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                match e.name().as_ref() {
                    b"node" => {
                        let id = attr(e, b"id").and_then(|s| parse_id(&s)).unwrap_or(0);
                        let lat = deg7(&attr(e, b"lat").unwrap_or_default());
                        let lon = deg7(&attr(e, b"lon").unwrap_or_default());
                        let n = Cur::Node { id, lat, lon, tags: Vec::new() };
                        if empty {
                            close(&mut osm, n);
                        } else {
                            cur = n;
                        }
                    }
                    b"way" => {
                        let id = attr(e, b"id").and_then(|s| parse_id(&s)).unwrap_or(0);
                        if !empty {
                            cur = Cur::Way(RawWay { id, refs: Vec::new(), tags: Vec::new() });
                        }
                    }
                    b"relation" => {
                        if !empty {
                            cur = Cur::Rel { tags: Vec::new(), members: Vec::new() };
                        }
                    }
                    b"nd" => {
                        if let Cur::Way(w) = &mut cur {
                            if let Some(r) = attr(e, b"ref").and_then(|s| parse_id(&s)) {
                                w.refs.push(r);
                            }
                        }
                    }
                    b"member" => {
                        if let Cur::Rel { members, .. } = &mut cur {
                            let t = match attr(e, b"type").as_deref() {
                                Some("node") => 0,
                                Some("way") => 1,
                                _ => 2,
                            };
                            let r = attr(e, b"ref").and_then(|s| parse_id(&s)).unwrap_or(0);
                            members.push((t, r, attr(e, b"role").unwrap_or_default()));
                        }
                    }
                    b"tag" => {
                        let (Some(k), Some(v)) = (attr(e, b"k"), attr(e, b"v")) else { continue };
                        match &mut cur {
                            Cur::Node { tags, .. } => {
                                if k == "highway" || k == "crossing" {
                                    tags.push((k, v));
                                }
                            }
                            Cur::Way(w) => {
                                if keep_key(&k) {
                                    w.tags.push((k, v));
                                }
                            }
                            Cur::Rel { tags, .. } => tags.push((k, v)),
                            Cur::None => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                if matches!(e.name().as_ref(), b"node" | b"way" | b"relation") {
                    let c = std::mem::replace(&mut cur, Cur::None);
                    close(&mut osm, c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(osm)
}
