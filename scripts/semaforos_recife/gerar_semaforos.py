#!/usr/bin/env python3
"""Base georreferenciada dos semáforos do Recife para o simulador.

Baixa a base oficial da CTTU (Portal de Dados Abertos do Recife), os limites oficiais
de bairros e, como fonte complementar, semáforos e vias do OpenStreetMap (Overpass).
Audita, normaliza, agrupa por interseção, compara com o OSM e lista os parâmetros de
simulação que não são públicos. Nada aqui inventa tempos, fases ou sentidos.

Uso:
    python scripts/semaforos_recife/gerar_semaforos.py [--saida data/semaforos_recife]
        [--brutos data/semaforos_recife/_brutos] [--atualizar-osm]

Dependências: pandas, geopandas, shapely, requests (ver requirements.txt).
"""
import argparse
import difflib
import json
import re
import sys
import time
import unicodedata
from collections import Counter
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd
import requests
from shapely.geometry import LineString, Point

CKAN = "https://dados.recife.pe.gov.br/api/3/action/package_show?id={}"
DATASET = "localizacao-dos-semaforos"
DATASET_URL = f"https://dados.recife.pe.gov.br/dataset/{DATASET}"
BAIRROS_DATASET = "bairros"
BAIRROS_URL = ("https://dados.recife.pe.gov.br/dataset/1702c764-ec7a-4ef0-9861-8acd0e40c819/resource/"
               "5c67ce14-1799-40c4-a37c-9daa04d1761c/download/bairros-do-recife.geojson")
OVERPASS = ["https://overpass-api.de/api/interpreter", "https://overpass.kumi.systems/api/interpreter"]
UA = "simulador-transito-recife/0.1 (processamento de dados abertos)"
AREA_RECIFE = 'area["boundary"="administrative"]["admin_level"="8"]["name"="Recife"]["IBGE:GEOCODIGO"="2611606"]->.a;'
Q_SINAIS = f'[out:json][timeout:180];{AREA_RECIFE}(node["highway"="traffic_signals"](area.a);node["crossing"="traffic_signals"](area.a););out body;'
Q_VIAS = (f'[out:json][timeout:300];{AREA_RECIFE}way["highway"~"^(motorway|trunk|primary|secondary|tertiary|'
          'unclassified|residential|living_street|service|motorway_link|trunk_link|primary_link|secondary_link|'
          'tertiary_link|busway)$"](area.a);out tags geom;')
CRS_METRICO = "EPSG:31985"  # SIRGAS 2000 / UTM 24S

# Recursos do dataset (nome no CKAN -> arquivo local)
RECURSOS = {
    "Semáforos do Recife": "semaforos-do-recife.csv",
    "Semáforos equipados com sinalizador para ciclistas": "semaforos-ciclistas.csv",
    "Semáforos com iluminação suspensa para pedestres": "semaforos-iluminacao-pedestres.csv",
    "Semáforos equipados para deficientes visuais": "semaforos-deficientes-visuais.csv",
}
FONTE_CTTU = "CTTU"

# Agrupamento por interseção
RAIO_PROXIMO_M = 25.0      # liga quaisquer dois semáforos a até 25 m
RAIO_MESMAS_VIAS_M = 60.0  # liga até 60 m se citarem o mesmo par de vias
DIAMETRO_REVISAR_M = 80.0
QTD_REVISAR = 5
# Comparação com o OSM
OSM_PROVAVEL_M, OSM_DIVERGENTE_M = 15.0, 40.0
# Vias do OSM procuradas em torno de cada ponto
RAIO_VIAS_M = 60.0
VIA_ALTA_M, VIA_SEC_ALTA_M = 30.0, 50.0

TIPO_CONTROLE = {
    "E/GIT": "Eletrônico (CES) integrado ao GIT – Gerenciador Inteligente de Trânsito",
    "E": "Eletrônico (CES)",
    "E/DIG": "não documentado no dicionário (E/DIG)",
}
TIPO_SEMAFORO = {  # destinação publicada -> tipo_semaforo
    "veicular": "veicular",
    "veic.c/laco det.": "veicular",
    "ocasional/ped.": "pedestre",
    "pedestre": "pedestre",
    "veic.c/ped.": "misto",
    "veic.c/ped.ocas.": "misto",
    "veic.c/ped.paral.": "misto",
}
FUNCIONAMENTO_DESC = {
    "veicular": "Veicular",
    "veic.c/laco det.": "Veicular com laço detector",
    "ocasional/ped.": "Ocasional de pedestre",
    "pedestre": "Pedestre",
    "veic.c/ped.": "Veicular com pedestre",
    "veic.c/ped.ocas.": "Veicular com pedestre ocasional",
    "veic.c/ped.paral.": "Veicular com pedestre paralelo",
}

# Nomes de vias: abreviações usadas pela CTTU e palavras ignoradas na comparação
ABREV = {
    "av": "avenida", "avn": "avenida", "r": "rua", "est": "estrada", "estr": "estrada", "trav": "travessa",
    "tv": "travessa", "pc": "praca", "pca": "praca", "lg": "largo", "rod": "rodovia", "gov": "governador",
    "gal": "general", "gen": "general", "cons": "conselheiro", "consel": "conselheiro", "masc": "mascarenhas",
    "prof": "professor", "profa": "professora", "dr": "doutor", "dra": "doutora", "sto": "santo",
    "sta": "santa", "s": "sao", "eng": "engenheiro", "des": "desembargador", "pe": "padre", "mal": "marechal",
    "alm": "almirante", "cel": "coronel", "cap": "capitao", "ten": "tenente", "pres": "presidente",
    "dep": "deputado", "sen": "senador", "min": "ministro", "ver": "vereador", "maj": "major", "brig": "brigadeiro",
    "cmte": "comandante", "com": "comendador", "visc": "visconde", "bar": "barao", "jorn": "jornalista",
    "n": "nossa", "sra": "senhora", "nsa": "nossa", "d": "dom", "frei": "frei", "cde": "conde",
}
TIPOS_VIA = {"rua", "avenida", "estrada", "travessa", "praca", "largo", "ponte", "viaduto", "cais", "rodovia",
             "tunel", "alameda", "beco", "via", "ladeira", "vila", "parque", "passarela", "acesso", "binario"}
