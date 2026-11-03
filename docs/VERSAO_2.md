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
  confiável).
- **M2b — edição sem reconstruir a rede**, feito (2026-11-02), no caso que a interface mais usa: edições que só mudam
  o perfil de trechos existentes (nº de faixas, largura, ônibus, mão única). `Net::update_segs` refaz o perfil dos
  trechos editados, os recuos dos nós deles e a numeração das faixas; `render::update_tiles_partial` recalcula a
  assinatura só desses trechos e nós. Inverter sentido, via nova, demolição e ligar/desligar semáforo continuam pela
  reconstrução completa. Teste `fast_segment_edits_match_full_rebuild` (e `fast_segment_edits_recife` com
  `PERF_OSM`): rede e ladrilhos idênticos à reconstrução completa. Junto: `meta_json` serializado por structs (era
  ~21 ms por edição no Recife) e sem debounce na reconstrução (as edições durante uma reconstrução já viram uma só).
  | Edição de faixas no Recife | Antes | Depois |
  |---|---|---|
  | `set_edits` nativo | 97,6 ms | 13,5 ms |
  | Worker (edição + cópia dos arrays) | 133 ms | ~39 ms |
  | Clique → mapa atualizado | ~210 ms | **~95 ms** |

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

**Concluída (2026-11-02).** A fonte mudou do plano: o OSM tem só 87 relações `route=bus` no Recife, mas o Grande
Recife publica um **GTFS oficial** (`granderecife.pe.gov.br/gtfs/gtfs.zip`, 38 MB, `v2026.10.26`: 395 rotas de
ônibus, 1.424 shapes, 7.239 paradas, horário de cada viagem). O GTFS é a fonte única de itinerário, paradas e
**horários reais** (sem intervalo sintético); as relações do OSM não são usadas. Detalhes em
`data/onibus_recife/metodologia_onibus.md`.
- Dia útil de referência (quarta sem exceção no calendário); um padrão por `shape_id`.
- Itinerário recortado pelo mapa do app e **já casado com os ways do `mapa-recife.osm.gz`** (Viterbi com rumo e mão
  única do núcleo; buracos curtos ligados pelo menor caminho dirigido). Saem `ways` com sinal = sentido, polilinha
  simplificada, paradas como distância no trecho e o instante de entrada de cada viagem no trecho.
- Ajustes em `ajustes_manuais.json` (desligar linha/padrão, trocar `ways` de um trecho, parada baia/mover/remover),
  reaplicados por `aplicar_ajustes.py` sem baixar nada; rodar o gerador sem rede dá o mesmo JSON (conferido).

Cobertura na conclusão da fase 3: 292 linhas, 999 padrões, 700 contínuos (70 %), 92 pares de lacunas. Os números
atuais, depois das exceções da fase 4, estão na seção seguinte e em `data/onibus_recife/relatorio_onibus.md`.

### Fase 4 — Núcleo: encaixe na rede
**Concluída (2026-11-03).**

**Exceções para ônibus** (`excecoes` em `scripts/onibus_recife/ajustes_manuais.json`, usadas pelo gerador e pelo
núcleo). Olhando as piores lacunas, quase todas eram coisas que o OSM não descreve para ônibus: corredores e terminais
mapeados como `service` (que o núcleo não carrega), corredor do BRT sem nó comum com a pista, contrafluxo e
conversões proibidas no OSM que o itinerário oficial faz.
- `incluir` (vias `service` viram vias só de ônibus), `ligar` (ligação de mão única só de ônibus entre dois nós),
  `mao_dupla` (faixa de ônibus no contrafluxo), `conversao` (conversão só de ônibus num nó). Restrição do OSM com
  `except=bus`/`psv` passa a valer só para carros.
- O gerador atravessa vias `service` para ligar buracos do itinerário e lista as usadas em `incluir_auto` (30).
  Também sugere um `ligar` por lacuna (`lacunas.csv`) e passou a exigir coerência de sentido em cada passagem entre
  ways, descartando ruído do casamento e aceitando retorno no ponto final.
