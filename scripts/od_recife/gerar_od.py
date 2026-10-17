#!/usr/bin/env python3
"""Fluxos origem-destino de carro no Recife/RMR a partir da Pesquisa OD Metropolitana 2018.

Baixa os arquivos oficiais (Portal de Dados Abertos do Recife), monta as viagens
de trabalho e educação por zona de tráfego, associa zonas a bairros e grava os CSVs
em --saida. Unidade de todos os fluxos: pessoas (soma do fator EXPANSAO).

Uso:
    python scripts/od_recife/gerar_od.py [--saida data/od_recife] [--brutos data/od_recife/_brutos]

Dependências: pandas, geopandas, libarchive-c (o "zip" das zonas é na verdade um RAR).
"""
import argparse
import os
import re
import sys
import unicodedata
import urllib.request
from pathlib import Path

import geopandas as gpd
import numpy as np
import pandas as pd

PORTAL = "https://dados.recife.pe.gov.br/dataset/e2213899-a081-49e5-8b05-7b27e856847d/resource"
FONTES = {
    "od2018.csv": f"{PORTAL}/0ceadbca-6581-4b1f-8269-d5e94fd92a17/download/pesquisa-origem-destino-metropolitana-2018.csv",
    "zonas2018.rar": f"{PORTAL}/e0aad253-82f0-4241-8d7f-6abd3e3ff011/download/zonas-de-trafego-pesquisa-od-metropolitana-2018.zip",
    "legenda.pdf": f"{PORTAL}/f63f5a6d-a285-45aa-8bda-34490f30cbbc/download/metadados-legenda-das-respostas.pdf",
    "sobre.pdf": f"{PORTAL}/4ed00f48-ceec-4a88-a921-7206831cdc43/download/sobre-a-pesquisa-origem-destino.pdf",
    "bairros.geojson": "https://dados.recife.pe.gov.br/dataset/1702c764-ec7a-4ef0-9861-8acd0e40c819/resource/5c67ce14-1799-40c4-a37c-9daa04d1761c/download/bairros-do-recife.geojson",
}
FONTE_TXT = "OD Metropolitana 2018 (ICPS/Grande Recife)"
ANO = 2018
# Matrizes oficiais de carro (CTTU/ICPS, 20/10/2020), usadas só para validação.
MATRIZES_OFICIAIS = {
    "oficial_trabalho_carro.xlsx": "1CwbUs65F898R7rpwbF37ABIoCSPMKJ2s",
    "oficial_educacao_carro.xlsx": "109A6isLFRhbEYczBm977kuqsBI9WrAL_",
}
DRIVE = "https://drive.usercontent.google.com/download?id={}&export=download&confirm=t"
CRS_METRICO = "EPSG:31985"  # SIRGAS 2000 / UTM 24S, o mesmo do shapefile das zonas

PICOS = {"pico_manha": (6.0, 9.0), "pico_tarde": (17.0, 20.0)}  # partida em [ini, fim)
MODOS_CARRO = ["carro_dirigindo", "carro_passageiro", "taxi", "aplicativo", "motocicleta", "mototaxi"]
TOP_N = 10

MODOS = {
    "CARRO (DIRIGINDO)": "carro_dirigindo",
    "CARRO (COMO PASSAGEIRO)": "carro_passageiro",
    "TAXI": "taxi",
    "UBER": "aplicativo",
    "MOTOCICLETA": "motocicleta",
    "MOTOTAXI": "mototaxi",
    "SOMENTE ONIBUS (APENAS UMA LINHA)": "transporte_publico",
    "SOMENTE ONIBUS (DUAS OU MAIS LINHAS)": "transporte_publico",
    "ONIBUS + METRO": "transporte_publico",
    "SOMENTE METRO": "transporte_publico",
    "BICICLETA + ONIBUS": "transporte_publico",
    "BICICLETA + ONIBUS + METRO": "transporte_publico",
    "VLT (VEICULO LEVE SOBRE TRILHOS)": "transporte_publico",
    "VLT(VEICULO LEVE SOBRE TRILHOS)": "transporte_publico",
    "BARCO": "transporte_publico",
    "SOMENTE A PE": "a_pe",
    "SOMENTE BICICLETA": "bicicleta",
    "FRETADO": "fretado",
    "TRANSPORTE ESCOLAR": "transporte_escolar",
    "CAMINHAO": "caminhao",
    "OUTRO": "outro",
}
# Ponto médio de cada classe de duração da viagem, em horas.
DURACAO = {
    "MENOS DE 15MIN": 7.5, "ENTRE 15MIN E 30MIN": 22.5, "ENTRE 30MIN E 45MIN": 37.5,
    "ENTRE 45MIN E 1H00": 52.5, "ENTRE 1H00 E 1H30": 75, "ENTRE 1H30 E 2H00": 105, "MAIS DE 2H00": 135,
}
DURACAO = {k: v / 60 for k, v in DURACAO.items()}


