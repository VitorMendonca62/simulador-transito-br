#!/usr/bin/env python3
"""Linhas de ônibus do Recife para o simulador (itinerários, paradas e horários).

Fonte única: o GTFS oficial do Grande Recife Consórcio de Transporte (itinerário = shape, paradas, horário de cada
viagem). O itinerário é recortado pelo mapa do app (public/data/mapa-recife.osm.gz) e casado com os ways desse mesmo
arquivo, para o núcleo encaixar as linhas por id de way. Paradas do OSM (Overpass) só dizem quais são baias.

Uso:
    python scripts/onibus_recife/gerar_onibus.py [--saida data/onibus_recife] [--brutos data/onibus_recife/_brutos]
        [--app public/data/onibus_recife.json] [--atualizar] [--inspecionar 2450,101 --inspecao arquivo.geojson]

Baixa só o que falta em _brutos (o GTFS e uma consulta Overpass); --atualizar baixa de novo.
Dependências: numpy, shapely, requests (ver requirements.txt).
"""
import argparse
import csv
import datetime as dt
import gzip
import heapq
import io
import json
import math
import sys
import time
import zipfile
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import requests
import shapely
from shapely.geometry import LineString

GTFS_URL = "https://www.granderecife.pe.gov.br/gtfs/gtfs.zip"
GTFS_PAGINA = "https://www.granderecife.pe.gov.br/transporte/gtfs/"
OVERPASS = ["https://overpass-api.de/api/interpreter", "https://overpass.kumi.systems/api/interpreter"]
UA = "simulador-transito-recife/0.1 (processamento de dados abertos)"
AREA_RECIFE = 'area["boundary"="administrative"]["admin_level"="8"]["name"="Recife"]["IBGE:GEOCODIGO"="2611606"]->.a;'
Q_PARADAS = (f'[out:json][timeout:180];{AREA_RECIFE}(node["highway"="bus_stop"](area.a);'
             'node["public_transport"~"^(platform|stop_position)$"]["bus"="yes"](area.a);'
             'node["bus_bay"](area.a);way["bus_bay"](area.a););out tags center;')
MAPA = Path("public/data/mapa-recife.osm.gz")

# vias que o núcleo aceita (core/src/net.rs, parse_highway); `_link` incluso
VIAS = {"motorway", "trunk", "primary", "secondary", "tertiary", "unclassified", "residential", "living_street",
        "busway"}
# projeção local (metros), como o núcleo: equiretangular em torno do centro do Recife
LAT0, LON0 = -8.05, -34.92
KX, KY = 111_320.0 * math.cos(math.radians(LAT0)), 110_574.0

# recorte pelo mapa: amostras perto de alguma via veicular
PERTO_M = 60.0
FORA_CURTO_M = 500.0    # saída do mapa mais curta que isso conta como dentro (praça, parque)
TRECHO_MIN_M = 300.0    # trecho dentro do mapa mais curto que isso é descartado
# casamento com os ways
PASSO_M = 10.0
RAIO_M = 20.0
RUMO_MAX = 45.0
C_DIST = 1 / 4.0        # custo por metro de distância ao way
C_VIZINHO = 2.0         # trocar para um way que encosta no anterior
C_DOIS = 6.0            # trocar para um way ligado por um terceiro (curto, entre duas amostras)
C_RETORNO = 30.0        # inverter o sentido no mesmo way
C_LACUNA = 25.0         # trocar para um way sem ligação com o anterior
DESVIO_M = 3000.0       # na classificação das lacunas: caminho dirigido até essa distância = desvio longo
PONTE_FATOR, PONTE_FOLGA_M = 2.0, 300.0  # lacuna ligada pelo menor caminho de até 2× a reta + 300 m
# paradas
PARADA_RAIO_M = 30.0    # parada a até essa distância do itinerário entra no padrão
BAIA_RAIO_M = 25.0
OSM_PARADA_RAIO_M = 30.0
SIMPLIFICA_M = 10.0
HORA_PICO = 7


def xy(lat, lon):
    return (np.asarray(lon) - LON0) * KX, (np.asarray(lat) - LAT0) * KY


def latlon(x, y):
    return np.asarray(y) / KY + LAT0, np.asarray(x) / KX + LON0


# ---------------------------------------------------------------- downloads

def _get(url, dest: Path):
    r = requests.get(url, headers={"User-Agent": UA}, timeout=600)
    r.raise_for_status()
    dest.write_bytes(r.content)


def overpass(query: str, dest: Path, atualizar: bool):
    if dest.exists() and dest.stat().st_size > 1000 and not atualizar:
        return
    espera = 15
    for tentativa in range(8):
        url = OVERPASS[tentativa % len(OVERPASS)]
        try:
            r = requests.post(url, data={"data": query}, headers={"User-Agent": UA}, timeout=360)
            if r.ok and r.content.lstrip().startswith(b"{"):
                dest.write_bytes(r.content)
                return
            print(f"overpass {url}: HTTP {r.status_code}, nova tentativa em {espera}s", file=sys.stderr)
        except requests.RequestException as e:
            print(f"overpass {url}: {e}, nova tentativa em {espera}s", file=sys.stderr)
        time.sleep(espera)
        espera = min(espera * 2, 240)
    sys.exit(f"Overpass indisponível; tente de novo mais tarde ({dest.name})")


