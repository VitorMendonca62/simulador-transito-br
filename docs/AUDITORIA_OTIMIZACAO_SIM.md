# Auditoria: custo de CPU da simulação ("sim" alto na barra de status)

> 2026-10-20. Objetivo: baixar o indicador **CPU · sim** (tempo do worker calculando a simulação).
> Escopo: `core/src/sim/mod.rs`, `core/src/sim/route.rs`, `core/src/sim/metrics.rs`, `src/workers/core.worker.ts`.
> Medições: `~/Downloads/mapa-recife-novo.osm` (69.594 faixas, 189.977 arestas = faixas + conversões, 25.730 nós),
> `perf_sim` (4 zonas grandes, ~19 mil veículos aos 12 min) e uma variante com **256 zonas** (grade 16×16, como a OD do Recife).
> Nativo `--release` e WASM (wasm-pack `--release`, rodando no Node). O código já incluía as mudanças ainda não commitadas em `sim/mod.rs`.

## 1. O que o indicador mede

`sim %` = tempo que o worker passou em `simSlice` ÷ tempo real. O worker roda passos fixos de 0,2 s. Para acompanhar
o tempo real numa velocidade `N×` ele precisa de `5·N` passos por segundo:

```
sim % ≈ 0,5 × N × (ms por passo)   (+ quadros: 15/s × custo do snapshot)
```

| ms/passo | 1× | 5× | 10× | 20× | 30× |
|---|---|---|---|---|---|
| 5 ms | 2,5 % | 12,5 % | 25 % | 50 % | 75 % |
| 10 ms (hoje, ~19 mil veíc.) | 5 % | 25 % | 50 % | **100 %** | não acompanha |

Ou seja: o número sobe **linearmente com a velocidade escolhida e com o número de veículos**. Em 20× ou 30× com a
rede cheia, 100 % não é defeito do indicador, é o teto. Para baixar, só há dois caminhos: **menos ms por passo** ou
**menos passos** (dt maior, ou tirar trabalho do worker da simulação).

## 2. Medições

### 2.1 Passo completo

| Cenário | Nativo | WASM (Node) |
|---|---|---|
| 4 zonas, 13 mil veículos | 7,3–9,0 ms | 7,35 ms |
| 4 zonas, 19 mil veículos | 9,5–13 ms | 10,0 ms |
| 256 zonas, 13 mil veículos | 10,6 ms | 10,1 ms |
| Passo **sem nenhum veículo** (custo fixo, 4 zonas) | ~1,0 ms | — |
| `sim_init` com 256 zonas | — | **4,9 s** |
| `snapshot` (19 mil veículos) | 1,6 ms | 2,9 ms |

Três conclusões:

1. **O WASM não é o gargalo.** No Node ele ficou igual ao nativo (± ruído). Ganho de compilador tem pouco a dar; o
   ganho está no algoritmo e no acesso à memória.
2. **Houve regressão.** A auditoria de roteamento (2026-10-18) mediu 6,9 ms com ~19 mil veículos no mesmo cenário;
   hoje são 10–13 ms. Os candidatos são a antecipação de nó à frente (`ahead_stop_at`), a cortesia (`courtesy`) e a
   ocupação do ponto de conflito. A ocupação foi medida sozinha (desligada numa cópia): custa só **~0,35 ms/passo
   (3–4 %)**, então não explica a maior parte da regressão.
3. **Com muitas zonas, o roteamento em tempo real pesa.** Com 256 zonas, `route_step` sobe de 0,43 para **1,8 ms por
   passo** em média, porque o orçamento por passo vai de 2.000 para 12.000 nós. Em alguns blocos a fase de sinais e
   rotas chega a 4 ms.

### 2.2 Onde vai o tempo (4 zonas, média dos 12 min, ~10 mil veículos em média)

Instrumentação com `rdtsc` numa cópia do núcleo. `Instant::now()` custa caro demais aqui e distorcia tudo, ver §6.