- Propostas a partir dos dados, cada uma com nota: 14 `incluir`, 46 `ligar`, 4 `mao_dupla`, 35 `conversao`. As
  marcadas "conferir"/"DUVIDOSO" (79) pedem conferência local.

**Encaixe** (`core/src/transit.rs`): o JSON entra no núcleo por `import_osm_full(bytes, semáforos, linhas)`. As
exceções mudam o OSM bruto antes do `BaseCache` (como a CTTU). O encaixe roda sob demanda depois de cada edição
(`Network::transit_fit`, `transit_json()`), para não pesar na edição.
- Cada item `±way` vira os trechos dele na rede atual, com o sentido pela geometria do way bruto (vale para way
  dividido por via nova, `oneway=-1` e inversão). Dijkstra sobre (trecho dirigido, item), passando só por conversões
  ligadas (`CONN_ALLOWED` ou o novo `CONN_BUS`). Way que o itinerário só toca num nó é aceito.
- Conversão só de ônibus: estado `CONN_BUS = 3` em `node_conns`. Carros e o roteador continuam só com `CONN_ALLOWED`;
  a interface o desenha como ligada.
- Trecho que não encaixa fica desligado com aviso (lacuna, via inexistente, conversão proibida ou ausente no nó X,
  vias que não se encontram). Nada é pulado. Paradas: projetadas em ordem, a até 30 m, na faixa mais à direita (a de
  ônibus, se houver).

| Medida | Valor |
|---|---|
| Padrões contínuos no gerador | 853 de 995 (86 %; eram 70 %) |
| Trechos encaixados no núcleo (Recife) | 874 de 1.077 (81 %) |
| Falhas restantes | 143 lacunas, 46 vias que não se encontram (retorno no ponto final que o casamento não registra), 14 conversões ausentes |
| Paradas encaixadas / fora do caminho | 21.120 / 715 (por trecho) |
| Encaixe nativo, Recife | ~110 ms (meta de 10 ms não batida; fica fora da edição por ser sob demanda) |
| `recife_behavior` sem linhas | idêntico ao de antes |

Testes: `transit_fits_route_and_stop`, `transit_demolished_way_disables_and_undo_restores`,
`transit_way_split_by_new_road_still_fits`, `transit_ligar_creates_bus_only_link`, `transit_mao_dupla_adds_bus_contraflow`,
`transit_conversao_allows_bus_only_turn`, `restriction_with_bus_exception_is_bus_only`,
`transit_fit_same_after_fast_and_full_rebuild`; diagnósticos ignorados `onibus_recife` (`RECIFE_OSM`, `SUGERIR=1`
emite as conversões que faltam como exceções) e `onibus_padrao` (`PADRAO=<shape>`).

Fica para as próximas fases: o worker e a interface ainda não carregam as linhas (fase 6); os ônibus ainda não usam
`CONN_BUS` nem seguem o caminho encaixado (fase 5); o encaixe pode ficar mais rápido se for preciso refazê-lo a cada
edição.

### Fase 5 — Simulação
- **Geração por linha**: cada viagem entra no trecho no horário do GTFS (`partidas`, com pequena variação sorteada
  pela semente), segue a rota fixa até o fim e termina no terminal ou na borda do mapa. Substitui o `bus_share` quando há linhas carregadas;
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
| 2 — Desempenho | concluída (2026-11-02); falta só perfilar o desenho do mapa com o painel visível |
| 3 — Dados de ônibus | concluída (2026-11-02): GTFS oficial, 290 linhas, 86 % dos padrões contínuos (com as exceções da fase 4) |
| 4 — Encaixe na rede | concluída (2026-11-03): exceções para ônibus, 81 % dos trechos encaixados |
| 5 — Simulação de ônibus | pendente |
| 6 — Interface de linhas | pendente |
| 7 — Moto como tipo | pendente |
| 8 — Corredor | pendente |
| 9 — Motos por parcela | pendente |