def norm(s: pd.Series) -> pd.Series:
    """Maiúsculas, sem acento, sem espaços nas pontas; '0' e vazio viram NaN."""
    def f(x):
        if not isinstance(x, str):
            return np.nan
        x = unicodedata.normalize("NFKD", x).encode("ascii", "ignore").decode().upper().strip()
        x = re.sub(r"\s+", " ", x)
        return np.nan if x in ("", "0") else x
    return s.map(f)


def baixar(brutos: Path, validar: bool):
    brutos.mkdir(parents=True, exist_ok=True)
    fontes = dict(FONTES)
    if validar:
        fontes |= {n: DRIVE.format(i) for n, i in MATRIZES_OFICIAIS.items()}
    for nome, url in fontes.items():
        dest = brutos / nome
        if dest.exists() and dest.stat().st_size > 0:
            continue
        print(f"baixando {nome} ...", file=sys.stderr)
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=600) as r, open(dest, "wb") as f:
            while chunk := r.read(1 << 20):
                f.write(chunk)
    shp_dir = brutos / "zonas2018"
    if not shp_dir.exists():
        import libarchive
        shp_dir.mkdir()
        cwd = Path.cwd()
        try:
            os.chdir(shp_dir)
            libarchive.extract_file(str((brutos / "zonas2018.rar").resolve()))
        finally:
            os.chdir(cwd)


def ler_zonas(brutos: Path) -> gpd.GeoDataFrame:
    shp = next((brutos / "zonas2018").rglob("*.[sS][hH][pP]"))
    z = gpd.read_file(shp).to_crs(CRS_METRICO)
    z["zona"] = z["CODIGOZONA"].astype(int)
    z["municipio"] = z["MUNICIPIO"].str.strip()
    return z[["zona", "municipio", "geometry"]]


def correspondencia_zona_bairro(zonas: gpd.GeoDataFrame, brutos: Path):
    """Interseção zonas x bairros. Devolve (tabela completa, pesos por zona, rótulo principal)."""
    b = gpd.read_file(brutos / "bairros.geojson").to_crs(CRS_METRICO)
    b = b[["EBAIRRNOMEOF", "geometry"]].rename(columns={"EBAIRRNOMEOF": "bairro"})
    b["area_bairro"] = b.area
    z = zonas.copy()
    z["area_zona"] = z.area
    inter = gpd.overlay(z, b, how="intersection", keep_geom_type=True)
    inter["area_inter"] = inter.area
    inter["pct_da_zona"] = inter.area_inter / inter.area_zona
    inter = inter[inter.pct_da_zona >= 0.005]  # descarta lascas de limites desalinhados
    inter["pct_do_bairro"] = inter.area_inter / inter.area_bairro
    tab = inter[["zona", "municipio", "bairro", "area_inter", "pct_da_zona", "pct_do_bairro"]].copy()

    # Pesos para a divisão proporcional por área. Zonas do Recife: renormaliza entre os
    # bairros (as sobras são água/limites desalinhados). Fora do Recife: o resto da zona
    # vai para o rótulo do município.
    linhas = []
    for zona, mun in zonas[["zona", "municipio"]].itertuples(index=False):
        t = tab[tab.zona == zona]
        soma = t.pct_da_zona.sum()
        recife = mun.upper() == "RECIFE"
        if recife and soma > 0:
            linhas += [(zona, r.bairro, r.pct_da_zona / soma) for r in t.itertuples()]
        else:
            linhas += [(zona, r.bairro, r.pct_da_zona) for r in t.itertuples()]
            if 1 - soma > 1e-6:
                linhas.append((zona, f"{mun} (fora do Recife)", 1 - soma))
    pesos = pd.DataFrame(linhas, columns=["zona", "bairro", "peso"])

    principal = pesos.sort_values("peso", ascending=False).drop_duplicates("zona")
    principal = principal.set_index("zona")
    rotulo = principal.bairro
    tab = tab.merge(principal.peso.rename("pct_bairro_principal"), left_on="zona", right_index=True, how="left")
    tab["bairro_principal"] = tab.bairro == tab.zona.map(rotulo)
    tab["area_km2_intersecao"] = tab.area_inter / 1e6
    tab = tab.drop(columns="area_inter").sort_values(["zona", "pct_da_zona"], ascending=[True, False])
    return tab, pesos, rotulo, principal.peso


