//! Grafo viário: nós, segmentos (ways OSM quebradas nos cruzamentos), faixas.

use crate::geo::*;
use crate::osm::RawOsm;
use std::collections::HashMap;

pub const HW_MOTORWAY: u8 = 0;
pub const HW_TRUNK: u8 = 1;
pub const HW_PRIMARY: u8 = 2;
pub const HW_SECONDARY: u8 = 3;
pub const HW_TERTIARY: u8 = 4;
pub const HW_UNCLASSIFIED: u8 = 5;
pub const HW_RESIDENTIAL: u8 = 6;
pub const HW_LIVING: u8 = 7;
pub const HW_SERVICE: u8 = 8;
pub const HW_BUSWAY: u8 = 9;

pub const NO_NAME: u32 = u32::MAX;

/// Bits de manobra (máscara) e códigos de tipo de conexão.
pub const K_LEFT: u8 = 0;
pub const K_STRAIGHT: u8 = 1;
pub const K_RIGHT: u8 = 2;
pub const K_UTURN: u8 = 3;

fn parse_highway(v: &str) -> Option<(u8, bool)> {
    let (base, link) = match v.strip_suffix("_link") {
        Some(b) => (b, true),
        None => (v, false),
    };
    let hw = match base {
        "motorway" => HW_MOTORWAY,
        "trunk" => HW_TRUNK,
        "primary" => HW_PRIMARY,
        "secondary" => HW_SECONDARY,
        "tertiary" => HW_TERTIARY,
        "unclassified" => HW_UNCLASSIFIED,
        "residential" => HW_RESIDENTIAL,
        "living_street" => HW_LIVING,
        "service" => HW_SERVICE,
        "busway" => HW_BUSWAY,
        _ => return None,
    };
    Some((hw, link))
}

pub struct Node {
    pub pos: V2,
    pub osm_id: i64,
    pub signal: bool,
    pub crossing: bool,
    /// ids de ponta (seg*2 + 0 = início, seg*2 + 1 = fim), em ordem horária na tela.
    pub ends: Vec<u32>,
}

pub struct Seg {
    pub way_id: i64,
    pub from: u32,
    pub to: u32,
    pub poly: Poly,
    pub hw: u8,
    pub link: bool,
    pub oneway: bool,
    pub roundabout: bool,
    pub lf: u8,
    pub lb: u8,
    pub w: f64,
    pub name: u32,
    pub maxspeed: u16,
    pub bus_f: Vec<bool>,
    pub bus_b: Vec<bool>,
    pub turn_f: Vec<u8>,
    pub turn_b: Vec<u8>,
    pub cut_from: f64,
    pub cut_to: f64,
    pub lane_base: u32,
}

impl Seg {
    pub fn p0(&self) -> f64 {
        if self.oneway {
            -(self.lf as f64) * self.w / 2.0
        } else {
            0.0
        }
    }
    pub fn left_ext(&self) -> f64 {
        if self.oneway {
            self.lf as f64 * self.w / 2.0
        } else {
            self.lb as f64 * self.w
        }
    }
    pub fn right_ext(&self) -> f64 {
        if self.oneway {
            self.lf as f64 * self.w / 2.0
        } else {
            self.lf as f64 * self.w
        }
    }
    /// Deslocamento lateral (direita da direção "from→to" positiva) do centro da faixa.
    pub fn lane_off(&self, dir: u8, idx: u8) -> f64 {
        if dir == 0 {
            self.p0() + (idx as f64 + 0.5) * self.w
        } else {
            -(idx as f64 + 0.5) * self.w
        }
    }
}

pub struct Lane {
    pub seg: u32,
    /// 0 = acompanha from→to, 1 = contrário.
    pub dir: u8,
    /// 0 = faixa mais à esquerda no sentido de deslocamento.
    pub idx: u8,
    pub from: u32,
    pub to: u32,
    pub bus: bool,
    pub tmask: u8,
    pub start: [f32; 2],
    pub sdir: [f32; 2],
    pub end: [f32; 2],
    pub edir: [f32; 2],
}

pub struct Conn {
    pub node: u32,
    pub from_lane: u32,
    pub to_lane: u32,
    pub kind: u8,
    pub allowed: bool,
    pub curve: [f32; 8],
}

pub struct Phase {
    pub green: f32,
    pub yellow: f32,
    pub all_red: f32,
    pub ends: Vec<u32>,
}

pub struct SignalEnd {
    pub end: u32,
    pub pos: [f32; 2],
    pub angle: f32,
}

