#!/usr/bin/env python3
"""Mescla os ajustes conferidos à mão (ajustes_manuais.json) na lista de semáforos lida pelo simulador.

Chamado por gerar_semaforos.py ao exportar; também roda sozinho, sem baixar nada, para reaplicar os
ajustes no JSON já gerado:
    python scripts/semaforos_recife/aplicar_ajustes.py [--app public/data/semaforos_cttu.json]

Só usa a biblioteca padrão.
"""
import argparse
import json
import sys
from pathlib import Path

AJUSTES = Path(__file__).with_name("ajustes_manuais.json")
CAMPOS = ("nos", "pontos", "fases")


def aplicar(itens: list[dict], path: Path = AJUSTES) -> int:
    """Grava `nos`, `pontos` e `fases` de cada ajuste no semáforo de mesmo id (troca os de uma aplicação anterior).

    Um ajuste com `lat`/`lon` e id que não está na base entra como semáforo novo.
    """
    ajustes = json.loads(path.read_text())["ajustes"]
    por_id = {s["id"]: s for s in itens}
    for s in itens:
        for c in CAMPOS:
            s.pop(c, None)
    for a in ajustes:
        s = por_id.get(a["id"])
        if s is None:
            if "lat" not in a or "lon" not in a:
                raise SystemExit(f"ajuste {a['id']}: não está na base da CTTU e não tem lat/lon")
            s = {"id": a["id"], "lat": a["lat"], "lon": a["lon"], "cruzamento": a.get("cruzamento", True),
                 "tipo": a.get("tipo", "veicular"), "nome": a.get("nome", f"Semáforo {a['id']}")}
            itens.append(s)
            por_id[s["id"]] = s
        for c in CAMPOS:
            if a.get(c):
                s[c] = a[c]
    return len(ajustes)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--app", type=Path, default=Path("public/data/semaforos_cttu.json"), help="JSON lido pelo simulador")
    a = ap.parse_args()
    itens = json.loads(a.app.read_text())
    n = aplicar(itens)
    a.app.write_text(json.dumps(itens, ensure_ascii=False, separators=(",", ":")))
    print(f"ok: {n} ajustes manuais em {a.app}", file=sys.stderr)


if __name__ == "__main__":
    main()
