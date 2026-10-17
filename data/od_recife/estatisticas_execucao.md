# Estatísticas da execução (gerado por gerar_od.py)

- Linhas no CSV: 167308; descartadas sem EXPANSAO válida: 1987
- Soma de EXPANSAO (linhas válidas): 4,069,376 pessoas
- trabalho_ida: 73219
- trabalho_volta: 70183
- educacao_ida: 12894
- educacao_volta: 12408
- dependente_ida: 32652
- dependente_volta: 32652
- modos_nao_mapeados: []
- Pernas carro_dirigindo: 34555 respostas, 325,793 pessoas-viagem/dia útil
- ...com horário de partida conhecido: 99.4%
- ...com origem aproximada pela residência: 1.7%
- carro_dirigindo em pico_manha (06h-09h): 121,410 pessoas (37.3% do dia)
- carro_dirigindo em pico_tarde (17h-20h): 106,310 pessoas (32.6% do dia)

## Carro dirigindo em motivos sem zona de destino (fora da matriz)

- MODO_TRANSP_OUTRO_TRAB: 15,807 pessoas (n=2079)
- MODO_TRANSP_COMPRAS: 90,970 pessoas (n=9683)
- MODO_TRANSP_MEDICO: 26,221 pessoas (n=2541)
- MODO_TRANSP_SERVICO: 34,486 pessoas (n=3343)

## 15 maiores fluxos carro_dirigindo, dia útil, trabalho+educação (pessoas = veículos)

| # | origem | destino | fluxo | respostas |
|---|---|---|---:|---:|
| 1 | 253 (GOIANA (fora do Recife)) | 253 (GOIANA (fora do Recife)) | 1,706 | 38 |
| 2 | 71 (Boa Viagem) | 71 (Boa Viagem) | 1,334 | 266 |
| 3 | 77 (Boa Viagem) | 77 (Boa Viagem) | 1,236 | 98 |
| 4 | 72 (Boa Viagem) | 72 (Boa Viagem) | 1,006 | 86 |
| 5 | 124 (Graças) | 124 (Graças) | 887 | 77 |
| 6 | 127 (Graças) | 127 (Graças) | 819 | 68 |
| 7 | 2 (ILHA DE ITAMARACÁ (fora do Recife)) | 78 (Ibura) | 733 | 1 |
| 8 | 78 (Ibura) | 2 (ILHA DE ITAMARACÁ (fora do Recife)) | 733 | 1 |
| 9 | 69 (Imbiribeira) | 87 (Areias) | 695 | 73 |
| 10 | 87 (Areias) | 69 (Imbiribeira) | 695 | 73 |
| 11 | 74 (Boa Viagem) | 74 (Boa Viagem) | 626 | 58 |
| 12 | 65 (Pina) | 65 (Pina) | 619 | 20 |
| 13 | 77 (Boa Viagem) | 71 (Boa Viagem) | 593 | 72 |
| 14 | 72 (Boa Viagem) | 71 (Boa Viagem) | 591 | 67 |
| 15 | 71 (Boa Viagem) | 77 (Boa Viagem) | 591 | 72 |

## 15 maiores fluxos entre zonas diferentes, com ao menos 10 respostas

| # | origem | destino | fluxo | respostas |
|---|---|---|---:|---:|
| 1 | 69 (Imbiribeira) | 87 (Areias) | 695 | 73 |
| 2 | 87 (Areias) | 69 (Imbiribeira) | 695 | 73 |
| 3 | 77 (Boa Viagem) | 71 (Boa Viagem) | 593 | 72 |
| 4 | 72 (Boa Viagem) | 71 (Boa Viagem) | 591 | 67 |
| 5 | 71 (Boa Viagem) | 77 (Boa Viagem) | 591 | 72 |
| 6 | 71 (Boa Viagem) | 72 (Boa Viagem) | 578 | 66 |
| 7 | 23 (PAULISTA (fora do Recife)) | 59 (Boa Vista) | 496 | 40 |
| 8 | 59 (Boa Vista) | 124 (Graças) | 484 | 39 |
| 9 | 59 (Boa Vista) | 23 (PAULISTA (fora do Recife)) | 453 | 40 |
| 10 | 124 (Graças) | 59 (Boa Vista) | 450 | 37 |
| 11 | 59 (Boa Vista) | 37 (OLINDA (fora do Recife)) | 418 | 26 |
| 12 | 124 (Graças) | 215 (JABOATÃO DOS GUARARAPES (fora do Recife)) | 415 | 24 |
| 13 | 215 (JABOATÃO DOS GUARARAPES (fora do Recife)) | 124 (Graças) | 415 | 24 |
| 14 | 59 (Boa Vista) | 215 (JABOATÃO DOS GUARARAPES (fora do Recife)) | 414 | 30 |
| 15 | 215 (JABOATÃO DOS GUARARAPES (fora do Recife)) | 59 (Boa Vista) | 414 | 30 |

## Validação contra as matrizes oficiais de 2020 (só ida, residência -> destino)

| motivo   | modo                             |   respostas_oficial |   respostas_csv |   pessoas_oficial |   pessoas_csv |   corr_pares_od |   corr_total_por_origem |   corr_total_por_destino |
|:---------|:---------------------------------|--------------------:|----------------:|------------------:|--------------:|----------------:|------------------------:|-------------------------:|
| trabalho | carro_dirigindo+carro_passageiro |               20147 |           18220 |            193782 |        180140 |           0.863 |                   0.989 |                    0.974 |
| educacao | carro_dirigindo+carro_passageiro |               15945 |           10243 |            139400 |         95110 |           0.847 |                   0.974 |                    0.94  |
