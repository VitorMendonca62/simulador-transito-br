# Linhas de ônibus do Recife: metodologia

Itinerários, paradas e horários das linhas de ônibus para o simulador (versão 2, fase 3). Os números de cada execução
ficam em [`relatorio_onibus.md`](relatorio_onibus.md). O script é
[`scripts/onibus_recife/gerar_onibus.py`](../../scripts/onibus_recife/gerar_onibus.py).

```bash
python3 -m venv .venv && .venv/bin/pip install -r scripts/onibus_recife/requirements.txt
.venv/bin/python scripts/onibus_recife/gerar_onibus.py               # usa o cache em data/onibus_recife/_brutos
.venv/bin/python scripts/onibus_recife/gerar_onibus.py --atualizar   # baixa de novo o GTFS e refaz o Overpass
python3 scripts/onibus_recife/aplicar_ajustes.py                     # só reaplica ajustes_manuais.json
.venv/bin/python scripts/onibus_recife/gerar_onibus.py --inspecionar 101,2450   # + inspecao.geojson (não versionado)
```

## Fontes

| fonte | uso | licença |
|---|---|---|
| [GTFS do Grande Recife Consórcio de Transporte](https://www.granderecife.pe.gov.br/transporte/gtfs/) (`gtfs.zip`) | linhas, itinerários (`shapes`), paradas, horário de cada viagem | dado aberto publicado pelo consórcio para uso público; citar a fonte |
| `public/data/mapa-recife.osm.gz` (o mapa do app) | recorte e casamento do itinerário com os ways | ODbL (OpenStreetMap) |
| OSM via Overpass: paradas no Recife (`highway=bus_stop`, `public_transport=platform/stop_position`, `bus_bay`) | nome da parada quando o GTFS só traz o código; tipo baia | ODbL |

As relações `route=bus` do OSM **não** são usadas: são 87 no Recife contra ~390 linhas no GTFS, e o GTFS é a base
oficial. Não há raspagem de site.

## Passos

1. **Dia de referência**: a primeira quarta-feira a partir da data do feed (`feed_version`) sem nenhuma exceção em
   `calendar_dates.txt`. Entram as viagens dos serviços ativos nesse dia (`route_type=3`: metrô e VLT ficam de fora).
2. **Padrão** = um `shape_id` com viagem no dia. A viagem com mais paradas é a representativa (sequência de paradas).
3. **Recorte**: o shape é amostrado a cada 10 m; uma amostra está "no mapa" se há via veicular do mapa a até 60 m
   (o mapa é o município). Saídas de até 500 m contam como dentro (praças, terminais); pedaços dentro com menos de
   300 m saem. Cada pedaço é um **trecho** (`entra_na_borda` / `sai_na_borda` dizem se começa ou termina no limite
   do mapa e não no terminal).
4. **Casamento com os ways** (por trecho): candidatos são os ways que o núcleo carrega (mesmas classes de
   `core/src/net.rs`, `parse_highway`) a até 20 m da amostra, com rumo a até 45° do sentido do way (ou do contrário,
   se não for mão única; mão única lida como em `way_attrs`). Viterbi: custo = distância/4 + troca de way (2 se os
   ways encostam, 25 se não, 30 para inverter no mesmo way). Entre dois ways seguidos que não encostam, entra o
   **menor caminho dirigido** no mapa se ele tiver até 2× a distância reta + 300 m (amostras longe de via: terminal,
   rua `service`, curva cortada pelo shape); senão fica uma **lacuna**.
5. **Paradas**: projetadas em ordem no shape (a primeira posição a até 30 m depois da anterior, descendo até o mínimo
   local); fora de 30 m ou fora do trecho, saem. Distância em metros ao longo da polilinha do trecho.
   **Baia** se houver parada do OSM com `bus_bay` a até 25 m; senão na faixa (regra da fase 4).
6. **Horários**: o instante em que cada viagem do dia entra no trecho, interpolado entre as paradas em volta do
   início do trecho pelo comprimento do shape (viagem com sequência de paradas diferente da representativa: desloca
   pela primeira parada). Segundos do dia, módulo 24 h (viagens depois da meia-noite caem na madrugada).
7. **Ajustes manuais** e **validação** (horários ordenados, distâncias das paradas crescentes, todo way existe no
   mapa, sentido respeita a mão única, ways seguidos encostam salvo nas lacunas). Falha interrompe sem gravar.

## Formato (`public/data/onibus_recife.json`)

```json
{
  "fonte": {"gtfs": "v2026.10.26", "dia_referencia": "2026-10-28", "mapa_osm_base": "..."},
  "paradas": {"<stop_id>": {"nome": "... ou null", "lat": 0, "lon": 0, "baia": false}},
  "linhas": [{"id": "<route_id>", "numero": "101", "nome": "...", "operadora": "<agency_id>",
    "padroes": [{"id": "<shape_id>", "sentido": 0, "destino": "<trip_headsign>",
      "trechos": [{"ways": [123, -456], "pontos": [[lat, lon]], "lacunas": [5],
                   "entra_na_borda": false, "sai_na_borda": true,
                   "paradas": [["<stop_id>", 120]], "partidas": [18000, 18420]}]}]}]
}
```

- `ways`: id do way OSM com sinal = sentido de passagem (positivo na ordem dos nós do way).
- `lacunas`: índices em `ways` em que o way não encosta no anterior (ver o relatório e `lacunas.csv`).
- `pontos`: polilinha do trecho simplificada a 10 m, para o encaixe quando um way foi editado ou dividido.
- `paradas`: `[id, metros desde o início do trecho]`. `partidas`: segundos do dia em que cada viagem entra no trecho.
- Campos dos ajustes: `desligada` (linha), `desligado` (padrão), `removida` (parada); `_orig` guarda o valor gerado.

## Limites

- Casamento geométrico: onde o OSM e o shape divergem (via nova, contramão de ônibus, conversão proibida no mapa)
  sobra lacuna; o relatório classifica cada uma (desvio longo / só contra a mão única / sem ligação no mapa).
- Um dia útil só; sábado e domingo não entram.
- O OSM do Recife não marca `bus_bay` em parada nenhuma (consulta de 2026-11): todas ficam na faixa até haver ajuste.
- Paradas sem nome: o GTFS traz o código no lugar do nome em boa parte delas.
- Tamanho: ~2,7 MB (~0,5 MB comprimido no envio).