STOP = {"de", "da", "do", "das", "dos", "e", "a", "o"}
_UNID = "um dois tres quatro cinco seis sete oito nove".split()
_NUM = {str(i + 1): [w] for i, w in enumerate(_UNID)}
_NUM |= {str(10 + i): [w] for i, w in enumerate("dez onze doze treze quatorze quinze dezesseis dezessete dezoito dezenove".split())}
_NUM |= {str(d * 10 + u): [nome] + ([_UNID[u - 1]] if u else []) for d, nome in ((2, "vinte"), (3, "trinta")) for u in range(10)}
PREFIXO_VIA = re.compile(r"^(rua|r\.|av\b|av\.|avenida|estrada|est\b|est\.|travessa|trav|pra[cç]a|largo|ponte|"
                         r"viaduto|cais|rodovia|br[- ]?\d|pe[- ]?\d|t[uú]nel|alameda|beco|ladeira|via\b|vila)",
                         re.I)
REFERENCIA = re.compile(r"(em frente|frente|entrada|sa[ií]da|pr[oó]x|esta[cç][aã]o|est\.? brt|n[ºo°]\s*\d|"
                        r"sentido|altura|acesso|antes|ap[oó]s|depois|retorno|faixa|travessia|meio)", re.I)


def sem_acento(s: str) -> str:
    return "".join(c for c in unicodedata.normalize("NFKD", s) if not unicodedata.combining(c))


def chave(s) -> str:
    """Texto em minúsculas, sem acento e com espaços simples (para comparar rótulos)."""
    if not isinstance(s, str):
        return ""
    return re.sub(r"\s+", " ", sem_acento(s).lower()).strip()


def tokens_via(nome) -> list[str]:
    """Tokens do nome-núcleo da via (sem tipo, sem preposições, abreviações expandidas)."""
    s = chave(nome).replace(".", ". ").replace("/", " ").replace("-", " ")
    out = []
    for t in re.findall(r"[a-z0-9]+", s):
        t = ABREV.get(t, t)
        if t in TIPOS_VIA or t in STOP:
            continue
        out.extend(_NUM.get(t, [t]))
    return out


def _fonetico(t: str) -> str:
    """Variantes de grafia comuns em nomes de rua (Morais/Moraes, Góis/Góes, Manoel/Manuel, Cleóphas)."""
    t = t.replace("ph", "f").replace("th", "t").replace("y", "i").replace("w", "v")
    t = t.replace("e", "i").replace("o", "u")
    return re.sub(r"(.)\1+", r"\1", t)


def _tok_igual(a: str, b: str) -> bool:
    if a == b:
        return True
    a, b = _fonetico(a), _fonetico(b)
    if a == b:
        return True
    if min(len(a), len(b)) >= 3 and (a.startswith(b) or b.startswith(a)):  # "masc." vs "mascarenhas"
        return True
    return len(a) > 4 and len(b) > 4 and difflib.SequenceMatcher(None, a, b).ratio() >= 0.85


def mesma_via(a, b) -> bool:
    """Heurística: os nomes a e b designam a mesma via?"""
    ta, tb = tokens_via(a), tokens_via(b)
    if not ta or not tb:
        return False
    comuns = sum(any(_tok_igual(x, y) for y in tb) for x in ta)
    menor = min(len(ta), len(tb))
    # nomes de um só token ("Rua Real") exigem igualdade do token; demais, 2/3 dos tokens do menor
    return comuns >= 1 if menor == 1 and max(len(ta), len(tb)) <= 2 else comuns / menor >= 0.66 and comuns >= min(2, menor)


def _txt(v):
    return v if isinstance(v, str) and v else None


def limpa(s):
    if not isinstance(s, str):
        return None
    s = re.sub(r"\s+", " ", s).strip()
    return s or None


def via_padronizada(s):
    """Só espaçamento e pontuação; o texto oficial não é reescrito."""
    s = limpa(s)
    if s is None:
        return None
    return re.sub(r"(?<=\w)\(", " (", re.sub(r"\.(?=\S)", ". ", s)).replace(" ,", ",")


# ---------------------------------------------------------------- downloads

def _get(url, dest: Path, **kw):
    r = requests.get(url, headers={"User-Agent": UA}, timeout=300, **kw)
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


def baixar(brutos: Path, atualizar_osm: bool) -> dict:
    brutos.mkdir(parents=True, exist_ok=True)
    meta_path = brutos / "metadados_ckan.json"
    if not meta_path.exists():
        print("baixando metadados do conjunto ...", file=sys.stderr)
        _get(CKAN.format(DATASET), meta_path)
    meta = json.loads(meta_path.read_text())["result"]
    for r in meta["resources"]:
        nome = RECURSOS.get(r["name"].strip()) or Path(r["url"]).name
        if r["format"].upper() == "JSON" or r["format"].upper() == "PDF":
            nome = Path(r["url"]).name
        dest = brutos / nome
        r["_arquivo"] = nome
        if not dest.exists():
            print(f"baixando {nome} ...", file=sys.stderr)
            _get(r["url"].replace(":443", ""), dest)
    meta_path.write_text(json.dumps({"result": meta}, ensure_ascii=False, indent=1))
    if not (brutos / "bairros.geojson").exists():
        print("baixando bairros.geojson ...", file=sys.stderr)
        _get(BAIRROS_URL, brutos / "bairros.geojson")
    print("consultando Overpass (semáforos) ...", file=sys.stderr)
    overpass(Q_SINAIS, brutos / "osm_semaforos.json", atualizar_osm)
    print("consultando Overpass (vias) ...", file=sys.stderr)
    overpass(Q_VIAS, brutos / "osm_vias.json", atualizar_osm)
    return meta


# ---------------------------------------------------------------- leitura e auditoria

def coord(v):
    """(valor, problema) a partir do texto publicado."""
    if not isinstance(v, str) or not v.strip():
        return np.nan, "ausente"
    s = v.strip()
    prob = None
    if s.endswith(","):
        s, prob = s.rstrip(","), "vírgula final removida"
    if re.fullmatch(r"-?\d+,\d+", s):
        s, prob = s.replace(",", "."), "vírgula decimal convertida"
    try:
        return float(s), prob
    except ValueError:
        return np.nan, f"não numérico ({v!r})"


def ler_oficial(path: Path) -> pd.DataFrame:
    df = pd.read_csv(path, sep=";", dtype=str, keep_default_na=False, na_values=[""])
    df.columns = [c.strip() for c in df.columns]
    df = df.rename(columns={"semafororo": "semaforo"})
    return df


def ler_bairros(brutos: Path) -> gpd.GeoDataFrame:
    b = gpd.read_file(brutos / "bairros.geojson")
    b = b[["EBAIRRNOMEOF", "CBAIRRCODI", "geometry"]].rename(columns={"EBAIRRNOMEOF": "bairro", "CBAIRRCODI": "cod_bairro"})
    return b.to_crs(CRS_METRICO)


