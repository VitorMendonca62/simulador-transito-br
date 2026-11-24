# Auditoria de desempenho e meta (2026-11-24)

> Continuação de `AUDITORIA_OTIMIZACAO_SIM.md` (outubro) e da fase 2 de `VERSAO_2.md`. Desde então entraram as linhas
> de ônibus (fases 3–6) e as motos (fases 7–9), e o passo ganhou mais ~15 % (commit `5f0a262`). Esta auditoria mede o
> estado atual com o cenário completo do app, aponta onde ainda há ganho e fixa uma **meta de desempenho** mensurável.

## 1. Resumo

1. **O maior ganho imediato não está no núcleo, está no laço do worker.** Cada fatia de 10 ms de simulação é seguida
   de `setTimeout(simLoop, 4)`, que na prática pausa ~3–4 ms. Isso limita o worker a **~78 % de ocupação**: quando a
   simulação está atrasada, ela fica parada um quarto do tempo. Trocando a pausa por um retorno imediato ao laço de
   eventos, o ritmo a 30× subiu **+37 a +50 %** (A/B pareado: 14,7 → 20,2× e 12,9 → 19,4×), com o mesmo custo por passo.
2. **O passo nativo melhorou muito desde outubro** (4 zonas, 19 mil veículos: 7,9 → **3,7 ms**). No cenário real
   (OD 2018 com 130 zonas, CTTU, linhas, 25 % de motos) ele custa **5,5 ms com 15,6 mil veículos**, metade na aceleração.
3. **No navegador o WASM custa ~1,5× o nativo**, e a compilação não resolve: `wasm-opt -O3` e SIMD (`+simd128`)
   ficaram dentro do ruído. O ganho que falta no passo tem de vir do algoritmo: `stay_cost` e `may_enter_from`
   (~16 % das amostras) e a criação de pontos de destino (`make_target`), que causa passos de 20–45 ms.
4. **A abertura e o início ficaram mais lentos** do que o registrado na fase 2: abrir o Recife 1,6 → **2,3 s** e
   iniciar a simulação 0,6 → **1,4–1,6 s**. As linhas de ônibus e a matriz de motos entraram depois daquela medida.
5. **A thread principal não pôde ser medida aqui** (contêiner sem GPU: o WebGL roda por software e ocupa 99 % da
   thread). O JavaScript do app, porém, pesa menos de 1 %. A meta de ≥ 55 fps da fase 2 continua sem medição numa
   máquina real. O indicador "CPU tela" engana nesse caso: marca 1 % com a página a 5 quadros por segundo.

## 2. Meta de desempenho

### Cenário de referência
Recife aberto pelo diálogo com **zonas da OD 2018 + semáforos da CTTU + linhas de ônibus**, parâmetros padrão
(OD 100 %, 7h, 25 % de motos, parcelas típicas), velocidade pedida **30×**, painéis padrão. Aos ~20 min simulados há
~17–19 mil veículos na rede. É o que `scripts/perf/bench-browser.mjs` monta sozinho.

### Meta principal: "Recife a 30×"
> Com o cenário de referência, numa máquina de 4 núcleos ou mais, a simulação **acompanha 30×** (ritmo ≥ 28×) durante a
> 1ª hora simulada, **sem travadas visíveis** (nenhum passo > 15 ms depois do 1º minuto) e com o mapa a **≥ 55 fps** no
> pan/zoom. Sem mudar o resultado da simulação nos itens marcados "=".

### Métricas

O ritmo no navegador depende da máquina e, neste contêiner, varia até 40 % de um momento para outro. Por isso o núcleo
é acompanhado pelo nativo (`perf_sim`, mais estável), e o produto pelo ritmo no navegador, sempre em A/B na mesma
máquina e no mesmo momento.