def baixar(brutos: Path, atualizar: bool):
    brutos.mkdir(parents=True, exist_ok=True)
    gtfs = brutos / "gtfs.zip"
    if atualizar or not gtfs.exists():
        print("baixando gtfs.zip do Grande Recife ...", file=sys.stderr)
        _get(GTFS_URL, gtfs)
    print("consultando Overpass (paradas) ...", file=sys.stderr)
    overpass(Q_PARADAS, brutos / "osm_paradas.json", atualizar)


# ---------------------------------------------------------------- mapa

class Mapa:
    """Ways veiculares do mapa do app, em pedaços de dois pontos para a busca espacial."""

    def __init__(self, path: Path):
        coords, ways, paradas = {}, {}, []
        self.osm_base = None
        with gzip.open(path) as f:
            for ev, el in ET.iterparse(f, events=("end",)):
                tag = el.tag
                if tag == "node":
                    coords[int(el.get("id"))] = (float(el.get("lat")), float(el.get("lon")))
                    t = {c.get("k"): c.get("v") for c in el.iter("tag")}
                    if t.get("highway") == "bus_stop":
                        paradas.append((int(el.get("id")), float(el.get("lat")), float(el.get("lon")), t.get("name")))
                elif tag == "way":
                    t = {c.get("k"): c.get("v") for c in el.iter("tag")}
                    hw = t.get("highway", "")
                    if hw.removesuffix("_link") in VIAS and t.get("area") != "yes":
                        ways[int(el.get("id"))] = ([int(n.get("ref")) for n in el.iter("nd")], t)
                elif tag == "meta":
                    self.osm_base = el.get("osm_base")
                elif tag == "relation":
                    pass
                else:
                    continue
                el.clear()
        self.paradas_osm = paradas
        self.ids, self.nos, self.sentido, self.xy = [], [], [], []
        px0, py0, px1, py1, pw = [], [], [], [], []
        self.no_ways = defaultdict(set)
        for wid, (nds, t) in ways.items():
            nds = [n for n in nds if n in coords]
            if len(nds) < 2:
                continue
            k = len(self.ids)
            self.ids.append(wid)
            self.nos.append(nds)
            self.sentido.append(sentido_unico(t))
            la = np.array([coords[n][0] for n in nds])
            lo = np.array([coords[n][1] for n in nds])
            wx, wy = xy(la, lo)
            self.xy.append((wx, wy))
            for n in nds:
                self.no_ways[n].add(k)
            px0.extend(wx[:-1]); py0.extend(wy[:-1]); px1.extend(wx[1:]); py1.extend(wy[1:]); pw.extend([k] * (len(nds) - 1))
        self.idx = {w: k for k, w in enumerate(self.ids)}
        # grafo dirigido nó -> [(nó, metros, way, sentido)], com a mão única do núcleo
        self.grafo = defaultdict(list)
        self.no_xy = {}
        for k, (nds, (wx, wy)) in enumerate(zip(self.nos, self.xy)):
            for i in range(len(nds) - 1):
                c = float(math.hypot(wx[i + 1] - wx[i], wy[i + 1] - wy[i]))
                if self.sentido[k] in (0, 1):
                    self.grafo[nds[i]].append((nds[i + 1], c, k, 1))
                if self.sentido[k] in (0, -1):
                    self.grafo[nds[i + 1]].append((nds[i], c, k, -1))
            for n, a, b in zip(nds, wx, wy):
                self.no_xy[n] = (float(a), float(b))
        self.nos_set = [set(n) for n in self.nos]
        self.p = np.array([px0, py0, px1, py1]).T
        self.pw = np.array(pw)
        self.pang = np.degrees(np.arctan2(self.p[:, 3] - self.p[:, 1], self.p[:, 2] - self.p[:, 0]))
        self.tree = shapely.STRtree(shapely.linestrings(np.stack([self.p[:, :2], self.p[:, 2:]], axis=1)))
        self._viz = {}

    def dentro(self, x, y):
        """Amostras a até PERTO_M de alguma via do mapa (o mapa é o Recife; fora dele não há vias)."""
        ip, _ = self.tree.query(shapely.points(x, y), predicate="dwithin", distance=PERTO_M)
        d = np.zeros(len(x), bool)
        d[ip] = True
        return d

    def vizinhos(self, a: int, b: int) -> bool:
        k = (a, b) if a < b else (b, a)
        v = self._viz.get(k)
        if v is None:
            v = self._viz[k] = not self.nos_set[a].isdisjoint(self.nos_set[b])
        return v

    def caminho(self, a: int, p, b: int, q, teto=None, dirigido=True):
        """Menor caminho dirigido do way a (saindo perto do ponto p) ao way b (chegando perto de q), como lista de
        (way, sentido) entre os dois; None se não houver um com até PONTE_FATOR × a distância reta + PONTE_FOLGA_M
        (ou `teto` metros). `dirigido=False` ignora a mão única (só para classificar lacunas)."""
        if teto is None:
            teto = PONTE_FATOR * math.hypot(q[0] - p[0], q[1] - p[1]) + PONTE_FOLGA_M
        grafo = self.grafo if dirigido else self.grafo_livre()
        dist, de, fila = {}, {}, []
        for n in self.nos[a]:
            c = math.hypot(self.no_xy[n][0] - p[0], self.no_xy[n][1] - p[1])
            if c < dist.get(n, 1e18):
                dist[n] = c
                heapq.heappush(fila, (c, n))
        alvo = self.nos_set[b]
        while fila:
            c, n = heapq.heappop(fila)
            if c > dist.get(n, 1e18) or c > teto:
                continue
            if n in alvo and n not in self.nos_set[a]:
                fim = c + math.hypot(self.no_xy[n][0] - q[0], self.no_xy[n][1] - q[1])
                if fim > teto:
                    continue
                seq = []
                while n in de:
                    n, w, s = de[n]
                    if not seq or seq[-1] != (w, s):
                        seq.append((w, s))
                seq.reverse()
                return [e for e in seq if e[0] not in (a, b)]
            for m, cc, w, s in grafo.get(n, ()):
                nc = c + cc
                if nc < dist.get(m, 1e18):
                    dist[m] = nc
                    de[m] = (n, w, s)
                    heapq.heappush(fila, (nc, m))
        return None