def montar_oficial(brutos: Path, meta: dict, bairros: gpd.GeoDataFrame):
    arq = brutos / RECURSOS["Semáforos do Recife"]
    bruto = ler_oficial(arq)
    aud = {"arquivo": arq.name, "colunas": list(bruto.columns), "total": len(bruto)}
    df = pd.DataFrame({f"orig_{c}": bruto[c] for c in bruto.columns})

    lat = bruto["latitude"].map(coord)
    lon = bruto["longitude"].map(coord)
    df["latitude"] = [v for v, _ in lat]
    df["longitude"] = [v for v, _ in lon]
    probs = [", ".join(f"{eixo}: {p}" for eixo, (_, p) in (("latitude", a), ("longitude", o)) if p)
             for a, o in zip(lat, lon)]
    df["_prob_coord"] = probs
    faixa_ok = df.latitude.between(-8.20, -7.90) & df.longitude.between(-35.05, -34.80)
    df["_coord_valida"] = df.latitude.notna() & df.longitude.notna() & faixa_ok
    aud["coord_ausente"] = int((df.latitude.isna() | df.longitude.isna()).sum())
    aud["coord_corrigida"] = int(sum(bool(p) and "ausente" not in p and "não numérico" not in p for p in probs))
    aud["coord_fora_faixa"] = int((df.latitude.notna() & df.longitude.notna() & ~faixa_ok).sum())
    aud["ids_corrigidos"] = [(i, p) for i, p in zip(bruto["semaforo"], probs) if p]

    df["id_oficial"] = bruto["semaforo"].str.strip()
    aud["id_duplicado"] = int(df.id_oficial.duplicated(keep=False).sum())
    aud["id_vazio"] = int(df.id_oficial.isna().sum())
    ids_num = pd.to_numeric(df.id_oficial, errors="coerce")
    faltando = sorted(set(range(1, int(ids_num.max()) + 1)) - set(ids_num.dropna().astype(int)))
    aud["ids_ausentes_na_sequencia"] = faltando

    g = gpd.GeoDataFrame(df, geometry=gpd.points_from_xy(df.longitude, df.latitude), crs="EPSG:4326").to_crs(CRS_METRICO)
    g.loc[~g._coord_valida, "geometry"] = None

    # duplicatas: coordenada idêntica, texto idêntico e pontos quase coincidentes (< 3 m)
    validos = g[g._coord_valida]
    aud["coord_identica"] = int(validos.duplicated(["latitude", "longitude"], keep=False).sum())
    chave_txt = bruto["localizacao1"].map(chave) + "|" + bruto["localizacao2"].map(chave)
    dup_txt = chave_txt.duplicated(keep=False)
    df_dup_txt = df.loc[dup_txt, ["id_oficial"]].assign(k=chave_txt[dup_txt])
    aud["texto_identico_grupos"] = df_dup_txt.groupby("k").id_oficial.apply(list).tolist()
    perto = []
    idx = validos.sindex
    for i, geom in zip(validos.index, validos.geometry):
        for j in idx.query(geom.buffer(3.0)):
            jj = validos.index[j]
            if jj > i:
                perto.append((g.at[i, "id_oficial"], g.at[jj, "id_oficial"], round(geom.distance(g.at[jj, "geometry"]), 1)))
    aud["pares_menos_3m"] = perto

    # bairro: associação espacial com os limites oficiais
    sj = gpd.sjoin(g[g._coord_valida][["geometry"]], bairros, how="left", predicate="within")
    sj = sj[~sj.index.duplicated()]
    g["bairro"] = sj["bairro"]
    g["cod_bairro"] = sj["cod_bairro"]
    fora = g._coord_valida & g.bairro.isna()
    # pontos fora de todo polígono: bairro mais próximo, se a até 200 m (margem de rio/limite), senão fora do município
    g["_dist_bairro_m"] = 0.0
    for i in g.index[fora]:
        d = bairros.distance(g.at[i, "geometry"])
        k = d.idxmin()
        g.at[i, "_dist_bairro_m"] = round(float(d[k]), 1)
        if d[k] <= 200:
            g.at[i, "bairro"], g.at[i, "cod_bairro"] = bairros.at[k, "bairro"], bairros.at[k, "cod_bairro"]
    g["_fora_municipio"] = g._coord_valida & (g._dist_bairro_m > 200)
    aud["fora_poligonos_bairro"] = int(fora.sum())
    aud["fora_municipio"] = int(g._fora_municipio.sum())
    aud["fora_poligonos_ids"] = list(zip(g.loc[fora, "id_oficial"], g.loc[fora, "_dist_bairro_m"]))

    # bairro publicado x bairro espacial (grafias equivalentes não contam como divergência)
    def bairro_k(s):
        s = re.sub(r"^bairro d[oa] ", "", chave(s))
        s = re.sub(r"^sto\.? ", "santo ", re.sub(r"^sta\.? ", "santa ", s))
        return {"recife antigo": "recife"}.get(s, s)
    oficiais = {bairro_k(n): n for n in bairros.bairro}
    cttu_b = bruto["bairro"].map(bairro_k)
    g["_bairro_diverge"] = g.bairro.notna() & (cttu_b != g.bairro.map(bairro_k))
    motivo = []
    for i in g.index:
        if not g.at[i, "_bairro_diverge"]:
            motivo.append(None)
        elif cttu_b[i] not in oficiais:
            motivo.append("nome publicado não é bairro oficial")
        else:
            d = bairros.loc[bairros.bairro == oficiais[cttu_b[i]]].distance(g.at[i, "geometry"]).min()
            motivo.append(f"ponto a {d:.0f} m do bairro publicado")
    g["_bairro_motivo"] = motivo
    aud["bairro_divergente"] = int(g._bairro_diverge.sum())
    m = pd.Series(motivo).dropna()
    aud["bairro_divergente_nao_oficial"] = int(m.str.startswith("nome").sum())
    dists = m[m.str.startswith("ponto")].str.extract(r"(\d+)")[0].astype(float)
    aud["bairro_divergente_ate_100m"] = int((dists <= 100).sum())
    aud["bairro_divergente_mais_100m"] = int((dists > 100).sum())

    # tipo / funcionamento
    tipo_n = bruto["tipo"].map(lambda s: limpa(s).upper() if isinstance(s, str) else None)
    g["tipo_controle_codigo"] = tipo_n
    g["tipo_controle"] = tipo_n.map(TIPO_CONTROLE).fillna("não informado")
    func_k = bruto["funcionamento"].map(chave).str.replace("ç", "c")
    g["funcionamento_descricao"] = func_k.map(FUNCIONAMENTO_DESC).fillna("não informado")
    g["tipo_semaforo"] = func_k.map(TIPO_SEMAFORO).fillna("não informado")
    aud["valores_tipo"] = bruto["tipo"].value_counts(dropna=False).to_dict()
    aud["valores_funcionamento"] = bruto["funcionamento"].value_counts(dropna=False).to_dict()
    col_extra = [c for c in bruto.columns if c.lower().startswith("unnamed")]
    aud["colunas_nao_documentadas"] = {c: bruto[c].value_counts(dropna=False).to_dict() for c in col_extra}

    g["endereco_original"] = (bruto["localizacao1"].fillna("").str.strip() + " / " +
                              bruto["localizacao2"].fillna("").str.strip()).str.strip(" /")
    g["via_principal"] = bruto["localizacao1"].map(via_padronizada)
    loc2 = bruto["localizacao2"].map(via_padronizada)
    eh_via = loc2.map(lambda s: bool(s) and bool(PREFIXO_VIA.match(s)) and not REFERENCIA.search(s))
    g["via_secundaria"] = loc2.where(eh_via)
    g["ponto_referencia"] = loc2.where(~eh_via)
    g["intersecao"] = np.where(eh_via, g.via_principal + " × " + g.via_secundaria.fillna(""),
                               g.via_principal + " (" + loc2.fillna("sem referência") + ")")
    g["tipo_local"] = np.where(eh_via, "cruzamento", "travessia/meio de quadra")

    data_fonte = next((r.get("last_modified") or r.get("created") for r in meta["resources"]
                       if r["name"].strip() == "Semáforos do Recife"), meta.get("metadata_modified"))
    g["data_atualizacao_fonte"] = (data_fonte or "")[:10]
    return g, aud