| # | Métrica | Como medir | Hoje | Meta |
|---|---|---|---|---|
| M1 | **Ritmo a 30×** (s simulados por s real), rede cheia | `bench-browser.mjs`, 2ª metade de 120 s | 13–20× (contêiner, 4 vCPU) | **≥ 28×** |
| M2 | Ocupação do worker quando atrasado | idem (`workerPct`) | 78 % (teto do laço) | ≥ 95 % |
| M3 | **Passo nativo, cenário real**, min 20 | `perf_sim` com `CTTU=1 BUS=1 PERF_SIM_JSON` | 5,5 ms (15,6 mil veíc.) | **≤ 3,5 ms** (−35 %) |
| M4 | Pior passo nativo depois do 1º minuto | idem | 23–31 ms | **≤ 15 ms** |
| M5 | Passo WASM, cenário real, 2ª metade de 20 min | `bench-wasm.cjs` | 5,6–9,1 ms | −35 % sobre a base da mesma máquina |
| M6 | Iniciar → simulação pronta | `bench-browser.mjs` (`iniciarMs`) | 1,4–1,6 s | **≤ 0,8 s** |
| M7 | Abrir o Recife (clique → mapa com OD) | idem (`abrirMs`) | 2,3–2,4 s | **≤ 1,5 s** |
| M8 | fps no pan/zoom com a simulação a 30× | máquina real com GPU, painel visível | não medido | **≥ 55 fps**, p95 do quadro ≤ 25 ms |
| M9 | Memória WASM aos 60 min simulados | barra de status / `mem` no Node | 308 MB | ≤ 250 MB |
| M10 | Download do Recife (gzip) | build | ~3,5 MB | não passar de 3,5 MB |
| — | Comportamento | `recife_behavior`, `sim_is_deterministic` | — | itens "=" idênticos; "≈" com 6 sementes |

Prioridade: M1/M2 e M4 são o que o usuário sente (velocidade e travadas). M3/M5 são o caminho para M1 em máquinas mais
fracas. M6–M8 tratam a experiência fora da simulação. M9–M10 são guarda-corpos.

## 3. Medições

Ambiente: contêiner Linux com 4 vCPU, sem GPU (Chromium headless com WebGL por software), commit `5f0a262`. WASM
compilado **sem** `wasm-opt` (o binaryen não baixou aqui; ver §6, que mostra que ele não muda o passo). Navegador pelo
servidor do Vite (React em modo de desenvolvimento; o JS da página pesa < 1 %, então isso não altera as conclusões).

### 3.1 Navegador, cenário de referência a 30×

| Medida | Valor |
|---|---|
| Abrir o Recife (OSM + CTTU + linhas + OD) | 2,26–2,40 s |
| Iniciar → simulação rodando | 1,41–1,58 s |
| Veículos na rede (2ª metade, 1–2 min de relógio) | 18,4–19,7 mil |
| Ritmo obtido pedindo 30× | 13–20× (variou com o momento da máquina) |
| Ocupação do worker | 64–78 %, nunca acima |
| ms por passo no worker (ocupado ÷ passos, inclui o quadro) | 7,8–12,2 ms |
| Pior passo por segundo | 15–49 ms |
| Memória WASM / heap JS | 301–308 MB / ~167 MB (modo dev) |
| Thread principal | ~5 quadros/s e ~1,1 s de tarefas longas por segundo (desenho por software) |

**Perfil da thread principal** (10 s com a simulação a 30×): 98,8 % "(program)", código nativo do navegador, aqui a
rasterização WebGL por software. Do app: `VehicleLayer.write` 0,1 %, Pixi (`getVao`, `getGlBuffer`, `drawElements`)
~0,3 %, React ~0,1 %. O desenho já interpola os veículos na GPU e recorta por ladrilho; não há gargalo de JavaScript.

### 3.2 A/B do laço do worker (pausa de 4 ms → retorno imediato)

Mesma máquina, rodadas alternadas, 100 s cada, rede cheia:

| Rodada | Ritmo (base → mudança) | Ocupação do worker | ms por passo |
|---|---|---|---|
| 1 | 14,7× → **20,2×** (+37 %) | 78 % → 100 % | 10,6 → 10,0 |
| 2 | 12,9× → **19,4×** (+50 %) | 78 % → 99 % | 12,2 → 10,2 |
| Exploratória (outro momento) | 20,1× → **27,6×** (+37 %) | 78 % → 100 % | 7,8 → 7,3 |

O custo por passo não muda (é o mesmo código): o ganho vem só de o worker parar de dormir.

### 3.3 Núcleo nativo (`perf_sim`, `--release`, núcleo fixo)

| Cenário | Veículos | Passo médio | Pior passo | `Sim::new` |
|---|---|---|---|---|
| 4 zonas sintéticas, min 12 | 19,2 mil | **3,74 ms** (out.: 7,93) | 16–37 ms | — |
| **Real** (OD 2018 + CTTU + linhas + motos), min 20 | 15,6 mil | **5,54 ms** | 23–31 ms | 1,69 s |