| Parte do passo | ms/passo | % | Chamadas/passo |
|---|---|---|---|
| **Laço de aceleração** (total) | **4,12** | **52 %** | |
| ↳ `leader` (inclui os dois abaixo) | 2,12 | 27 % | 9.729 |
| ↳↳ `may_enter` (a cada 2 passos por veículo) | 0,84 | 11 % | 1.434 |
| ↳↳ `ahead_stop_at` (**todo passo**) | 0,59 | 7 % | 2.826 |
| ↳ `occ_obstacle` | 0,41 | 5 % | 2.283 |
| ↳ `courtesy` | 0,33 | 4 % | 9.162 |
| ↳ varredura das 190 mil arestas (inclusive as vazias) | 0,35 | 4 % | 1 |
| ↳ IDM, antecipação de curva, resto | ~0,9 | 12 % | |
| Geração (`spawn`) | 0,83 | 10 % | 1 |
| Mudança de aresta + `enforce_gaps` ×2 | ~0,8 | 10 % | |
| Integração + `metrics.sample` | 0,66 | 8 % | |
| Troca de faixa (`consider_lane_change`) | 0,60 | 7 % | 1.144 |
| `route_step` (4 zonas; **1,8 ms com 256 zonas**) | 0,43 | 5 % | 1 |
| `update_signals` | 0,13 | 2 % | 1 |
| `check_overlaps` (a cada 5 passos), `metrics.end_tick` | 0,08 | 1 % | |

## 3. Problemas encontrados

| # | Problema | Onde | Impacto |
|---|---|---|---|
| P1 | O laço de aceleração percorre **todas as 190 mil arestas** a cada passo, inclusive as vazias (`Vec<Vec<u32>>`, ~4,5 MB de cabeçalhos). | `step`, laço `for e in 0..edge_veh.len()` | 0,35 ms fixos, com ou sem veículos |
| P2 | `ahead_stop_at` roda **todo passo** para todo primeiro da fila, e chama `stay_cost` e `may_enter_from`. O `may_enter` da própria linha já é reavaliado só a cada 2 passos. | `leader` | 0,6 ms |
| P3 | `route_step`: o orçamento por passo cresce com `nz × nl` (até 12 mil nós de Dijkstra por passo) e roda no **mesmo** worker da simulação. Com a OD do Recife (253 zonas), é a maior parcela isolada. | `route_step`, `job_run` | 0,4 ms (4 zonas), 1,8–4 ms (256 zonas) |
| P4 | `spawn` percorre as **69.594 filas de entrada** (`pending`, uma `VecDeque` por faixa) a cada passo e todos os pares `nz²` (65 mil com 256 zonas). | `spawn` | 0,4–0,8 ms |
| P5 | Os campos do veículo estão em ~40 vetores SoA, mas o acesso é por vizinho (líder, conflito, fila): cada `s[j]`, `v[j]`, `vlen[j]`, `wait[j]` é uma linha de cache diferente. Com 20 mil veículos o estado passa de 2 MB e sai do L2. | `leader`, `may_enter_from`, `occ_by`, `courtesy`, `around` | é o "resto" caro do laço de aceleração (~30–40 % dele) |
| P6 | `courtesy` é chamada para **todo** veículo em faixa sempre que existe algum pedido de passagem no mapa (quase sempre), com busca binária em `wants`. | laço de aceleração | 0,33 ms |
| P7 | `update_signals` zera e percorre **todas** as pontas (`end_state`, `red_age`), não só as semaforizadas. | `update_signals` | 0,13 ms |
| P8 | `metrics.sample` escreve em 6–8 vetores diferentes por veículo (`occ`, `spd`, `w_occ`, `w_spd`, `stop`, `w_stop`…), indexados por trecho. | integração | parte dos 0,66 ms |
| P9 | Alocações por passo: `movers` (`Vec` novo), `HashSet` novo em `check_overlaps`, `Rc` clonado a cada `conflicts_of`. | `step`, `check_overlaps` | pequeno, mas evitável |
| P10 | Os conflitos são calculados sob demanda: o 1º minuto tem picos de 17–23 ms ("pior" passo) enquanto os cruzamentos são vistos pela primeira vez, e o mesmo acontece em `make_target` (árvores locais de destino). | `conflicts_of`, `pick_target` | travadas no início |
| P11 | Com 256 zonas, `sim_init` leva 4,9 s, e `zone_dist` (f32) + `zone_free` (u16) ocupam **256 × 69.594 × 6 B ≈ 107 MB**, mais até 64 MB de árvores locais (`LOCAL_BUDGET`). | `Sim::new` | início lento e "RAM WASM" alta |
| P12 | `wasm-opt = false` no `Cargo.toml` (o binaryen não está instalado). | build | 5–15 % (estimativa, a medir) |
| P13 | Perfilador interno com bug: não há `lap!(2)`, então a aceleração é somada à "integração" (`prof[3]`) e `prof[2]` fica sempre 0. | `step` | medições enganosas |

