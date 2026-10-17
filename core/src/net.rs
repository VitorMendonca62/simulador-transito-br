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

pub const MAX_LANES: u8 = 8;

/// Identifica um trecho de forma estável entre reconstruções: (way OSM, nó OSM inicial, nó OSM final)
/// na orientação original (antes de qualquer inversão editada).
pub type SegKey = [i64; 3];

/// Edição de um trecho: valores finais, já na orientação resultante (`reverse` aplica a inversão).
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SegEdit {
    pub way: i64,
    pub from: i64,
    pub to: i64,
    pub oneway: bool,
    pub lf: u8,
    pub lb: u8,
    /// largura da faixa (m); 0 mantém a padrão da classe
    pub w: f64,
    pub bus_f: Vec<bool>,
    pub bus_b: Vec<bool>,
    pub reverse: bool,
}

impl SegEdit {
    pub fn key(&self) -> SegKey {
        [self.way, self.from, self.to]
    }
}

/// Fase editada: durações (s) e aproximações (chaves de `approach_key`) que recebem verde.
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PhaseEdit {
    pub green: f32,
    pub yellow: f32,
    pub all_red: f32,
    pub approaches: Vec<String>,
}

/// Configuração de semáforo de um nó (id OSM): liga/desliga e, opcionalmente, as fases.
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SigEdit {
    pub node: i64,
    pub enabled: bool,
    /// defasagem do ciclo (s), p.ex. para onda verde
    pub offset: f64,
    pub phases: Option<Vec<PhaseEdit>>,
}

/// Ponto de uma via nova (metros locais). `node` encaixa num nó existente; `way` encaixa no corpo de uma via.
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NewPt {
    pub x: f64,
    pub y: f64,
    pub node: Option<i64>,
    pub way: Option<i64>,
}

/// Via criada pelo usuário (estilo construtor do Cities: Skylines).
#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NewRoad {
    /// id único da via (vira o `way` das chaves de trecho)
    pub id: i64,
    /// classe OSM: residential, tertiary, secondary, primary, trunk, service...
    pub hw: String,
    pub name: String,
    pub oneway: bool,
    pub lf: u8,
    pub lb: u8,
    pub pts: Vec<NewPt>,
}

/// Todas as edições do usuário aplicadas na reconstrução da rede.
#[derive(Default, Clone)]
pub struct Edits {
    pub segs: HashMap<SegKey, SegEdit>,
    pub sigs: HashMap<i64, SigEdit>,
    pub roads: Vec<NewRoad>,
    /// trechos demolidos
    pub removed: std::collections::HashSet<SegKey>,
    /// conversões editadas, agrupadas pelo id OSM do nó: (chave de `conn_key`, estado)
    pub conns: HashMap<i64, Vec<(String, u8)>>,
    /// sinalização de preferência por aproximação: `nóOsm|approach_key` -> nível (1 preferencial, 2 dê a preferência, 3 pare)
    pub prios: HashMap<String, u8>,
}

impl Edits {
    /// Agrupa um mapa chave -> estado (`nóOsm|faixa>faixa`) por nó.
    pub fn set_conns(&mut self, m: HashMap<String, u8>) {
        self.conns.clear();
        for (k, v) in m {
            if let Some(osm) = k.split('|').next().and_then(|s| s.parse::<i64>().ok()) {
                self.conns.entry(osm).or_default().push((k, v));
            }
        }
    }
}

/// Id determinístico de um nó criado pelo editor, a partir da posição (resolução de 10 cm).
fn coord_id(p: V2) -> i64 {
    let xi = ((p[0] * 10.0).round() as i64 + (1 << 23)).clamp(0, (1 << 24) - 1);
    let yi = ((p[1] * 10.0).round() as i64 + (1 << 23)).clamp(0, (1 << 24) - 1);
    -((xi << 24 | yi) + 1)
}

/// Via com geometria projetada (base do OSM ou criada pelo usuário).
struct W<'a> {
    id: i64,
    tags: &'a [(String, String)],
    hw: u8,
    link: bool,
    pts: Vec<(i64, V2)>,
    attrs: std::borrow::Cow<'a, Attrs>,
}