Fases do cenário real (último minuto): aceleração 2,89 ms (52 %), troca de faixa 0,80 (15 %), sinais e rotas 0,77
(14 %), integração 0,51 (9 %), mudança de aresta 0,36 (7 %), geração 0,09, folgas 0,08. No nativo as rotas rodam no
próprio passo; no navegador vão para os workers de rotas.

**Perfil por função** (`perf record`, cenário real, 16 min, inclui o início):

| Função | Amostras | Observação |
|---|---|---|
| `Sim::step` (corpo, com o que foi embutido) | 28,1 % | laço de aceleração, integração |
| `LaneRouter::tree` | 16,4 % | rotas; no navegador, nos workers de rotas |
| `stay_cost` | 8,8 % | chamada de 14 lugares; percorre todas as conversões de saída a cada chamada |
| `may_enter_from` | 7,2 % | decisão de entrada no nó |
| `spawn_vehicle` | 4,3 % | quase tudo é `make_target` embutido (árvore local do ponto de destino) |
| `lane_end_lead` | 4,1 % | |
| `Sim::new_ext` | 3,1 % | início |
| `ahead_stop_at`, `merge_ghost`, `occ_by`, `leader`, `conflicts_of`, `enforce_gaps`, `occ_obstacle` | 1,5–2,7 % cada | |

### 3.4 WASM fora do navegador (Node, 1 núcleo, rotas no passo)

| Medida | Valor |
|---|---|
| `import_osm_full` (OSM + CTTU + linhas) | 1,30–1,46 s |
| `sim_init` | 2,6–3,3 s (com todas as árvores no mesmo thread; no app vão para os workers) |
| Passo, 2ª metade de 20 min (~15 mil veíc.) | 5,6–9,1 ms entre rodadas; ≈ 1,5× o nativo |
| Pior passo | 27–45 ms |
| `sim_snapshot`, 15–17 mil veículos | 1,8–2,7 ms, ~28 B por veículo (467 KB) |
| Memória: depois do import / do `sim_init` / 30 / 60 / 90 min | 109 / 247 / 293 / 308 / 313 MB (estabiliza, sem vazamento) |

### 3.5 Download

JS 591 KB (181 KB gzip), WASM ~0,9 MB com `-O3` no CI (~500 KB gzip), mapa 2,0 MB (`.osm.gz`), linhas 2,7 MB (511 KB
gzip), OD 711 KB (210 KB gzip), CTTU 18 KB gzip. Total do Recife ≈ 3,5 MB comprimido. `/assets/*` tem cache imutável;
`/data/*` revalida a cada abertura (barato, 304).

## 4. Achados

| # | Achado | Onde | Impacto |
|---|---|---|---|
| A1 | **Teto de 78 % no worker**: cada fatia de 10 ms é seguida de `setTimeout(simLoop, 4)`. Atrasado, o worker dorme ~¼ do tempo. | `core.worker.ts`, `simSlice` | **−27 a −33 % de ritmo** em 30× com a rede cheia |
| A2 | `stay_cost` é recalculado do zero a cada chamada: percorre as conversões de saída, calcula `noise()` por conversão e lê `zone_dist[z][nx]` em ordem aleatória (tabela de 130 zonas × faixas). Os 14 chamadores, sobretudo a troca de faixa, repetem a mesma faixa várias vezes por passo. | `sim/mod.rs:2078` | ~9 % das amostras |
| A3 | `may_enter_from` e `lane_end_lead` somam ~11 %. A auditoria de outubro já reduziu acessos; o que resta é cálculo por veículo na linha de retenção. | `sim/mod.rs` | ~11 % |
| A4 | **Passos de 20–45 ms** quando um veículo sorteia um ponto de destino novo: `make_target` roda uma árvore local da zona dentro do passo. Ocorre a sessão inteira (sempre aparecem pontos novos até `LOCAL_BUDGET`). | `pick_target`, `make_target` | travadas curtas; ~4 % do passo |
| A5 | **WASM ≈ 1,5× o nativo** e as opções de compilação não ajudam (§6). | build | só o algoritmo reduz |
| A6 | **Início e abertura regrediram** desde a fase 2 (0,6 → 1,4–1,6 s; 1,6 → 2,3 s). O `sim_init` monta a matriz de carros e a de motos, as linhas e as árvores iniciais; não há medição por parte no navegador. | `sim_init`, `loadRecife` | espera perceptível ao abrir e ao iniciar |
| A7 | `sim_init` soma **+138 MB** de WASM: `zone_dist` (f32) e `zone_free` (u16) por zona × faixa, árvores locais até `LOCAL_BUDGET` (64 MB) e o grafo do roteador. Cada worker de rotas tem a própria cópia do grafo (não medida). | `Sim::new` | memória; D5 de outubro segue pendente |
| A8 | **Indicadores enganam em máquina fraca**: "CPU tela" soma só o JS do laço de desenho (marcou 1 % com a página a 5 fps); "sim %" passa de 100 % (134–151 %) em alguns segundos, porque o tempo ocupado chega do worker em janelas que não coincidem com o relógio da página. | `PerfStats.tsx`, `perf.ts` | diagnóstico errado do usuário |
| A9 | Quadro de veículos (`sim_snapshot`) a 15 Hz: 2,3 ms × 15 = ~3,5 % do worker. Em 20–30× a interpolação na GPU esconderia 10 Hz. | `core.worker.ts` | ~1 % de ritmo |
| A10 | O desenho não foi medido com GPU real. A arquitetura está boa (interpolação no shader, recorte por ladrilho, LOD), mas a meta de 55 fps da fase 2 nunca foi conferida com o painel visível. | `MapView`, `VehicleLayer` | desconhecido |