def _grafo_livre(self):
    if not hasattr(self, "_livre"):
        self._livre = defaultdict(list)
        for n, arcos in self.grafo.items():
            for m, c, w, s in arcos:
                self._livre[n].append((m, c, w, s))
                self._livre[m].append((n, c, w, -s))
    return self._livre


Mapa.grafo_livre = _grafo_livre


def sentido_unico(t: dict) -> int:
    """Mesma leitura do núcleo (core/src/net.rs, way_attrs): 1 só no sentido do way, -1 só no contrário, 0 os dois."""
    v = t.get("oneway")
    if v in ("yes", "true", "1"):
        return 1
    if v in ("-1", "reverse"):
        return -1
    if v in ("no", "false", "0"):
        return 0
    return 1 if t.get("junction") in ("roundabout", "circular") or t.get("highway") == "motorway" else 0


# ---------------------------------------------------------------- GTFS

def ler_csv(z: zipfile.ZipFile, nome: str):
    with z.open(nome) as f:
        yield from csv.DictReader(io.TextIOWrapper(f, encoding="utf-8-sig"))


def hms(s: str) -> int:
    h, m, ss = s.split(":")
    return int(h) * 3600 + int(m) * 60 + int(ss)


def dia_referencia(z: zipfile.ZipFile, versao_data: dt.date):
    """Primeira quarta-feira a partir da data do feed em que nenhum serviço de dia útil tem exceção."""
    cal = list(ler_csv(z, "calendar.txt"))
    exc = defaultdict(dict)
    for r in ler_csv(z, "calendar_dates.txt"):
        exc[r["date"]][r["service_id"]] = r["exception_type"]
    d = versao_data + dt.timedelta(days=(2 - versao_data.weekday()) % 7)
    for _ in range(60):
        ds = d.strftime("%Y%m%d")
        if not exc.get(ds):
            ativos = {r["service_id"] for r in cal if r["wednesday"] == "1" and r["start_date"] <= ds <= r["end_date"]}
            return d, ativos
        d += dt.timedelta(days=7)
    sys.exit("sem quarta-feira sem exceções no calendário")


def ler_gtfs(path: Path):
    z = zipfile.ZipFile(path)
    info = next(ler_csv(z, "feed_info.txt"))
    versao = info["feed_version"]
    vd = dt.date.fromisoformat(versao.lstrip("v").replace(".", "-"))
    dia, servicos = dia_referencia(z, vd)
    rotas = {r["route_id"]: r for r in ler_csv(z, "routes.txt") if r["route_type"] == "3"}
    viagens = {r["trip_id"]: r for r in ler_csv(z, "trips.txt")
               if r["route_id"] in rotas and r["service_id"] in servicos}
    paradas = {r["stop_id"]: r for r in ler_csv(z, "stops.txt")}
    usados = {v["shape_id"] for v in viagens.values()}
    shapes = defaultdict(list)
    for r in ler_csv(z, "shapes.txt"):
        if r["shape_id"] in usados:
            shapes[r["shape_id"]].append((int(r["shape_pt_sequence"]), float(r["shape_pt_lat"]), float(r["shape_pt_lon"])))
    for s in shapes.values():
        s.sort()
    print(f"lendo stop_times ({len(viagens)} viagens do dia {dia}) ...", file=sys.stderr)
    horarios = defaultdict(list)
    with z.open("stop_times.txt") as f:
        rd = csv.reader(io.TextIOWrapper(f, encoding="utf-8-sig"))
        cab = next(rd)
        it, ia, isq, ist = cab.index("trip_id"), cab.index("arrival_time"), cab.index("stop_sequence"), cab.index("stop_id")
        for r in rd:
            if r[it] in viagens:
                horarios[r[it]].append((int(r[isq]), r[ist], hms(r[ia])))
    for h in horarios.values():
        h.sort()
    return {"versao": versao, "dia": dia, "rotas": rotas, "viagens": viagens, "paradas": paradas, "shapes": shapes,
            "horarios": horarios, "total_rotas": sum(1 for _ in ler_csv(z, "routes.txt")),
            "rotas_onibus": len(rotas)}


# ---------------------------------------------------------------- geometria

