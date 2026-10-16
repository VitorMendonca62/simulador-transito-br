//! Métricas da simulação: por trecho direcionado (velocidade, fila, vazão), por cruzamento (atraso, fila),
//! por par de zonas (tempo de viagem e atraso) e série global no tempo.

use serde_json::json;

pub const BUCKET_S: f64 = 60.0;
const HIST_MAX: usize = 360;
pub const NO_DATA: u8 = 255;

pub struct Metrics {
    nd: usize,
    nz: usize,
    // balde atual
    occ: Vec<f32>,
    spd: Vec<f32>,
    stop: Vec<f32>,
    entries: Vec<u16>,
    bucket_start: f64,
    // janela curta para o mapa de calor em tempo real
    w_occ: Vec<f32>,
    w_spd: Vec<f32>,
    w_stop: Vec<f32>,
    ema_ratio: Vec<f32>,
    ema_queue: Vec<f32>,
    seen: Vec<bool>,
    window_start: f64,
    // totais da execução (para comparar cenários)
    run_occ: Vec<f32>,
    run_spd: Vec<f32>,
    run_entries: Vec<u32>,
    /// por balde finalizado: razão de velocidade (0..254) por trecho direcionado
    pub hist_speed: Vec<Vec<u8>>,
    // cruzamentos
    pub node_delay: Vec<f32>,
    pub node_queue_max: Vec<u16>,
    stopped_now: Vec<u16>,
    touched: Vec<u32>,
    // viagens
    od_n: Vec<u32>,
    od_travel: Vec<f64>,
    od_delay: Vec<f64>,
    pub trips: u64,
    pub travel_sum: f64,
    pub delay_sum: f64,
    // série global (uma linha por balde): t, vivos, concluídos no balde, vel. média (km/h), fila de entrada, parados (fração)
    pub series: Vec<[f32; 6]>,
    bucket_finished: u32,
    // acumuladores globais de velocidade (ponderados por tempo de veículo)
    veh_time: f64,
    veh_speed_time: f64,
    run_veh_time: f64,
    run_speed_time: f64,
}

impl Metrics {
    pub fn new(nd: usize, nodes: usize, nz: usize) -> Metrics {
        Metrics {
            nd,
            nz,
            occ: vec![0.0; nd],
            spd: vec![0.0; nd],
            stop: vec![0.0; nd],
            entries: vec![0; nd],
            bucket_start: -1.0,
            w_occ: vec![0.0; nd],
            w_spd: vec![0.0; nd],
            w_stop: vec![0.0; nd],
            ema_ratio: vec![1.0; nd],
            ema_queue: vec![0.0; nd],
            seen: vec![false; nd],
            window_start: -1.0,
            run_occ: vec![0.0; nd],
            run_spd: vec![0.0; nd],
            run_entries: vec![0; nd],
            hist_speed: Vec::new(),
            node_delay: vec![0.0; nodes],
            node_queue_max: vec![0; nodes],
            stopped_now: vec![0; nodes],
            touched: Vec::new(),
            od_n: vec![0; nz * nz],
            od_travel: vec![0.0; nz * nz],
            od_delay: vec![0.0; nz * nz],
            trips: 0,
            travel_sum: 0.0,
            delay_sum: 0.0,
            series: Vec::new(),
            bucket_finished: 0,
            veh_time: 0.0,
            veh_speed_time: 0.0,
            run_veh_time: 0.0,
            run_speed_time: 0.0,
        }
    }

    /// Um veículo numa faixa do trecho direcionado `d` durante `dt` com velocidade `v`.
    #[inline]
    pub fn sample(&mut self, d: usize, v: f32, dt: f32, node: usize, dist_to_end: f32) {
        self.occ[d] += dt;
        self.spd[d] += v * dt;
        self.w_occ[d] += dt;
        self.w_spd[d] += v * dt;
        self.veh_time += dt as f64;
        self.veh_speed_time += (v * dt) as f64;
        self.run_veh_time += dt as f64;
        self.run_speed_time += (v * dt) as f64;
        if v < 1.4 {
            self.stop[d] += dt;
            self.w_stop[d] += dt;
            if dist_to_end < 45.0 {
                self.node_delay[node] += dt;
                if self.stopped_now[node] == 0 {
                    self.touched.push(node as u32);
                }
                self.stopped_now[node] += 1;
            }
        }
    }

    pub fn entered(&mut self, d: usize) {
        self.entries[d] = self.entries[d].saturating_add(1);
        self.run_entries[d] += 1;
    }

    pub fn trip_done(&mut self, from: usize, to: usize, travel: f64, free: f64) {
        let delay = (travel - free).max(0.0);
        self.trips += 1;
        self.travel_sum += travel;
        self.delay_sum += delay;
        self.bucket_finished += 1;
        if from < self.nz && to < self.nz {
            let k = from * self.nz + to;
            self.od_n[k] += 1;
            self.od_travel[k] += travel;
            self.od_delay[k] += delay;
        }
    }