## 5. Plano para bater a meta

Risco: **=** resultado idêntico; **≈** comportamento quase igual (6 sementes no `recife_behavior`); **≠** muda o modelo.

### Etapa 1: laço do worker e indicadores (1 dia, =)

| Item | O que fazer | Ganho esperado | Métrica |
|---|---|---|---|
| 1.1 (A1) | Se a fatia saiu por estourar o orçamento (`simCarry ≥ SIM_DT`), devolver o controle com `MessageChannel` (sem atraso mínimo; mensagens das rotas e do ack continuam sendo atendidas entre fatias). Se a simulação está em dia, `setTimeout` até o próximo passo devido. Importante: o retorno imediato **sempre** deixaria o worker a 100 % mesmo em 1×. | **+37–50 % de ritmo** (medido) | M1, M2 |
| 1.2 (A8) | "CPU tela" pela soma das tarefas longas (`PerformanceObserver('longtask')`) ou pelo intervalo entre `requestAnimationFrame`, não só pelo JS do laço. `busyMs` com o carimbo de tempo do worker, para a taxa não passar de 100 %. | (diagnóstico) | — |
| 1.3 (A9) | Quadro a 10 Hz quando a velocidade ≥ 20×. | ~1 % | M1 |

### Etapa 2: passo do núcleo (3–5 dias, =)

| Item | O que fazer | Ganho esperado | Métrica |
|---|---|---|---|
| 2.1 (A2) | Cache de `stay_cost` por veículo: chave (faixa, versão da árvore da zona de destino). A versão sobe quando chega um lote de rotas para a zona. `k.pen` e o ruído do motorista não mudam durante a viagem. Invalidar quando muda o comportamento da moto (contramão) ou a faixa de ônibus liberada. | −5 a −8 % do passo | M3, M5 |
| 2.2 (A4) | Tirar `make_target` do passo: pedir aos workers de rotas as árvores locais dos pontos ainda não criados (mesmo esquema de lote com latência fixa do D3, que já é determinístico). No nativo e nos testes, pré-criar no `Sim::new`. Alternativa ≈: limitar a 1–2 pontos novos por passo. | pior passo −50 %, −3 a −4 % do passo | M4, M3 |
| 2.3 (A3) | Perfilar `may_enter_from`/`lane_end_lead` linha a linha (`perf annotate`) e cortar o que for repetido por passo para o mesmo nó (o `may_enter` da linha já é reavaliado só a cada 2 passos). | −3 a −6 % | M3 |
| 2.4 | Troca de faixa (15 % do passo): avaliar cada veículo a cada N passos (hoje já há `cool`), reaproveitando 2.1. | −3 a −5 % | M3 |

Soma esperada da etapa 2: **−15 a −25 % no passo**. Com a etapa 1, isso leva o ritmo a 30× numa máquina de 4 núcleos
(150 passos/s × ~5 ms = 75 % de um núcleo).

