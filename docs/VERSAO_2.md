# Versão 2 — plano

> Criado em 2026-10-31. Atualize o status de cada fase ao concluir e registre as decisões no `CONTEXT.md`.

## Objetivo
1. **Remover os cenários A/B** (simplificar a interface e o worker).
2. **Deixar o sistema mais rápido** (simulação, edição e interface).
3. **Linhas de ônibus com paradas**, no lugar do "% de ônibus" sorteado no fluxo.
4. **Motos**, com corredor entre faixas e comportamento de motoboy por parcela.

Ordem: remoção primeiro (diminui o que precisa ser mantido), depois desempenho (os ônibus vão somar carga, então é bom
ter a linha de base medida e folga), e por último os ônibus, em fases.

---

## Fase 1 — Remover cenários A/B

**O que sai**
- UI: `src/ui/ScenarioPanel.tsx`, item `scenarios` do trilho (`src/ui/Dock.tsx`), estilos ligados a ele em `styles.css`.
- Store (`src/model/store.ts`): `scenarios`, `bench`, `saveScenario`, `deleteScenario`, `runScenario`, `cancelBench`.
- Ponte e worker: `runBench` (`src/model/importer.ts`), mensagens `bench`, `benchCancel`, `benchProgress`, `benchDone`,
  `benchError` (`src/workers/core.worker.ts`, `src/model/types.ts`), tipo `BenchResult`.
- Persistência: chave IDB `scenarios`, `ScenarioRec`, `saveScenarios`/`loadScenarios` (`src/model/persistence.ts`).
  Apagar a chave do IndexedDB na carga, uma vez, para não deixar resultados antigos ocupando espaço.
- Mapa de diferença (B − A) no `HeatLayer`, se só o A/B usa.

**O que fica**
- **Exportar/Importar cenário (`.json`)**: são as edições da rede, não o A/B.
- Métricas do núcleo (`sim/metrics.rs`), mapa de calor, gráficos e replay da simulação em andamento.
- `sim_report` / `report()`: verificar se algo além do A/B usa. Se só testes usam (`sim_ab_bottleneck_makes_things_worse`),
  manter no núcleo para os testes e tirar do WASM.

**Pronto quando**: build e `cargo test` passam, nenhum resto de `bench`/`Scenario` A/B no `src/` (grep), importar e
exportar cenário `.json` continuam funcionando, `CONTEXT.md` atualizado (seção M4).

---

## Fase 2 — Desempenho

### 2.0 Linha de base (antes de mexer)
Medir e registrar aqui, com o Recife completo (OD 2018 às 7h, escala 15 % e 100 %, semáforos da CTTU):

Base medida em 2026-10-31 (commit `5f50e1b`). Navegador: Chromium do app, Recife com OD 2018 + portais + CTTU às 7h,
130 zonas, medido por script no console (`useStore`/`perf`); o painel estava oculto, então fps e UI ficam para o passo 5.
Nativo: `--release`, `taskset -c 2`, 2 rodadas (iguais).

| Medida | Como | Base | Meta |
|---|---|---|---|
| ms/passo, OD 100 % (~9,8 mil veíc., 19 min) | WASM, `busyMs` ÷ passos a 30× | **10,0 ms** | −30 % |
| ms/passo, OD 15 % (~1,2 mil veíc.) | idem | **4,0 ms** (custo fixo alto: rotas de 130 zonas) | −50 % |
| Velocidade que acompanha, OD 100 % | velocidade efetiva pedindo 30× | **15,5×** | ≥ 20× |
| Velocidade que acompanha, OD 15 % | idem | 30× (60 % de CPU) | — |
| `sim_init` (Iniciar → 1º quadro), Recife | navegador | **3,0–4,5 s** | < 1,5 s |
| `Sim::new` 256 zonas / 4 zonas | `perf_sim` nativo | 3,85–4,1 s / 0,16 s | < 1,5 s |
| Passo nativo, 4 zonas (19 mil veíc., min 12) | `perf_sim` | 8,9–9,1 ms | — |
| Passo nativo, 256 zonas (13 mil veíc., min 12) | `perf_sim PERF_GRID=1` | 9,2 ms (idade das rotas 249 s) | — |
| `recife_behavior` (3 sementes) | nativo | concluídos 425, atraso 36,7 veíc·h, passo 1,18 ms | igual nos passos "=" |
| Editar faixas (clique → mapa atualizado) | `PerfStats` | ~210 ms (medir no passo 4) | < 100 ms |
| Abrir o Recife (clique → mapa pronto, com OD e CTTU) | navegador | **2,75 s** | medir partes |
| fps em pan/zoom com a simulação rodando | painel visível | (passo 5) | ≥ 55 fps |