pub struct Signal {
    pub node: u32,
    pub phases: Vec<Phase>,
    pub ends: Vec<SignalEnd>,
}

#[derive(Default)]
pub struct Net {
    pub nodes: Vec<Node>,
    pub segs: Vec<Seg>,
    pub lanes: Vec<Lane>,
    pub conns: Vec<Conn>,
    /// CSR: conexões do nó n em conns[conn_off[n]..conn_off[n+1]].
    pub conn_off: Vec<u32>,
    pub signals: Vec<Signal>,
    pub names: Vec<String>,
    pub bounds: [f64; 4],
}

struct Tags<'a>(&'a [(String, String)]);
impl<'a> Tags<'a> {
    fn get(&self, k: &str) -> Option<&'a str> {
        self.0.iter().find(|t| t.0 == k).map(|t| t.1.as_str())
    }
    fn int(&self, k: &str) -> Option<u8> {
        self.get(k).and_then(|v| v.trim().parse::<u8>().ok()).filter(|&n| n > 0 && n <= 12)
    }
}

fn parse_turn_mask(tok: &str) -> u8 {
    let mut m = 0u8;
    for p in tok.split(';') {
        let p = p.trim();
        if p.contains("left") {
            m |= 1 << K_LEFT;
        } else if p.contains("right") {
            m |= 1 << K_RIGHT;
        } else if p == "through" {
            m |= 1 << K_STRAIGHT;
        } else if p == "reverse" {
            m |= 1 << K_UTURN;
        }
    }
    m
}

fn parse_lane_list<T>(v: Option<&str>, n: usize, f: impl Fn(&str) -> T, default: T) -> Vec<T>
where
    T: Clone,
{
    let mut out = vec![default; n];
    if let Some(v) = v {
        for (i, tok) in v.split('|').enumerate() {
            if i < n {
                out[i] = f(tok);
            }
        }
    }
    out
}

struct Attrs {
    oneway: i8,
    roundabout: bool,
    lf: u8,
    lb: u8,
    w: f64,
    maxspeed: u16,
    bus_f: Vec<bool>,
    bus_b: Vec<bool>,
    turn_f: Vec<u8>,
    turn_b: Vec<u8>,
}