### Etapa 3: abrir e iniciar (2–3 dias, =)

| Item | O que fazer | Ganho esperado | Métrica |
|---|---|---|---|
| 3.1 (A6) | Medir por parte no navegador: download, descompressão, `import_osm_full`, OD, `sim_init` (matrizes, linhas, envio das árvores). Corrigir o que regrediu desde a fase 2. | — | M6, M7 |
| 3.2 | Guardar a rede processada do Recife no IndexedDB (bytes do núcleo, chave = hash dos arquivos de dados) e pular o parse do OSM nas aberturas seguintes. | abrir −1 s nas aberturas seguintes | M7 |
| 3.3 (A7) | D5 de outubro: `zone_dist` em `u16` e calculado só para zonas que são destino; `LOCAL_BUDGET` menor se 2.2 pré-criar os pontos. | −50 a −60 MB; `sim_init` mais rápido | M9, M6 |

### Etapa 4: conferir o desenho (1 dia de medição)

Rodar `bench-browser.mjs` com o navegador visível numa máquina real (`headless: false`), com pan/zoom automatizado,
e preencher M8. Só abrir trabalho no desenho se ficar abaixo de 55 fps.

### Depois, só se a meta não for batida
- **E3 (multithread)** da auditoria de outubro: aceleração em duas fases com wasm threads e `SharedArrayBuffer`
  (cabeçalhos COOP/COEP no Cloudflare). 2–3× com 4 núcleos, mas é a maior refatoração do núcleo.
- dt maior ou integração em várias taxas (E1/E2): mudam o modelo, fora enquanto houver calibração.

## Status

**2026-11-24: etapa 1 implementada** (`core.worker.ts`, `PerfStats.tsx`, `perf.ts`).
- 1.1: a fatia que termina com passo ainda devido agenda a próxima por `MessageChannel` (sem atraso mínimo). Em dia,
  `setTimeout` até o próximo passo vencer (no máximo 66 ms); esperando lote de rotas, 1 ms.
- 1.2: "CPU tela" = o maior entre o JS do app e as tarefas longas (`longtask`, só Chromium). "sim %" pelo relógio do
  worker (a mensagem `perf` leva `at`). O `bench-browser.mjs` também usa o relógio do worker.
- 1.3: quadros a 10 Hz a partir de 20× (15 Hz abaixo).
- Resultado da simulação inalterado (só muda quando as fatias rodam, não o que cada passo calcula).

Medido no mesmo contêiner, rodadas alternadas, 100 s com a rede cheia (~19 mil veículos):

| Rodada | Ritmo a 30× (antes → depois) | Ocupação do worker | ms por passo |
|---|---|---|---|
| 1 | 16,0× → **20,3×** (+27 %) | ~78 % → 99,6 % | 9,9 → 9,8 |
| 2 | 16,0× → **20,4×** (+28 %) | ~78 % → 99,6 % | 10,0 → 9,8 |

| Velocidade pedida | Ritmo obtido | Ocupação do worker |
|---|---|---|
| 1× | 0,98× | 1,5 % (não fica girando quando está em dia) |
| 5× | 5,2× | 15 % |
| 10× | 9,9× | 42 % |

Barra de status, mesmo cenário a 30×: "CPU tela" passou de 1 % para 99–100 % (a thread estava de fato saturada pelo
desenho por software deste contêiner); "sim" fica em ≤ 100 % (antes chegava a 151 %).

Com o worker a ~100 %, o ritmo agora é o limite do passo (1000 ÷ (5 passos × ~9,8 ms) ≈ 20×). Para chegar a M1
(≥ 28×) falta a etapa 2 (passo do núcleo).

**2026-11-24: etapa 2 implementada** (`sim/mod.rs`, `sim/route.rs`), toda com resultado idêntico: o estado de todos
os veículos (hash do quadro a cada bloco de 50 s, impresso agora pelo `perf_sim`) e o `recife_behavior` (3 sementes)
bateram com a base linha por linha; 91 testes passam.
- 2.1: cache de `stay_cost` por veículo (`stay_cost_of`, 4 entradas por veículo), chave = faixa, ponto de destino,
  versão da árvore da zona (`zver`, sobe a cada lote de rotas) e flags do motorista. Usado na troca de faixa, na escolha
  da conversão e em `ahead_stop_at`. Corta pela metade as instruções de `stay_cost`.
