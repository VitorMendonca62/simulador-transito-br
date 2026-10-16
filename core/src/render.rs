//! Geração das malhas estáticas (asfalto, marcações) a partir do grafo.

use crate::geo::*;
use crate::net::*;

#[derive(Default)]
pub struct Meshes {
    pub asphalt: Mesh,
    pub bus: Mesh,
    pub white: Mesh,
    pub yellow: Mesh,
}

pub fn build_meshes(net: &Net) -> Meshes {
    let mut m = Meshes::default();

    for s in &net.segs {
        let l = s.poly.length();
        let (s0, s1) = (s.cut_from, l - s.cut_to);
        if s1 - s0 < 0.5 {
            continue;
        }
        let sl = s.poly.slice(s0, s1);
        if sl.len() < 2 {
            continue;
        }
        let (left, right_e, w, p0) = (s.left_ext(), s.right_ext(), s.w, s.p0());
        m.asphalt.strip(&sl, -left, right_e);

        for i in 0..s.lf as usize {
            if s.bus_f[i] {
                let a = p0 + i as f64 * w;
                m.bus.strip(&sl, a, a + w);
            }
        }
        for j in 0..s.lb as usize {
            if s.bus_b[j] {
                m.bus.strip(&sl, -(j as f64 + 1.0) * w, -(j as f64) * w);
            }
        }

        // separadores brancos entre faixas do mesmo sentido
        for k in 1..s.lf as i32 {
            m.white.dashes(&offset(&sl, p0 + k as f64 * w), 3.0, 5.0, 0.13);
        }
        for k in 1..s.lb as i32 {
            m.white.dashes(&offset(&sl, -(k as f64) * w), 3.0, 5.0, 0.13);
        }

        // eixo amarelo entre sentidos opostos
        if !s.oneway {
            let big = s.hw <= HW_SECONDARY || s.lf as u32 + s.lb as u32 >= 4;
            if big {
                m.yellow.strip(&sl, -0.34, -0.12);
                m.yellow.strip(&sl, 0.12, 0.34);
            } else if s.hw != HW_SERVICE {
                m.yellow.dashes(&sl, 3.0, 5.0, 0.13);
            }
        }

        // linhas de borda em vias principais
        if s.hw <= HW_SECONDARY && !s.link {
            m.white.strip(&sl, -left + 0.3, -left + 0.5);
            m.white.strip(&sl, right_e - 0.5, right_e - 0.3);
        }

        // faixa de retenção e faixa de pedestres nas pontas
        let inner_len = s1 - s0;
        for at_to in [false, true] {
            let ni = if at_to { s.to } else { s.from };
            let node = &net.nodes[ni as usize];
            let deg = node.ends.len();
            let mw = s.hw == HW_MOTORWAY || s.link;
            let has_in = if at_to { s.lf > 0 } else { s.lb > 0 };
            let zebra = (node.signal || node.crossing) && deg >= 2 && !mw;
            let stop = deg >= 3 && has_in && !mw && !s.roundabout;
            let (se, dir) = if at_to { (s1, -1.0) } else { (s0, 1.0) };
            let mut cursor = 0.4;
            if zebra && inner_len > 7.0 {
                let (a, b) = (se + dir * cursor, se + dir * (cursor + 2.4));
                let part = s.poly.slice(a.min(b), a.max(b));
                let mut p = -left + 0.45;
                while p + 0.225 <= right_e {
                    m.white.strip(&part, p - 0.225, p + 0.225);
                    p += 0.9;
                }
                cursor += 3.0;
            }
            if stop && inner_len > cursor + 2.0 {
                let (a, b) = (se + dir * cursor, se + dir * (cursor + 0.5));
                let part = s.poly.slice(a.min(b), a.max(b));
                let (lo, hi) = if at_to { (p0 + 0.15, p0 + s.lf as f64 * w - 0.15) } else { (-(s.lb as f64) * w + 0.15, -0.15) };
                m.white.strip(&part, lo, hi);
            }
        }
    }

    // polígonos de interseção
    for node in &net.nodes {
        let n = node.ends.len();
        if n < 2 {
            continue;
        }
        let mut ring: Vec<V2> = Vec::with_capacity(n * 2);
        for &e in &node.ends {
            let s = &net.segs[(e >> 1) as usize];
            let l = s.poly.length();
            let (sv, sign, cut) = if e & 1 == 0 { (s.cut_from, 1.0, s.cut_from) } else { (l - s.cut_to, -1.0, s.cut_to) };
            let _ = cut;
            let (p, t) = s.poly.at(sv);
            let u = mul(t, sign);
            let (cw, ccw) = net.ext_cw_ccw(e);
            ring.push(sub(p, mul(right(u), ccw)));
            ring.push(add(p, mul(right(u), cw)));
        }
        m.asphalt.fan(node.pos, &ring);
    }
    m
}