def anexar_complementares(g: gpd.GeoDataFrame, brutos: Path, aud: dict):
    """Flags dos três recortes oficiais (sonoro, ciclista, iluminação)."""
    especs = {
        "sinal_sonoro_deficiente_visual": "Semáforos equipados para deficientes visuais",
        "sinalizador_ciclista": "Semáforos equipados com sinalizador para ciclistas",
        "iluminacao_suspensa_pedestre": "Semáforos com iluminação suspensa para pedestres",
    }
    pos = g.set_index("id_oficial")
    aud["complementares"] = {}
    g["_coord_alt"], g["_coord_alt_m"], g["_alt_xy"] = None, 0.0, None
    linha = dict(zip(g.id_oficial, g.index))
    for col, rec in especs.items():
        c = ler_oficial(brutos / RECURSOS[rec])
        ids = c["semaforo"].str.strip().str.zfill(3)
        g[col] = g.id_oficial.isin(set(ids))
        info = {"registros": len(c), "colunas": list(c.columns), "ids_sem_par_na_base": sorted(set(ids) - set(g.id_oficial))}
        # mesma coordenada que a base principal?
        difs = []
        for i, la, lo in zip(ids, c["latitude"], c["longitude"]):
            if i in pos.index and pos.at[i, "geometry"] is not None:
                y, _ = coord(la)
                x, _ = coord(lo)
                if not np.isnan(y) and not np.isnan(x):
                    p = gpd.GeoSeries([Point(x, y)], crs="EPSG:4326").to_crs(CRS_METRICO).iloc[0]
                    d = p.distance(pos.at[i, "geometry"])
                    if d > 5:
                        difs.append((i, round(d, 1)))
                        k = linha[i]
                        g.at[k, "_alt_xy"] = (y, x)
                        txt = f"recorte oficial '{rec}' publica coordenada a {d:.0f} m ({y:.6f}, {x:.6f})"
                        g.at[k, "_coord_alt"] = "; ".join(filter(None, [g.at[k, "_coord_alt"], txt]))
                        g.at[k, "_coord_alt_m"] = max(g.at[k, "_coord_alt_m"], d)
        info["coord_difere_da_base_mais_5m"] = difs
        info["virgula_decimal"] = int(c["longitude"].str.contains(",", na=False).sum() + c["latitude"].str.contains(",", na=False).sum())
        if "horario" in c.columns:
            h = dict(zip(ids, c["horario"].map(limpa)))
            g["horario_sinal_sonoro"] = g.id_oficial.map(h)
        aud["complementares"][rec] = info


# ---------------------------------------------------------------- OSM

def ler_osm(brutos: Path):
    s = json.loads((brutos / "osm_semaforos.json").read_text())
    rows = [{"osm_id": e["id"], "lat": e["lat"], "lon": e["lon"],
             "osm_tags": ";".join(f"{k}={v}" for k, v in sorted(e.get("tags", {}).items()))}
            for e in s["elements"] if e["type"] == "node"]
    sinais = gpd.GeoDataFrame(rows, geometry=gpd.points_from_xy([r["lon"] for r in rows], [r["lat"] for r in rows]),
                              crs="EPSG:4326").to_crs(CRS_METRICO)
    v = json.loads((brutos / "osm_vias.json").read_text())
    vrows, geoms = [], []
    for e in v["elements"]:
        t = e.get("tags", {})
        nomes = [t[k] for k in ("name", "official_name", "alt_name", "old_name", "short_name") if t.get(k)]
        if e["type"] != "way" or not nomes or len(e.get("geometry", [])) < 2:
            continue
        vrows.append({"way_id": e["id"], "nome": t.get("name") or nomes[0], "nomes": nomes,
                      "highway": t.get("highway"), "oneway": t.get("oneway")})
        geoms.append(LineString([(p["lon"], p["lat"]) for p in e["geometry"]]))
    vias = gpd.GeoDataFrame(vrows, geometry=geoms, crs="EPSG:4326").to_crs(CRS_METRICO)
    return sinais, vias, s["osm3s"]["timestamp_osm_base"]


