# Relatório — semáforos do Recife

Gerado por `scripts/semaforos_recife/gerar_semaforos.py`. Fonte oficial: [Localização dos Semáforos](https://dados.recife.pe.gov.br/dataset/localizacao-dos-semaforos) (Autarquia de Trânsito e Transporte Urbano do Recife - CTTU; licença Open Data Commons Open Database License (ODbL); recurso CSV atualizado em 2026-03-02). OSM: snapshot Overpass `2026-10-18T18:45:03Z` (ODbL).

## Números principais

- Semáforos oficiais (registros no CSV): **712**
- Interseções/locais semaforizados (após agrupamento): **680** (523 cruzamentos, 157 travessias/meio de quadra; 32 com mais de um semáforo; 0 marcados para revisão manual)
- Coordenadas ausentes: 0; corrigidas na leitura: 1 (693: latitude: vírgula final removida); fora da faixa plausível: 0
- IDs duplicados: 0; coordenadas idênticas: 0; pares a menos de 3 m: 0 ; grupos com endereço textual idêntico: 2
- Fora dos polígonos de bairro: 1 [('091', 1.6)]; fora do município (> 200 m de qualquer bairro): 0
- Bairro publicado diferente do bairro espacial: 195 (26 com nome que não é bairro oficial, 98 a até 100 m do bairro publicado, 71 a mais de 100 m)
- Confiança da localização: alta 659, média 35, baixa 18
- OSM: 1376 nós com semáforo no município. Oficiais com correspondência provável (≤ 15 m): 482; possível divergência (15–40 m): 140; sem correspondência (> 40 m): 90; sem coordenada: 0. Nós OSM sem semáforo oficial a até 40 m: 139.
- Atenção: 988 dos 1376 nós OSM têm `source=Dados Abertos Prefeitura do Recife` (import de 2014 desta mesma base); entre as correspondências prováveis, 410 são nós importados. Essas coincidências **não** são validação independente. Oficiais com nó OSM de mesmo `ref` importado: 573 (mediana de distância 7.6 m; > 40 m: 29).

## Auditoria do CSV oficial

Arquivo `semaforos-do-recife.csv`, colunas: `semaforo`, `localizacao1`, `localizacao2`, `bairro`, `latitude`, `longitude`, `tipo`, `funcionamento`, `Unnamed: 8`.

- Valores de `tipo`: {'E/Git': 557, 'E': 137, 'E/DIG': 15, 'E/GIt': 3}
- Valores de `funcionamento`: {'Veicular': 336, 'Ocasional/Ped.': 217, 'Veic.C/Ped.Paral.': 58, 'Veic.C/Ped.Ocas.': 48, 'Veic.C/Ped.': 32, 'Pedestre': 14, 'Veic.c/Ped.Ocas.': 6, 'Veic.C/Laço Det.': 1}
- Colunas não documentadas no dicionário: {'Unnamed: 8': {'1': 626, nan: 86}}
- IDs ausentes na sequência 1..máx: nenhum
- Endereço textual idêntico (possível duplicata lógica ou semáforos em sentidos opostos): [['336', '617'], ['515', '590']]

### Recortes oficiais complementares

- **Semáforos equipados para deficientes visuais**: 67 registros; colunas ['semaforo', 'localizacao1', 'localizacao2', 'bairro', 'latitude', 'longitude', 'horario', 'tipo', 'funcionamento']; IDs sem par na base principal: nenhum; coordenada > 5 m da base principal: [('017', 16.1), ('196', 6.2), ('255', 4119.2), ('342', 8.9)]; valores com vírgula decimal: 0
- **Semáforos equipados com sinalizador para ciclistas**: 47 registros; colunas ['semaforo', 'localizacao1', 'localizacao2', 'bairro', 'latitude', 'longitude', 'tipo', 'funcionamento']; IDs sem par na base principal: nenhum; coordenada > 5 m da base principal: [('118', 11.9), ('150', 12.7)]; valores com vírgula decimal: 0
- **Semáforos com iluminação suspensa para pedestres**: 37 registros; colunas ['semaforo', 'localizacao1', 'localizacao2', 'bairro', 'latitude', 'longitude', 'tipo', 'funcionamento']; IDs sem par na base principal: nenhum; coordenada > 5 m da base principal: [('137', 7.1), ('309', 22.6)]; valores com vírgula decimal: 37

## Distribuição por bairro

| bairro                |   semaforos |   intersecoes |
|:----------------------|------------:|--------------:|
| Boa Viagem            |         105 |           105 |
| Santo Amaro           |          44 |            42 |
| Imbiribeira           |          43 |            39 |
| São José              |          28 |            27 |
| Madalena              |          27 |            24 |
| Santo Antônio         |          26 |            25 |
| Cordeiro              |          24 |            21 |
| Afogados              |          22 |            22 |
| Graças                |          22 |            22 |
| Ibura                 |          21 |            20 |
| Boa Vista             |          21 |            21 |
| Pina                  |          20 |            20 |
| Iputinga              |          16 |            14 |
| Casa Amarela          |          16 |            16 |
| Tamarineira           |          14 |            13 |
| Encruzilhada          |          14 |            14 |
| Espinheiro            |          13 |            13 |
| Derby                 |          12 |            10 |
| Ipsep                 |          11 |            11 |
| Campo Grande          |          11 |            11 |
| Prado                 |          11 |            10 |
| Areias                |          10 |             7 |
| Recife                |           9 |             8 |
| Torre                 |           9 |             9 |
| Parnamirim            |           9 |             9 |
| Caxangá               |           9 |             7 |
| Soledade              |           8 |             8 |
| Barro                 |           7 |             7 |
| Estância              |           7 |             7 |
| Água Fria             |           6 |             6 |
| San Martin            |           6 |             6 |
| Ilha do Leite         |           5 |             5 |
| Apipucos              |           5 |             5 |
| Casa Forte            |           5 |             5 |
| Dois Irmãos           |           5 |             4 |
| Ilha do Retiro        |           5 |             4 |
| Tejipió               |           4 |             4 |
| Arruda                |           4 |             4 |
| Beberibe              |           4 |             4 |
| Aflitos               |           4 |             4 |
| Porto da Madeira      |           4 |             4 |
| Macaxeira             |           4 |             4 |
| Jaqueira              |           4 |             4 |
| Paissandu             |           4 |             3 |
| Monteiro              |           4 |             4 |
| Caçote                |           3 |             3 |
| Cabanga               |           3 |             2 |
| Zumbi                 |           3 |             3 |
| Torreão               |           3 |             3 |
| Torrões               |           3 |             2 |
| Várzea                |           3 |             3 |
| Vasco da Gama         |           2 |             2 |
| Alto do Mandu         |           2 |             2 |
| Cohab - Ibura de Cima |           2 |             2 |
| Jardim São Paulo      |           2 |             2 |
| Hipódromo             |           2 |             2 |
| Cidade Universitária  |           2 |             2 |
| Rosarinho             |           2 |             2 |
| Morro da Conceição    |           2 |             2 |
| Alto José do Pinho    |           1 |             1 |
| Fundão                |           1 |             1 |
| Engenho do Meio       |           1 |             1 |
| Campina do Barreto    |           1 |             1 |
| Bongi                 |           1 |             1 |
| Coelhos               |           1 |             1 |
| Coqueiral             |           1 |             1 |
| Curado                |           1 |             1 |
| Ilha Joana Bezerra    |           1 |             1 |
| Poço                  |           1 |             1 |
| Jordão                |           1 |             1 |
| Mangueira             |           1 |             1 |
| Linha do Tiro         |           1 |             1 |
| Ponto de Parada       |           1 |             1 |
| Sancho                |           1 |             1 |
| Santana               |           1 |             1 |

## Vias com mais semáforos (via principal publicada)

| via (rótulo mais frequente)        |   semaforos |
|:-----------------------------------|------------:|
| Av. Caxangá                        |          32 |
| Av. Norte Miguel Arraes de Alencar |          29 |
| Av. Boa Viagem                     |          28 |
| Av. Mascarenhas de Morais          |          28 |
| Av. Recife                         |          24 |
| Av. Gov. Agamenon Magalhães        |          20 |
| Av. Conselheiro Aguiar             |          19 |
| Av. Beberibe                       |          19 |
| Av. Domingos Ferreira              |          19 |
| Av. Cruz Cabugá                    |          14 |
| Av. Abdias de Carvalho             |          14 |
| Av. Dr. José Rufino                |          13 |
| Av. Conde da Boa Vista             |          12 |
| Av. 17 de Agosto                   |          12 |
| Av. Dantas Barreto                 |           9 |

## Interseções marcadas para revisão manual

| intersection_id   | intersecao   | bairro   | qtd_semaforos   | diametro_m   | motivo_revisao   | ids_oficiais   |
|-------------------|--------------|----------|-----------------|--------------|------------------|----------------|

## Disponibilidade dos parâmetros de simulação (nº de interseções)

| dado_necessario                  |   disponível na fonte |   indisponível publicamente |   inferência possível, mas não confirmada |
|:---------------------------------|----------------------:|----------------------------:|------------------------------------------:|
| coordenacao_onda_verde           |                     0 |                         680 |                                         0 |
| deteccao_veicular                |                     1 |                         679 |                                         0 |
| estado_operacional_tempo_real    |                     0 |                         680 |                                         0 |
| grupos_semaforicos               |                     0 |                         680 |                                         0 |
| numero_fases                     |                     0 |                           0 |                                       680 |
| operacao_adaptativa              |                     0 |                         148 |                                       532 |
| plano_por_horario                |                     0 |                         680 |                                         0 |
| preempcao_emergencia             |                     0 |                         680 |                                         0 |
| prioridade_onibus                |                     0 |                         680 |                                         0 |
| prioridade_pedestre              |                     0 |                         332 |                                       348 |
| sentidos_e_movimentos_permitidos |                     0 |                           0 |                                       680 |
| tempo_ciclo                      |                     0 |                         680 |                                         0 |
| tempos_verde_amarelo_vermelho    |                     0 |                         680 |                                         0 |
