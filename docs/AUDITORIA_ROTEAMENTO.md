# Auditoria: roteamento e comportamento dos veículos

> 2026-10-18. Objetivo: rotas e decisões dos motoristas parecidas com as do Waze.
> Escopo: `core/src/sim/route.rs`, `core/src/sim/mod.rs`, `core/src/sim/metrics.rs`, `core/src/conn.rs`.
> **Status (2026-10-18): R5, R7, D, A, F, R6 e R8 implementados** — ver o fim do arquivo.
> Medições: nativo `--release`, `~/Downloads/mapa-recife-novo.osm` (84.122 faixas, 143.838 conversões ligadas).

## O que "igual ao Waze" quer dizer aqui

O Waze faz quatro coisas que o simulador ainda não faz:

1. **Usa o tempo real de cada trecho**, medido pelos próprios usuários, e não a velocidade da via.
2. **Recalcula a rota durante a viagem** quando aparece um caminho melhor.
3. **Aprende o custo de cada cruzamento e conversão**: esquerda difícil, semáforo demorado, fila de saída.
4. **Espalha os usuários.** Como cada um recebe a rota num momento diferente, nem todos vão pelo mesmo caminho, e o fluxo se distribui entre as alternativas.

Na literatura, isso é **atribuição dinâmica de tráfego com informação em tempo real** (rerroteamento reativo en-route).

## Medições de base

| Item | Valor |
|---|---|
| `LaneRouter::new` (grafo por faixa) | 20 ms |
| Uma árvore de Dijkstra reversa (uma zona) | **~20 ms** nativo (WASM estimado: 30–40 ms) |
| Recalcular todas as 128 zonas do Recife | ~2,6 s nativo / ~4–5 s WASM |
| Passo da simulação (4 zonas grandes, ~19 mil veículos) | 6,9 ms médio, 13 ms pior |
| Memória de `zone_dist` (zonas × faixas × 4 B) | 128 zonas: 43 MB; 253 zonas: 85 MB |

Conclusão: recalcular tudo de uma vez trava a simulação. Recalcular **uma zona por vez, distribuído entre os passos**, cabe no orçamento.

---

## Problemas encontrados

Complexidade: **P** = até 1 dia, **M** = 2–4 dias, **G** = 1–2 semanas.

### R1. A rota nunca muda com o congestionamento (crítico)
- **Onde:** `Sim::new` calcula `zone_dist` uma vez só, com tempo livre (`len / (vfree × 0.85)`). O próprio `CONTEXT.md` lista isso como pendência.
- **Efeito:** um engarrafamento não desvia ninguém. Num teste A/B, fechar uma faixa só aumenta a fila no mesmo caminho. No Waze, parte do fluxo iria para as paralelas. O resultado é que o impacto de mudanças viárias sai superestimado.
- **Complexidade:** M.

### R2. Todos vão pelo mesmo caminho (crítico)
- **Onde:** `stay_cost` e `choose_conn` escolhem sempre a conversão de menor custo, sem nenhuma variação entre motoristas.
- **Efeito:** é uma atribuição "tudo ou nada". O caminho mais curto recebe 100% do fluxo de um par de zonas, e uma paralela 5 s mais lenta recebe 0%. Isso piora R1: se o roteamento dinâmico entrar sem espalhamento, todos trocam de rota ao mesmo tempo e o fluxo oscila entre duas vias ("flip-flop").
- **Complexidade:** P.

### R3. Penalidades de cruzamento fixas e cegas (alto)
- **Onde:** `route.rs::turn_penalty`. Esquerda +5 s, direita +2 s, semáforo +14 s e nó de grau ≥ 3 +3 s, sempre os mesmos valores.
- **Efeito:** a esquerda sem semáforo cruzando uma avenida cheia custa 8 s, e numa rua vazia também. Um semáforo com ciclo de 120 s e 20 s de verde custa os mesmos 14 s que um de 60/30. O Waze evita justamente esses cruzamentos difíceis, porque mede a espera neles.
- **Complexidade:** M.

