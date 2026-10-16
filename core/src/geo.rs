//! Geometria 2D básica (coordenadas em metros, eixo Y para baixo, como na tela).
//! Convenção: "direita" de uma direção d = (-d.y, d.x). Ângulos crescem no sentido horário na tela.

pub type V2 = [f64; 2];

#[inline]
pub fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
#[inline]
pub fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
#[inline]
pub fn mul(a: V2, k: f64) -> V2 {
    [a[0] * k, a[1] * k]
}
#[inline]
pub fn dot(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
#[inline]
pub fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
#[inline]
pub fn len(a: V2) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
#[inline]
pub fn norm(a: V2) -> V2 {
    let l = len(a);
    if l < 1e-12 {
        [1.0, 0.0]
    } else {
        [a[0] / l, a[1] / l]
    }
}
#[inline]
pub fn right(d: V2) -> V2 {
    [-d[1], d[0]]
}

/// Polilinha com comprimento acumulado.
#[derive(Clone, Debug, Default)]
pub struct Poly {
    pub pts: Vec<V2>,
    pub cum: Vec<f64>,
}

impl Poly {
    pub fn new(raw: Vec<V2>) -> Poly {
        let mut pts: Vec<V2> = Vec::with_capacity(raw.len());
        for p in raw {
            if pts.last().map_or(true, |l| len(sub(p, *l)) > 0.05) {
                pts.push(p);
            }
        }
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for i in 0..pts.len() {
            if i > 0 {
                acc += len(sub(pts[i], pts[i - 1]));
            }
            cum.push(acc);
        }
        Poly { pts, cum }
    }

    pub fn ok(&self) -> bool {
        self.pts.len() >= 2
    }

    pub fn length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Posição e tangente (unitária) no comprimento de arco `s`.
    pub fn at(&self, s: f64) -> (V2, V2) {
        let s = s.clamp(0.0, self.length());
        let n = self.pts.len();
        let i = self.cum.partition_point(|&c| c <= s).saturating_sub(1).min(n - 2);
        let a = self.pts[i];
        let b = self.pts[i + 1];
        let l = self.cum[i + 1] - self.cum[i];
        let t = if l > 0.0 { (s - self.cum[i]) / l } else { 0.0 };
        (add(a, mul(sub(b, a), t)), norm(sub(b, a)))
    }

    /// Sub-polilinha entre os comprimentos de arco s0 e s1.
    pub fn slice(&self, s0: f64, s1: f64) -> Vec<V2> {
        let l = self.length();
        let (s0, s1) = (s0.clamp(0.0, l), s1.clamp(0.0, l));
        if s1 - s0 < 1e-6 {
            return Vec::new();
        }
        let mut out = vec![self.at(s0).0];
        let mut i = self.cum.partition_point(|&c| c <= s0);
        while i < self.pts.len() && self.cum[i] < s1 {
            out.push(self.pts[i]);
            i += 1;
        }
        out.push(self.at(s1).0);
        out
    }
}

/// Vetor de deslocamento (já com correção de esquina/miter) por vértice, para offset positivo à direita.
pub fn miter(pts: &[V2]) -> Vec<V2> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let d_prev = if i > 0 { norm(sub(pts[i], pts[i - 1])) } else { norm(sub(pts[1], pts[0])) };
        let d_next = if i + 1 < n { norm(sub(pts[i + 1], pts[i])) } else { d_prev };
        let n1 = right(d_prev);
        let n2 = right(d_next);
        let m = add(n1, n2);
        let ml = len(m);
        if ml < 1e-9 {
            out.push(n1);
            continue;
        }
        let m = mul(m, 1.0 / ml);
        let c = dot(m, n1).max(0.35);
        out.push(mul(m, 1.0 / c));
    }
    out
}

pub fn offset(pts: &[V2], p: f64) -> Vec<V2> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let m = miter(pts);
    pts.iter().zip(m).map(|(q, m)| add(*q, mul(m, p))).collect()
}

/// Malha de triângulos (posições xy intercaladas + índices).
#[derive(Default)]
pub struct Mesh {
    pub pos: Vec<f32>,
    pub idx: Vec<u32>,
}

impl Mesh {
    pub fn vert(&mut self, p: V2) -> u32 {
        let i = (self.pos.len() / 2) as u32;
        self.pos.push(p[0] as f32);
        self.pos.push(p[1] as f32);
        i
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.idx.extend_from_slice(&[a, b, c]);
    }

    /// Faixa ao longo de `pts` entre os deslocamentos laterais `a` < `b` (direita positiva).
    pub fn strip(&mut self, pts: &[V2], a: f64, b: f64) {
        if pts.len() < 2 {
            return;
        }
        let m = miter(pts);
        let mut prev: Option<(u32, u32)> = None;
        for i in 0..pts.len() {
            let va = self.vert(add(pts[i], mul(m[i], a)));
            let vb = self.vert(add(pts[i], mul(m[i], b)));
            if let Some((pa, pb)) = prev {
                self.tri(pa, pb, va);
                self.tri(pb, vb, va);
            }
            prev = Some((va, vb));
        }
    }

    /// Linha tracejada centrada em `pts`.
    pub fn dashes(&mut self, pts: &[V2], dash: f64, gap: f64, half_w: f64) {
        let poly = Poly::new(pts.to_vec());
        if !poly.ok() {
            return;
        }
        let l = poly.length();
        let n = ((l + gap) / (dash + gap)).floor() as i64;
        if n < 1 {
            return;
        }
        let used = n as f64 * dash + (n - 1) as f64 * gap;
        let start = (l - used) / 2.0;
        for k in 0..n {
            let s = start + k as f64 * (dash + gap);
            let sl = poly.slice(s, s + dash);
            self.strip(&sl, -half_w, half_w);
        }
    }

    /// Polígono estrelado preenchido a partir de `center`.
    pub fn fan(&mut self, center: V2, ring: &[V2]) {
        if ring.len() < 3 {
            return;
        }
        let c = self.vert(center);
        let first = (self.pos.len() / 2) as u32;
        for p in ring {
            self.vert(*p);
        }
        let n = ring.len() as u32;
        for k in 0..n {
            self.tri(c, first + k, first + (k + 1) % n);
        }
    }
}