## 4. Opções de solução

Legenda de risco: **=** resultado idêntico (mesma semente, mesmos números); **≈** comportamento muda pouco, revalidar
com `recife_behavior`; **≠** muda o modelo, exige recalibração.

### A. Ganhos rápidos, sem mudar resultado

| Opção | Resolve | Ganho esperado | Risco | Esforço |
|---|---|---|---|---|
| A1. **Bitset de arestas ocupadas** (1 bit por aresta, 24 KB). Percorrer só os bits ligados, em ordem crescente, preserva a ordem atual, que importa porque `leader` escreve `enter_ok`/`ahead_held`, lidos por outros. | P1 | −0,35 ms fixo | = | baixo |
| A2. **Lista ordenada das filas de entrada não vazias** em vez de varrer as 69 mil `pending`. Para os pares `nz²`, um *heap* por instante com reprocessamento em ordem `(i, j)` mantém a mesma sequência do PRNG. | P4 | −0,3 a −0,6 ms | = | baixo |
| A3. `update_signals` só nas pontas semaforizadas (lista pré-calculada em `Sim::new`). | P7 | −0,1 ms | = | baixo |
| A4. Reaproveitar buffers (`movers`, `HashSet` de `check_overlaps`). | P9 | pequeno | = | baixo |
| A5. Pular `courtesy` quando a faixa não tem pedidos (flag por faixa preenchida junto com `wants`). | P6 | −0,2 ms | = | baixo |
| A6. Corrigir o `lap!(2)` e deixar contadores por função atrás de uma *feature* `prof`, com `rdtsc` ou `performance.now` no WASM. | P13 | (medição) | = | baixo |
| A7. Instalar o binaryen no build e no CI (Cloudflare) e ligar `wasm-opt -O3`. Medir antes de adotar. | P12 | 5–15 % | = | baixo |

**Soma esperada do bloco A:** −1,0 a −1,5 ms/passo, ou 10–15 % com 19 mil veículos e mais da metade do custo
fixo com a rede vazia.

### B. Layout de memória, sem mudar resultado

| Opção | Resolve | Ganho esperado | Risco | Esforço |
|---|---|---|---|---|
| B1. **Agrupar os campos "quentes"** do veículo num só registro (`struct Hot { s, v, vlen, wait }`, 16 B) em vez de 4 vetores. Assim, ler o vizinho custa 1 linha de cache em vez de 4. Fazer por acessores para limitar a mudança. | P5 | 15–25 % do laço de aceleração (−0,6 a −1 ms) | = | médio (muitos pontos de uso) |
| B2. Acumuladores de `metrics` por trecho num só struct (`[f32; 6]` por trecho). | P8 | −0,1 a −0,2 ms | = | baixo |
| B3. Conflitos num vetor plano (CSR: `conf_off[c]..conf_off[c+1]`) em vez de `Rc<Vec>` por conversão, preenchido sob demanda por nó. | P9/P10 | pequeno no passo; tira a indireção | = | baixo |

### C. Decisões menos frequentes (comportamento quase igual)