    /// Fim do passo: fila máxima por cruzamento, janela de calor e balde de 60 s.
    pub fn end_tick(&mut self, now: f64, free_speed: &dyn Fn(usize) -> f32, alive: usize, queued: u64) {
        for &n in &self.touched {
            let n = n as usize;
            self.node_queue_max[n] = self.node_queue_max[n].max(self.stopped_now[n]);
            self.stopped_now[n] = 0;
        }
        self.touched.clear();
        if self.bucket_start < 0.0 {
            self.bucket_start = now;
            self.window_start = now;
        }
        // janela de calor (5 s): mistura exponencial; sem veículos, volta lentamente ao fluxo livre
        if now - self.window_start >= 5.0 {
            for d in 0..self.nd {
                if self.w_occ[d] > 0.0 {
                    let ratio = ((self.w_spd[d] / self.w_occ[d]) / free_speed(d).max(0.5)).clamp(0.0, 1.0);
                    let q = (self.w_stop[d] / self.w_occ[d]).clamp(0.0, 1.0);
                    if self.seen[d] {
                        self.ema_ratio[d] = 0.65 * self.ema_ratio[d] + 0.35 * ratio;
                        self.ema_queue[d] = 0.65 * self.ema_queue[d] + 0.35 * q;
                    } else {
                        self.ema_ratio[d] = ratio;
                        self.ema_queue[d] = q;
                        self.seen[d] = true;
                    }
                    self.w_occ[d] = 0.0;
                    self.w_spd[d] = 0.0;
                    self.w_stop[d] = 0.0;
                } else if self.seen[d] {
                    self.ema_ratio[d] += (1.0 - self.ema_ratio[d]) * 0.12;
                    self.ema_queue[d] *= 0.88;
                }
            }
            self.window_start = now;
        }
        if now - self.bucket_start >= BUCKET_S {
            let mut row = vec![NO_DATA; self.nd];
            for d in 0..self.nd {
                if self.occ[d] > 0.0 {
                    let ratio = ((self.spd[d] / self.occ[d]) / free_speed(d).max(0.5)).clamp(0.0, 1.0);
                    row[d] = (ratio * 254.0).round() as u8;
                    self.run_occ[d] += self.occ[d];
                    self.run_spd[d] += self.spd[d];
                }
                self.occ[d] = 0.0;
                self.spd[d] = 0.0;
                self.stop[d] = 0.0;
                self.entries[d] = 0;
            }
            if self.hist_speed.len() >= HIST_MAX {
                self.hist_speed.remove(0);
            }
            self.hist_speed.push(row);
            let mean_kmh = if self.veh_time > 0.0 { (self.veh_speed_time / self.veh_time * 3.6) as f32 } else { 0.0 };
            self.series.push([now as f32, alive as f32, self.bucket_finished as f32, mean_kmh, queued as f32, 0.0]);
            self.bucket_finished = 0;
            self.veh_time = 0.0;
            self.veh_speed_time = 0.0;
            self.bucket_start = now;
        }
    }

    /// Valores 0..254 por trecho direcionado para o mapa de calor (255 = sem dados).
    /// `kind`: 0 = razão de velocidade (254 = livre), 1 = fração parada (254 = tudo parado).
    pub fn heat(&self, kind: u8) -> Vec<u8> {
        (0..self.nd)
            .map(|d| {
                if !self.seen[d] {
                    NO_DATA
                } else if kind == 0 {
                    (self.ema_ratio[d] * 254.0).round() as u8
                } else {
                    (self.ema_queue[d] * 254.0).round() as u8
                }
            })
            .collect()
    }

    /// Razão média de velocidade na execução inteira por trecho direcionado (0..254, 255 = sem dados).
    pub fn run_ratio(&self, free_speed: &dyn Fn(usize) -> f32) -> Vec<u8> {
        (0..self.nd)
            .map(|d| {
                let (o, s) = (self.run_occ[d] + self.occ[d], self.run_spd[d] + self.spd[d]);
                if o <= 0.0 {
                    NO_DATA
                } else {
                    (((s / o) / free_speed(d).max(0.5)).clamp(0.0, 1.0) * 254.0).round() as u8
                }
            })
            .collect()
    }

    /// Velocidade média (km/h) ponderada por tempo de veículo, em toda a execução.
    pub fn mean_speed_kmh(&self) -> f64 {
        if self.run_veh_time > 0.0 {
            self.run_speed_time / self.run_veh_time * 3.6
        } else {
            0.0
        }
    }

    pub fn series_flat(&self) -> Vec<f32> {
        self.series.iter().flat_map(|r| r.iter().copied()).collect()
    }

    pub fn report(&self, zone_ids: &[i64], node_osm: &dyn Fn(usize) -> i64, elapsed: f64, alive: usize, queued: u64, spawned: u64) -> serde_json::Value {
        let mut od = Vec::new();
        for i in 0..self.nz {
            for j in 0..self.nz {
                let k = i * self.nz + j;
                if self.od_n[k] > 0 {
                    od.push(json!({"from": zone_ids[i], "to": zone_ids[j], "n": self.od_n[k], "meanTravel": self.od_travel[k] / self.od_n[k] as f64, "meanDelay": self.od_delay[k] / self.od_n[k] as f64}));
                }
            }
        }
        let mut jn: Vec<(usize, f32)> = self.node_delay.iter().enumerate().filter(|(_, d)| **d > 1.0).map(|(i, d)| (i, *d)).collect();
        jn.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let junctions: Vec<_> = jn.iter().take(25).map(|(n, d)| json!({"node": n, "osm": node_osm(*n) as f64, "delayVehH": *d as f64 / 3600.0, "queueMax": self.node_queue_max[*n]})).collect();
        let n = self.trips.max(1) as f64;
        json!({
            "elapsed": elapsed,
            "spawned": spawned,
            "trips": self.trips,
            "alive": alive,
            "queued": queued,
            "meanTravel": self.travel_sum / n,
            "meanDelay": self.delay_sum / n,
            "totalDelayVehH": self.delay_sum / 3600.0,
            "throughputPerH": if elapsed > 0.0 { self.trips as f64 / (elapsed / 3600.0) } else { 0.0 },
            "maxQueue": self.node_queue_max.iter().copied().max().unwrap_or(0),
            "od": od,
            "junctions": junctions,
        })
    }
}