### R4. O destino exato dentro da zona é ignorado no trecho de fora (médio)
- **Onde:** `dist` e `stay_cost`. Fora da zona, o custo é `zone_dist` (até o **primeiro** nó da zona) mais `t.mean`, uma média constante da zona.
- **Efeito:** o veículo entra pela borda mais próxima da zona, não pela mais próxima do ponto sorteado, e depois cruza a zona por dentro. Em zonas grandes, isso gera desvios que o Waze não faria.
- **Agravante:** dentro da zona, `stay_cost` prefere o caminho interno **sempre que existe**, mesmo quando sair e contornar seria mais rápido.
- **Complexidade:** M (opção leve) ou G (rota por endereço).

### R5. Freada de 0,9 g em toda conversão (alto, comportamento)
- **Onde:** `desired_speed` e `leader`. A velocidade de conversão (`vturn`: 5,5 m/s na esquerda, 6,5 m/s na direita) só passa a valer quando o veículo **já entrou** na curva.
- **Efeito:** um carro a 50 km/h que entra numa esquerda tem termo livre do IDM `1 − (13,9/5,5)⁴ ≈ −40`. A aceleração vai para o limite de −9 m/s². Isso é irreal, desperdiça capacidade e distorce o tempo de conversão que a rota usa. Verificado só por leitura do código.
- **Complexidade:** P. A correção é antecipar a curva: um teto de velocidade `v_max(d) = sqrt(vturn² + 2·b·d)` em função da distância até a linha.

### R6. Troca de faixa simplificada, sem MOBIL nem cooperação (médio)
- **Onde:** `consider_lane_change` e `try_change`.
- **Troca voluntária:** só acontece com líder lento, folga > 35 m e mais de 60 m até o fim da faixa. Não há incentivo nem polidez (MOBIL), nem a regra de manter a direita e ultrapassar pela esquerda (CTB art. 29).
- **Troca obrigatória:**
  - ninguém cede passagem: não existe veículo que reduz para abrir espaço;
  - quem precisa entrar não reduz para achar um vão;
  - em fila, isso termina em espera e, depois, em manobra irregular (`illegal_moves`).
- **Complexidade:** M.

### R7. Desbloqueio arbitrário quando o veículo empaca (baixo)
- **Onde:** `reroute_if_stuck` usa `lanes[e].conns.first()`, a primeira conversão da lista, e não a melhor das disponíveis.
- **Efeito:** depois de 12 s preso, o veículo pode ir para qualquer lado e fazer uma volta enorme.
- **Complexidade:** P.

### R8. O custo de trocar de faixa depende só do comprimento (baixo)
- **Onde:** `lane_change_cost(len)`.
- **Efeito:** numa avenida cheia, trocar 3 faixas em 150 m "custa" 12 s, com fila ou sem ela. O plano conta com trocas que não cabem no tráfego real, e isso alimenta o R6.
- **Complexidade:** P, a partir do R1, multiplicando o custo pela ocupação da faixa de destino.

### R9. Matriz OD e tempo livre usam rotas estáticas (baixo)
- **Onde:** `zone_minutes` e `dists`, do roteador por trecho.
- **Efeito:** o modelo gravitacional não reage ao congestionamento. Para um simulador de cenários isso é aceitável, porque a demanda fica fixa e só a rota muda. Vale só deixar documentado.

---

## Opções de solução

### Para R1 (tempo real) e R2 (espalhamento)