| Opção | Resolve | Ganho esperado | Risco | Esforço |
|---|---|---|---|---|
| C1. **`ahead_stop_at` no mesmo ritmo do `may_enter`** (a cada 2 passos por veículo, resultado guardado). A decisão de entrada já é reaproveitada por 1 passo; a antecipação passa a seguir a mesma regra. | P2 | −0,3 ms | ≈ | baixo |
| C2. `occ_obstacle` na faixa só quando a frente está a menos da distância de frenagem + margem (hoje: < 40 m fixos). | parte do laço | −0,1 a −0,2 ms | ≈ | baixo |
| C3. Pré-aquecer os conflitos dos nós semaforizados e das vias principais no `Sim::new` (ou em fatias nos primeiros passos). | P10 | tira os picos do 1º minuto | = | baixo |

### D. Roteamento (o maior item com a OD do Recife)

| Opção | Resolve | Ganho esperado | Risco | Esforço |
|---|---|---|---|---|
| D1. **Orçamento por tempo, não por `nz × nl`**: um teto fixo de nós por passo (ex.: 3–4 mil). A volta completa fica mais longa com muitas zonas, porque as árvores se atualizam com menos frequência. | P3 | com 256 zonas: −1 a −1,3 ms | ≈ (atualização mais lenta) | baixo |
| D2. **Priorizar zonas com demanda**: recalcular primeiro as zonas de destino com mais veículos na rede, ou só zonas com atração > 0 no horário. Zonas sem ninguém indo para elas esperam. | P3 | proporcional às zonas ociosas | ≈ | baixo |
| D3. **Roteador num 2º worker** (sem `SharedArrayBuffer`): a cada volta, o worker da simulação envia os pesos vivos (`live_pen`, `change_mul`: ~1 MB, transferível) e recebe as árvores prontas (278 KB por zona). Tira o roteamento inteiro do worker da simulação e aproveita outro núcleo. | P3 | −0,4 ms (4 zonas) a −1,8/−4 ms (256 zonas), **20–35 %** do passo com a OD | = no modelo (só o momento de chegada das árvores muda; ≈ no resultado) | médio |
| D4. Árvores por **trecho direcionado** em vez de por faixa (~2× menos nós), com o custo de troca de faixa aplicado localmente na escolha. Reduz CPU **e** os 107 MB. | P3, P11 | ~2× no roteamento e na memória | ≠ (escolha de faixa muda) | alto |
| D5. `zone_dist` em `u16` (meio segundo, como `zone_free`) e calculado sob demanda para zonas que nunca são destino. | P11 | −50 % da memória das árvores; `sim_init` mais rápido | ≈ | médio |

### E. Menos passos ou mais núcleos (estruturais)

| Opção | Ganho | Risco | Comentário |
|---|---|---|---|
| E1. dt 0,2 → 0,25 s | −20 % de passos | ≠ | O IDM aguenta, mas brecha, ocupação, amarelo e cortesia foram calibrados com 0,2 s. **Não recomendado agora.** |
| E2. Atualização em várias taxas: veículos em fluxo livre, longe de nó e de líder, integram a cada 2 passos | 10–30 % com rede pouco carregada | ≠ | Complexo de manter determinístico; só depois de A–D. |
| E3. Multithread (wasm threads + `SharedArrayBuffer` + rayon), com a aceleração em duas fases (decidir e depois aplicar `enter_ok`/`ahead_held`/`held_box`) | 2–3× com 4 núcleos | = se bem feito | Exige cabeçalhos COOP/COEP no Cloudflare Pages (`_headers`), `build-std` no nightly e uma refatoração do laço. É o portão de multithread previsto no CONTEXT. |
| E4. Teto automático de velocidade: a UI mostra a "velocidade efetiva" (sim-s por s real) e não oferece 30× quando o passo não comporta | não reduz CPU | = | Evita a sensação de que algo está errado quando o pedido é maior que a capacidade. |

### F. Fora do passo (thread da simulação, mas não `step`)

- `snapshot`: 2,9 ms em WASM a 15 Hz dá ~4 % de "sim" com 19 mil veículos. Opções: quantizar para `Int16`/`Uint8`
  (24 → 8 B por veículo), enviar só os veículos da área visível, ou cair para 10 Hz em velocidades altas.
- `sim_track` → `explain` a 4 Hz e `report` (~0,8 ms) a 1 Hz: custo pequeno, mas dá para pular quando o painel
  está fechado.

## 5. Recomendação

Por ordem (ganho ÷ risco):