def hora(h: pd.Series, m: pd.Series) -> pd.Series:
    """Hora decimal; 00:00 e horas inválidas viram NaN (no formulário '0' é 'não respondido')."""
    h = pd.to_numeric(h, errors="coerce")
    m = pd.to_numeric(m, errors="coerce").fillna(0)
    t = h + m / 60
    return t.where((h >= 0) & (h <= 23) & (m < 60) & (t > 0))


def freq_fator(s: pd.Series) -> pd.Series:
    """Fração dos dias úteis em que a viagem ocorre: min(vezes por semana, 5) / 5."""
    n = pd.to_numeric(norm(s).str.extract(r"^(\d)")[0], errors="coerce")
    return (n.clip(upper=5) / 5).fillna(1.0)


def montar_pernas(df: pd.DataFrame, zonas_validas: set) -> tuple[pd.DataFrame, dict]:
    stats = {}
    zres = pd.to_numeric(df.ZONA_RES, errors="coerce")
    ztrab = pd.to_numeric(df.ZONA_TRAB, errors="coerce")
    zedu = pd.to_numeric(df.ZONA_EDU, errors="coerce")
    ok = lambda z: z.isin(zonas_validas)
    filho = df.CODIGO.str.endswith("-FILHO")
    peso = df.peso

    def origem(saida, alternativas):
        s = norm(saida)
        o = pd.Series(np.nan, index=df.index)
        aprox = pd.Series(False, index=df.index)
        o[s == "RESIDENCIA"] = zres
        for rot, z in alternativas.items():
            m = s.fillna("").str.replace(" ", "").str.startswith(rot) & ok(z)
            o[m] = z[m]
        falta = o.isna()
        o[falta] = zres[falta]
        aprox[falta & s.notna() & (s != "RESIDENCIA")] = True
        return o, aprox, s

    pernas = []

    def add(mask, motivo, modo, o, d, partida, fator, sentido, aprox):
        m = mask & ok(o) & ok(d) & modo.notna()
        pernas.append(pd.DataFrame({
            "origem_zona": o[m].astype(int), "destino_zona": d[m].astype(int),
            "motivo": motivo, "modo": modo[m], "sentido": sentido,
            "partida_h": partida[m] if partida is not None else np.nan,
            "peso": peso[m], "peso_freq": peso[m] * fator[m], "origem_aproximada": aprox[m],
        }))
        return int(m.sum())

    # Trabalho (respondente)
    modo_t = norm(df.MODO_TRANSP_TRAB).map(MODOS)
    o_t, aprox_t, s_t = origem(df.SAIDA_TRABALHO, {"ESCOLA": zedu})
    ini_t = hora(df.HORA_INICIO_TRAB, df.MIN_INICIO_TRAB)
    fim_t = hora(df.HORA_FIM_TRAB, df.MIN_FIM_TRAB)
    dur_t = norm(df.TEMPO_TRAB).map(DURACAO)
    f_t = freq_fator(df.FREQ_IR_TRAB_DA_ORIGEM)
    tem_t = ~filho & modo_t.notna() & ok(ztrab)

    # Educação (respondente)
    modo_e = norm(df.MODO_TRANSP_AULA).map(MODOS)
    o_e, aprox_e, s_e = origem(df.SAIDA_AULA, {"TRABALHO": ztrab})
    ini_e = hora(df.HORA_INICIO_ESTUDO, df.MIN_INICIO_ESTUDO)
    fim_e = hora(df.HORA_FIM_ESTUDO, df.MIN_FIM_ESTUDO)
    dur_e = norm(df.TEMPO_AULA).map(DURACAO)
    f_e = freq_fator(df.FREQ_IR_AULA_DA_ORIGEM)
    tem_e = ~filho & modo_e.notna() & ok(zedu)

    # Volta: para a residência, a partir do horário de término. Se a pessoa encadeia
    # trabalho -> escola (ou escola -> trabalho), a volta da primeira atividade é a ida da
    # segunda e não é contada de novo.
    trab_vai_escola = tem_e & s_e.fillna("").str.startswith("TRABALHO") & ok(ztrab)
    escola_vai_trab = tem_t & s_t.fillna("").str.replace(" ", "").str.startswith("ESCOLA") & ok(zedu)
    sem_aprox = pd.Series(False, index=df.index)

    stats["trabalho_ida"] = add(tem_t, "trabalho", modo_t, o_t, ztrab, ini_t - dur_t, f_t, "ida", aprox_t)
    stats["trabalho_volta"] = add(tem_t & ~trab_vai_escola, "trabalho", modo_t, ztrab, zres, fim_t, f_t, "volta", sem_aprox)
    stats["educacao_ida"] = add(tem_e, "educacao", modo_e, o_e, zedu, ini_e - dur_e, f_e, "ida", aprox_e)
    stats["educacao_volta"] = add(tem_e & ~escola_vai_trab, "educacao", modo_e, zedu, zres, fim_e, f_e, "volta", sem_aprox)

    # Dependentes (linhas "-FILHO"): só residência -> escola, sem horário nem frequência.
    # A criança não dirige: "carro (dirigindo)" aqui é o responsável dirigindo com ela.
    modo_f = norm(df.MODO_TRANSP_FILHOS_ESCOLA).map(MODOS).replace({"carro_dirigindo": "carro_passageiro"})
    um = pd.Series(1.0, index=df.index)
    stats["dependente_ida"] = add(filho, "educacao_dependente", modo_f, zres, zedu, None, um, "ida", sem_aprox)
    stats["dependente_volta"] = add(filho, "educacao_dependente", modo_f, zedu, zres, None, um, "volta", sem_aprox)

    p = pd.concat(pernas, ignore_index=True)
    p["partida_h"] = p.partida_h % 24
    stats["modos_nao_mapeados"] = sorted(
        (set(norm(df.MODO_TRANSP_TRAB).dropna()) | set(norm(df.MODO_TRANSP_AULA).dropna())
         | set(norm(df.MODO_TRANSP_FILHOS_ESCOLA).dropna())) - set(MODOS))
    return p, stats