- Semáforos: `rem_pos` no lugar de `rem_euclid` (o `fmod` em software custava ~1,7 % do passo); bit a bit igual
  (prova no comentário e teste `rem_pos_matches_rem_euclid`, 2 milhões de casos).
- Troca de faixa por conforto (MOBIL): vizinhos e acelerações da faixa atual só quando há faixa ao lado candidata.
- Conflitos (`conflicts_of`): pares de conversões cujas caixas dos pontos de controle estão afastadas mais que a
  distância de quase-conflito (+0,5 m) não são comparados (a curva fica dentro do envoltório dos pontos de controle).
  −27 % de instruções em `conflicts_of`.
- `tree_local` genérico (sem `&dyn Fn`): sem ganho medido, mantido por ser inofensivo.
- `perf_sim`: `PERF_EXT=1` calcula as árvores de rotas fora do passo medido (como os workers do app), hash do estado e,
  nos passos lentos, quantos pontos de destino e conflitos foram montados.

| Medida | Base (etapa 1) | Etapa 2 |
|---|---|---|
| Instruções em `Sim::step`, nativo, 500 s, rotas fora (callgrind) | 21,78 bi | 20,35 bi (**−6,6 %**) |
| Ciclos estimados (instr. + 10×falta L1 + 100×falta L2) | 27,8 bi | 26,2 bi (−5,5 %) |
| Navegador, ms por passo a 30× (3 rodadas alternadas) | 9,77 / 8,34 / 9,76 | 9,10 / 8,25 / 8,83 (**−6 %** na média) |
| Navegador, ritmo a 30× | 20,7× / 24,0× / 20,4× | 22,1× / 24,4× / 22,9× (+7 %) |
| Navegador, pior passo | 45 / 48 / 35 ms | 30 / 37 / 29 ms |

**Abaixo do esperado (−15 a −25 %).** O que sobra no passo, medido com o callgrind:
- O corpo do `step` concentra 43 % das faltas de cache L1, espalhadas pelos ~40 vetores por veículo (`s`, `v`, `edge`,
  `vfac`…): é o laço por veículo, sem um ponto quente único (agrupar campos, B1, já foi testado sem ganho).
- `lane_end_lead` → `may_enter`/`ahead_stop_at`/`merge_ghost`: ~24 % das instruções, lógica de decisão no nó que não
  dá para pular sem mudar o comportamento.
- `make_target` (árvore local do ponto de destino, ~230 mil instruções cada; 12,8 mil pontos em 1.000 s) e
  `conflicts_of` (~33 mil por conversão; 55 mil conversões): custo diluído, que só some mudando o sorteio dos pontos
  (≈) ou guardando a geometria das curvas (dezenas de MB, contra M9).
- Os picos de passo restantes não têm causa única (0–4 pontos novos e 3–17 conflitos montados nos piores passos); boa
  parte é ruído desta máquina virtual.

Para M1 (≥ 28×) com ~9 ms por passo seria preciso −35 %. Os caminhos restantes mudam o modelo (≈: troca de faixa e
`may_enter` em ritmo menor, menos pontos de destino) ou a arquitetura (E3, multithread).

**2026-11-24: etapa 3 implementada** (3.1 completo; 3.2 e 3.3 não feitos, ver abaixo). Resultado idêntico: estado do
`perf_sim` (CTTU + linhas + motos) igual nos 20 blocos, rede igual (impressão digital nova do `perf_import`), 91 testes.

Medido por fase no navegador (logs temporários, já removidos), antes das correções:

| Fase | Abrir | Fase | Iniciar |
|---|---|---|---|
| `import_osm_full` (worker) | 799 ms | `sim_init_ext` (worker) | **916 ms**, dos quais `contra::plan` 638 ms |
| **Baixar a OD** (só depois do mapa) | **741 ms** | árvores iniciais (2 workers de rotas) | 626 ms |
| `buildOdImport` (thread principal) | 303 ms | | |
| instalar no mapa / IndexedDB / extrair | 88 / 55 / 70 ms | | |

Correções:
- **`contra::plan`** (contramão das motos, fase 9; era a regressão do início): cada trecho candidato varria todas as
  ~69 mil faixas da rede duas vezes atrás das que chegam/saem de um nó. Agora as faixas são indexadas por nó uma vez.
  638 → 8 ms (nativo), mesma sequência de faixas.