1. **Bloco A inteiro + C3** (resultado idêntico, 1–2 dias). Validar com `sim_is_deterministic` e com os números de
   `recife_behavior` antes e depois, que devem bater exatamente. Ganho: 10–15 % e metade do custo fixo.
2. **D1 + D2** (orçamento de rotas fixo e zonas priorizadas). É o maior ganho para quem usa a OD do Recife, com
   pouco código. Revalidar o atraso total do `recife_behavior` com rotas em tempo real.
3. **B1** (campos quentes juntos). É o maior ganho que sobra no laço de aceleração sem tocar no modelo.
4. **C1** (antecipação a cada 2 passos). Revalidar colisões, freadas e furos de vermelho no `recife_behavior`.
5. **D3** (roteador num 2º worker), se depois de 1–4 o roteamento ainda for > 15 % do passo com a OD.
6. **E3** (multithread) só se a meta for 50 mil veículos a 10× ou mais. Deixar E1/E2 de fora enquanto o
   comportamento estiver em calibração.

Estimativa somando 1–4: **~10 ms → ~6–7 ms por passo** com 19 mil veículos (4 zonas), e **~10 → ~5–6 ms** com 256
zonas. Em 10× isso leva o indicador de ~50 % para ~30 %, e passa a acompanhar ~30× onde hoje para em ~20×.

## Status

**2026-10-20: A1–A6 implementados** (`sim/mod.rs`). A7 (wasm-opt) está pendente porque depende de instalar o binaryen.
- A1: `occ_bits` (1 bit por aresta, atualizado em `insert_sorted`/`unlink`). O laço de aceleração percorre só a
  lista `active`, em ordem crescente.
- A2: `pend_gates` (faixas com fila de entrada) e `row_next` (próximo nascimento por zona de origem, recalculado em
  `refresh_matrix` e depois de cada linha processada). A soma da fila no fim do passo também usa `pend_gates`.
- A3: `sig_ends` (pontas semaforizadas). A5: `want_lane`. A4: `movers`, `inside` e `overlap_now` reaproveitados.
  A6: `lap!(2)`, então `prof[2]` agora é a aceleração e `prof[3]` só a integração.
- Validação: `recife_behavior` (3 sementes, demanda 1× e 2×) e a população minuto a minuto do `perf_sim` ficaram
  **idênticos** à base, linha por linha. Os 50 testes passam.

| Medida (nativo) | Antes | Depois |
|---|---|---|
| `recife_behavior`, demanda 1× | 1,85 ms/passo | **0,78 ms** (−58 %) |
| `recife_behavior`, demanda 2× | 2,35 ms/passo | **1,05 ms** (−55 %) |
| `perf_sim`, 19 mil veículos (min 12) | 10,05 ms/passo | **7,93 ms** (−21 %) |
| `perf_sim`, 13 mil veículos (min 8) | 7,58 ms/passo | **6,21 ms** (−18 %) |

O ganho é maior com a rede pouco carregada porque o que saiu era quase todo custo fixo, que não depende do número
de veículos. Agora o laço de aceleração é ~55 % do passo, e o próximo alvo é B1 (campos quentes juntos).

## 6. Como medir (para repetir antes e depois)

- Passo e fases: `PERF_OSM=~/Downloads/mapa-recife-novo.osm cargo test --release perf_sim -- --ignored --nocapture`
  (corrigir o `lap!(2)` antes, ver P13).
- Comportamento (deve ficar igual nos itens "="): `recife_behavior` com `SIM_SEED0` fixo; `sim_is_deterministic`.
- Por função: nesta auditoria, macro `tm!(k, expr)` com `core::arch::x86_64::_rdtsc()` acumulando em `thread_local`
  (custo ~10 ns; `Instant::now()` custava ~1 µs aqui e multiplicava o passo por 10). Vale manter como *feature* `prof`.
- WASM: `wasm-pack build core --target nodejs --release` numa pasta temporária e um script Node chamando
  `import_osm` → `sim_init` → `sim_step(1)` em laço (mesmas zonas do `perf_sim`). No navegador, o indicador
  `sim %` da barra de status.