/// Via do OSM já filtrada, projetada e com atributos interpretados: independe das edições, então é
/// calculada uma vez e reaproveitada em toda reconstrução.
pub struct BaseWay {
    pub raw_idx: usize,
    pub hw: u8,
    pub link: bool,
    pub pts: Vec<(i64, V2)>,
    pub attrs: Attrs,
    /// por vértice: é nó de quebra segundo só o OSM (válido enquanto não há vias novas)
    pub split_flags: Vec<bool>,
}

pub struct BaseCache {
    pub ways: Vec<BaseWay>,
    /// nós de quebra só pelo OSM (usados 2+ vezes, pontas, semáforos, travessias), ordenados
    pub split: Vec<i64>,
    /// Origem da projeção: [lat_centro, lon_centro, metros por grau de longitude].
    pub geo: [f64; 3],
}

impl BaseCache {
    pub fn new(raw: &RawOsm) -> Result<BaseCache, String> {
        // 1. vias aceitas, com coordenadas resolvidas
        let mut base: Vec<(usize, u8, bool, Vec<(i64, i32, i32)>)> = Vec::new();
        for (i, w) in raw.ways.iter().enumerate() {
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
            let pts: Vec<(i64, i32, i32)> = w.refs.iter().filter_map(|&r| raw.coord(r).map(|(la, lo)| (r, la, lo))).collect();
            if pts.len() >= 2 {
                base.push((i, hw, link, pts));
            }
        }
        if base.is_empty() {
            return Err("Nenhuma via veicular encontrada no arquivo".into());
        }
        // 2. origem da projeção (só das vias do OSM: estável diante de edições)
        let (mut la0, mut la1, mut lo0, mut lo1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for w in &base {
            for p in &w.3 {
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
        let mut ways: Vec<BaseWay> = base
            .into_iter()
            .map(|(raw_idx, hw, link, pts)| {
                let attrs = way_attrs(&Tags(&raw.ways[raw_idx].tags), hw, link);
                BaseWay { raw_idx, hw, link, pts: pts.iter().map(|p| (p.0, proj(p.1, p.2))).collect(), attrs, split_flags: Vec::new() }
            })
            .collect();
        let split = split_nodes(ways.iter().map(|w| &w.pts[..]), raw, &Default::default());
        for w in ways.iter_mut() {
            let last = w.pts.len() - 1;
            w.split_flags = w.pts.iter().enumerate().map(|(k, p)| k == 0 || k == last || split.binary_search(&p.0).is_ok()).collect();
        }
        Ok(BaseCache { ways, split, geo: [lat_c, lon_c, kx] })
    }
}

/// Nós de quebra: usados 2+ vezes, pontas de via, semáforos e travessias.
fn split_nodes<'a>(ways: impl Iterator<Item = &'a [(i64, V2)]> + Clone, raw: &RawOsm, forced: &std::collections::HashSet<i64>) -> Vec<i64> {
    let mut all: Vec<i64> = Vec::new();
    for w in ways.clone() {
        all.extend(w.iter().map(|p| p.0));
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
        if j - i >= 2 || raw.signals.contains(&id) || raw.crossings.contains(&id) || forced.contains(&id) {
            split.push(id);
        }
        i = j;
    }
    for w in ways {
        split.push(w[0].0);
        split.push(w[w.len() - 1].0);
    }
    split.sort_unstable();
    split.dedup();
    split
}

fn seg_cross(a: V2, b: V2, c: V2, d: V2) -> Option<(f64, f64)> {
    let r = sub(b, a);
    let s = sub(d, c);
    let den = cross(r, s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = cross(sub(c, a), s) / den;
    let u = cross(sub(c, a), r) / den;
    let e = 1e-6;
    (t > e && t < 1.0 - e && u > e && u < 1.0 - e).then_some((t, u))
}

/// Ponto mais próximo de `p` na polilinha de `w`: (aresta, t, posição, distância).
fn nearest_on(w: &W, p: V2) -> Option<(usize, f64, V2, f64)> {
    let mut best: Option<(usize, f64, V2, f64)> = None;
    for i in 0..w.pts.len().saturating_sub(1) {
        let (a, b) = (w.pts[i].1, w.pts[i + 1].1);
        let v = sub(b, a);
        let l2 = dot(v, v).max(1e-12);
        let t = (dot(sub(p, a), v) / l2).clamp(0.0, 1.0);
        let q = add(a, mul(v, t));
        let d = len(sub(p, q));
        if best.map_or(true, |x| d < x.3) {
            best = Some((i, t, q, d));
        }
    }
    best
}

/// Insere (ou reaproveita, se estiver a menos de 1 m de um vértice) um nó em `w` perto de `p`.
fn insert_on(w: &mut W, p: V2) -> Option<(i64, V2)> {
    let (i, t, q, _) = nearest_on(w, p)?;
    for v in [&w.pts[i], &w.pts[i + 1]] {
        if len(sub(v.1, q)) < 1.0 {
            return Some(*v);
        }
    }
    let _ = t;
    let id = coord_id(q);
    w.pts.insert(i + 1, (id, q));
    Some((id, q))
}

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
    pub key: SegKey,
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

/// Estado de uma conexão: ligada, proibida por restrição OSM (mostrada em vermelho) ou ausente (candidata
/// que o usuário pode ligar arrastando, como no TM:PE).
pub const CONN_BLOCKED: u8 = 0;
pub const CONN_ALLOWED: u8 = 1;
pub const CONN_ABSENT: u8 = 2;

pub struct Conn {
    pub node: u32,
    pub from_lane: u32,
    pub to_lane: u32,
    pub kind: u8,
    pub state: u8,
    /// estado antes das edições do usuário (padrão + restrições OSM)
    pub base: u8,
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
    pub key: String,
    pub pos: [f32; 2],
    pub angle: f32,
}

pub struct Signal {
    pub node: u32,
    pub offset: f32,
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
    /// Origem da projeção (ver `BaseCache::geo`): x = (lon − lon_c)·kx, y = −(lat − lat_c)·M_PER_DEG.
    pub geo: [f64; 3],
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

#[derive(Clone)]
pub struct Attrs {
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

pub fn way_attrs(t: &Tags, hw: u8, link: bool) -> Attrs {
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

/// Cronometragem de estágios (só nativo, para o teste de desempenho; no wasm não faz nada).
#[cfg(not(target_arch = "wasm32"))]
pub fn stage_log() -> &'static std::sync::Mutex<Vec<(&'static str, f64)>> {
    static LOG: std::sync::OnceLock<std::sync::Mutex<Vec<(&'static str, f64)>>> = std::sync::OnceLock::new();
    LOG.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}
macro_rules! stage {
    ($t:ident, $name:expr) => {
        #[cfg(not(target_arch = "wasm32"))]
        {
            stage_log().lock().unwrap().push(($name, $t.elapsed().as_secs_f64() * 1e3));
            $t = std::time::Instant::now();
        }
    };
}

const M_PER_DEG: f64 = 111_319.490_793_273_57;

impl Net {
    pub fn build(raw: &RawOsm, edits: &Edits, base: &BaseCache) -> Result<Net, String> {
        #[cfg(not(target_arch = "wasm32"))]
        let mut _t = std::time::Instant::now();
        let mut ways: Vec<W> = base
            .ways
            .iter()
            .map(|b| {
                let w = &raw.ways[b.raw_idx];
                W { id: w.id, tags: &w.tags, hw: b.hw, link: b.link, pts: b.pts.clone(), attrs: std::borrow::Cow::Borrowed(&b.attrs) }
            })
            .collect();
        let base_len = ways.len();
        stage!(_t, "1+2 vias do OSM (cache)");

        // 2b. vias criadas pelo usuário: encaixes, divisões e cruzamentos automáticos
        let road_tags: Vec<Vec<(String, String)>> = edits.roads.iter().map(road_tags).collect();
        // semáforos "em trecho": pontos (hw = "mark") onde se força um nó no corpo de uma via existente
        let mut marks: std::collections::HashSet<i64> = std::collections::HashSet::new();
        if !edits.roads.is_empty() {
            let mut known: HashMap<i64, V2> = HashMap::new();
            for w in &ways {
                for p in &w.pts {
                    known.insert(p.0, p.1);
                }
            }
            let by_id = |ways: &Vec<W>, id: i64| ways.iter().position(|w| w.id == id);
            for (ri, r) in edits.roads.iter().enumerate() {
                let Some((hw, link)) = parse_highway(&r.hw) else { continue };
                let mut pts: Vec<(i64, V2)> = Vec::new();
                for p in &r.pts {
                    let pos = [p.x, p.y];
                    let mut got: Option<(i64, V2)> = None;
                    if let Some(n) = p.node {
                        // o id é a autoridade (também vale para nós criados por cruzamentos automáticos)
                        got = Some((n, known.get(&n).copied().unwrap_or(pos)));
                    }
                    if got.is_none() {
                        if let Some(wid) = p.way {
                            if let Some(wi) = by_id(&ways, wid) {
                                got = insert_on(&mut ways[wi], pos);
                            }
                        }
                    }
                    let (id, q) = got.unwrap_or_else(|| (coord_id(pos), pos));
                    known.insert(id, q);
                    if pts.last().map_or(true, |l: &(i64, V2)| l.0 != id) {
                        pts.push((id, q));
                    }
                }
                if pts.len() >= 2 {
                    let attrs = way_attrs(&Tags(&road_tags[ri]), hw, link);
                    ways.push(W { id: r.id, tags: &road_tags[ri], hw, link, pts, attrs: std::borrow::Cow::Owned(attrs) });
                }
            }

            // cruzamentos automáticos: onde a via nova atravessa outra, vira interseção
            let bbox = |w: &W| -> [f64; 4] {
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for p in &w.pts {
                    b[0] = b[0].min(p.1[0]);
                    b[1] = b[1].min(p.1[1]);
                    b[2] = b[2].max(p.1[0]);
                    b[3] = b[3].max(p.1[1]);
                }
                b
            };
            for ri in base_len..ways.len() {
                let rb = bbox(&ways[ri]);
                let mut ins_r: Vec<(usize, f64, (i64, V2))> = Vec::new();
                let mut ins_o: Vec<(usize, usize, f64, (i64, V2))> = Vec::new();
                for oi in 0..ri {
                    let ob = bbox(&ways[oi]);
                    if ob[0] > rb[2] || ob[2] < rb[0] || ob[1] > rb[3] || ob[3] < rb[1] {
                        continue;
                    }
                    for e in 0..ways[ri].pts.len() - 1 {
                        let (a, b) = (ways[ri].pts[e].1, ways[ri].pts[e + 1].1);
                        for f in 0..ways[oi].pts.len() - 1 {
                            let (c, d) = (ways[oi].pts[f].1, ways[oi].pts[f + 1].1);
                            if let Some((t, u)) = seg_cross(a, b, c, d) {
                                let q = add(a, mul(sub(b, a), t));
                                // perto de um vértice da outra via: reaproveita-o
                                let near = [ways[oi].pts[f], ways[oi].pts[f + 1]].into_iter().find(|v| len(sub(v.1, q)) < 1.0);
                                if let Some(v) = near {
                                    ins_r.push((e, t, v));
                                } else {
                                    let node = (coord_id(q), q);
                                    ins_r.push((e, t, node));
                                    ins_o.push((oi, f, u, node));
                                }
                            }
                        }
                    }
                }
                ins_r.sort_by(|x, y| (y.0, y.1).partial_cmp(&(x.0, x.1)).unwrap());
                for (e, _, node) in ins_r {
                    if ways[ri].pts[e].0 != node.0 && ways[ri].pts[e + 1].0 != node.0 {
                        ways[ri].pts.insert(e + 1, node);
                    }
                }
                ins_o.sort_by(|x, y| (y.0, y.1, y.2).partial_cmp(&(x.0, x.1, x.2)).unwrap());
                for (oi, f, _, node) in ins_o {
                    if ways[oi].pts[f].0 != node.0 && ways[oi].pts[f + 1].0 != node.0 {
                        ways[oi].pts.insert(f + 1, node);
                    }
                }
            }
        }

        for r in edits.roads.iter().filter(|r| r.hw == "mark") {
            let Some(pt) = r.pts.first() else { continue };
            let pos = [pt.x, pt.y];
            let wi = match pt.way {
                Some(wid) => ways.iter().position(|w| w.id == wid),
                None => None,
            };
            if let Some(wi) = wi {
                // só vale perto da via (o clique pode ter ficado obsoleto depois de uma edição)
                if let Some((_, _, _, d)) = nearest_on(&ways[wi], pos) {
                    if d < 3.0 {
                        if let Some((id, _)) = insert_on(&mut ways[wi], pos) {
                            marks.insert(id);
                        }
                    }
                }
            }
        }
        stage!(_t, "2b vias novas");
        // 3. nós de quebra: sem vias novas, o conjunto do OSM é reaproveitado
        let split: Vec<i64> = if edits.roads.is_empty() { base.split.clone() } else { split_nodes(ways.iter().map(|w| &w.pts[..]), raw, &marks) };
        stage!(_t, "3 nós de quebra (ordenação)");
        let mut net = Net { geo: base.geo, ..Net::default() };
        let mut node_pos: Vec<Option<V2>> = vec![None; split.len()];
        let mut names: HashMap<String, u32> = HashMap::new();

        // 4. segmentos
        let use_flags = edits.roads.is_empty();
        for (wi, w) in ways.iter().enumerate() {
            let t = Tags(w.tags);
            let a: &Attrs = &w.attrs;
            let name = match t.get("name") {
                Some(n) => match names.get(n) {
                    Some(&i) => i,
                    None => {
                        net.names.push(n.to_string());
                        let i = (net.names.len() - 1) as u32;
                        names.insert(n.to_string(), i);
                        i
                    }
                },
                None => NO_NAME,
            };
            let mut pts: Vec<(i64, V2)> = w.pts.clone();
            if a.oneway < 0 {
                pts.reverse();
            }
            let last = pts.len() - 1;
            let flags: Option<Vec<bool>> = if use_flags && wi < base_len {
                let mut f = base.ways[wi].split_flags.clone();
                if a.oneway < 0 {
                    f.reverse();
                }
                Some(f)
            } else {
                None
            };
            let is_split = |k: usize, id: i64| match &flags {
                Some(f) => f[k],
                None => k == 0 || k == last || split.binary_search(&id).is_ok(),
            };
            let mut start = 0usize;
            for k in 1..pts.len() {
                if !is_split(k, pts[k].0) {
                    continue;
                }
                let key: SegKey = [w.id, pts[start].0, pts[k].0];
                if edits.removed.contains(&key) {
                    start = k;
                    continue;
                }
                let edit = edits.segs.get(&key);
                let reverse = edit.map_or(false, |e| e.reverse);
                let (mut from_i, mut to_i) = (start, k);
                let mut pp: Vec<V2> = pts[start..=k].iter().map(|p| p.1).collect();
                if reverse {
                    pp.reverse();
                    (from_i, to_i) = (k, start);
                }
                let poly = Poly::new(pp);
                let from = split.binary_search(&pts[from_i].0).unwrap();
                let to = split.binary_search(&pts[to_i].0).unwrap();
                if node_pos[from].is_none() {
                    node_pos[from] = Some(pts[from_i].1);
                }
                if node_pos[to].is_none() {
                    node_pos[to] = Some(pts[to_i].1);
                }
                start = k;
                if !poly.ok() {
                    continue;
                }

                let mut oneway = a.oneway != 0;
                let (mut lf, mut lb) = (a.lf, if oneway { 0 } else { a.lb });
                let (mut bus_f, mut bus_b) = (a.bus_f.clone(), a.bus_b.clone());
                let (mut turn_f, mut turn_b) = (a.turn_f.clone(), a.turn_b.clone());
                let mut lane_w = a.w;
                if reverse && !oneway {
                    std::mem::swap(&mut lf, &mut lb);
                    std::mem::swap(&mut bus_f, &mut bus_b);
                    std::mem::swap(&mut turn_f, &mut turn_b);
                }
                if let Some(e) = edit {
                    oneway = e.oneway;
                    let old = (lf, lb);
                    lf = e.lf.clamp(1, MAX_LANES);
                    lb = if oneway { 0 } else { e.lb.clamp(1, MAX_LANES) };
                    if e.w >= 2.0 && e.w <= 5.0 {
                        lane_w = e.w;
                    }
                    bus_f = (0..lf as usize).map(|i| e.bus_f.get(i).copied().unwrap_or(false)).collect();
                    bus_b = (0..lb as usize).map(|i| e.bus_b.get(i).copied().unwrap_or(false)).collect();
                    // máscaras de curva do OSM só valem se o nº de faixas não mudou
                    if old.0 != lf {
                        turn_f = vec![0; lf as usize];
                    }
                    if old.1 != lb {
                        turn_b = vec![0; lb as usize];
                    }
                }
                turn_f.resize(lf as usize, 0);
                turn_b.resize(lb as usize, 0);
                bus_f.resize(lf as usize, false);
                bus_b.resize(lb as usize, false);

                net.segs.push(Seg {
                    key,
                    way_id: w.id,
                    from: from as u32,
                    to: to as u32,
                    poly,
                    hw: w.hw,
                    link: w.link,
                    oneway,
                    roundabout: a.roundabout,
                    lf,
                    lb,
                    w: lane_w,
                    name,
                    maxspeed: a.maxspeed,
                    bus_f,
                    bus_b,
                    turn_f,
                    turn_b,
                    cut_from: 0.0,
                    cut_to: 0.0,
                    lane_base: 0,
                });
            }
        }

        stage!(_t, "4 segmentos (attrs, polilinhas)");
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
                signal: edits.sigs.get(id).map_or(raw.signals.contains(id) || marks.contains(id), |e| e.enabled),
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

        stage!(_t, "5 nós");
        net.sort_ends();
        stage!(_t, "6 ordenar pontas");
        net.compute_cuts();
        stage!(_t, "7 recuos");
        net.build_lanes();
        stage!(_t, "8 faixas");
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

/// Chave estável de uma aproximação (ponta de trecho) de um nó: way:de:para:lado, na orientação original.
pub fn approach_key(net: &Net, node: u32, e: u32) -> String {
    let s = &net.segs[(e >> 1) as usize];
    let osm = net.nodes[node as usize].osm_id;
    let side = if s.key[1] != s.key[2] {
        (osm != s.key[1]) as u32
    } else {
        e & 1
    };
    format!("{}:{}:{}:{}", s.key[0], s.key[1], s.key[2], side)
}

/// Tags OSM sintéticas de uma via criada (para reaproveitar `way_attrs`).
fn road_tags(r: &NewRoad) -> Vec<(String, String)> {
    let lf = r.lf.clamp(1, MAX_LANES);
    let lb = r.lb.clamp(1, MAX_LANES);
    let mut t = vec![("highway".to_string(), r.hw.clone())];
    if !r.name.is_empty() {
        t.push(("name".into(), r.name.clone()));
    }
    if r.oneway {
        t.push(("oneway".into(), "yes".into()));
        t.push(("lanes".into(), lf.to_string()));
    } else {
        t.push(("oneway".into(), "no".into()));
        t.push(("lanes:forward".into(), lf.to_string()));
        t.push(("lanes:backward".into(), lb.to_string()));
        t.push(("lanes".into(), (lf as u32 + lb as u32).to_string()));
    }
    t
}