def densificar(x, y, passo):
    """Amostras a cada `passo` metros ao longo da polilinha; devolve (x, y, distância acumulada)."""
    d = np.concatenate([[0], np.cumsum(np.hypot(np.diff(x), np.diff(y)))])
    if d[-1] <= 0:
        return x[:1], y[:1], d[:1]
    s = np.linspace(0, d[-1], max(2, int(d[-1] // passo) + 1))
    return np.interp(s, d, x), np.interp(s, d, y), s


def recortar(mapa: Mapa, sx, sy, s):
    """Faixas [i0, i1) de amostras dentro do mapa, ignorando saídas curtas e descartando trechos curtos."""
    dentro = mapa.dentro(sx, sy)
    faixas, i = [], 0
    n = len(dentro)
    while i < n:
        j = i
        while j < n and dentro[j] == dentro[i]:
            j += 1
        faixas.append([bool(dentro[i]), i, j])
        i = j
    # saídas curtas entre dois pedaços dentro contam como dentro
    for k in range(1, len(faixas) - 1):
        f = faixas[k]
        if not f[0] and s[f[2] - 1] - s[f[1]] < FORA_CURTO_M:
            f[0] = True
    out = []
    for d, a, b in faixas:
        if d and out and out[-1][1] == a:
            out[-1][1] = b
        elif d:
            out.append([a, b])
    return [(a, b) for a, b in out if s[b - 1] - s[a] >= TRECHO_MIN_M]


def rumos(x, y):
    dx = np.gradient(x)
    dy = np.gradient(y)
    return np.degrees(np.arctan2(dy, dx))


def candidatos(mapa: Mapa, x, y):
    """Por amostra: {(way, sentido): distância} dos ways a até RAIO_M com rumo compatível."""
    pts = shapely.points(x, y)
    ip, iq = mapa.tree.query(pts, predicate="dwithin", distance=RAIO_M)
    seg = mapa.p[iq]
    ax, ay = seg[:, 0], seg[:, 1]
    vx, vy = seg[:, 2] - ax, seg[:, 3] - ay
    t = np.clip(((x[ip] - ax) * vx + (y[ip] - ay) * vy) / np.maximum(vx * vx + vy * vy, 1e-9), 0, 1)
    dist = np.hypot(ax + t * vx - x[ip], ay + t * vy - y[ip])
    r = rumos(x, y)
    dif = (r[ip] - mapa.pang[iq] + 180) % 360 - 180
    cands = [dict() for _ in range(len(x))]
    for p, q, d, a in zip(ip.tolist(), iq.tolist(), dist.tolist(), dif.tolist()):
        w = int(mapa.pw[q])
        su = mapa.sentido[w]
        if abs(a) <= RUMO_MAX:
            s = 1
        elif abs(a) >= 180 - RUMO_MAX:
            s = -1
        else:
            continue
        if su not in (0, s):
            continue
        c = cands[p]
        if d < c.get((w, s), 1e9):
            c[(w, s)] = d
    return cands


def casar(mapa: Mapa, x, y):
    """Viterbi sobre os ways candidatos. Devolve (ways com sinal = sentido, lacunas (índices em ways), fração casada,
    nº de ligações pelo menor caminho, lacunas como (way, ponto, way, ponto))."""
    cands = candidatos(mapa, x, y)
    obs = [i for i, c in enumerate(cands) if c]
    if not obs:
        return [], [], 0.0, 0, []
    estados = [list(cands[i].items()) for i in obs]
    custo = [d * C_DIST for _, d in estados[0]]
    volta = []
    for k in range(1, len(estados)):
        novo, vk = [], []
        for (w, s), d in estados[k]:
            melhor, arg = 1e18, 0
            for j, ((wa, sa), _) in enumerate(estados[k - 1]):
                if wa == w:
                    tr = 0.0 if sa == s else C_RETORNO
                elif mapa.vizinhos(wa, w):
                    tr = C_VIZINHO
                else:
                    tr = C_LACUNA
                c = custo[j] + tr
                if c < melhor:
                    melhor, arg = c, j
            novo.append(melhor + d * C_DIST)
            vk.append(arg)
        custo = novo
        volta.append(vk)
    j = int(np.argmin(custo))
    cam = [(estados[-1][j][0], obs[-1])]
    for k in range(len(volta) - 1, -1, -1):
        j = volta[k][j]
        cam.append((estados[k][j][0], obs[k]))
    cam.reverse()
    # corridas (way, sentido) com a primeira e a última amostra
    seq = []
    for e, i in cam:
        if seq and seq[-1][0] == e:
            seq[-1][2] = i
        else:
            seq.append([e, i, i])
    # ligações: menor caminho no mapa entre o fim de uma corrida e o começo da seguinte, ou lacuna
    ways, lacunas, pontes, sem_ponte = [seq[0][0]], [], 0, []
    for k in range(1, len(seq)):
        (a, _), _, ia = seq[k - 1]
        (b, sb), ib, _ = seq[k]
        if a != b and not mapa.vizinhos(a, b):
            ponte = mapa.caminho(a, (x[ia], y[ia]), b, (x[ib], y[ib]))
            if ponte is None:
                lacunas.append(len(ways))
                sem_ponte.append((a, (x[ia], y[ia]), b, (x[ib], y[ib])))
            else:
                pontes += 1
                for e in ponte:
                    if ways[-1] != e:
                        ways.append(e)
        if ways[-1] != (b, sb):
            ways.append((b, sb))
    return [mapa.ids[w] * s for w, s in ways], lacunas, len(obs) / len(x), pontes, sem_ponte


def projetar(x, y, s, px, py, desde):
    """Distância ao longo da polilinha da parada (px, py), a partir de `desde` (paradas em ordem); (dist, afastamento)."""
    ax, ay, bx, by = x[:-1], y[:-1], x[1:], y[1:]
    vx, vy = bx - ax, by - ay
    t = np.clip(((px - ax) * vx + (py - ay) * vy) / np.maximum(vx * vx + vy * vy, 1e-9), 0, 1)
    d = np.hypot(ax + t * vx - px, ay + t * vy - py)
    pos = s[:-1] + t * np.diff(s)
    ok = pos >= desde - 1
    if not ok.any():
        return None, None
    perto = np.flatnonzero(ok & (d <= PARADA_RAIO_M))
    i = perto[0] if len(perto) else np.flatnonzero(ok)[np.argmin(d[ok])]
    # o primeiro ponto perto pode estar no começo da aproximação: desce até o mínimo local
    while i + 1 < len(d) and ok[i + 1] and d[i + 1] < d[i]:
        i += 1
    return max(float(pos[i]), desde), float(d[i])


# ---------------------------------------------------------------- montagem

def ler_paradas_osm(path: Path):
    """Paradas do OSM (Overpass): pontos (x, y) das baias (`bus_bay` diferente de no), pontos e nomes das que têm
    nome, e o total de elementos."""
    els = json.loads(path.read_text())["elements"]
    baias, nomes = [], []
    for e in els:
        t = e.get("tags", {})
        if e["type"] != "node":
            continue
        if t.get("bus_bay", "no") not in ("no", ""):
            baias.append(xy(e["lat"], e["lon"]))
        if t.get("name"):
            nomes.append((*xy(e["lat"], e["lon"]), t["name"]))
    return np.array(baias, dtype=float).reshape(-1, 2), nomes, len(els)


def nome_parada(p, nomes, nx, ny):
    """Nome do GTFS sem o código; quando o GTFS só traz o código, o da parada do OSM mais próxima (até 30 m)."""
    nome = p["stop_name"].rsplit(" - ", 1)[0].strip()
    if nome and nome != p["stop_id"] and not nome.isdigit():
        return nome
    if not nomes:
        return None
    px, py = xy(float(p["stop_lat"]), float(p["stop_lon"]))
    d = np.hypot(nx - px, ny - py)
    i = int(np.argmin(d))
    return nomes[i][2] if d[i] <= OSM_PARADA_RAIO_M else None


def montar(g, mapa: Mapa, baias, nomes_osm):
    paradas_gtfs = g["paradas"]
    por_shape = defaultdict(list)
    for tid, v in g["viagens"].items():
        if g["horarios"].get(tid):
            por_shape[v["shape_id"]].append(tid)
    linhas = {}
    usadas = Counter()
    stats = []
    buracos = {}
    for sid, tids in sorted(por_shape.items()):
        pts = g["shapes"].get(sid)
        if not pts or len(pts) < 2:
            continue
        tids.sort(key=lambda t: g["horarios"][t][0][2])
        rep = max(tids, key=lambda t: len(g["horarios"][t]))  # viagem com mais paradas
        v0 = g["viagens"][rep]
        rota = g["rotas"][v0["route_id"]]
        X, Y = xy([p[1] for p in pts], [p[2] for p in pts])
        sx, sy, ss = densificar(X, Y, PASSO_M)
        faixas = recortar(mapa, sx, sy, ss)
        st = {"linha": rota["route_short_name"], "padrao": sid, "sentido": v0["direction_id"],
              "destino": v0["trip_headsign"], "km_total": round(ss[-1] / 1000, 2), "trechos": len(faixas),
              "km_no_mapa": 0.0, "casado": 1.0, "lacunas": 0, "pontes": 0, "paradas": 0, "partidas_dia": 0, f"onibus_{HORA_PICO}h": 0}
        stats.append(st)
        if not faixas:
            continue
        # paradas da viagem representativa, projetadas no shape inteiro, em ordem
        seq = g["horarios"][rep]
        proj, desde = [], 0.0
        for _, stop, _ in seq:
            p = paradas_gtfs.get(stop)
            if p is None:
                proj.append(None)
                continue
            px, py = xy(float(p["stop_lat"]), float(p["stop_lon"]))
            pos, afast = projetar(sx, sy, ss, float(px), float(py), desde)
            if pos is None or afast > PARADA_RAIO_M:
                proj.append(None)
                continue
            proj.append(pos)
            desde = pos
        trechos = []
        for a, b in faixas:
            tx, ty, ts = sx[a:b], sy[a:b], ss[a:b]
            ways, lacunas, frac, pontes, sem_ponte = casar(mapa, tx, ty)
            for a_, p_, b_, q_ in sem_ponte:
                buracos.setdefault((a_, b_), [p_, q_, set()])[2].add(sid)
            s0, s1 = ts[0], ts[-1]
            par = []
            for (_, stop, _), pos in zip(seq, proj):
                if pos is not None and s0 <= pos <= s1:
                    par.append([stop, int(round(pos - s0))])
                    usadas[stop] += 1
            # entrada no trecho: interpola os horários das paradas em volta de s0 (pelo comprimento do shape)
            conhecidos = [(pos, k) for k, pos in enumerate(proj) if pos is not None]
            partidas = []
            for tid in tids:
                h = g["horarios"][tid]
                if len(h) != len(seq):
                    # viagem com outra sequência de paradas: desloca pela 1ª parada
                    t = h[0][2] + entrada_rel(conhecidos, g["horarios"][rep], s0)
                else:
                    t = entrada(conhecidos, h, s0)
                partidas.append(int(round(t)) % 86400)
            partidas.sort()
            lin = LineString(np.column_stack([tx, ty])).simplify(SIMPLIFICA_M)
            la, lo = latlon(*np.array(lin.coords).T)
            trechos.append({"ways": ways, "pontos": [[round(float(p), 5), round(float(q), 5)] for p, q in zip(la, lo)],
                            "lacunas": lacunas, "entra_na_borda": a > 0, "sai_na_borda": b < len(sx),
                            "paradas": par, "partidas": partidas,
                            "_casado": frac, "_km": (s1 - s0) / 1000, "_pontes": pontes})
        st["km_no_mapa"] = round(sum(t["_km"] for t in trechos), 2)
        st["casado"] = round(min(t["_casado"] for t in trechos), 3)
        st["lacunas"] = sum(len(t["lacunas"]) for t in trechos)
        st["pontes"] = sum(t["_pontes"] for t in trechos)
        st["paradas"] = sum(len(t["paradas"]) for t in trechos)
        st["partidas_dia"] = len(trechos[0]["partidas"])
        st[f"onibus_{HORA_PICO}h"] = sum(t // 3600 == HORA_PICO for t in trechos[0]["partidas"])
        st["continuo"] = st["lacunas"] == 0
        for t in trechos:
            del t["_casado"], t["_km"], t["_pontes"]
        rid = v0["route_id"]
        if rid not in linhas:
            linhas[rid] = {"id": rid, "numero": rota["route_short_name"], "nome": rota["route_long_name"],
                           "operadora": rota["agency_id"], "padroes": []}
        linhas[rid]["padroes"].append({"id": sid, "sentido": int(v0["direction_id"] or 0),
                                       "destino": v0["trip_headsign"], "trechos": trechos})
    paradas = {}
    nx = np.array([n[0] for n in nomes_osm])
    ny = np.array([n[1] for n in nomes_osm])
    for stop in sorted(usadas):
        p = paradas_gtfs[stop]
        px, py = xy(float(p["stop_lat"]), float(p["stop_lon"]))
        baia = bool(len(baias)) and float(np.min(np.hypot(baias[:, 0] - px, baias[:, 1] - py))) <= BAIA_RAIO_M
        paradas[stop] = {"nome": nome_parada(p, nomes_osm, nx, ny), "lat": round(float(p["stop_lat"]), 6),
                         "lon": round(float(p["stop_lon"]), 6), "baia": baia}
    return sorted(linhas.values(), key=lambda l: l["id"]), paradas, stats, usadas, buracos


def entrada(conhecidos, h, s0):
    """Horário em que a viagem passa na distância s0 do shape, interpolando entre as paradas vizinhas."""
    antes = [(p, k) for p, k in conhecidos if p <= s0]
    depois = [(p, k) for p, k in conhecidos if p >= s0]
    if antes and depois:
        (pa, ka), (pb, kb) = antes[-1], depois[0]
        if pb > pa:
            return h[ka][2] + (h[kb][2] - h[ka][2]) * (s0 - pa) / (pb - pa)
        return h[ka][2]
    if depois:  # antes da 1ª parada: o terminal
        return h[depois[0][1]][2] if depois[0][0] - s0 > 50 else h[0][2]
    return h[antes[-1][1]][2]


def entrada_rel(conhecidos, h_rep, s0):
    return entrada(conhecidos, h_rep, s0) - h_rep[0][2]


# ---------------------------------------------------------------- saída

def validar(dados, mapa: Mapa):
    for l in dados["linhas"]:
        for p in l["padroes"]:
            for t in p["trechos"]:
                assert t["partidas"] == sorted(t["partidas"]), p["id"]
                assert all(0 <= t_ < 86400 for t_ in t["partidas"]), p["id"]
                ds = [d for _, d in t["paradas"]]
                assert ds == sorted(ds), p["id"]
                assert all(s in dados["paradas"] for s, _ in t["paradas"]), p["id"]
                assert all(abs(w) in mapa.idx for w in t["ways"]), p["id"]
                ks = [mapa.idx[abs(w)] for w in t["ways"]]
                assert all(mapa.sentido[k] in (0, 1 if w > 0 else -1) for k, w in zip(ks, t["ways"])), p["id"]
                lac = set(t["lacunas"])
                assert all(i in lac or mapa.vizinhos(ks[i - 1], ks[i]) for i in range(1, len(ks))), p["id"]


def tabela_md(linhas, cols):
    out = ["| " + " | ".join(cols) + " |", "|" + "---|" * len(cols)]
    out += ["| " + " | ".join(str(r.get(c, "")) for c in cols) + " |" for r in linhas]
    return "\n".join(out)


def classificar_lacunas(mapa: Mapa, buracos):
    """Uma linha por par de ways sem ligação, com o motivo provável e os padrões afetados (mais afetados primeiro)."""
    out = []
    for (a, b), (p, q, pads) in buracos.items():
        if mapa.caminho(a, p, b, q, teto=DESVIO_M) is not None:
            motivo = "desvio longo (conversão que o mapa não permite)"
        elif mapa.caminho(a, p, b, q, teto=DESVIO_M, dirigido=False) is not None:
            motivo = "só contra a mão única"
        else:
            motivo = "sem ligação no mapa"
        la, lo = latlon((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
        out.append({"de_way": mapa.ids[a], "para_way": mapa.ids[b], "motivo": motivo,
                    "distancia_m": round(math.hypot(q[0] - p[0], q[1] - p[1])), "padroes": len(pads),
                    "lat": round(float(la), 6), "lon": round(float(lo), 6), "exemplo": min(pads)})
    out.sort(key=lambda r: (-r["padroes"], r["de_way"]))
    return out


def relatorio(saida: Path, g, dados, stats, usadas, mapa: Mapa, n_osm_paradas, lacunas):
    no_mapa = [s for s in stats if s["trechos"]]
    linhas_mapa = {s["linha"] for s in no_mapa}
    cont = [s for s in no_mapa if s.get("continuo")]
    motivos = Counter(r["motivo"] for r in lacunas)
    paradas = dados["paradas"]
    baias = sum(p["baia"] for p in paradas.values())
    # paradas OSM do mapa sem parada GTFS por perto
    gx, gy = xy([p["lat"] for p in paradas.values()], [p["lon"] for p in paradas.values()])
    sem = []
    for oid, la, lo, nome in mapa.paradas_osm:
        ox, oy = xy(la, lo)
        if not len(gx) or float(np.min(np.hypot(gx - ox, gy - oy))) > OSM_PARADA_RAIO_M:
            sem.append(oid)
    # paradas GTFS dentro do mapa que nenhum padrão do dia usa
    todas = g["paradas"]
    tx, ty = xy([float(p["stop_lat"]) for p in todas.values()], [float(p["stop_lon"]) for p in todas.values()])
    dentro = mapa.dentro(tx, ty)
    ids = list(todas)
    no_mapa_ids = [ids[i] for i in np.flatnonzero(dentro) if todas[ids[i]].get("location_type", "0") in ("", "0")]
    sem_padrao = [s for s in no_mapa_ids if s not in usadas]
    por_hora = [0] * 24
    for l in dados["linhas"]:
        for p in l["padroes"]:
            for t in p["trechos"]:
                for t_ in t["partidas"]:
                    por_hora[t_ // 3600] += 1
    L = ["# Relatório — linhas de ônibus do Recife", "",
         f"Gerado por `scripts/onibus_recife/gerar_onibus.py`. Fonte: [GTFS do Grande Recife Consórcio de Transporte]"
         f"({GTFS_PAGINA}) `{g['versao']}`; dia de referência **{g['dia']:%Y-%m-%d}** (quarta-feira, dia útil sem "
         f"exceção no calendário). Mapa: `public/data/mapa-recife.osm.gz` (OSM `{mapa.osm_base}`, ODbL).", "",
         "## Números principais", "",
         f"- Rotas no GTFS: {g['total_rotas']} ({g['rotas_onibus']} de ônibus; metrô e VLT ficam de fora). "
         f"Com viagem no dia de referência: {len({v['route_id'] for v in g['viagens'].values()})}.",
         f"- Linhas com algum trecho no mapa do Recife: **{len(linhas_mapa)}**; padrões (shapes) no mapa: "
         f"**{len(no_mapa)}** de {len(stats)} com viagem no dia.",
         f"- Padrões com itinerário contínuo (sem lacuna): **{len(cont)}** ({len(cont) / max(1, len(no_mapa)):.0%}). "
         f"Ligações refeitas pelo menor caminho no mapa (trecho sem amostra perto de via, como terminal ou rua "
         f"`service`): {sum(s.get('pontes', 0) for s in no_mapa)}.",
         f"- Lacunas: **{len(lacunas)}** pares de ways distintos, em {sum(s['lacunas'] for s in no_mapa)} ocorrências "
         f"({'; '.join(f'{m}: {n}' for m, n in motivos.most_common())}).",
         f"- Amostras do itinerário a até {RAIO_M:.0f} m de um way compatível (mediana por padrão): "
         f"{np.median([s['casado'] for s in no_mapa]):.0%}.",
         f"- Padrões que saem e voltam ao mapa (mais de um trecho): {sum(s['trechos'] > 1 for s in no_mapa)}.",
         f"- Ônibus entrando em trechos do mapa no dia: **{sum(por_hora)}**; às {HORA_PICO}h: **{por_hora[HORA_PICO]}**.",
         f"- Paradas usadas: **{len(paradas)}** (baias pelo OSM: {baias}). Paradas GTFS dentro do mapa sem padrão no "
         f"dia: {len(sem_padrao)}. Sem nome (o GTFS só traz o código e não há parada do OSM com nome a até "
         f"{OSM_PARADA_RAIO_M:.0f} m): {sum(p['nome'] is None for p in paradas.values())}.",
         f"- Paradas do OSM no mapa (`highway=bus_stop`): {len(mapa.paradas_osm)}; sem parada GTFS a até "
         f"{OSM_PARADA_RAIO_M:.0f} m: {len(sem)}. Elementos de parada no Overpass (Recife): {n_osm_paradas}.",
         "", "## Ônibus por hora no mapa", "",
         tabela_md([{"hora": f"{h}h", "ônibus": n} for h, n in enumerate(por_hora)], ["hora", "ônibus"]),
         "", "## Lacunas", "",
         "Dois ways seguidos do itinerário que o mapa não liga por um caminho de até "
         f"{PONTE_FATOR:.0f}× a distância reta + {PONTE_FOLGA_M:.0f} m. *Desvio longo*: o mapa liga, mas por outro "
         "caminho (conversão proibida ou ausente). *Só contra a mão única*: o ônibus passa onde o OSM marca mão única "
         "(o núcleo não lê exceção para ônibus). *Sem ligação no mapa*: via que o núcleo não carrega (`service`, "
         "terminal) ou que falta no OSM. São os candidatos a `ajustes_manuais.json`; lista completa em `lacunas.csv`.", "",
         tabela_md(lacunas[:30], ["de_way", "para_way", "motivo", "distancia_m", "padroes", "exemplo", "lat", "lon"]), ""]
    (saida / "relatorio_onibus.md").write_text("\n".join(L))


def gravar_csv(path: Path, linhas, cols):
    with path.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(linhas)


def inspecao(path: Path, dados, mapa: Mapa, numeros):
    feats = []
    for l in dados["linhas"]:
        if l["numero"] not in numeros:
            continue
        for p in l["padroes"]:
            for i, t in enumerate(p["trechos"]):
                feats.append({"type": "Feature", "properties": {"linha": l["numero"], "padrao": p["id"], "trecho": i,
                                                                 "tipo": "shape"},
                              "geometry": {"type": "LineString", "coordinates": [[b, a] for a, b in t["pontos"]]}})
                for w in t["ways"]:
                    w, s = abs(w), (1 if w > 0 else -1)
                    wx, wy = mapa.xy[mapa.idx[w]]
                    la, lo = latlon(wx, wy)
                    feats.append({"type": "Feature", "properties": {"linha": l["numero"], "padrao": p["id"],
                                                                     "tipo": "way", "way": w, "sentido": s},
                                  "geometry": {"type": "LineString",
                                               "coordinates": [[round(float(b), 6), round(float(a), 6)] for a, b in zip(la, lo)]}})
                for stop, _ in t["paradas"]:
                    q = dados["paradas"][stop]
                    feats.append({"type": "Feature", "properties": {"linha": l["numero"], "tipo": "parada", "id": stop,
                                                                     "nome": q["nome"], "baia": q["baia"]},
                                  "geometry": {"type": "Point", "coordinates": [q["lon"], q["lat"]]}})
    path.write_text(json.dumps({"type": "FeatureCollection", "features": feats}, ensure_ascii=False))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--saida", type=Path, default=Path("data/onibus_recife"))
    ap.add_argument("--brutos", type=Path, default=None)
    ap.add_argument("--app", type=Path, default=Path("public/data/onibus_recife.json"), help="JSON lido pelo simulador")
    ap.add_argument("--atualizar", action="store_true", help="baixa de novo o GTFS e refaz a consulta Overpass")
    ap.add_argument("--inspecionar", default="", help="números de linha (vírgula) para o GeoJSON de inspeção")
    ap.add_argument("--inspecao", type=Path, default=None, help="GeoJSON de inspeção (padrão: <saida>/inspecao.geojson)")
    a = ap.parse_args()
    brutos = a.brutos or a.saida / "_brutos"
    a.saida.mkdir(parents=True, exist_ok=True)

    baixar(brutos, a.atualizar)
    print("lendo o mapa ...", file=sys.stderr)
    mapa = Mapa(MAPA)
    g = ler_gtfs(brutos / "gtfs.zip")
    baias, nomes_osm, n_osm = ler_paradas_osm(brutos / "osm_paradas.json")
    print("recortando e casando os itinerários ...", file=sys.stderr)
    linhas, paradas, stats, usadas, buracos = montar(g, mapa, baias, nomes_osm)
    dados = {"fonte": {"gtfs": g["versao"], "dia_referencia": f"{g['dia']:%Y-%m-%d}", "mapa_osm_base": mapa.osm_base},
             "paradas": paradas, "linhas": linhas}

    sys.path.insert(0, str(Path(__file__).parent))
    from aplicar_ajustes import aplicar
    aplicar(dados)
    validar(dados, mapa)
    a.app.parent.mkdir(parents=True, exist_ok=True)
    a.app.write_text(json.dumps(dados, ensure_ascii=False, separators=(",", ":")))

    gravar_csv(a.saida / "padroes.csv", stats,
               ["linha", "padrao", "sentido", "destino", "km_total", "km_no_mapa", "trechos", "casado", "lacunas",
                "pontes", "continuo", "paradas", "partidas_dia", f"onibus_{HORA_PICO}h"])
    gravar_csv(a.saida / "paradas.csv",
               [{"id": k, **v, "padroes": usadas[k]} for k, v in paradas.items()],
               ["id", "nome", "lat", "lon", "baia", "padroes"])
    lacunas = classificar_lacunas(mapa, buracos)
    gravar_csv(a.saida / "lacunas.csv", lacunas,
               ["de_way", "para_way", "motivo", "distancia_m", "padroes", "exemplo", "lat", "lon"])
    relatorio(a.saida, g, dados, stats, usadas, mapa, n_osm, lacunas)
    if a.inspecionar:
        inspecao(a.inspecao or a.saida / "inspecao.geojson", dados, mapa, set(a.inspecionar.split(",")))
    n_pad = sum(len(l["padroes"]) for l in linhas)
    print(f"ok: {len(linhas)} linhas, {n_pad} padrões, {len(paradas)} paradas -> {a.app} "
          f"({a.app.stat().st_size / 1e6:.2f} MB)", file=sys.stderr)


if __name__ == "__main__":
    main()
