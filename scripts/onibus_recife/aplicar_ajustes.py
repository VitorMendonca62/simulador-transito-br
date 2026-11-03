#!/usr/bin/env python3
"""Mescla os ajustes conferidos à mão (ajustes_manuais.json) nas linhas de ônibus lidas pelo simulador.

Chamado por gerar_onibus.py ao exportar; também roda sozinho, sem baixar nada, para reaplicar os ajustes no JSON já
gerado:
    python scripts/onibus_recife/aplicar_ajustes.py [--app public/data/onibus_recife.json]

Cada item alterado guarda os valores gerados em `_orig`; reaplicar volta a eles antes de aplicar a lista atual.
Só usa a biblioteca padrão.
"""
import argparse
import json
import sys
from pathlib import Path

AJUSTES = Path(__file__).with_name("ajustes_manuais.json")


def _restaura(item: dict):
    for k, v in item.pop("_orig", {}).items():
        if v is None:
            item.pop(k, None)
        else:
            item[k] = v


def _troca(item: dict, campo: str, valor):
    orig = item.setdefault("_orig", {})
    if campo not in orig:
        orig[campo] = item.get(campo)
    item[campo] = valor


def aplicar(dados: dict, path: Path = AJUSTES) -> int:
    """Linhas e padrões: `desligar`; padrões: `ways` (lista por trecho, null mantém); paradas: `baia`, `lat`, `lon`,
    `remover`. Id inexistente é erro (o ajuste ficou velho)."""
    aj = json.loads(path.read_text())
    linhas = {l["id"]: l for l in dados["linhas"]}
    padroes = {p["id"]: p for l in dados["linhas"] for p in l["padroes"]}
    for l in dados["linhas"]:
        _restaura(l)
        for p in l["padroes"]:
            for t in p["trechos"]:
                _restaura(t)
            _restaura(p)
    for s in dados["paradas"].values():
        _restaura(s)

    def busca(tab, a, tipo):
        if a["id"] not in tab:
            raise SystemExit(f"ajuste de {tipo} {a['id']}: não está nos dados gerados")
        return tab[a["id"]]

    n = 0
    for a in aj.get("linhas", []):
        if a.get("desligar"):
            _troca(busca(linhas, a, "linha"), "desligada", True)
        n += 1
    for a in aj.get("padroes", []):
        p = busca(padroes, a, "padrão")
        if a.get("desligar"):
            _troca(p, "desligado", True)
        for t, ways in zip(p["trechos"], a.get("ways") or []):
            if ways is not None:
                _troca(t, "ways", ways)
                _troca(t, "lacunas", [])
        n += 1
    for a in aj.get("paradas", []):
        s = busca(dados["paradas"], a, "parada")
        for c in ("baia", "lat", "lon"):
            if c in a:
                _troca(s, c, a[c])
        if a.get("remover"):
            _troca(s, "removida", True)
        n += 1
    return n


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--app", type=Path, default=Path("public/data/onibus_recife.json"), help="JSON lido pelo simulador")
    a = ap.parse_args()
    dados = json.loads(a.app.read_text())
    n = aplicar(dados)
    a.app.write_text(json.dumps(dados, ensure_ascii=False, separators=(",", ":")))
    print(f"ok: {n} ajustes manuais em {a.app}", file=sys.stderr)


if __name__ == "__main__":
    main()