| Opção | Como funciona | Prós | Contras | Complexidade |
|---|---|---|---|---|
| **A. Rerroteamento reativo com árvores rotativas** | A cada passo, recalcula a árvore de 1 ou 2 zonas com o tempo **medido** (`len / (vfree × ema_ratio)`, que já existe em `metrics.rs`). Uma volta completa leva 2–5 min simulados. Como o veículo já decide a conversão a cada faixa olhando a árvore atual, o rerroteamento durante a viagem vem de graça. | Reaproveita tudo o que existe. Custo previsível (~0,1–0,2 árvore por passo ≈ +2–4 ms nativo). Fica igual ao Waze: decide com o trânsito de agora. | Sozinho, oscila (R2). A medição é por trecho, não por conversão. | M |
| **B. Equilíbrio iterativo (DUE / MSA)** | Roda a simulação várias vezes e mistura as rotas até estabilizar. | Padrão acadêmico para estudar cenários; resultado estável. | 5–20 rodadas por cenário: pesado no navegador e longe do "Waze ao vivo". | G |
| **C. Rota por veículo (A\* ou CCH)** | Cada veículo calcula o próprio caminho até o nó exato no nascimento e, depois, em intervalos. | O mais fiel ao Waze: rota por endereço, histerese natural, resolve também o R4. | Uma A\* simples a ~14 nascimentos/s não cabe a 30×. Precisa de CCH (*Customizable Contraction Hierarchies*): pré-processamento de segundos, pesos atualizáveis em ~100 ms, consulta em ~0,1 ms. Muito código novo. | G |
| **D. Espalhamento por percepção (logit)** | Cada veículo tem um fator de percepção fixo por trecho (hash de id e trecho, ±10–20%) somado ao custo em `stay_cost`. Opcionalmente, um parâmetro "% usando app": os demais motoristas usam o tempo livre ou histórico. | Barato (sem memória extra), determinístico por semente. Elimina o tudo ou nada e amortece a oscilação de A. | Um parâmetro a calibrar. | P |
| **E. Histerese** | Quem já está numa rota só troca se o ganho for > max(30 s, 10%). | Remove o flip-flop residual. | Com árvores, exige guardar a versão anterior da árvore (2× memória) ou guardar um "custo prometido" por veículo. Com a opção C sai naturalmente. | P–M |

### Para R3 (custo dos cruzamentos)
- **F. Atraso medido por conversão:** acumula o tempo entre parar na linha e entrar na conversão (já há `wait` e `node_delay`), com média exponencial por `ConnRt`. A penalidade vira `base + atraso medido`, e o semáforo deixa de ter os 14 s fixos.
  - É o que mais aproxima o simulador do "evita cruzamentos difíceis" do Waze.
  - Separa a fila da esquerda da fila do "em frente", que a medição por trecho mistura.
  - **Complexidade:** M. É feito junto com a opção A.
- **F′ (alternativa leve):** estimar a penalidade sem medir.
  - Semáforo: atraso de Webster, ≈ `C·(1−g/C)²/2`.
  - Esquerda sem semáforo: fluxo oposto.
  - **Complexidade:** P, mas é menos fiel.

### Para R4 (destino na zona)
- **G1. Portas da zona com custo do destino** (M). O primeiro nó da zona continua no `zone_dist`, mas cada ponto de destino guarda o custo local **a partir de cada porta** (faixa que entra na zona). Na última decisão antes da zona, escolhe-se a porta pelo menor `zone_dist até a porta + local`. Resolve a maior parte do desvio sem memória por destino.
- **G2. Rota por veículo**, a opção C (G).
- **G3. Correção pontual** (P): em `stay_cost`, comparar o caminho interno com o externo em vez de preferir sempre o interno.

### Para R5–R8 (comportamento)

| Problema | Opção | Complexidade |
|---|---|---|
| R5 | Teto de velocidade antecipado antes da conversão (ver R5) | P |
| R6 | MOBIL: incentivo, polidez p≈0,3, regra de manter a direita. Para a troca obrigatória: o veículo de trás cede (reduz para abrir espaço) quando o vizinho está com a seta ligada e parado há mais de X s. Quem precisa entrar ajusta a velocidade ao vão. A seta (`blink_to`) já existe. | M |
| R7 | Escolher a melhor conversão disponível por `stay_cost` | P |
| R8 | `lane_change_cost × (1 + k·ocupação da faixa vizinha)`, com a ocupação vinda do R1 | P |