- **Encaixe da CTTU** (`extsig`): `near_segs` varria todos os trechos do mapa para cada ponto, e o cruzamento/pistas
  auxiliares, todas as interseções. Agora uma grade (CSR) devolve os candidatos em ordem crescente (o desempate entre
  trechos da mesma via depende da ordem). `import_osm_full` nativo 430 → 360 ms; rede idêntica.
- **OD antecipada**: `loadRecife` pede o JSON da OD (`fetchOd2018`, com cache) junto com o mapa; ele chega enquanto o
  worker monta a rede.
- `perf_import` (teste ignorado): tempo da importação como no app e impressão digital da rede.

| Medida (navegador, este contêiner) | Antes | Depois | Meta |
|---|---|---|---|
| M7 Abrir o Recife | 2,26–2,40 s | **1,06–1,27 s** | ≤ 1,5 s ✔ |
| M6 Iniciar → simulação pronta | 1,41–1,58 s | **0,88–1,09 s** | ≤ 0,8 s (quase) |
| `Sim::new` nativo, com as árvores iniciais | 2,20 s | 1,21 s | — |

O que sobra em iniciar é sobretudo a árvore inicial de cada uma das 130 zonas (~640 ms com 2 workers de rotas neste
contêiner de 4 vCPU; com 8 threads são 4 workers, ~metade). **Testado e revertido:** o worker da simulação calcular uma
parte das árvores enquanto espera (43 de 130) — aqui ficou pior (0,94 → 1,11 s), porque um 3º thread de cálculo
disputa os núcleos com o desenho por software; numa máquina com GPU pode valer, mas não deu para comprovar.

**Não feitos:**
- 3.2 (rede processada no IndexedDB): com `import_osm_full` em ~0,5 s no WASM e a meta de abrir já batida, serializar
  a `Network` inteira não compensa o risco agora.
- 3.3 (D5, `zone_dist` em `u16`): com a OD do Recife todas as zonas são destino, então o ganho seria só a quantização
  das árvores, que **muda** as rotas (≈, não =). Fica para quando a memória (M9) for prioridade; `LOCAL_BUDGET` menor
  também muda o sorteio dos pontos.
- `buildOdImport` (~0,3–0,4 s na thread principal, espalhado entre portais, ponto no polígono e chaves de texto nos
  mapas da matriz): não otimizado; candidato a ir para o worker se a tela travar ao abrir numa máquina real.

## 6. Testado e descartado nesta auditoria

| Opção | Resultado |
|---|---|
| `wasm-opt -O3` (binaryen via npm) | passo médio 6,10 ms contra 5,57 ms sem ele (ruído; mesmos veículos minuto a minuto). Continua no CI pelo arquivo menor. |
| SIMD (`-C target-feature=+simd128`) | 5,93 ms contra 5,57 ms (ruído). O passo não tem laços vetorizáveis. |

## 7. Como medir

Antes e depois de cada item, na mesma máquina e no mesmo momento, alternando as versões:

- **Navegador (M1, M2, M6, M7)**: `npm run dev` e, em outro terminal,
  `node scripts/perf/bench-browser.mjs http://localhost:5173/ 120 30`. O resumo da 2ª metade traz `ritmo`,
  `msPorPasso`, `piorPasso`, `workerPct`, `abrirMs`, `iniciarMs`. `PROF=30` grava o perfil da thread principal.
- **Núcleo nativo (M3, M4)**: baixar o JSON do cenário ("Baixar JSON da simulação" no painel de depuração) e rodar
  `CTTU=1 BUS=1 PERF_OSM=recife.osm PERF_SIM_JSON=simulacao.json PERF_MIN=20 taskset -c 2 cargo test --release perf_sim -- --ignored --nocapture`
  (o `perf_sim` agora aceita `CTTU` e `BUS`, como o `recife_behavior`).
- **WASM (M5)**: `node scripts/perf/bench-wasm.cjs <pkg nodejs> recife.osm simulacao.json 20`.
- **Por função**: `perf record` sobre o binário de teste compilado com `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`.
- **Comportamento**: itens "=" reproduzem `recife_behavior` e `sim_is_deterministic` linha a linha; itens "≈"
  comparam 6 sementes pareadas.