def validar_vias(g: gpd.GeoDataFrame, vias: gpd.GeoDataFrame):
    """Confere os nomes oficiais com as vias do OSM próximas e define a confiança da localização."""
    sidx = vias.sindex
    osm1, osm2, d1s, d2s, conf, obs = [], [], [], [], [], []
    for _, r in g.iterrows():
        o = []
        if r._prob_coord:
            o.append(f"coordenada publicada com problema ({r._prob_coord})")
        if not r._coord_valida:
            osm1.append(None); osm2.append(None); d1s.append(None); d2s.append(None)
            conf.append("baixa"); obs.append("; ".join(o + ["sem coordenada válida"]))
            continue
        cand = vias.iloc[sidx.query(r.geometry.buffer(RAIO_VIAS_M))]
        dist = cand.distance(r.geometry).sort_values()
        nomes_prox = []
        for k in dist.index:
            n = cand.at[k, "nome"]
            if n not in nomes_prox:
                nomes_prox.append(n)
        osm1.append(nomes_prox[0] if nomes_prox else None)
        osm2.append(nomes_prox[1] if len(nomes_prox) > 1 else None)

        def dist_via(nome):
            if not nome:
                return None
            ok = [dist[k] for k in dist.index if any(mesma_via(nome, n) for n in cand.at[k, "nomes"])]
            return round(float(min(ok)), 1) if ok else None

        sec = _txt(r.via_secundaria)
        dp, ds = dist_via(_txt(r.via_principal)), dist_via(sec)
        d1s.append(dp); d2s.append(ds)
        if r._fora_municipio:
            c = "baixa"; o.append("fora do limite municipal")
        elif dp is None:
            c = "baixa"; o.append(f"via principal não encontrada no OSM a até {RAIO_VIAS_M:.0f} m")
        elif dp <= VIA_ALTA_M and (not sec or (ds is not None and ds <= VIA_SEC_ALTA_M)):
            c = "alta"
        else:
            c = "média"
            if dp > VIA_ALTA_M:
                o.append(f"via principal a {dp:.0f} m no OSM")
            if sec and ds is None:
                o.append(f"via secundária não encontrada no OSM a até {RAIO_VIAS_M:.0f} m")
            elif sec and ds > VIA_SEC_ALTA_M:
                o.append(f"via secundária a {ds:.0f} m no OSM")
        if r._prob_coord and c == "alta":
            c = "média"
        if r._bairro_diverge:
            o.append(f"bairro publicado '{r.orig_bairro}' ≠ bairro espacial '{r.bairro}' ({r._bairro_motivo})")
        if isinstance(r._coord_alt, str):
            o.append(r._coord_alt)
            if r._coord_alt_m > 100:
                c = "baixa"
        conf.append(c); obs.append("; ".join(o))
    g["via_osm_mais_proxima"] = osm1
    g["via_osm_segunda"] = osm2
    g["dist_via_principal_osm_m"] = d1s
    g["dist_via_secundaria_osm_m"] = d2s
    g["confianca_localizacao"] = conf
    g["observacoes"] = obs


def _tag(tags, k):
    m = re.search(rf"(?:^|;){re.escape(k)}=([^;]*)", tags or "")
    return m.group(1) if m else None


def _ref_igual(tags, id_oficial) -> bool:
    ref = _tag(tags, "ref")
    return bool(ref) and ref.strip().lstrip("0") == str(id_oficial).lstrip("0")


def _importado(tags) -> bool:
    """Nó criado a partir da própria base da Prefeitura (import de 2014): não é validação independente."""
    return "prefeitura do recife" in chave(_tag(tags, "source") or "")


def comparar_osm(g: gpd.GeoDataFrame, sinais: gpd.GeoDataFrame) -> pd.DataFrame:
    sidx = sinais.sindex
    por_ref = {}
    for _, s in sinais.iterrows():
        ref = _tag(s.osm_tags, "ref")
        if ref and _importado(s.osm_tags) and ref.strip().isdigit():
            por_ref.setdefault(ref.strip().lstrip("0"), []).append(s.geometry)
    linhas = []
    for _, r in g.iterrows():
        if not r._coord_valida:
            linhas.append({"id_oficial": r.id_oficial, "osm_id": None, "distancia_metros": None,
                           "classificacao": "sem coordenada oficial", "acao_recomendada": "corrigir coordenada na fonte"})
            continue
        cand = sinais.iloc[sidx.query(r.geometry.buffer(200))]
        if cand.empty:
            d, oid, n40, tags = None, None, 0, None
        else:
            dist = cand.distance(r.geometry)
            k = dist.idxmin()
            d, oid, tags = float(dist[k]), int(cand.at[k, "osm_id"]), cand.at[k, "osm_tags"]
            n40 = int((dist <= OSM_DIVERGENTE_M).sum())
        if d is not None and d <= OSM_PROVAVEL_M:
            cl, ac = "correspondência provável", "nenhuma; manter posição oficial"
        elif d is not None and d <= OSM_DIVERGENTE_M:
            cl, ac = "possível divergência", "conferir em imagem/campo qual posição representa a linha de retenção ou o grupo focal"
        else:
            cl, ac = "sem correspondência", "conferir se o semáforo existe/está ativo; candidato a inclusão no OSM após verificação"
        linhas.append({"id_oficial": r.id_oficial, "osm_id": oid,
                       "distancia_metros": None if d is None else round(d, 1), "classificacao": cl,
                       "acao_recomendada": ac, "qtd_osm_ate_40m": n40,
                       "osm_ref_igual_id": None if d is None or d > OSM_DIVERGENTE_M else _ref_igual(tags, r.id_oficial),
                       "osm_importado_base_prefeitura": None if d is None or d > OSM_DIVERGENTE_M else _importado(tags),
                       "dist_no_osm_mesmo_ref_m": (round(min(p.distance(r.geometry) for p in por_ref[k]), 1)
                                                   if (k := str(r.id_oficial).lstrip("0")) in por_ref else None),
                       "osm_tags": tags if d is not None and d <= OSM_DIVERGENTE_M else None})
    comp = pd.DataFrame(linhas)
    # pontos do OSM sem nenhum semáforo oficial a até 40 m: complementares, nunca incorporados
    ofi = g[g._coord_valida]
    oidx = ofi.sindex
    for _, s in sinais.iterrows():
        cand = ofi.iloc[oidx.query(s.geometry.buffer(OSM_DIVERGENTE_M))]
        if cand.empty:
            perto = ofi.iloc[oidx.query(s.geometry.buffer(1000))]
            d = round(float(perto.distance(s.geometry).min()), 1) if not perto.empty else None
            linhas.append({"id_oficial": None, "osm_id": int(s.osm_id), "distancia_metros": d,
                           "classificacao": "somente OSM",
                           "acao_recomendada": "registro complementar (fonte OSM); verificar com a CTTU antes de usar como oficial",
                           "qtd_osm_ate_40m": None, "osm_ref_igual_id": None,
                           "osm_importado_base_prefeitura": _importado(s.osm_tags), "osm_tags": s.osm_tags,
                           "osm_lat": round(float(s.lat), 7), "osm_lon": round(float(s.lon), 7)})
    return pd.DataFrame(linhas)


# ---------------------------------------------------------------- interseções

class UF:
    def __init__(self, n):
        self.p = list(range(n))

    def find(self, a):
        while self.p[a] != a:
            self.p[a] = self.p[self.p[a]]
            a = self.p[a]
        return a

    def union(self, a, b):
        self.p[self.find(a)] = self.find(b)