---

## Recomendação

A ordem prioriza o maior ganho de realismo por custo e prepara cada passo para o seguinte:

1. **R5 + R7** (P). São correções de comportamento, sem risco, e melhoram o tempo medido usado pelos passos seguintes.
2. **D (espalhamento) + A (árvores rotativas com tempo medido)** (M). É o núcleo do "Waze":
   - rotas reagem ao trânsito, recalculadas durante a viagem, sem oscilação;
   - parâmetro novo no painel: "% de motoristas com app", padrão de 60%.
   - Taxa de atualização inicial: 1 zona a cada 3 passos, ou seja, todo o Recife em ~2 min simulados, com impacto de ~+7 ms nativo por passo dividido por 3.
   - Para conter a memória: guardar `zone_dist` em `u16`, em meio segundo, o que dá 43 → 21 MB.
3. **F (atraso medido por conversão)** (M). Junto com o passo 2, entra como peso no Dijkstra.
4. **E (histerese)** (P–M), só se a oscilação ainda aparecer depois do passo 2.
5. **G1 (portas da zona)** (M). Importa mais com as zonas grandes do modo gravitacional. As zonas da OD 2018 são pequenas.
6. **R6 (MOBIL + cooperação)** (M). Melhora as filas em convergências e bifurcações e reduz `illegal_moves`.
7. **Opção C (CCH)** fica para depois, se for preciso rota por endereço exato ou ETA por veículo. É a versão "Waze completa", mas custa semanas.

**Como medir o ganho** (sugestão de testes):
- Um teste A/B com duas rotas paralelas: fechar uma faixa na principal deve desviar parte do fluxo para a paralela. Hoje isso dá 0%.
- Um teste de não oscilação: o fluxo na paralela não pode alternar mais de X% entre minutos.
- Desaceleração máxima ≤ 4 m/s² em conversões livres (R5).
- `illegal_moves` e `lost` no Recife antes e depois.

---

## Implementado (2026-10-18)

| Item | O que entrou | Resultado no Recife (15 min, 4 zonas, média de 3 sementes) |
|---|---|---|
| R5 | Freada antecipada até `vturn` antes da linha | freadas > 5 m/s²: 21,9 mil → 11,3 mil |
| R7 | Desbloqueio pela saída que mais aproxima do destino | sem efeito mensurável neste cenário (raro) |
| A | Árvores por zona recalculadas em fatias com a velocidade medida | atraso total −9%, fila de entrada −21%, concluídos +1% |
| D | Percepção por motorista (por via) + % com app (padrão 60) | idem acima; passo +0,3 ms (1,30 → 1,62 ms) |
| F | Atraso medido por conversão como penalidade no roteador (em vez de semáforo = 14 s fixo); peso da faixa volta a ser o livre | atraso total 50,2 → 39,3 veíc·h (−29% sobre o sem rotas em tempo real), fila 278 → 256; passo +0,65 ms no total; linha contínua ~2,2 → ~3,2 mil |
| R8 | Custo de troca × (1 + 4·ocupação da faixa de destino) | ver linha abaixo (medido junto) |
| R6 | MOBIL (polidez 0,3, manter a direita) + cortesia de 70% na troca obrigatória + ajuste de velocidade de quem entra | linha contínua ~3,2 → ~0,75 mil; conversões proibidas 11 → 2; atraso total 39,3 → 44,5 veíc·h (ceder custa tempo) |

Pendentes: E (histerese, só se houver oscilação), G1; calibração com dados reais (contagens/velocidades).
As árvores locais de cada destino (`Target.local`, dentro da zona) continuam em fluxo livre.
