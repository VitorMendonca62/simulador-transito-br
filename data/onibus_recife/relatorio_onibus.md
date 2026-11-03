# Relatório — linhas de ônibus do Recife

Gerado por `scripts/onibus_recife/gerar_onibus.py`. Fonte: [GTFS do Grande Recife Consórcio de Transporte](https://www.granderecife.pe.gov.br/transporte/gtfs/) `v2026.10.26`; dia de referência **2026-10-28** (quarta-feira, dia útil sem exceção no calendário). Mapa: `public/data/mapa-recife.osm.gz` (OSM `2026-10-17T16:24:20Z`, ODbL).

## Números principais

- Rotas no GTFS: 400 (395 de ônibus; metrô e VLT ficam de fora). Com viagem no dia de referência: 388.
- Linhas com algum trecho no mapa do Recife: **290**; padrões (shapes) no mapa: **995** de 1340 com viagem no dia.
- Padrões com itinerário contínuo (sem lacuna): **853** (86%). Ligações refeitas pelo menor caminho no mapa (trecho sem amostra perto de via, como terminal ou rua `service`): 640.
- Lacunas: **56** pares de ways distintos, em 178 ocorrências (desvio longo (ligação curta ausente no mapa): 49; sem ligação no mapa: 5; só contra a mão única: 2).
- Amostras do itinerário a até 20 m de um way compatível (mediana por padrão): 98%.
- Padrões que saem e voltam ao mapa (mais de um trecho): 75.
- Ônibus entrando em trechos do mapa no dia: **31456**; às 7h: **1991**.
- Paradas usadas: **2868** (baias pelo OSM: 0). Paradas GTFS dentro do mapa sem padrão no dia: 22. Sem nome (o GTFS só traz o código e não há parada do OSM com nome a até 30 m): 1179.
- Exceções para ônibus (`ajustes_manuais.json`): conversao: 35, incluir: 14, ligar: 46, mao_dupla: 4. Vias `service` incluídas automaticamente (o shape passa por elas): 30 (lista em `incluir_auto.csv`).
- Paradas do OSM no mapa (`highway=bus_stop`): 188; sem parada GTFS a até 30 m: 36. Elementos de parada no Overpass (Recife): 3660.

## Ônibus por hora no mapa

| hora | ônibus |
|---|---|
| 0h | 141 |
| 1h | 81 |
| 2h | 78 |
| 3h | 86 |
| 4h | 620 |
| 5h | 1816 |
| 6h | 2207 |
| 7h | 1991 |
| 8h | 1859 |
| 9h | 1610 |
| 10h | 1480 |
| 11h | 1521 |
| 12h | 1579 |
| 13h | 1666 |
| 14h | 1822 |
| 15h | 1967 |
| 16h | 1985 |
| 17h | 1844 |
| 18h | 1676 |
| 19h | 1372 |
| 20h | 1229 |
| 21h | 1207 |
| 22h | 1053 |
| 23h | 566 |

## Lacunas

Dois ways seguidos do itinerário que o mapa não liga por um caminho de até 2× a distância reta + 300 m. *Desvio longo*: o mapa liga, mas só por um caminho bem mais longo (falta uma ligação curta: corredor que não encosta na pista, alça, acesso de terminal). *Só contra a mão única*: o ônibus passa onde o OSM marca mão única (o OSM do Recife não marca `oneway:bus`). *Sem ligação no mapa*: via que o núcleo não carrega (`service`, terminal) ou que falta no OSM. Cada uma se resolve com uma exceção para ônibus em `ajustes_manuais.json` (`ligar`, `mao_dupla`) ou fica como aviso; lista completa em `lacunas.csv`.

| de_way | para_way | motivo | distancia_m | padroes | exemplo | lat | lon |
|---|---|---|---|---|---|---|---|
| 339192844 | 151153196 | desvio longo (ligação curta ausente no mapa) | 82 | 19 | 011-1120-2 | -8.085873 | -34.891135 |
| 332536308 | 479907716 | só contra a mão única | 10 | 12 | 049-665-2 | -8.06867 | -34.990636 |
| 336100412 | 222795597 | sem ligação no mapa | 105 | 10 | 120-1184-2 | -8.110037 | -34.963229 |
| 222795597 | 336100412 | sem ligação no mapa | 95 | 8 | 120-1184-1 | -8.110033 | -34.963213 |
| 869890189 | 984862014 | desvio longo (ligação curta ausente no mapa) | 480 | 7 | 2459-1357-2 | -8.035496 | -34.984779 |
| 984862014 | 869890189 | desvio longo (ligação curta ausente no mapa) | 480 | 7 | 2459-1357-1 | -8.035484 | -34.984765 |
| 30812855 | 222100952 | sem ligação no mapa | 276 | 6 | 219-682-2 | -8.076808 | -34.980316 |
| 316808706 | 901242726 | desvio longo (ligação curta ausente no mapa) | 10 | 6 | 152-343-2 | -8.141269 | -34.933348 |
| 772524562 | 1126443037 | desvio longo (ligação curta ausente no mapa) | 59 | 6 | 516-201-1 | -8.058321 | -34.898531 |
| 901242726 | 316808706 | desvio longo (ligação curta ausente no mapa) | 10 | 6 | 152-343-1 | -8.141278 | -34.933361 |
| 194287395 | 129483732 | desvio longo (ligação curta ausente no mapa) | 10 | 5 | 014-1121-1 | -8.08488 | -34.887178 |
| 221642218 | 323308461 | desvio longo (ligação curta ausente no mapa) | 224 | 5 | 331-675-2 | -8.087069 | -34.970444 |
| 701900698 | 701900697 | desvio longo (ligação curta ausente no mapa) | 14 | 5 | 134-1031-1 | -8.123609 | -34.912389 |
| 703620185 | 772281235 | desvio longo (ligação curta ausente no mapa) | 44 | 5 | 108-624-1 | -8.088645 | -34.946305 |
| 128333663 | 367517835 | desvio longo (ligação curta ausente no mapa) | 10 | 4 | 710-519-1 | -8.064749 | -34.89703 |
| 161802350 | 901242726 | desvio longo (ligação curta ausente no mapa) | 593 | 4 | 152-343-1 | -8.139142 | -34.935262 |
| 222795414 | 430383804 | desvio longo (ligação curta ausente no mapa) | 200 | 4 | 149-1035-1 | -8.108743 | -34.962352 |
| 901242726 | 161802350 | desvio longo (ligação curta ausente no mapa) | 592 | 4 | 152-343-2 | -8.139145 | -34.935259 |
| 230514550 | 598232863 | desvio longo (ligação curta ausente no mapa) | 10 | 3 | 191-300-1 | -8.073769 | -34.893898 |
| 703556861 | 128446976 | desvio longo (ligação curta ausente no mapa) | 10 | 3 | 101-605-1 | -8.06798 | -34.883495 |
| 777105568 | 883617690 | desvio longo (ligação curta ausente no mapa) | 10 | 3 | 200-1037-1 | -8.077493 | -34.896675 |
| 136957397 | 137186650 | desvio longo (ligação curta ausente no mapa) | 55 | 2 | 858-1364-1 | -8.077236 | -34.906766 |
| 137186650 | 129812361 | desvio longo (ligação curta ausente no mapa) | 10 | 2 | 858-1364-1 | -8.077382 | -34.907022 |
| 161802350 | 707366299 | desvio longo (ligação curta ausente no mapa) | 16 | 2 | 125-306-1 | -8.137015 | -34.937009 |
| 222795597 | 553472082 | sem ligação no mapa | 522 | 2 | 149-1500-1 | -8.112781 | -34.963533 |
| 703620185 | 1183809384 | desvio longo (ligação curta ausente no mapa) | 66 | 2 | 202-1060-2 | -8.087703 | -34.945832 |
| 707366299 | 161802350 | desvio longo (ligação curta ausente no mapa) | 18 | 2 | 125-306-2 | -8.137022 | -34.937023 |
| 707366299 | 901242726 | desvio longo (ligação curta ausente no mapa) | 607 | 2 | 153-349-1 | -8.139083 | -34.935331 |
| 707366299 | 336096294 | desvio longo (ligação curta ausente no mapa) | 7 | 2 | 153-349-2 | -8.134145 | -34.943867 |
| 901242726 | 707366299 | desvio longo (ligação curta ausente no mapa) | 607 | 2 | 153-349-2 | -8.139067 | -34.935357 |