def par_vias(r):
    return [v for v in (_txt(r.via_principal), _txt(r.via_secundaria)) if v]


def mesmas_vias(a, b) -> bool:
    va, vb = par_vias(a), par_vias(b)
    if len(va) < 2 or len(vb) < 2:
        return False
    return ((mesma_via(va[0], vb[0]) and mesma_via(va[1], vb[1])) or
            (mesma_via(va[0], vb[1]) and mesma_via(va[1], vb[0])))


def agrupar(g: gpd.GeoDataFrame, bairros: gpd.GeoDataFrame) -> pd.DataFrame:
    v = g[g._coord_valida].reset_index()
    uf = UF(len(v))
    sidx = v.sindex
    for i, geom in enumerate(v.geometry):
        for j in sidx.query(geom.buffer(RAIO_MESMAS_VIAS_M)):
            if j <= i:
                continue
            d = geom.distance(v.geometry.iloc[j])
            if d <= RAIO_PROXIMO_M or mesmas_vias(v.iloc[i], v.iloc[j]):
                uf.union(i, j)
    v["_g"] = [uf.find(i) for i in range(len(v))]
    grupos = []
    for _, m in v.groupby("_g"):
        ids = sorted(m.id_oficial)
        iid = f"INT-{ids[0]}"
        pts = list(m.geometry)
        diam = max((a.distance(b) for a in pts for b in pts), default=0.0)
        cent = m.geometry.union_all().centroid
        cruz = m[m.tipo_local == "cruzamento"]
        rot = Counter((cruz if len(cruz) else m).intersecao).most_common(1)[0][0]
        motivos = []
        if len(m) >= QTD_REVISAR:
            motivos.append(f"{len(m)} semáforos")
        if diam > DIAMETRO_REVISAR_M:
            motivos.append(f"diâmetro {diam:.0f} m")
        pares = {tuple(sorted(chave(x) for x in par_vias(r))) for _, r in cruz.iterrows()}
        if len(pares) > 2:
            motivos.append(f"{len(pares)} pares de vias distintos")
        grupos.append({"_idx": list(m["index"]), "intersection_id": iid, "intersecao": rot, "centro": cent,
                       "qtd_semaforos": len(m), "ids_oficiais": ";".join(ids),
                       "tipos_encontrados": ";".join(sorted(set(m.tipo_semaforo))),
                       "tipos_controle": ";".join(sorted(set(m.tipo_controle_codigo.dropna()))),
                       "tipo_local": "cruzamento" if len(cruz) else "travessia/meio de quadra",
                       "diametro_m": round(diam, 1), "revisar_manual": bool(motivos),
                       "motivo_revisao": "; ".join(motivos),
                       "fonte_grupo": "CTTU" if len(m) == 1 else "inferência geográfica (agrupamento espacial de pontos CTTU)"})
    gi = pd.DataFrame(grupos)
    for _, r in gi.iterrows():
        g.loc[r._idx, "intersection_id"] = r.intersection_id
    cent = gpd.GeoDataFrame(gi, geometry=list(gi.centro), crs=CRS_METRICO)
    sj = gpd.sjoin_nearest(cent[["geometry"]], bairros, how="left", max_distance=200)
    sj = sj[~sj.index.duplicated()]
    gi["bairro"] = sj["bairro"]
    ll = cent.to_crs("EPSG:4326").geometry
    gi["latitude_centro"] = ll.y.round(6)
    gi["longitude_centro"] = ll.x.round(6)
    return gi.drop(columns=["_idx", "centro"])


# ---------------------------------------------------------------- parâmetros pendentes

PARAMS = [
    # (dado, disponibilidade padrão, recomendação)
    ("tempo_ciclo", "indisponível publicamente", "solicitar planos semafóricos à CTTU via LAI; na falta, coleta em campo ou cenário sintético explicitamente rotulado"),
    ("tempos_verde_amarelo_vermelho", "indisponível publicamente", "solicitar à CTTU via LAI (tabela de tempos por grupo e plano); coleta em campo por cronometragem"),
    ("numero_fases", "inferência possível, mas não confirmada", "inferir da geometria e da destinação apenas como cenário sintético rotulado; confirmar com a CTTU"),
    ("grupos_semaforicos", "indisponível publicamente", "solicitar diagrama de estágios/grupos à CTTU via LAI ou levantar em campo"),
    ("coordenacao_onda_verde", "indisponível publicamente", "solicitar à CTTU defasagens e corredores coordenados; não assumir onda verde"),
    ("plano_por_horario", "indisponível publicamente", "solicitar tabela de planos por faixa horária à CTTU via LAI"),
    ("prioridade_pedestre", None, "confirmar modo de acionamento (botoeira/fixo) e tempos de pedestre com a CTTU"),
    ("prioridade_onibus", "indisponível publicamente", "solicitar à CTTU; cruzar com dataset 'faixas-e-corredores-de-onibus' apenas como contexto"),
    ("preempcao_emergencia", "indisponível publicamente", "solicitar à CTTU; não modelar sem confirmação"),
    ("operacao_adaptativa", None, "confirmar com a CTTU se o controlador opera em tempo real ou plano fixo"),
    ("deteccao_veicular", None, "confirmar existência de laços/câmeras de contagem com a CTTU"),
    ("estado_operacional_tempo_real", "indisponível publicamente", "solicitar acesso a API/feed da central à CTTU; até lá, assumir operação normal como hipótese rotulada"),
    ("sentidos_e_movimentos_permitidos", "inferência possível, mas não confirmada", "usar oneway/turn:lanes do OSM como dado complementar rotulado; validar em campo"),
]
FONTE_VERIFICADA = "Portal de Dados Abertos do Recife (dataset localizacao-dos-semaforos + busca no catálogo)"