### Progresso
- **Passo 1 — wasm-opt -O3 (A7)**, feito: `.wasm` 1.076 → 896 KB (−17 %). Passo no navegador sem ganho mensurável
  (OD 100 %: 10,0 → 10,1 ms; OD 15 %: 4,0 → 3,8 ms, dentro do ruído). Mantido pelo download menor. O deploy
  (GitHub Actions) baixa o binaryen pelo wasm-pack.
- **Passo 2 — rotas fora do worker da simulação (D3) + init**, feito. Lotes de árvores com latência fixa de 15 passos
  (resultado igual com ou sem workers, teste `external_route_trees_match_local`), 1–4 workers de rotas, árvores
  iniciais em paralelo, matriz OD com o tempo livre das árvores por faixa (sai o roteador por trecho) e pré-filtro por
  retângulo nos testes de ponto no polígono.
  | Medida | Base | Depois |
  |---|---|---|
  | ms/passo, OD 100 % (~10 mil veíc.) | 10,0 ms | **7,2 ms** (−28 %) |
  | Velocidade que acompanha, OD 100 % | 15,5× | **21,3×** |
  | ms/passo, OD 15 % (~1,2 mil veíc.) | 4,0 ms | **2,0 ms** (−50 %) |
  | 1º quadro da simulação, Recife | 3,0–4,5 s | **0,6 s** |
  | `Sim::new` nativo, 256 zonas (sem workers) | 3,9 s | 1,9 s |
  | `recife_behavior`, 6 sementes pareadas | concluídos 427, atraso 40,7 veíc·h | 437, 41,7 veíc·h (ruído) |
  O `sim %` da barra de desempenho agora não inclui o tempo dos workers de rotas.
- **Passo 3 — quadro de veículos menor**, medido e descartado: dos 1,81 ms do `snapshot` com 19 mil veículos, 1,61 ms
  (89 %) são o cálculo da posição de cada veículo; compactar de 24 para 12 bytes pouparia ~0,2 ms × 15 quadros/s
  (~0,3 % de CPU). Mandar só os visíveis também rende pouco com o Recife inteiro na tela.
- **Passo 4 — abertura e edição**:
  - Abrir o Recife com a CTTU importava o mapa com os semáforos do OSM e remontava tudo com os da CTTU (~1 s a mais).
    Agora a 1ª importação já usa a CTTU. Abrir com OD + CTTU: **2,2–2,75 s → ~1,6 s** (OSM 0,9 s, OD 0,3 s).
  - Edição de faixas no Recife: **~210 ms** = debounce 40 ms + worker 133 ms (reconstrução completa `Net::build`
    ~84 ms nativo, malhas dos ladrilhos sujos, cópia dos arrays da rede) + instalação no mapa 25 ms. A meta de 100 ms
    só sai com a reconstrução incremental (M2b), que fica por último na fase.
- **Passo 5 — velocidade efetiva (E4)**, feito: o painel da simulação mostra "Rodando a N×: o computador não
  acompanha M×" quando a velocidade real fica abaixo de 85 % da pedida (medida a ~1 Hz, zerada ao iniciar, pausar ou
  mudar a velocidade). Fatia do worker de 16 ms testada e descartada (20,2× contra 21,3× com 10 ms). **Pendente:**
  perfilar o thread principal com o painel visível (no painel oculto do teste o mapa desenhava a <1 fps, sem medida
  confiável) e o M2b.

### 2.1 Simulação (itens da `AUDITORIA_OTIMIZACAO_SIM.md` ainda pendentes)
- **A7** — `wasm-opt -O3` (binaryen) no build. Ganho esperado 5–15 %, resultado idêntico.
- **D3** — roteador em tempo real num 2º worker (pesos vão, árvores voltam como transferíveis). Tira 20–35 % do passo
  com a OD do Recife.
- **D5** — `zone_dist` em `u16` e calculado sob demanda para zonas que nunca são destino: metade da memória das árvores
  e `sim_init` mais rápido.
- **B2/B3/C2** — acumuladores de métricas num struct, conflitos em CSR, `occ_obstacle` pela distância de frenagem.
  Ganhos pequenos; fazer só se a medição mostrar que valem.
- **E4** — mostrar a velocidade efetiva (sim-s por s real) e não oferecer N× que o passo não comporta.
- **E3 (multithread)** fica de fora salvo se depois disso a meta de 20× com a OD a 100 % não for batida.

Regra de validação: mudanças marcadas "=" na auditoria têm de reproduzir `recife_behavior` e `sim_is_deterministic`
linha por linha; mudanças "≈" comparam 6 sementes.

### 2.2 Fora do passo
- **Snapshot**: quantizar o quadro (`Int16` para posição relativa ao ladrilho, `Uint8` para velocidade/direção) e/ou
  mandar só os veículos visíveis. Hoje 2,9 ms a 15 Hz com 19 mil veículos.
