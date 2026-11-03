# Relatório — linhas de ônibus do Recife

Gerado por `scripts/onibus_recife/gerar_onibus.py`. Fonte: [GTFS do Grande Recife Consórcio de Transporte](https://www.granderecife.pe.gov.br/transporte/gtfs/) `v2026.10.26`; dia de referência **2026-10-28** (quarta-feira, dia útil sem exceção no calendário). Mapa: `public/data/mapa-recife.osm.gz` (OSM `2026-10-17T16:24:20Z`, ODbL).

## Números principais

- Rotas no GTFS: 400 (395 de ônibus; metrô e VLT ficam de fora). Com viagem no dia de referência: 388.
- Linhas com algum trecho no mapa do Recife: **292**; padrões (shapes) no mapa: **999** de 1340 com viagem no dia.
- Padrões com itinerário contínuo (sem lacuna): **700** (70%). Ligações refeitas pelo menor caminho no mapa (trecho sem amostra perto de via, como terminal ou rua `service`): 310.
- Lacunas: **92** pares de ways distintos, em 511 ocorrências (desvio longo (conversão que o mapa não permite): 62; sem ligação no mapa: 17; só contra a mão única: 13).
- Amostras do itinerário a até 20 m de um way compatível (mediana por padrão): 97%.
- Padrões que saem e voltam ao mapa (mais de um trecho): 82.
- Ônibus entrando em trechos do mapa no dia: **32095**; às 7h: **2037**.
- Paradas usadas: **2868** (baias pelo OSM: 0). Paradas GTFS dentro do mapa sem padrão no dia: 22. Sem nome (o GTFS só traz o código e não há parada do OSM com nome a até 30 m): 1179.
- Paradas do OSM no mapa (`highway=bus_stop`): 188; sem parada GTFS a até 30 m: 36. Elementos de parada no Overpass (Recife): 3660.

## Ônibus por hora no mapa

| hora | ônibus |
|---|---|
| 0h | 143 |
| 1h | 82 |
| 2h | 79 |
| 3h | 87 |
| 4h | 633 |
| 5h | 1864 |
| 6h | 2263 |
| 7h | 2037 |
| 8h | 1893 |
| 9h | 1637 |
| 10h | 1508 |
| 11h | 1549 |
| 12h | 1611 |
| 13h | 1703 |
| 14h | 1865 |
| 15h | 2012 |
| 16h | 2033 |
| 17h | 1884 |
| 18h | 1704 |
| 19h | 1389 |
| 20h | 1252 |
| 21h | 1228 |
| 22h | 1069 |
| 23h | 570 |

## Lacunas

Dois ways seguidos do itinerário que o mapa não liga por um caminho de até 2× a distância reta + 300 m. *Desvio longo*: o mapa liga, mas por outro caminho (conversão proibida ou ausente). *Só contra a mão única*: o ônibus passa onde o OSM marca mão única (o núcleo não lê exceção para ônibus). *Sem ligação no mapa*: via que o núcleo não carrega (`service`, terminal) ou que falta no OSM. São os candidatos a `ajustes_manuais.json`; lista completa em `lacunas.csv`.

| de_way | para_way | motivo | distancia_m | padroes | exemplo | lat | lon |
|---|---|---|---|---|---|---|---|
| 128451839 | 796498278 | desvio longo (conversão que o mapa não permite) | 1020 | 39 | 121-1116-2 | -8.078013 | -34.898104 |
| 678493300 | 128451839 | só contra a mão única | 948 | 39 | 121-1116-2 | -8.074915 | -34.88926 |
| 574139911 | 136667235 | desvio longo (conversão que o mapa não permite) | 10 | 24 | 202-1060-2 | -8.030867 | -34.956647 |
| 151153196 | 778595301 | só contra a mão única | 10 | 19 | 011-1120-2 | -8.087987 | -34.891545 |
| 695519696 | 129478578 | desvio longo (conversão que o mapa não permite) | 57 | 18 | 125-306-2 | -8.122797 | -34.912431 |
| 647530685 | 129479410 | só contra a mão única | 209 | 16 | 023-679-1 | -8.131974 | -34.916888 |
| 775253565 | 573293591 | desvio longo (conversão que o mapa não permite) | 60 | 15 | 1906-1541-2 | -8.008732 | -34.938013 |
| 995648132 | 627935214 | sem ligação no mapa | 33 | 14 | 2416-1245-2 | -8.050969 | -34.921544 |
| 134472028 | 87240009 | desvio longo (conversão que o mapa não permite) | 249 | 13 | 011-1120-2 | -8.11992 | -34.902017 |
| 809324597 | 626712794 | sem ligação no mapa | 9 | 13 | 2421-771-1 | -8.050311 | -34.922389 |
| 129478578 | 707010070 | desvio longo (conversão que o mapa não permite) | 175 | 12 | 023-679-1 | -8.122183 | -34.912353 |
| 332536308 | 479907716 | só contra a mão única | 10 | 12 | 049-665-2 | -8.06867 | -34.990636 |
| 222795597 | 336100412 | sem ligação no mapa | 95 | 10 | 120-1184-1 | -8.110033 | -34.963213 |
| 336100412 | 222795597 | sem ligação no mapa | 105 | 10 | 120-1184-2 | -8.110037 | -34.963229 |
| 762384521 | 128162317 | desvio longo (conversão que o mapa não permite) | 10 | 10 | 1062-1045-1 | -8.062309 | -34.878284 |
| 1107392940 | 706175626 | só contra a mão única | 10 | 9 | 011-1120-2 | -8.085328 | -34.89052 |
| 204618974 | 637666036 | desvio longo (conversão que o mapa não permite) | 100 | 8 | 202-599-1 | -8.031199 | -34.954846 |
| 339192846 | 706175626 | só contra a mão única | 10 | 7 | 014-1442-2 | -8.085318 | -34.8905 |
| 707010072 | 129478578 | desvio longo (conversão que o mapa não permite) | 66 | 7 | 020-241-1 | -8.122756 | -34.91243 |
| 777105569 | 796498278 | desvio longo (conversão que o mapa não permite) | 543 | 7 | 121-1312-2 | -8.078733 | -34.900134 |
| 869890189 | 984862014 | desvio longo (conversão que o mapa não permite) | 480 | 7 | 2459-1357-2 | -8.035496 | -34.984779 |
| 967932896 | 647530685 | só contra a mão única | 290 | 7 | 191-300-1 | -8.131651 | -34.91726 |
| 984862014 | 869890189 | desvio longo (conversão que o mapa não permite) | 480 | 7 | 2459-1357-1 | -8.035484 | -34.984765 |
| 30812855 | 222100952 | sem ligação no mapa | 276 | 6 | 219-682-2 | -8.076808 | -34.980316 |
| 137364477 | 628645780 | desvio longo (conversão que o mapa não permite) | 10 | 6 | 2493-1360-2 | -8.042886 | -34.934622 |
| 316808706 | 901242726 | desvio longo (conversão que o mapa não permite) | 10 | 6 | 152-343-2 | -8.141269 | -34.933348 |
| 573648907 | 628649580 | desvio longo (conversão que o mapa não permite) | 10 | 6 | 2493-1360-2 | -8.038196 | -34.943109 |
| 573648911 | 573648910 | desvio longo (conversão que o mapa não permite) | 10 | 6 | 2493-1360-1 | -8.042843 | -34.934963 |
| 628924113 | 626263369 | desvio longo (conversão que o mapa não permite) | 121 | 6 | 2429-1193-1 | -8.039164 | -34.943624 |
| 703515996 | 318869517 | desvio longo (conversão que o mapa não permite) | 10 | 6 | 2493-1360-1 | -8.039965 | -34.940183 |