def parametros_pendentes(g: gpd.GeoDataFrame, gi: pd.DataFrame) -> pd.DataFrame:
    linhas = []
    membros = g[g.intersection_id.notna()].groupby("intersection_id")
    for iid, m in membros:
        funcs = set(m.funcionamento_descricao)
        tipos = set(m.tipo_controle_codigo.dropna())
        for dado, disp, rec in PARAMS:
            obs = None
            if dado == "prioridade_pedestre":
                if any("edestre" in f for f in funcs):
                    disp = "inferência possível, mas não confirmada"
                    obs = "fonte oficial informa grupo de pedestre (" + ", ".join(sorted(f for f in funcs if "edestre" in f)) + "); acionamento e tempos não publicados"
                else:
                    disp = "indisponível publicamente"
            elif dado == "operacao_adaptativa":
                if "E/GIT" in tipos:
                    disp = "inferência possível, mas não confirmada"
                    obs = "tipo E/Git = integrado ao Gerenciador Inteligente de Trânsito (dicionário); não implica operação adaptativa"
                else:
                    disp = "indisponível publicamente"
            elif dado == "deteccao_veicular":
                if any("laço" in f for f in funcs):
                    disp, obs = "disponível na fonte", "funcionamento 'Veic.C/Laço Det.' (laço detector) – só a existência, sem parâmetros"
                else:
                    disp = "indisponível publicamente"
            elif dado == "plano_por_horario" and m.get("horario_sinal_sonoro") is not None and m.horario_sinal_sonoro.notna().any():
                obs = "só o horário do sinal sonoro é publicado: " + "; ".join(f"{i}: {h}" for i, h in zip(m.id_oficial, m.horario_sinal_sonoro) if isinstance(h, str))
            linhas.append({"intersection_id": iid, "dado_necessario": dado, "disponibilidade": disp,
                           "fonte_verificada": FONTE_VERIFICADA, "recomendacao": rec, "observacao": obs})
    return pd.DataFrame(linhas)


# ---------------------------------------------------------------- saída

COLS_NORM = ["id_oficial", "intersection_id", "latitude", "longitude", "endereco_original", "via_principal",
             "via_secundaria", "ponto_referencia", "intersecao", "tipo_local", "bairro", "cod_bairro",
             "tipo_semaforo", "funcionamento_descricao", "tipo_controle", "tipo_controle_codigo",
             "sinal_sonoro_deficiente_visual", "horario_sinal_sonoro", "sinalizador_ciclista",
             "iluminacao_suspensa_pedestre", "fonte", "confianca_localizacao", "data_atualizacao_fonte",
             "via_osm_mais_proxima", "via_osm_segunda", "dist_via_principal_osm_m", "dist_via_secundaria_osm_m",
             "osm_id_correspondente", "dist_osm_m", "observacoes"]


def tabela_md(df: pd.DataFrame) -> str:
    return df.to_markdown(index=False)


def relatorio(saida, g, gi, comp, pend, aud, meta, osm_ts, sinais):
    ofi = comp[comp.id_oficial.notna()]
    cls = ofi.classificacao.value_counts()
    so_osm = int((comp.classificacao == "somente OSM").sum())
    por_bairro = g.bairro.fillna("(sem bairro)").value_counts().rename_axis("bairro").reset_index(name="semaforos")
    int_bairro = gi.bairro.fillna("(sem bairro)").value_counts().rename_axis("bairro").reset_index(name="intersecoes")
    por_bairro = por_bairro.merge(int_bairro, on="bairro", how="outer").fillna(0).sort_values("semaforos", ascending=False)
    por_bairro[["semaforos", "intersecoes"]] = por_bairro[["semaforos", "intersecoes"]].astype(int)
    vias = g.via_principal.map(lambda s: " ".join(tokens_via(s)))
    rot = g.groupby(vias).via_principal.agg(lambda s: Counter(s).most_common(1)[0][0])
    top_vias = vias.value_counts().head(15).rename_axis("chave").reset_index(name="semaforos")
    top_vias.insert(0, "via (rótulo mais frequente)", top_vias.chave.map(rot))
    top_vias = top_vias.drop(columns="chave")
    disp = pend.groupby(["dado_necessario", "disponibilidade"]).intersection_id.nunique().unstack(fill_value=0)
    comp_info = aud["complementares"]
    L = [
        "# Relatório — semáforos do Recife",
        "",
        f"Gerado por `scripts/semaforos_recife/gerar_semaforos.py`. Fonte oficial: [{meta['title']}]({DATASET_URL}) "
        f"({meta['organization']['title']}; licença {meta.get('license_title')}; recurso CSV atualizado em "
        f"{g.data_atualizacao_fonte.iloc[0]}). OSM: snapshot Overpass `{osm_ts}` (ODbL).",
        "",
        "## Números principais",
        "",
        f"- Semáforos oficiais (registros no CSV): **{aud['total']}**",
        f"- Interseções/locais semaforizados (após agrupamento): **{len(gi)}** "
        f"({int((gi.tipo_local == 'cruzamento').sum())} cruzamentos, {int((gi.tipo_local != 'cruzamento').sum())} travessias/meio de quadra; "
        f"{int((gi.qtd_semaforos > 1).sum())} com mais de um semáforo; {int(gi.revisar_manual.sum())} marcados para revisão manual)",
        f"- Coordenadas ausentes: {aud['coord_ausente']}; corrigidas na leitura: {aud['coord_corrigida']} "
        f"({', '.join(f'{i}: {p}' for i, p in aud['ids_corrigidos']) or '—'}); fora da faixa plausível: {aud['coord_fora_faixa']}",
        f"- IDs duplicados: {aud['id_duplicado']}; coordenadas idênticas: {aud['coord_identica']}; "
        f"pares a menos de 3 m: {len(aud['pares_menos_3m'])} {aud['pares_menos_3m'] or ''}; "
        f"grupos com endereço textual idêntico: {len(aud['texto_identico_grupos'])}",
        f"- Fora dos polígonos de bairro: {aud['fora_poligonos_bairro']} {aud['fora_poligonos_ids'] or ''}; "
        f"fora do município (> 200 m de qualquer bairro): {aud['fora_municipio']}",
        f"- Bairro publicado diferente do bairro espacial: {aud['bairro_divergente']} "
        f"({aud['bairro_divergente_nao_oficial']} com nome que não é bairro oficial, {aud['bairro_divergente_ate_100m']} "
        f"a até 100 m do bairro publicado, {aud['bairro_divergente_mais_100m']} a mais de 100 m)",
        f"- Confiança da localização: " + ", ".join(f"{k} {v}" for k, v in g.confianca_localizacao.value_counts().items()),
        f"- OSM: {len(sinais)} nós com semáforo no município. Oficiais com correspondência provável (≤ {OSM_PROVAVEL_M:.0f} m): "
        f"{cls.get('correspondência provável', 0)}; possível divergência ({OSM_PROVAVEL_M:.0f}–{OSM_DIVERGENTE_M:.0f} m): "
        f"{cls.get('possível divergência', 0)}; sem correspondência (> {OSM_DIVERGENTE_M:.0f} m): {cls.get('sem correspondência', 0)}; "
        f"sem coordenada: {cls.get('sem coordenada oficial', 0)}. Nós OSM sem semáforo oficial a até {OSM_DIVERGENTE_M:.0f} m: {so_osm}.",
        f"- Atenção: {int(sinais.osm_tags.map(_importado).sum())} dos {len(sinais)} nós OSM têm `source=Dados Abertos Prefeitura do Recife` "
        f"(import de 2014 desta mesma base); entre as correspondências prováveis, {int((ofi.classificacao.eq('correspondência provável') & ofi.osm_importado_base_prefeitura.eq(True)).sum())} "
        f"são nós importados. Essas coincidências **não** são validação independente. "
        f"Oficiais com nó OSM de mesmo `ref` importado: {int(ofi.dist_no_osm_mesmo_ref_m.notna().sum())} "
        f"(mediana de distância {ofi.dist_no_osm_mesmo_ref_m.median():.1f} m; > 40 m: {int((ofi.dist_no_osm_mesmo_ref_m > 40).sum())}).",
        "",
        "## Auditoria do CSV oficial",
        "",
        f"Arquivo `{aud['arquivo']}`, colunas: `{'`, `'.join(aud['colunas'])}`.",
        "",
        f"- Valores de `tipo`: {aud['valores_tipo']}",
        f"- Valores de `funcionamento`: {aud['valores_funcionamento']}",
        f"- Colunas não documentadas no dicionário: {aud['colunas_nao_documentadas']}",
        f"- IDs ausentes na sequência 1..máx: {aud['ids_ausentes_na_sequencia'] or 'nenhum'}",
        f"- Endereço textual idêntico (possível duplicata lógica ou semáforos em sentidos opostos): {aud['texto_identico_grupos'] or 'nenhum'}",
        "",
        "### Recortes oficiais complementares",
        "",
    ]
    for rec, info in comp_info.items():
        L.append(f"- **{rec}**: {info['registros']} registros; colunas {info['colunas']}; IDs sem par na base principal: "
                 f"{info['ids_sem_par_na_base'] or 'nenhum'}; coordenada > 5 m da base principal: {info['coord_difere_da_base_mais_5m'] or 'nenhuma'}; "
                 f"valores com vírgula decimal: {info['virgula_decimal']}")
    L += ["", "## Distribuição por bairro", "", tabela_md(por_bairro), "",
          "## Vias com mais semáforos (via principal publicada)", "", tabela_md(top_vias), "",
          "## Interseções marcadas para revisão manual", "",
          tabela_md(gi[gi.revisar_manual][["intersection_id", "intersecao", "bairro", "qtd_semaforos", "diametro_m", "motivo_revisao", "ids_oficiais"]]), "",
          "## Disponibilidade dos parâmetros de simulação (nº de interseções)", "", disp.to_markdown(), ""]
    (saida / "relatorio_semaforos.md").write_text("\n".join(L))