- `sim_track`/`explain` (4 Hz) e o relatório (1 Hz) só com o painel aberto.

### 2.3 Edição e carga
- **M2b — grafo incremental com ids estáveis**: reconstruir só a vizinhança do trecho editado. É o que falta para
  < 100 ms ao editar.
- Abrir o Recife: medir parse do `.osm.gz` vs. construção; avaliar mandar o mapa pré-processado (binário do núcleo) em
  vez de reprocessar o OSM a cada abertura.

### 2.4 Interface
- Perfilar o thread principal com a simulação a 30× (React re-renderizando com estatísticas a 1 Hz, `HeatLayer`,
  rótulos DOM). Corrigir o que aparecer acima de 2 ms por quadro.

**Pronto quando**: tabela 2.0 preenchida com "antes" e "depois", metas batidas ou o motivo registrado, comportamento
validado como acima.

---

## Fases 3 a 6 — Linhas de ônibus e paradas

### Situação de hoje
- Ônibus = `bus_share` % das viagens entre zonas (sorteio em `sim/mod.rs`). Usam faixa exclusiva, mas não têm linha,
  itinerário nem parada.
- O `mapa-recife.osm.gz` traz **216 nós `highway=bus_stop`**, mas **nenhuma relação `route=bus`**. O recorte foi feito
  só com vias. O parser (`osm.rs`) também ignora relações que não são `restriction`.

### Fase 3 — Dados
Script `scripts/onibus_recife/` → `public/data/onibus_recife.json` (mesmo padrão dos semáforos da CTTU e da OD).
- **Itinerários e paradas**: relações `route=bus` (PTv2) do OSM via Overpass: sequência de ways por sentido e paradas
  (`public_transport=platform` / `stop_position`, `highway=bus_stop`).
- **Frequência por hora**: verificar se há GTFS público do Grande Recife Consórcio. Sem GTFS, usar intervalo sintético
  por classe de linha (ex.: troncal 5 min no pico, alimentadora 15 min), documentado como estimativa.
- Relatório de cobertura: quantas linhas, quantas com itinerário contínuo, quantas paradas ficaram sem linha.
- Ajustes manuais num `ajustes_manuais.json` próprio, como na CTTU.

**Pronto quando**: o JSON existe, gera de novo sem baixar nada (só mescla ajustes), e a cobertura está registrada.

### Fase 4 — Núcleo: encaixe na rede
- `core/src/transit.rs`: encaixar cada itinerário na rede (por id de way; onde o way foi dividido ou editado, pelo
  caminho mais curto entre os pontos) → sequência de **faixas permitidas a ônibus**. Paradas viram uma posição numa
  faixa (trecho + distância), na faixa da direita ou na baia.
- Tipo de parada: na faixa (bloqueia a faixa) ou baia (`bus_bay`, sai do fluxo). Sem tag, na faixa.
- Itinerário que não encaixa (via demolida, conversão proibida): aviso por linha, a linha é desligada. Nada de pular
  trechos.
- Edições da rede reencaixam as linhas (as linhas usam chaves estáveis, como os semáforos).
- Testes: itinerário encaixa no mapa de exemplo; linha com via demolida gera aviso; encaixe estável depois de rebuild.

### Fase 5 — Simulação
- **Geração por linha**: um ônibus parte do terminal a cada intervalo da hora (com pequena variação sorteada pela
  semente), segue a rota fixa até o fim e termina no terminal. Substitui o `bus_share` quando há linhas carregadas;
  mapa sem linhas mantém o comportamento atual.
- **Parada**: desacelera até a posição da parada, fica parado o tempo de embarque (tempo fixo + parte proporcional à
  demanda da hora, sorteada), sinaliza e retorna ao fluxo. Na baia: entra e sai cedendo a vez (mesma lógica de fusão
  já existente). Na faixa: carros atrás trocam de faixa ou esperam (já coberto pelo MOBIL/troca obrigatória).
  Sem sumir nem teleportar o ônibus em nenhum caso.
- **Comportamento por parcela** (configurável, padrão 0): carros que usam a faixa exclusiva, carros que param no ponto
  de ônibus.
- **Métricas por linha**: tempo de viagem do itinerário, velocidade comercial, atraso em paradas vs. em cruzamentos,
  regularidade (intervalo real entre ônibus, comboio).
- Testes: determinismo, ônibus nunca sai da rota, conservação (partidas = concluídos + na rede), tempo de parada dentro
  da faixa configurada, faixa bloqueada pela parada não gera colisão.