def agregar(p: pd.DataFrame) -> pd.DataFrame:
    """Expande por período e motivo agregado e soma por par OD."""
    blocos = [p.assign(periodo="dia_util")]
    for nome, (a, b) in PICOS.items():
        blocos.append(p[(p.partida_h >= a) & (p.partida_h < b)].assign(periodo=nome))
    x = pd.concat(blocos, ignore_index=True)
    x = pd.concat([x, x.assign(motivo="todos_motivos_disponiveis")], ignore_index=True)
    g = x.groupby(["origem_zona", "destino_zona", "periodo", "motivo", "modo"], as_index=False).agg(
        fluxo_pessoas=("peso", "sum"),
        fluxo_pessoas_ajustado_frequencia=("peso_freq", "sum"),
        n_respostas=("peso", "size"),
    )
    return g


def top_destinos(f: pd.DataFrame, origem: str, destino: str, rotulo=None) -> pd.DataFrame:
    linhas = []
    for (per, o), grp in f.groupby(["periodo", origem]):
        tot = grp.fluxo_pessoas.sum()
        grp = grp.sort_values("fluxo_pessoas", ascending=False)
        r = {"periodo": per, "origem": o}
        if rotulo is not None:
            r["origem_bairro_estimado"] = rotulo.get(o)
        r["total_saidas_carro"] = round(tot, 1)
        r["n_respostas"] = int(grp.n_respostas.sum()) if "n_respostas" in grp else None
        for i, d in enumerate(grp.head(TOP_N).itertuples(), 1):
            dz = getattr(d, destino)
            r[f"destino_{i}"] = dz
            if rotulo is not None:
                r[f"destino_{i}_bairro"] = rotulo.get(dz)
            r[f"fluxo_{i}"] = round(d.fluxo_pessoas, 1)
        r["percentual_dos_top_3"] = round(100 * grp.head(3).fluxo_pessoas.sum() / tot, 1) if tot else None
        r["percentual_dos_top_10"] = round(100 * grp.head(10).fluxo_pessoas.sum() / tot, 1) if tot else None
        linhas.append(r)
    out = pd.DataFrame(linhas)
    if rotulo is not None:  # códigos de zona como inteiro (colunas com vazio viram float)
        for c in [c for c in out if re.fullmatch(r"destino_\d+", c)]:
            out[c] = out[c].astype("Int64")
    return out.sort_values(["periodo", "total_saidas_carro"], ascending=[True, False])