def exportar_app(g: gpd.GeoDataFrame, path: Path):
    """Lista enxuta para o simulador (core/src/extsig.rs): um ponto por controlador CTTU.

    A posição é a oficial da base principal; só quando um recorte oficial dá outra posição a mais de
    100 m (caso do 255, cuja coordenada principal cai a 4 km do endereço) usa-se a do recorte.
    """
    itens = []
    for _, r in g[g._coord_valida].iterrows():
        lat, lon = r.latitude, r.longitude
        if r._coord_alt_m > 100 and r._alt_xy is not None:
            lat, lon = r._alt_xy
        itens.append({"id": int(r.id_oficial), "lat": round(float(lat), 6), "lon": round(float(lon), 6),
                      "cruzamento": r.tipo_local == "cruzamento", "tipo": r.tipo_semaforo, "nome": r.intersecao})
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(itens, ensure_ascii=False, separators=(",", ":")))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--saida", type=Path, default=Path("data/semaforos_recife"))
    ap.add_argument("--brutos", type=Path, default=None)
    ap.add_argument("--atualizar-osm", action="store_true", help="refaz as consultas Overpass")
    ap.add_argument("--app", type=Path, default=Path("public/data/semaforos_cttu.json"), help="JSON lido pelo simulador")
    a = ap.parse_args()
    brutos = a.brutos or a.saida / "_brutos"
    a.saida.mkdir(parents=True, exist_ok=True)

    meta = baixar(brutos, a.atualizar_osm)
    bairros = ler_bairros(brutos)
    g, aud = montar_oficial(brutos, meta, bairros)
    anexar_complementares(g, brutos, aud)
    sinais, vias, osm_ts = ler_osm(brutos)
    validar_vias(g, vias)
    comp = comparar_osm(g, sinais)
    m = comp[comp.id_oficial.notna()].set_index("id_oficial")
    g["osm_id_correspondente"] = g.id_oficial.map(m.osm_id.where(m.classificacao == "correspondência provável")).astype("Int64")
    g["dist_osm_m"] = g.id_oficial.map(m.distancia_metros)
    g["fonte"] = np.where(g.osm_id_correspondente.notna(), "ambos", FONTE_CTTU)
    gi = agrupar(g, bairros)
    pend = parametros_pendentes(g, gi)

    orig = [c for c in g.columns if c.startswith("orig_")]
    out = g[COLS_NORM + orig].copy()
    out["latitude"] = out.latitude.round(6)
    out["longitude"] = out.longitude.round(6)
    out.to_csv(a.saida / "semaforos_recife_normalizados.csv", index=False)
    geo = gpd.GeoDataFrame(out, geometry=gpd.points_from_xy(out.longitude, out.latitude), crs="EPSG:4326")
    geo = geo[g._coord_valida.values]
    geo.to_file(a.saida / "semaforos_recife_normalizados.geojson", driver="GeoJSON")
    cols_i = ["intersection_id", "intersecao", "bairro", "latitude_centro", "longitude_centro", "qtd_semaforos",
              "ids_oficiais", "tipos_encontrados", "fonte_grupo", "tipo_local", "tipos_controle", "diametro_m",
              "revisar_manual", "motivo_revisao"]
    gi[cols_i].rename(columns={"fonte_grupo": "fonte"}).sort_values("intersection_id").to_csv(
        a.saida / "intersecoes_semaforizadas.csv", index=False)
    comp["osm_id"] = comp.osm_id.astype("Int64")
    comp.to_csv(a.saida / "comparacao_cttu_osm.csv", index=False)
    pend.to_csv(a.saida / "parametros_simulacao_pendentes.csv", index=False)
    relatorio(a.saida, g, gi, comp, pend, aud, meta, osm_ts, sinais)
    exportar_app(g, a.app)
    print(f"ok: {len(g)} semáforos, {len(gi)} interseções -> {a.saida}", file=sys.stderr)


if __name__ == "__main__":
    main()