### Fase 6 — Interface
- **Camada Ônibus** (Camadas): itinerários coloridos por linha e paradas (ícone), com LOD por zoom.
- **Painel Linhas** no trilho: busca por número/nome, ligar/desligar linha, intervalo por hora editável, cor.
- Clicar numa parada: linhas que passam, próximos ônibus (na simulação), espera média. Clicar num ônibus: linha,
  próxima parada, atraso.
- **Editar paradas**: adicionar, remover e mover ao longo da via; trocar o tipo (faixa/baia). Persistência em IDB e no
  `.json` de cenário, desfazer como as outras edições.
- Na abertura do Recife (`RecifeDialog`), opção "Linhas de ônibus".
- Gráficos da simulação ganham velocidade comercial média dos ônibus.

**Pronto quando**: Recife abre com as linhas, a simulação roda sem travas de ônibus nas paradas (`recife_behavior`
com linhas: parados > 120 s = 0), desempenho dentro das metas da fase 2 com as linhas ligadas, `CONTEXT.md` atualizado.

---

## Fases 7 a 9 — Motos

### Situação de hoje
- Só há carro e ônibus, todos em fila única por faixa (`leader`, `may_enter`). O snapshot marca ônibus com comprimento
  negativo; não há campo de tipo.
- Moto andando na fila como um carro pequeno quase não muda o trânsito. O que muda é o **corredor** (e, no Recife, o
  comportamento de parte dos motociclistas).

### Fase 7 — Moto como tipo de veículo
- Tipo de veículo explícito no núcleo (carro, ônibus, moto) e no snapshot, no lugar do comprimento negativo. Fazer junto
  com a quantização do snapshot (2.2) se ela ainda não tiver sido feita, para mudar o formato uma vez só.
- Parâmetros da moto: ~2 m de comprimento, aceleração maior, vão mínimo e tempo de reação menores.
- **% de motos** na demanda (`SimParams`, painel de demanda, cenário), como o % de ônibus; valor típico do Recife em
  `TYPICAL_PARAMS`, documentado como estimativa. A OD 2018 tem modo moto: verificar se dá para usar a matriz em vez do %.
- `VehicleLayer`: malha/cor própria para moto.
- Testes: determinismo, conservação, com 0 % de motos o resultado é idêntico ao de antes.

### Fase 8 — Corredor
- **Faixa virtual entre duas faixas vizinhas do mesmo sentido**, só para motos, em vez de posição lateral livre (o
  modelo continua de fila única). Ativa quando as duas faixas estão lentas ou paradas (abaixo de um limite, ex.
  20 km/h); a moto anda até ~10–20 km/h acima do fluxo.
- Entrada e saída do corredor: a partir da faixa real, como uma troca de faixa (sem vão exigido, porque passa ao
  lado). Na linha de retenção, a moto para à frente do primeiro carro e volta para uma faixa real quando abre o verde,
  pela lógica de conflito existente. Sem sumir nem teleportar.
- Carro trocando de faixa precisa enxergar a moto no corredor (não pode atravessar por cima dela).
- Em via de 1 faixa por sentido, não há corredor (contramão fica na fase 9, por parcela).
- Testes: moto no corredor nunca sobrepõe carro, conservação, fila de semáforo com motos: elas saem na frente.

### Fase 9 — Comportamento por parcela
Sempre % configurável dos motociclistas, padrão 0 (como `illegalPct` e `boxBlockPct`):
- Avançar o vermelho com o cruzamento livre.
- Usar a faixa exclusiva de ônibus.
- Contramão em trecho curto.
- Corredor em velocidade alta (acima do limite da fase 8).

**Pronto quando**: Recife roda com motos sem travas novas (`recife_behavior`: parados > 120 s = 0), sem sobreposição
moto × carro, desempenho dentro das metas da fase 2, `CONTEXT.md` atualizado.

---

## Backlog (fora desta versão)
- **Estreitamento de faixas** (442 casos no Recife, ex.: 4 → 2): as faixas são juntadas em pares sem saber qual delas
  termina. Precisa de dado (OSM `lanes` + `turn:lanes`/`change:lanes`, ou ajuste manual).
- **Colisões de geometria** (curvas que passam perto demais com as duas aproximações no verde) e **vermelho geral fixo
  em 2 s** (calcular por (d2 + c)/v).
- **Anel de filas na Av. Dois Rios** (cenário do app às 7h): veículo da parcela que fecha o cruzamento parado na curva,
  com filas que voltam ao mesmo nó.

## Status
| Fase | Status |
|---|---|
| 1 — Remover A/B | concluída (2026-10-31) |
| 2 — Desempenho | pendente |
| 3 — Dados de ônibus | pendente |
| 4 — Encaixe na rede | pendente |
| 5 — Simulação de ônibus | pendente |
| 6 — Interface de linhas | pendente |
| 7 — Moto como tipo | pendente |
| 8 — Corredor | pendente |
| 9 — Motos por parcela | pendente |