def validar(df: pd.DataFrame, brutos: Path, saida: Path) -> pd.DataFrame:
    """Compara com as matrizes oficiais de 2020 (Carro = dirigindo + passageiro, residência -> destino)."""
    zres = pd.to_numeric(df.ZONA_RES, errors="coerce")
    filho = df.CODIGO.str.endswith("-FILHO")
    carro = ["carro_dirigindo", "carro_passageiro"]
    casos = {
        "trabalho": ("oficial_trabalho_carro.xlsx",
                     [(~filho, "MODO_TRANSP_TRAB", "ZONA_TRAB")]),
        "educacao": ("oficial_educacao_carro.xlsx",
                     [(~filho, "MODO_TRANSP_AULA", "ZONA_EDU"), (filho, "MODO_TRANSP_FILHOS_ESCOLA", "ZONA_EDU")]),
    }
    linhas = []
    for motivo, (arq, partes) in casos.items():
        of = pd.read_excel(brutos / arq, sheet_name="DADOS DE ENTRADA E CALCULOS", header=None,
                           skiprows=2, usecols=[0, 1, 2], names=["o", "d", "w"])
        of = of.apply(pd.to_numeric, errors="coerce").dropna()
        nos = []
        for m, cmodo, cdest in partes:
            d = pd.to_numeric(df[cdest], errors="coerce")
            k = m & norm(df[cmodo]).map(MODOS).isin(carro) & zres.notna() & d.notna()
            nos.append(pd.DataFrame({"o": zres[k], "d": d[k], "w": df.peso[k]}))
        nos = pd.concat(nos)
        a = of.groupby(["o", "d"]).w.sum()
        b = nos.groupby(["o", "d"]).w.sum()
        ab = pd.concat([a.rename("oficial"), b.rename("csv")], axis=1).fillna(0)
        ori = ab.groupby(level=0).sum()
        des = ab.groupby(level=1).sum()
        linhas.append({
            "motivo": motivo, "modo": "carro_dirigindo+carro_passageiro",
            "respostas_oficial": len(of), "respostas_csv": len(nos),
            "pessoas_oficial": round(of.w.sum()), "pessoas_csv": round(nos.w.sum()),
            "corr_pares_od": round(ab.oficial.corr(ab.csv), 3),
            "corr_total_por_origem": round(ori.oficial.corr(ori.csv), 3),
            "corr_total_por_destino": round(des.oficial.corr(des.csv), 3),
        })
    v = pd.DataFrame(linhas)
    v.to_csv(saida / "validacao_matriz_oficial.csv", index=False)
    return v


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    raiz = Path(__file__).resolve().parents[2]
    ap.add_argument("--saida", type=Path, default=raiz / "data/od_recife")
    ap.add_argument("--brutos", type=Path, default=raiz / "data/od_recife/_brutos")
    ap.add_argument("--sem-validacao", action="store_true",
                    help="não baixa as matrizes oficiais de 2020 (Google Drive) para comparar")
    a = ap.parse_args()
    a.saida.mkdir(parents=True, exist_ok=True)

    baixar(a.brutos, not a.sem_validacao)
    zonas = ler_zonas(a.brutos)
    tab_zb, pesos, rotulo, pct_principal = correspondencia_zona_bairro(zonas, a.brutos)
    municipio = zonas.set_index("zona").municipio
    tab_zb.to_csv(a.saida / "zona_bairro_correspondencia.csv", index=False, float_format="%.4f")

    cols = ["CODIGO", "ZONA_RES", "ZONA_TRAB", "ZONA_EDU", "EXPANSAO",
            "MODO_TRANSP_TRAB", "MODO_TRANSP_AULA", "MODO_TRANSP_FILHOS_ESCOLA",
            "SAIDA_TRABALHO", "SAIDA_AULA", "TEMPO_TRAB", "TEMPO_AULA",
            "HORA_INICIO_TRAB", "MIN_INICIO_TRAB", "HORA_FIM_TRAB", "MIN_FIM_TRAB",
            "HORA_INICIO_ESTUDO", "MIN_INICIO_ESTUDO", "HORA_FIM_ESTUDO", "MIN_FIM_ESTUDO",
            "FREQ_IR_TRAB_DA_ORIGEM", "FREQ_IR_AULA_DA_ORIGEM",
            "MODO_TRANSP_OUTRO_TRAB", "MODO_TRANSP_COMPRAS", "MODO_TRANSP_MEDICO", "MODO_TRANSP_SERVICO"]
    df = pd.read_csv(a.brutos / "od2018.csv", sep=";", dtype=str, usecols=cols)
    n_total = len(df)
    df["peso"] = pd.to_numeric(df.EXPANSAO.str.replace(",", "."), errors="coerce")
    sem_peso = df.peso.isna() | (df.peso <= 0)
    df = df[~sem_peso].copy()

    p, stats = montar_pernas(df, set(zonas.zona))

    g = agregar(p)
    g["origem_bairro_estimado"] = g.origem_zona.map(rotulo)
    g["destino_bairro_estimado"] = g.destino_zona.map(rotulo)
    # Um condutor = um veículo. Para os demais modos a pesquisa não traz ocupação.
    g["fluxo_veiculos_estimado"] = np.where(g.modo == "carro_dirigindo", g.fluxo_pessoas, np.nan)
    g["fonte"] = FONTE_TXT
    g["ano"] = ANO
    ordem = ["origem_zona", "origem_bairro_estimado", "destino_zona", "destino_bairro_estimado",
             "periodo", "motivo", "modo", "fluxo_pessoas", "fluxo_veiculos_estimado", "fonte", "ano",
             "fluxo_pessoas_ajustado_frequencia", "n_respostas"]
    fl = g[g.modo.isin(MODOS_CARRO)][ordem].sort_values(
        ["periodo", "motivo", "modo", "fluxo_pessoas"], ascending=[True, True, True, False])
    fl.to_csv(a.saida / "fluxos_carro_origem_destino.csv", index=False, float_format="%.2f")

    # Totais por modo e motivo (todos os modos), para mostrar o que é separável.
    tot = g[(g.periodo == "dia_util")].groupby(["motivo", "modo"], as_index=False)[
        ["fluxo_pessoas", "n_respostas"]].sum()
    ida = p[p.sentido == "ida"].groupby(["motivo", "modo"]).agg(
        pessoas_ida=("peso", "sum"), respostas_ida=("peso", "size")).reset_index()
    tot = tot.merge(ida, on=["motivo", "modo"], how="left").rename(
        columns={"fluxo_pessoas": "pessoas_viagem_dia_util_ida_e_volta", "n_respostas": "respostas_pernas"})
    tot.sort_values(["motivo", "pessoas_viagem_dia_util_ida_e_volta"], ascending=[True, False]).to_csv(
        a.saida / "totais_por_modo_motivo.csv", index=False, float_format="%.1f")

    car = g[(g.modo == "carro_dirigindo") & (g.motivo == "todos_motivos_disponiveis")]

    # Resumo e matriz por zona
    resumo = top_destinos(car, "origem_zona", "destino_zona", rotulo)
    resumo.insert(2, "origem_municipio", resumo.origem.map(municipio))
    resumo.to_csv(a.saida / "resumo_por_origem.csv", index=False)
    for per in ["dia_util", *PICOS]:
        m = car[car.periodo == per].pivot_table(index="origem_zona", columns="destino_zona",
                                                values="fluxo_pessoas", aggfunc="sum", fill_value=0)
        todas = sorted(set(m.index) | set(m.columns))
        m = m.reindex(index=todas, columns=todas, fill_value=0)
        m.index.name = "origem_zona\\destino_zona"
        sufixo = "" if per == "dia_util" else f"_{per}"
        m.to_csv(a.saida / f"matriz_od_carros{sufixo}.csv", float_format="%.1f")

    # Por bairro: divisão proporcional à área da zona em cada bairro (origem e destino)
    w = pesos.rename(columns={"zona": "z", "bairro": "b", "peso": "w"})
    cb = car.merge(w.rename(columns={"z": "origem_zona", "b": "origem_bairro", "w": "wo"}), on="origem_zona")
    cb = cb.merge(w.rename(columns={"z": "destino_zona", "b": "destino_bairro", "w": "wd"}), on="destino_zona")
    cb["fluxo_pessoas"] = cb.fluxo_pessoas * cb.wo * cb.wd
    cb = cb.groupby(["periodo", "origem_bairro", "destino_bairro"], as_index=False).fluxo_pessoas.sum()
    top_destinos(cb, "origem_bairro", "destino_bairro").drop(columns="n_respostas").to_csv(
        a.saida / "resumo_por_origem_bairro.csv", index=False)
    mb = cb[cb.periodo == "dia_util"].pivot_table(index="origem_bairro", columns="destino_bairro",
                                                  values="fluxo_pessoas", aggfunc="sum", fill_value=0)
    mb.index.name = "origem_bairro\\destino_bairro"
    mb.to_csv(a.saida / "matriz_od_carros_bairro.csv", float_format="%.1f")

    # Perfil horário de partida (carro dirigindo)
    pc = p[(p.modo == "carro_dirigindo") & p.partida_h.notna()].copy()
    pc["hora"] = pc.partida_h.astype(int)
    perfil = pc.groupby(["hora", "motivo", "sentido"], as_index=False).peso.sum().rename(
        columns={"peso": "pessoas"})
    perfil.to_csv(a.saida / "perfil_horario_carro.csv", index=False, float_format="%.1f")

    # Estatísticas da execução
    pcar = p[p.modo == "carro_dirigindo"]
    outros = {c: df.loc[norm(df[c]).map(MODOS) == "carro_dirigindo", "peso"].agg(["sum", "size"]).tolist()
              for c in ["MODO_TRANSP_OUTRO_TRAB", "MODO_TRANSP_COMPRAS", "MODO_TRANSP_MEDICO", "MODO_TRANSP_SERVICO"]}
    dia = car[car.periodo == "dia_util"]
    top15 = dia.nlargest(15, "fluxo_pessoas")
    top15_inter = dia[(dia.origem_zona != dia.destino_zona) & (dia.n_respostas >= 10)].nlargest(15, "fluxo_pessoas")
    val = None if a.sem_validacao else validar(df, a.brutos, a.saida)
    with open(a.saida / "estatisticas_execucao.md", "w") as f:
        f.write("# Estatísticas da execução (gerado por gerar_od.py)\n\n")
        f.write(f"- Linhas no CSV: {n_total}; descartadas sem EXPANSAO válida: {int(sem_peso.sum())}\n")
        f.write(f"- Soma de EXPANSAO (linhas válidas): {df.peso.sum():,.0f} pessoas\n")
        for k, v in stats.items():
            f.write(f"- {k}: {v}\n")
        f.write(f"- Pernas carro_dirigindo: {len(pcar)} respostas, {pcar.peso.sum():,.0f} pessoas-viagem/dia útil\n")
        f.write(f"- ...com horário de partida conhecido: {pcar.partida_h.notna().mean():.1%}\n")
        f.write(f"- ...com origem aproximada pela residência: {pcar.origem_aproximada.mean():.1%}\n")
        for per, (a0, b0) in PICOS.items():
            s = pcar[(pcar.partida_h >= a0) & (pcar.partida_h < b0)].peso.sum()
            f.write(f"- carro_dirigindo em {per} ({a0:02.0f}h-{b0:02.0f}h): {s:,.0f} pessoas ({s / pcar.peso.sum():.1%} do dia)\n")
        f.write("\n## Carro dirigindo em motivos sem zona de destino (fora da matriz)\n\n")
        for c, (s, n) in outros.items():
            f.write(f"- {c}: {s:,.0f} pessoas (n={int(n)})\n")
        tabelas = [("15 maiores fluxos carro_dirigindo, dia útil, trabalho+educação (pessoas = veículos)", top15),
                   ("15 maiores fluxos entre zonas diferentes, com ao menos 10 respostas", top15_inter)]
        for titulo, t in tabelas:
            f.write(f"\n## {titulo}\n\n")
            f.write("| # | origem | destino | fluxo | respostas |\n|---|---|---|---:|---:|\n")
            for i, r in enumerate(t.itertuples(), 1):
                f.write(f"| {i} | {r.origem_zona} ({rotulo.get(r.origem_zona)}) | {r.destino_zona} "
                        f"({rotulo.get(r.destino_zona)}) | {r.fluxo_pessoas:,.0f} | {r.n_respostas} |\n")
        if val is not None:
            f.write("\n## Validação contra as matrizes oficiais de 2020 (só ida, residência -> destino)\n\n")
            f.write(val.to_markdown(index=False) if hasattr(val, "to_markdown") else val.to_string(index=False))
            f.write("\n")
    print((a.saida / "estatisticas_execucao.md").read_text())


if __name__ == "__main__":
    main()
