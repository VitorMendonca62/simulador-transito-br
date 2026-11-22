"""Matriz horária de motos aproximada, sem os microdados da OD 2018.

O `gerar_od.py` exporta a matriz exata (`horasMoto`) a partir dos microdados. Quando eles não estão disponíveis, este
script acrescenta ao `public/data/od2018_zonas.json` uma versão aproximada, feita com o que já está processado em
`data/od_recife/fluxos_carro_origem_destino.csv`:

- por par de zonas, as viagens de moto (motocicleta + mototáxi, trabalho + educação, ajustadas pela frequência) do pico
  da manhã (partida em [6h, 9h)), do pico da tarde ([17h, 20h)) e do resto do dia;
- cada parte é distribuída pelas horas dela na forma horária total dos carros do próprio JSON.

O mesmo método aplicado aos carros é comparado com os totais por hora reais deles, para mostrar o erro da aproximação.
Por par, a aproximação espalha as viagens fora do pico por todas as horas (a matriz real é esparsa).
Uso: python3 -I scripts/od_recife/motos_aproximadas.py
"""

import csv
import json
from collections import defaultdict
from pathlib import Path

RAIZ = Path(__file__).resolve().parents[2]
FLUXOS = RAIZ / "data/od_recife/fluxos_carro_origem_destino.csv"
JSON_SIM = RAIZ / "public/data/od2018_zonas.json"
PERIODOS = {"pico_manha": range(6, 9), "pico_tarde": range(17, 20)}
FORA = [h for h in range(24) if all(h not in r for r in PERIODOS.values())]


def ler(modos: set) -> dict:
    """(periodo) -> {(o, d): viagens} para trabalho + educação."""
    out = defaultdict(lambda: defaultdict(float))
    with open(FLUXOS, newline="", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            if r["modo"] in modos and r["motivo"] in ("trabalho", "educacao"):
                v = float(r["fluxo_pessoas_ajustado_frequencia"] or 0)
                out[r["periodo"]][(int(r["origem_zona"]), int(r["destino_zona"]))] += v
    return out


def distribuir(per: dict, forma: list) -> list:
    horas = [defaultdict(float) for _ in range(24)]
    partes = {**{k: list(v) for k, v in PERIODOS.items()}, "fora": FORA}
    pesos = {k: sum(forma[h] for h in hs) for k, hs in partes.items()}
    for par, dia in per["dia_util"].items():
        m, t = per["pico_manha"].get(par, 0.0), per["pico_tarde"].get(par, 0.0)
        for nome, total in (("pico_manha", m), ("pico_tarde", t), ("fora", max(dia - m - t, 0.0))):
            if total <= 0 or pesos[nome] <= 0:
                continue
            for h in partes[nome]:
                horas[h][par] += total * forma[h] / pesos[nome]
    return horas


def plano(horas: list) -> list:
    return [[x for (o, d), v in sorted(m.items()) if round(v, 2) >= 0.01 for x in (o, d, round(v, 2))] for m in horas]


def main():
    sim = json.loads(JSON_SIM.read_text(encoding="utf-8"))
    forma = [sum(flat[i + 2] for i in range(0, len(flat), 3)) for flat in sim["horas"]]

    # validação: o mesmo método nos carros, contra a matriz horária real
    carro = distribuir(ler({"carro_dirigindo"}), forma)
    tc = [sum(m.values()) for m in carro]
    total = sum(forma)
    erro = sum(abs(a - b) for a, b in zip(tc, forma)) / total
    print(f"carros: {total:,.0f}/dia; diferença dos totais por hora entre aproximado e real: {erro:.1%} do dia")
    for h in (7, 12, 18):
        print(f"  {h:02d}h  real {forma[h]:8,.0f}  aproximado {tc[h]:8,.0f}")

    moto = distribuir(ler({"motocicleta", "mototaxi"}), forma)
    tm = [sum(m.values()) for m in moto]
    print(f"motos: {sum(tm):,.0f}/dia; 7h {tm[7]:,.0f}/h; 18h {tm[18]:,.0f}/h")
    sim["modoMoto"] = "motocicleta+mototaxi (1 viagem = 1 moto, hipótese; horas aproximadas por motos_aproximadas.py)"
    sim["horasMoto"] = plano(moto)
    JSON_SIM.write_text(json.dumps(sim, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")


if __name__ == "__main__":
    main()