fn way_attrs(t: &Tags, hw: u8, link: bool) -> Attrs {
    let roundabout = matches!(t.get("junction"), Some("roundabout") | Some("circular"));
    let oneway = match t.get("oneway") {
        Some("yes") | Some("true") | Some("1") => 1,
        Some("-1") | Some("reverse") => -1,
        Some("no") | Some("false") | Some("0") => 0,
        _ => {
            if roundabout || hw == HW_MOTORWAY {
                1
            } else {
                0
            }
        }
    };
    let multi = hw <= HW_TRUNK;
    let def = if multi { 2 } else { 1 };
    let total = t.int("lanes");
    let (lf, lb) = if oneway != 0 {
        (t.int("lanes:forward").or(total).unwrap_or(def), 0)
    } else {
        let f = t.int("lanes:forward");
        let b = t.int("lanes:backward");
        match (f, b, total) {
            (Some(f), Some(b), _) => (f, b),
            (Some(f), None, Some(n)) => (f, n.saturating_sub(f).max(1)),
            (None, Some(b), Some(n)) => (n.saturating_sub(b).max(1), b),
            (_, _, Some(1)) => (1, 1),
            (_, _, Some(n)) => (n - n / 2, n / 2),
            (Some(f), None, None) => (f, f),
            (None, Some(b), None) => (b, b),
            _ => (def, def),
        }
    };
    let w = match hw {
        HW_MOTORWAY | HW_TRUNK => 3.6,
        HW_PRIMARY | HW_SECONDARY | HW_BUSWAY => 3.4,
        HW_SERVICE => 2.7,
        _ => 3.1,
    };
    let maxspeed = t
        .get("maxspeed")
        .and_then(|v| v.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(0);

    let nf = lf as usize;
    let nb = lb as usize;
    let one = oneway != 0;
    let gl = |k1: &str, k2: &str| -> Option<String> {
        if one {
            t.get(k1).or(t.get(k2)).map(|s| s.to_string())
        } else {
            t.get(k2).map(|s| s.to_string())
        }
    };
    let gb = |k: &str| -> Option<String> { t.get(k).map(|s| s.to_string()) };

    let mut bus_f = parse_lane_list(gl("bus:lanes", "bus:lanes:forward").as_deref(), nf, |s| s.trim() == "designated", false);
    let mut bus_b = parse_lane_list(gb("bus:lanes:backward").as_deref(), nb, |s| s.trim() == "designated", false);
    let psv_f = parse_lane_list(gl("psv:lanes", "psv:lanes:forward").as_deref(), nf, |s| s.trim() == "designated", false);
    for i in 0..nf {
        bus_f[i] |= psv_f[i];
    }
    let right_bus = matches!(t.get("busway:right"), Some("lane")) || matches!(t.get("busway"), Some("lane"));
    let left_bus = matches!(t.get("busway:left"), Some("lane")) || matches!(t.get("busway"), Some("lane"));
    if right_bus && nf > 0 {
        bus_f[nf - 1] = true;
    }
    if left_bus {
        if one {
            bus_f[0] = true;
        } else if nb > 0 {
            bus_b[nb - 1] = true;
        }
    }
    if hw == HW_BUSWAY {
        bus_f.iter_mut().for_each(|b| *b = true);
        bus_b.iter_mut().for_each(|b| *b = true);
    }

    let tl_f = gl("turn:lanes", "turn:lanes:forward");
    let turn_f = parse_lane_list(tl_f.as_deref(), nf, parse_turn_mask, 0);
    let turn_b = parse_lane_list(gb("turn:lanes:backward").as_deref(), nb, parse_turn_mask, 0);
    let _ = link;
    Attrs { oneway, roundabout, lf, lb, w, maxspeed, bus_f, bus_b, turn_f, turn_b }
}

const M_PER_DEG: f64 = 111_319.490_793_273_57;

impl Net {
    pub fn build(raw: &RawOsm) -> Result<Net, String> {
        // 1. vias aceitas, com coordenadas resolvidas
        struct W<'a> {
            raw: &'a crate::osm::RawWay,
            hw: u8,
            link: bool,
            pts: Vec<(i64, i32, i32)>,
        }
        let mut ways: Vec<W> = Vec::new();
        for w in &raw.ways {
            let t = Tags(&w.tags);
            let Some((hw, link)) = t.get("highway").and_then(parse_highway) else { continue };
            if t.get("area") == Some("yes") {
                continue;
            }
            if hw == HW_SERVICE && matches!(t.get("service"), Some("driveway") | Some("parking_aisle")) {
                continue;
            }
            if matches!(t.get("access"), Some("no") | Some("private")) && hw == HW_SERVICE {
                continue;
            }
            let pts: Vec<(i64, i32, i32)> =
                w.refs.iter().filter_map(|&r| raw.coord(r).map(|(la, lo)| (r, la, lo))).collect();
            if pts.len() >= 2 {
                ways.push(W { raw: w, hw, link, pts });
            }
        }
        if ways.is_empty() {
            return Err("Nenhuma via veicular encontrada no arquivo".into());
        }

        // 2. origem da projeção
        let (mut la0, mut la1, mut lo0, mut lo1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for w in &ways {
            for p in &w.pts {
                la0 = la0.min(p.1);
                la1 = la1.max(p.1);
                lo0 = lo0.min(p.2);
                lo1 = lo1.max(p.2);
            }
        }
        let lat_c = (la0 as f64 + la1 as f64) / 2.0 * 1e-7;
        let lon_c = (lo0 as f64 + lo1 as f64) / 2.0 * 1e-7;
        let kx = M_PER_DEG * lat_c.to_radians().cos();
        let proj = |la: i32, lo: i32| -> V2 { [(lo as f64 * 1e-7 - lon_c) * kx, -(la as f64 * 1e-7 - lat_c) * M_PER_DEG] };

        // 3. nós de quebra: usados 2+ vezes, pontas, semáforos, travessias
        let mut all: Vec<i64> = Vec::new();
        for w in &ways {
            all.extend(w.pts.iter().map(|p| p.0));
        }
        all.sort_unstable();
        let mut split: Vec<i64> = Vec::new();
        let mut i = 0;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() && all[j] == all[i] {
                j += 1;
            }
            let id = all[i];
            if j - i >= 2 || raw.signals.contains(&id) || raw.crossings.contains(&id) {
                split.push(id);
            }
            i = j;
        }
        for w in &ways {
            split.push(w.pts[0].0);
            split.push(w.pts[w.pts.len() - 1].0);
        }
        split.sort_unstable();
        split.dedup();

        let mut net = Net::default();
        let mut node_pos: Vec<Option<V2>> = vec![None; split.len()];
        let mut names: HashMap<String, u32> = HashMap::new();

        // 4. segmentos
        for w in &ways {
            let t = Tags(&w.raw.tags);
            let a = way_attrs(&t, w.hw, w.link);
            let name = match t.get("name") {
                Some(n) => *names.entry(n.to_string()).or_insert_with(|| {
                    net.names.push(n.to_string());
                    (net.names.len() - 1) as u32
                }),
                None => NO_NAME,
            };
            let mut pts: Vec<(i64, V2)> = w.pts.iter().map(|p| (p.0, proj(p.1, p.2))).collect();
            if a.oneway < 0 {
                pts.reverse();
            }
            let last = pts.len() - 1;
            let is_split = |k: usize, id: i64| k == 0 || k == last || split.binary_search(&id).is_ok();
            let mut start = 0usize;
            for k in 1..pts.len() {
                if !is_split(k, pts[k].0) {
                    continue;
                }
                let poly = Poly::new(pts[start..=k].iter().map(|p| p.1).collect());
                let from = split.binary_search(&pts[start].0).unwrap();
                let to = split.binary_search(&pts[k].0).unwrap();
                if node_pos[from].is_none() {
                    node_pos[from] = Some(pts[start].1);
                }
                if node_pos[to].is_none() {
                    node_pos[to] = Some(pts[k].1);
                }
                start = k;
                if !poly.ok() {
                    continue;
                }
                net.segs.push(Seg {
                    way_id: w.raw.id,
                    from: from as u32,
                    to: to as u32,
                    poly,
                    hw: w.hw,
                    link: w.link,
                    oneway: a.oneway != 0,
                    roundabout: a.roundabout,
                    lf: a.lf,
                    lb: if a.oneway != 0 { 0 } else { a.lb },
                    w: a.w,
                    name,
                    maxspeed: a.maxspeed,
                    bus_f: a.bus_f.clone(),
                    bus_b: a.bus_b.clone(),
                    turn_f: a.turn_f.clone(),
                    turn_b: a.turn_b.clone(),
                    cut_from: 0.0,
                    cut_to: 0.0,
                    lane_base: 0,
                });
            }
        }

        // 5. nós (compactando os sem segmento)
        let mut remap = vec![u32::MAX; split.len()];
        for s in &net.segs {
            remap[s.from as usize] = 0;
            remap[s.to as usize] = 0;
        }
        for (i, id) in split.iter().enumerate() {
            if remap[i] == u32::MAX {
                continue;
            }
            remap[i] = net.nodes.len() as u32;
            net.nodes.push(Node {
                pos: node_pos[i].unwrap_or([0.0, 0.0]),
                osm_id: *id,
                signal: raw.signals.contains(id),
                crossing: raw.crossings.contains(id),
                ends: Vec::new(),
            });
        }
        for (si, s) in net.segs.iter_mut().enumerate() {
            s.from = remap[s.from as usize];
            s.to = remap[s.to as usize];
            net.nodes[s.from as usize].ends.push(si as u32 * 2);
            net.nodes[s.to as usize].ends.push(si as u32 * 2 + 1);
        }

        net.sort_ends();
        net.compute_cuts();
        net.build_lanes();
        net.bounds = net.compute_bounds();
        Ok(net)
    }

    fn compute_bounds(&self) -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for n in &self.nodes {
            b[0] = b[0].min(n.pos[0]);
            b[1] = b[1].min(n.pos[1]);
            b[2] = b[2].max(n.pos[0]);
            b[3] = b[3].max(n.pos[1]);
        }
        b
    }

    /// Direção unitária saindo do nó pela ponta `e`.
    pub fn outward(&self, e: u32) -> V2 {
        let s = &self.segs[(e >> 1) as usize];
        let l = s.poly.length();
        let d = (l * 0.4).min(6.0);
        if e & 1 == 0 {
            norm(sub(s.poly.at(d).0, s.poly.pts[0]))
        } else {
            norm(sub(s.poly.at(l - d).0, *s.poly.pts.last().unwrap()))
        }
    }

    /// Extensão lateral do lado horário (direita de quem sai do nó) e anti-horário.
    pub fn ext_cw_ccw(&self, e: u32) -> (f64, f64) {
        let s = &self.segs[(e >> 1) as usize];
        if e & 1 == 0 {
            (s.right_ext(), s.left_ext())
        } else {
            (s.left_ext(), s.right_ext())
        }
    }

    fn sort_ends(&mut self) {
        let ang: Vec<f64> = (0..self.segs.len() as u32 * 2)
            .map(|e| {
                let u = self.outward(e);
                u[1].atan2(u[0])
            })
            .collect();
        for n in &mut self.nodes {
            n.ends.sort_by(|a, b| ang[*a as usize].partial_cmp(&ang[*b as usize]).unwrap());
        }
    }

    fn compute_cuts(&mut self) {
        let mut cut = vec![0.0f64; self.segs.len() * 2];
        for node in &self.nodes {
            let n = node.ends.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let ei = node.ends[i];
                let ej = node.ends[(i + 1) % n];
                let ui = self.outward(ei);
                let uj = self.outward(ej);
                let a = self.ext_cw_ccw(ei).0;
                let b = self.ext_cw_ccw(ej).1;
                let ni = right(ui);
                let nj = right(uj);
                let big_a = mul(ni, a);
                let big_b = mul(nj, -b);
                let r = sub(big_b, big_a);
                let det = -cross(ui, uj);
                if det.abs() < 1e-3 {
                    continue;
                }
                let t = cross(uj, r) / det;
                let t2 = cross(ui, r) / det;
                cut[ei as usize] = cut[ei as usize].max(t.max(0.0));
                cut[ej as usize] = cut[ej as usize].max(t2.max(0.0));
            }
        }
        for (si, s) in self.segs.iter_mut().enumerate() {
            let l = s.poly.length();
            let cap = 22.0f64.min(l * 0.45);
            let mut a = cut[si * 2].min(cap);
            let mut b = cut[si * 2 + 1].min(cap);
            if a + b > l * 0.8 {
                let k = l * 0.8 / (a + b);
                a *= k;
                b *= k;
            }
            s.cut_from = a;
            s.cut_to = b;
        }
    }

    fn build_lanes(&mut self) {
        let mut lanes = Vec::new();
        for (si, s) in self.segs.iter_mut().enumerate() {
            s.lane_base = lanes.len() as u32;
            let l = s.poly.length();
            let s_start = s.cut_from;
            let s_end = l - s.cut_to;
            for dir in 0..2u8 {
                let n = if dir == 0 { s.lf } else { s.lb };
                for idx in 0..n {
                    let off = s.lane_off(dir, idx);
                    let at = |sv: f64, sign: f64| -> ([f32; 2], [f32; 2]) {
                        let (p, t) = s.poly.at(sv);
                        let q = add(p, mul(right(t), off));
                        let d = mul(t, sign);
                        ([q[0] as f32, q[1] as f32], [d[0] as f32, d[1] as f32])
                    };
                    let sign = if dir == 0 { 1.0 } else { -1.0 };
                    let (start, sdir, end, edir, from, to);
                    if dir == 0 {
                        (start, sdir) = at(s_start, sign);
                        (end, edir) = at(s_end, sign);
                        from = s.from;
                        to = s.to;
                    } else {
                        (start, sdir) = at(s_end, sign);
                        (end, edir) = at(s_start, sign);
                        from = s.to;
                        to = s.from;
                    }
                    let (bus, tmask) = if dir == 0 {
                        (s.bus_f[idx as usize], s.turn_f[idx as usize])
                    } else {
                        (s.bus_b[idx as usize], s.turn_b[idx as usize])
                    };
                    lanes.push(Lane { seg: si as u32, dir, idx, from, to, bus, tmask, start, sdir, end, edir });
                }
            }
        }
        self.lanes = lanes;
    }

    /// Faixas que chegam (in) e que saem (out) do nó pela ponta `e`, da esquerda para a direita.
    pub fn approach_lanes(&self, e: u32) -> (Vec<u32>, Vec<u32>) {
        let s = &self.segs[(e >> 1) as usize];
        let fwd: Vec<u32> = (0..s.lf as u32).map(|i| s.lane_base + i).collect();
        let bwd: Vec<u32> = (0..s.lb as u32).map(|i| s.lane_base + s.lf as u32 + i).collect();
        if e & 1 == 1 {
            (fwd, bwd)
        } else {
            (bwd, fwd)
        }
    }
}

impl Net {
    /// Sanidade usada nos testes: todos os índices de faixa/segmento são válidos.
    pub fn meshes_ok(&self) -> bool {
        self.lanes.iter().all(|l| (l.seg as usize) < self.segs.len())
            && self.segs.iter().all(|s| (s.from as usize) < self.nodes.len() && (s.to as usize) < self.nodes.len())
    }
}
