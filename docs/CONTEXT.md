# Simulador de Trânsito BR — Contexto do Projeto

> Arquivo de contexto persistente. Atualize sempre que uma decisão mudar.
> Última atualização: 2026-10-16 (fase 1 implementada).

## Visão do produto
Ferramenta web (roda no navegador, precisa ser muito otimizada) para **gerentes testarem mudanças viárias** e analisarem o impacto no trânsito, com **comportamento brasileiro** (CTB/CONTRAN, mão direita).
Tudo deve ser customizável no fim: quantidade de faixas, fluxo, criação de vias, criação/modificação de semáforos, proibição de conversões etc.

## Pedido original (fase 1)
1. Mapa construído a partir do grafo do OpenStreetMap.
2. Botão para importar mapa `.osm` ou `.pbf`.
3. Camada de semáforos alterável (toggle + edição de tempos).
4. Sinalização horizontal: faixas brancas e amarelas, faixa de pedestre, linha de retenção.
5. Nome das ruas visível.
6. Nós das vias visíveis.
7. Clicar num nó de cruzamento mostra, estilo **TM:PE (Cities: Skylines)**, os caminhos possíveis (frente, esquerda, direita…) com ícones de seta e **curvas tracejadas** entre faixas. Não precisa pintar seta no chão; só aparece ao selecionar.
8. Semáforos no mapa.
9. (Futuro) Editor completo + simulação de veículos + análise.

Referência visual: fundo escuro com grid, asfalto cinza-escuro, nós como círculos (verde = ponta/forma, laranja com halo = cruzamento), curvas tracejadas verdes, setas amarelas (↑ ↱) acima do nó.

## Decisões técnicas
| Tema | Decisão |
|---|---|
| Render | **PixiJS v8 (WebGL)**, fundo escuro estilizado, sem tiles externos |
| UI | **Vite + TypeScript + React** (React só para painéis; motor do mapa fora do React) |
| Núcleo | **Rust → WebAssembly** (`core/`, wasm-pack, roda em Web Worker): parse OSM/PBF, grafo, faixas/conexões, geometria, depois simulação |
| Escala | Bairro/região (dezenas de milhares de nós), processado no navegador |
| Semáforos | **Somente do OSM** (`highway=traffic_signals`) na fase 1; add/remover manual na fase 2 |
| Persistência | Cenário em **JSON versionado** exportável + **autosave em IndexedDB**; sem backend por enquanto |
| Regras BR | Amarelo separa sentidos opostos; branco separa faixas do mesmo sentido; ciclo verde/amarelo/vermelho; rotatórias (`junction=roundabout`, prioridade de quem circula); faixas de ônibus (`busway`, `lanes:bus`) |

Toolchain disponível: Node 24, rustc 1.98, wasm-pack, target `wasm32-unknown-unknown`.

## Arquitetura
```
core/                      # crate Rust (cdylib, wasm-bindgen)
  src/lib.rs               # API: load_osm(bytes, fmt) -> NetworkHandle; getters de buffers
  src/osm/{xml,pbf}.rs     # quick-xml + osmpbf (sem rayon); filtra highway/signals/crossing/restrictions
  src/projection.rs        # lat/lon -> metros locais
  src/network.rs           # split de ways em nós de cruzamento, segmentos, faixas
  src/connections.rs       # LaneConnections, turn:lanes, restrictions, curvas Bézier
  src/geometry.rs          # offset de polylines, polígonos de interseção, marcações -> vertex buffers
  src/spatial.rs           # índice espacial para hit-test (rstar)
  src/scenario.rs          # serde -> JSON versionado
src/
  workers/core.worker.ts   # carrega WASM, devolve Float32Array/Uint32Array como transferables
  main.tsx, App.tsx        # shell React
  ui/                      # ImportButton, LayerPanel, NodeInspector, SignalPanel
  engine/MapView.ts        # Pixi Application, câmera pan/zoom, culling por chunks, LOD
  engine/layers/           # RoadLayer, MarkingLayer, LabelLayer, NodeLayer, SignalLayer, TurnOverlay
  model/                   # types.ts (espelho do core), store.ts (zustand), persistence.ts (idb-keyval)
```
Princípios de performance: geometria estática gerada no Rust e enviada pronta (zero cópia) → `Mesh` estáticos no Pixi (nada de `Graphics` redesenhado por frame); LOD por zoom; culling espacial; nós via instancing/ParticleContainer.

## Modelo de dados
- `Node {id, x, y, osmId, kind: junction|shape|endpoint, signalId?}`
- `Segment {id, from, to, points[], name, highway, oneway, lanesFwd, lanesBwd, width, busLanes, maxspeed, roundabout}` — ways OSM quebradas nos cruzamentos.
- `Lane {segmentId, index, dir, type: car|bus, turnTags}` — usa `turn:lanes`; senão heurística (esquerda → esq/frente, direita → dir/frente).
- `LaneConnection {fromLane, toLane, kind: straight|left|right|uturn, allowed, curve}` — por nó; respeita relações `restriction=*`. É o que o overlay TM:PE mostra/edita e o que a simulação usará.
- `Signal {id, nodeId, phases[{greens: connectionIds[], green, yellow, allRed}]}` — padrão: 2 fases por eixo.
- IDs numéricos, arrays/`Map`, geometria em typed arrays.

## Roadmap
- **Fase 1 (atual)**: itens 1–8 — import, grafo, marcações, nomes, nós, overlay TM:PE, camada de semáforos.
- **Fase 2**: editor — nº de faixas, setas por faixa, proibir/permitir conexões no overlay, criar/excluir vias, adicionar/remover/configurar semáforos, fluxo de entrada (OD).
- **Fase 3**: simulação de veículos (IDM + MOBIL, troca de faixa, gap acceptance, prioridade em rotatória) no core WASM em worker, passo fixo; render por instancing.
- **Fase 4**: métricas e comparação de cenários (tempo de viagem, filas, atraso por cruzamento); backend para compartilhar cenários.

## Verificação (fase 1)
- `npm run dev`, importar `.osm` (export do openstreetmap.org, bairro de SP com semáforos e rotatória) e `.pbf` recortado (Geofabrik/BBBike).
- Conferir marcações por sentido, nomes, nós, semáforos; clicar cruzamento → setas + curvas tracejadas; restrictions respeitadas.
- `cargo test` no core (projeção, split, faixas/conexões, restrictions, round-trip JSON); `vitest` na ponte TS.
- Metas: 60 fps em pan/zoom e import < 3 s com ~50k nós.


---

## Estado atual (fase 1 implementada)

### Como rodar
```bash
npm install
npm run build:core     # Rust -> WASM (core/pkg), requer wasm-pack
npm run dev            # http://localhost:5173  (botão "Exemplo" abre samples/sample.osm)
npm test               # cargo test do núcleo (8 testes)
npm run sample         # regenera samples/sample.osm e sample.pbf (sintéticos, sem download)
npm run build          # produção (build:core + tsc + vite)
```

### Entregue (itens do pedido)
1. Grafo a partir do OSM: `core/src/net.rs` (ways quebradas em nós de cruzamento/semáforo/travessia, faixas por `lanes`, `lanes:forward/backward`, `oneway`, rotatória).
   - Níveis (`layer`, `bridge`, `tunnel`): nó que o OSM compartilha por erro entre vias de níveis diferentes que *passam* por ele (miolo das duas — ex.: viaduto da BR-101 sobre o Girador da UFPE) é duplicado por nível em `osm.rs::separate_levels`; cópias ganham id `id + ((nível+64) << 40)`. Transição na ponta de uma via (rampa → ponte) segue conectada. Vias com nível > 0 vão para a malha `hi` do ladrilho e são desenhadas acima do térreo.
2. Botão **Importar mapa** (`.osm`, `.pbf`; também arrastar-e-soltar; `.json` aplica cenário). Detecção do formato pelo conteúdo. PBF com zlib validado.
3. Camada de semáforos: toggle, relógio animado (pausar/velocidade), edição de verde/amarelo/vermelho-geral por fase (`SignalPanel`), restaurar padrão.
4. Sinalização de solo (`core/src/render.rs`): amarelo duplo/tracejado no eixo, branco tracejado entre faixas, linhas de borda em vias principais, faixa de pedestre (zebra) em nós com semáforo/travessia, linha de retenção, faixa de ônibus em vermelho.
5. Nomes de ruas (`LabelLayer`): **DOM sobre o canvas** (CSS transform), anti-colisão em tela, rotação ao longo da via, tamanho proporcional à largura. Antes era BitmapText/Text do Pixi, trocado porque texturas de texto geradas durante o zoom saíam corrompidas (letras trocadas, caixas brancas, trechos sem asfalto) de forma intermitente.
6. Nós visíveis: laranja = cruzamento (grau >= 3), verde = ponta/trecho, pontos de forma (zoom alto, toggle).
7. Clique em cruzamento -> overlay estilo TM:PE (`TurnOverlay`): curvas Bézier tracejadas verdes, conexões proibidas em vermelho, setas por faixa de entrada, pontos de entrada/saída; painel `Inspector` lista faixas e setas. Respeita `turn:lanes` e relações `restriction` (no_*/only_*).
8. Semáforos do OSM (`highway=traffic_signals`) com fases padrão por eixo (agrupadas por ângulo).

### Desvios do plano original (decididos na implementação)
- Sem `rstar`/`spatial.rs`: picking de nó é varredura linear em TS (cruzamentos selecionáveis), suficiente para dezenas de milhares de nós.
- Sem culling por chunks: malhas estáticas únicas por camada (asfalto, ônibus, branco, amarelo), recorte pela GPU; LOD por zoom liga/desliga camadas. Medido: 22,8 mil nós / 543 mil triângulos a ~59 fps.
- Pontos de nó são meshes em unidades do mundo (sem custo por frame); overlay/halo/hover usam `Graphics` com tamanho constante em pixels (redesenho só do nó selecionado).
- Render sob demanda (`dirty` flag), sem laço contínuo quando parado.
- Persistência: IndexedDB guarda o arquivo importado + edições de semáforo (por id OSM do nó); "Exportar cenário" gera JSON só com as edições.
- Sem testes `vitest` ainda (ponte TS); cobertura está nos 8 testes Rust.

### Medidas
- Mapa sintético 17 MB (158 mil nós OSM, 22,8 mil nós de grafo, 90 mil faixas, 270 mil conexões): núcleo nativo 470 ms; no navegador import total ~1,1 s (WASM 681 ms).

### Limitações conhecidas / próximos passos imediatos
- Estacionamento, ciclovias e calçadas não são desenhados; vias de serviço (`highway=service`: acessos, estacionamentos, vielas) são ignoradas.
- Restrições com `via` do tipo way não são tratadas (só via = nó). Direção das restrições vale por way de origem (assume way partido no nó via, como manda o OSM).
- Faixas de ônibus não restringem conexões ainda (fase 3).
- Largura de faixa fixa por classe de via (2,7–3,6 m); `width=*` do OSM não usado.
- Fase 2 deve mover `signals` para o núcleo (hoje a edição de tempos vive no TS/store) e adicionar edição de conexões (alternar `allowed` no overlay), nº de faixas, criação de vias.
- Dado real de teste: ainda não foi importado um recorte real do OSM (apenas sintético). Validar com um .osm/.pbf de bairro brasileiro.

### Lições (bug de zoom, 2026-10-16)
- Evitar texto no WebGL do Pixi com muitos rótulos criados durante zoom; usar DOM para rótulos.
- O laço de render (`MapView.loop`) captura exceções por quadro para nunca parar de re-agendar.
- Dados de teste real: `~/Downloads/map(2).osm` (Recife, 548 nós de grafo, 43 semáforos) — usar para validar mudanças visuais.

## Fase 2 — andamento

### Feito: conector de faixas estilo TM:PE (2026-10-16)
- Núcleo gera, além das conexões padrão, **todas as candidatas** entrada->saída do nó com estado `CONN_ABSENT` (`core/src/conn.rs`). Estados em `connU32[c*5+4]`: 1 ligada, 0 proibida por restrição OSM (vermelho tracejado), 2 ausente (invisível até ser ligada). Constantes em `net.rs` e `model/types.ts`.
- Overlay (`TurnOverlay.ts`): origens = pontos **grandes e coloridos** (paleta por faixa, contorno branco); destinos = pontos **pequenos**; curva de cada conexão na cor da origem. Origem ativa realça suas conexões e esmaece as demais.
- Interação (`MapView.ts`): com um cruzamento selecionado, **arrastar de uma origem até um destino** liga a conversão; arrastar o mesmo par de novo remove. Prévia branca durante o arrasto; destino sob o cursor ganha anel. Clique numa origem a deixa ativa; Esc ou clique no vazio solta. Clique numa curva remove; setas do painel ligam/removem em grupo. "Restaurar nó" desfaz.
- Persistência: diferença contra o estado original, chave `osmDoNó:faixaOrigem:faixaDestino` -> estado, em IndexedDB e no JSON de "Exportar cenário" (`connections`). Ids de faixa são determinísticos para o mesmo arquivo; o cenário só vale para o mesmo mapa.
- Custo: candidatas elevam as conexões (mapa sintético grande: 270 mil -> 361 mil; import nativo ~510 ms).
- A flag de estado ainda vive no TS; quando a simulação (fase 3) entrar, o cenário precisa ir ao núcleo.

### Feito: edição de faixas por trecho (2026-10-16)
- Modo **Faixas** (alternador na barra): clique numa via seleciona o **trecho** (segmento entre cruzamentos); realce verde com chevrons de sentido (→ = do início ao fim do desenho).
- Painel (`SegmentPanel`): mão única, nº de faixas por sentido (1–8, estilo "stepper"), largura da faixa (2,4–4,5 m), faixa de ônibus por faixa, inverter sentido, restaurar. Escopo: "Só este trecho" ou "Toda a via" (todos os trechos do mesmo way OSM).
- Arquitetura: tudo derivado vive no núcleo. `Network` (Rust) guarda o OSM bruto e `SegEdit`s; `apply_edits(json)` reconstrói grafo, faixas, conexões, semáforos e malhas (`core/src/lib.rs`, `net.rs`). O worker devolve os buffers novos; `MapView.updateNetwork` troca a cena mantendo câmera e seleção.
- Chave estável do trecho: `way:nóOsmInicial:nóOsmFinal` na orientação original (`SegKey`). Edições de trecho em `segRecord` (store.ts), IndexedDB (`seg-edits`) e `segments` no JSON de cenário.
- **Edições de conversão passaram a usar chaves estáveis** (`nóOsm|way:de:para.dir.idx>...`, IDB `conn-edits-v2`, `connFormat: 2`), por isso sobrevivem à renumeração de faixas; `connRecord` é a autoridade e é reaplicado após cada reconstrução. Conversões que apontam para faixas que não existem mais são descartadas.
- **Bifurcação em Y** (`conn.rs::lane_group`): quando duas ou mais saídas têm a mesma manobra (ex.: dois ramos a < 30° da via de chegada, ambos "em frente"), as faixas de entrada que fazem a manobra são divididas em grupos contíguos, da esquerda para a direita na ordem angular dos ramos (faixa do meio compartilhada se a divisão não é exata; com menos faixas que ramos, cada faixa leva a todos). Antes toda faixa ligava a todos os ramos, e a faixa mais à esquerda cruzava para o ramo da direita (147 de 148 forks do Recife).
- Ao mudar o nº de faixas, as conversões padrão do cruzamento vizinho são recalculadas (as conversões editadas que ainda existem são mantidas).
- Custo: reconstrução total a cada edição (debounce 90 ms). Mapa pequeno: poucos ms; mapa de 17 MB: ~0,5 s + transferência. **Otimização futura:** reconstrução incremental só da vizinhança do trecho.
- Limites: máscaras `turn:lanes` do OSM são descartadas se o nº de faixas do trecho muda; inverter sentido em via de mão dupla troca os sentidos e as faixas.

### Feito: adicionar, remover e configurar semáforos (2026-10-16)
- **Adicionar/remover**: no modo Conversões, com um cruzamento (3+ ramos) selecionado, o painel mostra "Adicionar semáforo" / "Remover semáforo". Serve para semáforos do OSM e para os adicionados. Muda a geometria (faixa de pedestre, marcadores), então vai ao núcleo: `Edits.sigs` (`SigEdit {node(osm), enabled, offset, phases?}`) em `core/src/net.rs`; `Node.signal` passa a refletir a edição; `conn::build_signals(net, edits)`.
- **Configurar** (painel Semáforos, editor por semáforo): tempos verde/amarelo/vermelho-geral por fase, **defasagem** do ciclo (onda verde), **adicionar/remover fases** e **quem recebe verde em cada fase** (botões numerados coloridos por aproximação, com seta de direção e "vindo do …"). Com um semáforo selecionado, os marcadores no mapa ganham um anel na cor da fase.
- Edições de fase/tempo são **locais** (sem reconstruir a rede): `editSignalLocal` no store atualiza `signals`, o desenho e o `sigRecord`; ao reconstruir, o núcleo reaplica as fases do `SigEdit` (aproximação identificada por chave estável `way:de:para:lado`, `approach_key`). Aproximação não listada em nenhuma fase ganha fase própria (todo ramo é sempre atendido). Fase vazia é permitida (aviso na UI).
- Payload de reconstrução: `{"segments":[SegEdit], "signals":[SigEdit]}` (`apply_edits`; lista simples de SegEdit ainda aceita). Persistência: IDB `sig-config` e `signalConfigs` no JSON de cenário. O formato antigo de tempos por nó (`signal-edits`) foi descartado.
- Restaurar: por semáforo (volta ao padrão do OSM, ou remove se foi adicionado) e "Restaurar todos".
- Limites: ciclo é de tempo fixo (sem detecção/atuado); sem fase de pedestres separada; sem conflito/semáforo-amarelo piscante; só em nós de grau >= 3.

### Feito: criar e apagar vias, estilo Cities: Skylines (2026-10-16)
- Ferramentas na barra: **Conversões | Faixas | Construir | Demolir**. Desfazer (Ctrl+Z / botão) cobre construir e demolir.
- **Construir** (`BuildPanel`, `MapView`): escolhe o tipo de via (`model/presets.ts`: rua local, coletora, avenida 2+2/3+3, mão única, via expressa, viela; cada um vira classe OSM + faixas), modo **Reta** ou **Curva** (Bézier quadrática: 1º clique início, 2º curvatura, 3º fim; depois continua suave). Clique a clique o traçado é confirmado e **continua da última ponta** (cadeia); Esc/botão direito encerra. Prévia com a largura real da via, rótulo de comprimento e dica ("conectar", "dividir a via", "curva muito fechada"; curva > 100° recusada; trecho < 6 m recusado).
- **Encaixe**: anel verde = nó existente (14 px); anel amarelo = corpo de uma via (divide a via e cria o cruzamento); senão ponto livre. **Cruzamentos automáticos** onde a via nova atravessa outra, sem clicar.
- **Demolir**: passar o mouse realça o trecho (entre dois nós) em vermelho; clique remove. Funciona em vias do mapa e criadas.
- Núcleo (`core/src/net.rs`): `Edits.roads: Vec<NewRoad>` (id, classe, nome, mão, faixas, pontos em **metros locais**; `node`/`way` indicam encaixe) e `Edits.removed: HashSet<SegKey>`. A projeção usa só as vias do OSM, então as coordenadas locais são estáveis. Nós criados pelo editor têm **id negativo determinístico** a partir da posição (`coord_id`, resolução 10 cm); isso faz pontos coincidentes se unirem. `insert_on` divide vias existentes; `seg_cross` acha cruzamentos. A via nova vira um `W` como as do OSM (tags sintéticas via `road_tags`), então faixas, conversões, semáforos e Faixas-mode funcionam nela.
- Payload de reconstrução: `{"segments","signals","roads","removed"}`. Persistência: IDB `new-roads`, `removed-segs`; `roads`/`removed` no JSON de cenário. O histórico de desfazer é reconstruído a partir deles ao recarregar.
- Limites: demolição é por chave de trecho (way:de:para) — se uma via nova depois dividir esse trecho, a demolição antiga deixa de casar; rotatórias/ilhas não têm ferramenta própria; sem ajuste de elevação (túneis/viadutos) nem snap de ângulo/grade; vias novas ficam em metros locais do mapa em uso (cenário só vale para o mesmo mapa).

## Fase 3 — andamento

### M0 — medições (mapa sintético de 17 MB: 22,8 mil nós, 45 mil trechos, 90 mil faixas)
Nativo (`PERF_OSM=... cargo test --release perf_stages -- --ignored --nocapture`):
| estágio | antes | depois |
|---|---|---|
| parse OSM | 180 ms | 180 ms (só na importação) |
| grafo+faixas (`Net::build`) | 124 ms | ~70 ms (cache do que só depende do OSM) |
| conexões (360 mil) | 68 ms | **0** (sob demanda por nó) |
| malhas (543 mil tri) | 121 ms | **só ladrilhos sujos** (diff por hash) |
| `set_edits` total | 304 ms | ~110 ms |
No navegador, editar faixas: 350–1000 ms → **~210 ms incluindo debounce** (~120 ms de trabalho). Meta de 100 ms **ainda não** batida: o que sobra é o `Net::build` completo (segmentos 35 ms, recuos 12, ordenação de pontas 7, faixas 7) + cópia dos arrays de rede. **M2b (grafo incremental com ids estáveis) fica pendente** e não bloqueia a simulação (editar durante a simulação reinicia a simulação).

### M1 — zonas e demanda (feito)
- Núcleo `core/src/demand.rs`: `gravity()` (T_ij = O_i·A_j·f(t_ij)/Σ, f = exp(-β·t) com t em minutos, células fixadas pelo usuário, intrazona opcional), `resolve()` (nós/km de via por zona, centróide, geração na hora, avisos), `profile_at()` com interpolação 23→0. Tempo entre zonas: reta × 1,3 a 30 km/h; **vai virar tempo roteado no M3**.
- UI: ferramenta **Zonas** (polígono por cliques; Enter/duplo clique/clique no 1º ponto fecha; arrastar vértices; botão direito remove vértice ou cancela), `ZonePanel` (nome, geração no pico, atração, perfil horário com gráfico, cor, hora do dia, β, intrazona, % ônibus, semente, **matriz OD editável** com células fixadas em laranja). Persistência IDB `demand` e no cenário (`demand`).
- Linhas de ônibus: **ainda não** (entram com a simulação).

### M2a — incremental (feito)
- **Conexões sob demanda**: `conn::node_conns` gera as conexões de um nó (padrão, candidatas, restrições OSM e edições do usuário); a UI pede por nó (`fetchNodeConns`) e `Net.conns` só existe para testes/simulação (`ensure_conns`). As edições de conversão agora também vão ao núcleo (`connections` no payload, `set_conn_edits` sem reconstrução).
- **Malhas em ladrilhos de 1 km** (`render.rs`: `TileSet`, `update_tiles`): assinatura (hash) por trecho e por nó; só os ladrilhos cujo conteúdo mudou são regenerados e enviados (`tiles_index/pos/idx`, delta acumulado até `clear_delta`). Teste de equivalência: incremental == reconstrução completa após uma sequência de edições (faixas, via nova, semáforo, demolição).
- `BaseCache` (`net.rs`): vias aceitas, projeção, atributos, nós de quebra do OSM calculados uma vez.

### M3 — simulação (feito, v1)
- `core/src/sim/`: `route.rs` (grafo de trechos direcionados com transições = conversões ligadas; **Dijkstra reverso por zona-destino** → `próximo trecho`; penalidades de conversão/semáforo; velocidade livre por classe/`maxspeed`) e `mod.rs` (veículos em SoA; arestas = faixas + conversões; **IDM**; troca de faixa obrigatória (rota) e discricionária simples; **semáforo** com amarelo/dilema de parada e esquerda permissiva; **preferência** (via principal > secundária, rotatória, regra da direita) por **aceitação de brecha** usando conflitos entre curvas Bézier (calculados sob demanda por conversão); **keep-clear**; convergências "quem chega primeiro entra"; geração de viagens Poisson por par de zonas com a matriz gravitacional **roteada** (recalculada a cada 10 min simulados com o perfil horário); filas de entrada; PRNG com semente (determinístico); passo fixo de 0,2 s).
- Estado da simulação vive no worker (`Network.sim`); editar a rede **encerra** a simulação. API WASM: `sim_init/step/snapshot/stats/stop`; o worker avança em fatias de ~10 ms e envia quadros a ≤30 Hz (`sim`), estatísticas a 1 Hz.
- UI: `SimPanel` (iniciar/pausar/parar, 1×–30×, relógio, na rede/gerados/concluídos/fila/viagem média, legenda) e `VehicleLayer` (5 malhas por faixa de velocidade: parado/lento/médio/livre/ônibus; retângulos orientados ampliados em zoom baixo).
- Testes: geração e conclusão de viagens, **nenhum veículo sobrepõe outro** (15 min), **nenhum avanço de sinal vermelho**, conservação (gerados = concluídos + perdidos + vivos), **determinismo** por semente, estresse (8 mil veíc./h na rede de exemplo: folga mínima 0,3 m, 0 perdidos).
- **Desempenho** (mapa sintético de 22,8 mil nós): nativo 4,8 ms/passo com 16 mil veículos (≈0,29 µs/veículo/passo); **no navegador (WASM) a 10× a simulação acompanhou com 11 mil veículos e chegou a 9,2× com 28 mil**; desenho ~43 fps com 30 mil veículos na tela. Extrapolação para 50 mil: ~35 ms/passo em WASM ⇒ ~5× em tempo real. O portão de multi-thread (SharedArrayBuffer) **não** foi acionado.
- Pendências do M3: linhas de ônibus com paradas (hoje ônibus = % do fluxo, sem paradas); re-roteamento por congestionamento (rotas seguem o tempo livre); ~~interpolação visual entre quadros; geometria instanciada~~ (feito, ver abaixo).

### M4 — análise (feito, v1)
- **Métricas no núcleo** (`sim/metrics.rs`): por trecho direcionado e balde de 60 s (ocupação, velocidade média, fração parada, entradas), janela de 5 s com média exponencial para o calor em tempo real, histórico de razão de velocidade por minuto (replay), atraso e fila máxima por cruzamento, matriz OD de tempo/atraso, série global por minuto e `report()` (JSON).
- **Armadilha corrigida (importante para o A/B)**: o tempo médio só das viagens *concluídas* "melhora" quando um gargalo impede a maioria de concluir. O relatório inclui **atraso total incluindo veículos ainda na rede e na fila de entrada** (`totalDelayAllVehH`, `meanDelayAll`), vazão e velocidade média; a tela de comparação mostra isso e avisa. Teste `sim_ab_bottleneck_makes_things_worse`.
- **Mapa de calor** (`HeatLayer.ts`): fita por trecho direcionado (metade da pista daquele sentido, sem os recuos), 5 níveis de cor; modos **Velocidade** e **Filas**; geometria única com só os índices trocados a 1 Hz; **replay** por minuto (clicar nos gráficos ou no controle); funciona a 30× com a simulação rodando.
- **Gráficos** (`Charts.tsx`, SVG): veículos na rede, velocidade média, concluídos/min, fila de entrada.
- **Cenários A/B** (`ScenarioPanel.tsx`): "Salvar atual" guarda as edições (JSON do núcleo) + demanda; "Rodar" executa **headless no worker** (`runBench`: rede nova a partir dos mesmos bytes OSM + edições, simulação por N minutos, mesma semente, progresso/cancelar); resultado salvo em IDB (`scenarios`). Comparação: tabela de métricas com deltas coloridos, séries sobrepostas, cruzamentos com maior variação de atraso, e **mapa de diferença** (B − A, alinhado por chave de trecho). Exemplo medido (avenida 2+2 → 1+1, 20 min, 3.600 veíc./h): vazão 2.040 → 1.095, atraso total 72 → 121 veíc·h, fila de entrada 124 → 412; o tempo médio das concluídas (203 s vs 189 s) sozinho enganaria.
- Cenários só valem para o mapa importado (limpos a cada nova importação).
- Pendências: exportar/importar cenários A/B no JSON; comparar por par OD na tela; calor 3D/instanciado; relatório em CSV.

### Próximas peças
fluxo de entrada (OD); reconstrução incremental (hoje reconstrói tudo a cada edição); fase 3 (simulação).

## Seleção de veículo e correção de conversão à esquerda
- Clique num veículo (qualquer modo, camada Veículos ligada) mostra a rota restante até a zona de destino (`Sim::track`, `sim_track` no WASM, atualizada a ~4 Hz pelo worker) e o destino; painel na SimPanel. Snapshot de veículos agora tem stride 7 (último campo = id).
- Correção: em cruzamento semaforizado, aproximações concorrentes com sinal vermelho não entram mais na checagem de brecha da esquerda permissiva (antes uma fila parada no vermelho transversal travava quem virava à esquerda).
- Anti-impasse em `may_enter` (sim/mod.rs): fila justa por faixa de saída (quem espera há mais tempo passa; empate por id), veículo parado na linha deixa de arbitrar a brecha de quem espera >25 s, keep-clear relaxa gradualmente após 30 s parado, e aproximação em vermelho não conta na brecha da esquerda. Diagnóstico: `Sim::stuck_report` + teste `sim_stuck_diag` (`--ignored`); `REASON` (thread_local) indica o motivo do bloqueio. Mesmo cenário de teste: concluídos em 10 min 327 → 510.
- Rota do veículo: lane_pts usa o sinal de `off` conforme o sentido (dir 1 invertido); JSON de `sim_track` inclui `id` para descartar respostas antigas.

## Semáforo fora de cruzamento
- Modo **Semáforo** (Toolbar): clique no corpo de uma via grava uma "via" especial `hw:"mark"` com 1 ponto `{x,y,way}` em `roadRecord` (persiste, entra em cenários e no desfazer como qualquer via). No núcleo (`net.rs`, após as vias novas) o ponto vira nó via `insert_on`, o trecho é dividido e o nó nasce com semáforo; padrão de 2 fases (trânsito 30/3/2 s + fase vazia de 12 s para pedestres, em `build_signals`). Remover o semáforo apaga o ponto. Teste: `signal_can_be_placed_mid_block`.

## Faixa errada / infração (Recife)
- Veículo sem conversão legal para a rota na sua faixa e que não consegue trocar de faixa (fila, trecho curto) espera 8 s e então usa a conversão de uma faixa vizinha do mesmo trecho (`take_illegal_conn`; conta em `illegalMoves`, mostrado no painel como "Manobras irregulares"). Sem caminho nenhum por 90 s: desiste (`lost`).
- Entradas de zona só nascem em faixas ≥ 15 m com caminho até o destino (antes nasciam em tocos de 4 m e furavam o vermelho).
- Faixa de destino curta (< 30 m) que termina em semáforo fechado vira retenção virtual também para quem ainda está na faixa anterior (`leader`).
- Teste de diagnóstico no mapa real: `PERF_OSM=~/Downloads/map(2).osm cargo test --release recife_stuck -- --ignored --nocapture` (parados > 120 s: 27 → 0; furos de vermelho: 195 → 0; concluídos em 15 min: 332 → 406).

## Roteamento por faixa (substitui o por trecho no veículo)
- `route.rs::LaneRouter`: grafo com um nó por faixa; arestas = conversões ligadas (custo = penalidade de manobra + tempo da faixa) e trocas entre faixas vizinhas (custo 4/8/20/45 s conforme o comprimento do trecho: em trecho curto não há espaço, então o planejamento escolhe a faixa antes). Dijkstra reverso por zona → `lane_dist[zona][faixa]`. O roteamento por trecho (`dists`) continua para matriz OD, tempo livre e atraso.
- Veículo: `choose_conn` escolhe a conversão da faixa atual de menor custo (se a faixa só dobra à direita, ele dá a volta); `consider_lane_change` troca de faixa quando outra faixa (custo + troca) é ≥ 2 s melhor; carros ignoram faixas de ônibus.
- Infração opcional: parâmetro `illegalPct` (% de motoristas "infratores", padrão 0; campo "Ignoram a sinalização" no painel de zonas). Infrator (ou qualquer um após 45 s parado) usa a conversão de faixa vizinha se economiza > 20 s ou se não há caminho legal. Conta em `illegalMoves`.
- Mitigações: tempo de viagem > max(3×livre, 15 min) vira infrator; > max(6×livre, 30 min) ou 90 s sem saída desiste (`lost`); entradas só em faixas com caminho até o destino.
- Medido no Recife (3 zonas, 15 min): parados > 120 s: 0; manobras irregulares: 0 (sem infração configurada); concluídos 406 → 425; passo ~0,25 ms com 4 mil veículos.

## Destino aleatório dentro da zona
- **Destino fixo por veículo**: ao nascer, cada veículo sorteia um nó qualquer da zona de destino (`pick_target`, uniforme entre os nós onde chega alguma faixa interna da zona) e é esse o ponto até o fim da viagem (`tgt`, nunca muda). Roteamento em duas fases, para não guardar uma árvore do mapa inteiro por ponto: (1) fora da zona vale `zone_dist` (uma árvore por faixa até o primeiro nó da zona, `route.rs::LaneRouter::tree`); (2) dentro da zona vale o `Target.local` do ponto sorteado, uma árvore só com as faixas internas da zona (`tree_local`, criada na primeira vez que o ponto é sorteado; ~centenas de floats). `stay_cost` prefere sempre a conversão que segue por dentro da zona até o ponto; sem caminho por dentro, o veículo sai no próximo nó da zona. `dist` (tempo livre/atraso) = local dentro da zona; fora, `zone_dist` + média interna do ponto. Pontos que alcançam < 30 % das faixas internas são evitados; teto `LOCAL_BUDGET` de floats nas árvores locais (passado dele só se reaproveitam pontos já criados). Antes, em mapas grandes (Recife: 84 k faixas × 253 zonas) o orçamento dava 1 nó por zona e todos iam ao mesmo ponto. Medido com 256 zonas em grade no Recife: ~4 mil destinos distintos em 15 min.
- Medido no Recife (3 zonas, 15 min): concluídos 314 (antes 425, as viagens ficaram mais longas por entrarem na zona), parados > 120 s: 0, infrações de segurança 49 (espera > 45 s sem saída legal).
- Custo de troca de faixa reajustado (`lane_change_cost`: 4/10/35/90/200 s para ≥150/≥80/≥40/≥20/<20 m): o plano passa a entrar na faixa certa pela conversão anterior em vez de contar com trocas que não cabem em fila. Manobras irregulares com infração desligada no Recife (15 min): 517 → 49 → 6.

## Cruzamento sem semáforo: menos espera + ferramentas de depuração
- `may_enter`: saída ocupada considera quem já anda na faixa de saída (posição projetada); carro parado há > 3 s na linha da via transversal não segura mais quem espera > 6 s (era 8/25 s); brecha reduzida após 10 s (era 25). Amostras de "parado > 3 s na frente" no Recife (10 min): 5433 → 2025; concluídos em 15 min 314 → 388.
- Nascimento: faixa de origem sorteada proporcionalmente ao comprimento (`gate_cum`) e, em faixas ≥ 45 m, em ponto qualquer ao longo da faixa (com folga para líder/seguidor), não só logo após o cruzamento.
- Depuração: clicar num veículo mostra o estado completo (`Sim::explain` → `VehicleInfo`): viagem, rua/faixa, velocidades, aceleração, tempo parado, líder, próxima manobra, sinal, "pode entrar?" com o motivo e o veículo que bloqueia (`BLOCKER`, linha laranja no mapa), custo por faixa até o destino, atraso. Ferramentas: seguir (câmera), copiar JSON, avançar +0,2/1/5/30 s com a simulação pausada, lista "Parados há mais tempo (gargalos)" (`sim_stuck`), clique em #id para saltar ao líder/bloqueador. Testes de diagnóstico no mapa real: `recife_wait_reasons`, `recife_blockers` (`--ignored`).

## Placas de preferência por ramo (estilo TM:PE)
- Com um cruzamento sem semáforo (3+ ramos) selecionado no modo Conversões, cada ramo com faixa de entrada ganha uma placa redonda (DOM, `engine/SignLayer.ts`); clicar abre: **Automático (A)**, **Via preferencial** (losango), **Dê a preferência** (triângulo), **Pare** (octógono). Em "A" vale a hierarquia da via (classe/rotatória).
- Chave estável: `nóOsm|approach_key` -> nível 1..3 (`Edits.prios`, `priorities` no payload de reconstrução, `set_prio_edits` sem reconstruir, IDB `prio-edits`, `priorities` no JSON de cenário e nos cenários A/B). `node_approaches_json(node)` lista os ramos (posição, direção, nome, nível, `auto`).
- Simulação (`Sim::new`, via `SimInit.prios`): preferencial → `rank -1` (vence tudo); dê a preferência/pare → `rank 100` (cede a qualquer outro). **Pare** exige parada total (≥1,5 s, a ≤5 m da linha) antes de avaliar a brecha (`stop_ok`, motivo 10). Ignorado onde há semáforo.
- Limites: vale a partir da próxima simulação (editar com a sim rodando não a reinicia nem altera); as placas só aparecem com o nó selecionado; o roteamento ainda não penaliza o tempo de parada.
- Testes: `priority_signs_list_approaches_and_apply_to_the_sim`, `priority_signs_survive_rebuild_payload`.

### Veículos: quadros a 15 Hz com instancing e interpolação na GPU (2026-10-17)
- `Sim::snapshot` devolve um quadro **indexado pelo slot** (id do veículo = posição): 6 floats por slot, `[x, y, dx, dy, velocidade, comprimento]` da frente do veículo (comprimento negativo = ônibus; slot livre = zeros). Direção em vetor (sem `atan2`). 50 mil veículos: 5,9 ms nativo (antes 9 ms), 1,2 MB.
- O worker envia no máximo 15 quadros/s (`SIM_FRAME_MS`) e só o próximo depois do `simAck` da UI (controle de vazão; sem ack em 1 s volta a enviar).
- `VehicleLayer`: um `Mesh` instanciado (shader próprio, `GlProgram`), dois buffers da GPU com os dois últimos quadros; o vertex shader interpola posição e direção (uniform `uT`), calcula o tamanho mínimo na tela (`uZoom`) e a cor por faixa de velocidade. Sem quadro anterior, ou com salto > 100 m (slot reaproveitado), o veículo não interpola. `MapView.frame()` avança `uT` a cada quadro de tela; "seguir" e o realce da rota usam a posição interpolada (`find`), e o clique (`pick`) também.
- Medido (Chromium, 50 mil veículos sintéticos): `set` 0,2–0,3 ms de CPU (antes 5,7–8,4 ms com 20 mil); render total 4–8 ms.


## Zonas da Pesquisa OD 2018 (Recife) — 2026-10-17
- Dados: `scripts/od_recife/gerar_od.py` gera, além dos CSVs em `data/od_recife/`, o `public/data/od2018_zonas.json`. Ele traz as 253 zonas de tráfego (lon/lat, simplificadas a 15 m) e 24 matrizes de **carro dirigindo** (trabalho + educação), em veíc./h por hora de partida. O balde h cobre as partidas em [h−0,5; h+0,5), com média de dia útil ajustada pela frequência semanal. Premissas e limitações estão em `data/od_recife/metadados_e_limitacoes.md`.
- Projeção: o núcleo expõe a origem da projeção em `meta.geo = [lat_c, lon_c, kx]` (`Net::geo`, vinda de `BaseCache`). As coordenadas locais são x = (lon − lon_c)·kx e y = −(lat − lat_c)·111319,49.
- Importação (`model/odImport.ts`, botão "Importar zonas da Pesquisa OD 2018" no painel de zonas):
  - ficam as zonas com ≥ 3 nós da rede dentro do polígono, recortadas à caixa da rede;
  - as demais são incorporadas à zona mantida de centróide mais próximo (o tráfego de fora entra pela borda);
  - substitui as zonas atuais.
  - `applyOdImport` calcula, para a escala e a opção intrazona:
    - o perfil horário próprio de cada zona (`Zone.profileValues`);
    - a geração (pico × escala);
    - a atração;
    - as células fixadas da hora atual (interpoladas entre horas, igual a `profile_at`).
  - Sem "intrazona", as células i>i saem da matriz e do perfil.
- Simulação: `SimParams.odHourly` (24 mapas) substitui `odManual` e acompanha o relógio (`Sim::od_at`, interpolado; a matriz é recalculada a cada 10 min simulados).
- Estado: `odImport` (nome, escala, horas, zonas incorporadas) é persistido na demanda (IDB e JSON de cenário). "Desligar matriz" mantém as zonas e volta ao gravitacional. Editar uma célula à mão vale até a próxima mudança de hora, escala ou intrazona.
- Medido no `mapa-recife-novo.osm`:
  - 128 zonas no mapa, 125 incorporadas;
  - 7h: 50,6 mil veíc./h com escala 100%;
  - com 15% às 7h: ~1,3 mil veículos ativos estáveis, 30× o tempo real, 16 desistências em 1.060 concluídos;
  - a tabela OD só aparece com ≤ 20 zonas.

## Entradas e saídas para as cidades vizinhas (OD 2018) — 2026-10-18
- Importador `src/model/gatewayImport.ts`, ligado por `buildOdImport(src, data, { gateways: true })`. Opção "Entradas e saídas para as cidades vizinhas…" no painel de zonas, ligada por padrão; desligada, volta o modo antigo (zona de fora incorporada à zona de borda mais próxima).
- **Zonas que ficam no mapa**: ≥ 3 nós e ≥ 50 % da área dentro da pegada da rede (células de 250 m com nó, com fechamento morfológico de raio 1 km, que preenche matas e campi). As do município principal (o com mais nós) ficam com ≥ 15 % (Várzea, Bairro do Recife). No `mapa-recife-novo.osm`: ficam as 108 do Recife; as 145 restantes são de fora.
- **Portais**: trecho de via `motorway`…`secondary` (sem alças) com um nó dentro de uma zona de fora e o outro fora de todas (lado de dentro com grau ≥ 2). Cruzamentos da mesma via a < 400 m, ou de qualquer via a < 120 m (pistas com nome trocado no OSM), viram um portal só. A zona do portal é o casco de octógonos de 30 m em volta dos nós de fora. Ids a partir de 1001, `Zone.gateway = true`. No Recife são 22: BR-101 (Mário Covas, Recuperação), BR-232 (Luiz Gonzaga), Agamenon, Cruz Cabugá, Cidade de Monteiro, Ponte da Convenção, Luís Correia de Brito (Olinda), Av. Boa Viagem e Visconde de Jequitinhonha (Piedade), Deus é Fiel (São Lourenço), Estrada para Guabiraba (Camaragibe) e outras.
- **Quantos carros**: a matriz horária da OD 2018 (carro dirigindo) é redistribuída. Fora → dentro e dentro → fora usam um logit sobre `capacidade × exp(−(d(origem, portal) + d(portal, destino) − mínimo) / 1,5 km)`, com capacidade = faixas no sentido × peso da classe (expressa 1, arterial 0,8, coletora 0,6). Fatias < 2 % saem. Fora → fora (passagem) vai de portal a portal só se o melhor caminho por dois portais (a ≥ 1,5 km um do outro) não passa de 1,2 × a reta; o resto é descartado (`OdImport.dropped`, viagens/dia). É uma hipótese: a pesquisa não diz por onde a viagem passa. Às 7h, com escala de 100 %: 51,5 mil veíc./h, dos quais 12,9 mil entram, 4,7 mil saem e 0,55 mil atravessam; às 18h, 3,7 mil entram e 9,5 mil saem.
- **Núcleo**: `ZoneIn.gateway`. Zona-portal nasce também em `motorway`/`trunk` (zona comum não) e não recebe o aviso "sem vias dentro da zona". Teste `gateway_zone_spawns_on_expressway`.
- **UI**: marcador amarelo de tamanho fixo no portal; tabela "Divisa às HH:MM" com veíc./h que entram e saem por portal na hora atual (clicar seleciona).
- Medido (nativo, 15 min às 8h, escala 15 %): 24 % dos veículos na rede vêm de um portal e 11 % vão para um. As manobras irregulares subiram de 63 para 473; 431 delas numa faixa de 13 m da **Rua Benfica** (Madalena) cuja única conversão leva à via de ônibus (`busway`). É um defeito da rede que o novo padrão de tráfego expôs, não do portal.
- Limites: só detecta divisa onde há zona da OD do outro lado. Mapa recortado no meio do Recife (sem zona "de fora" na borda) não ganha portais; aí vale o modo antigo.

## Rotas em tempo real e comportamento (2026-10-18) — ver `docs/AUDITORIA_ROTEAMENTO.md`
- **Rotas em tempo real (estilo Waze)** (`SimParams.reroute`, padrão ligado): `Sim::route_step` recalcula a árvore `zone_dist` de **uma zona por vez, em fatias** (`route.rs::TreeJob`, `job_start`/`job_run`; 2–12 mil nós por passo, volta completa em ~3 min simulados). Peso de cada faixa = comprimento / velocidade medida no trecho (`Metrics::speed_ratio`, média móvel de ~5 s; sem dado = fluxo livre; piso 5% da livre). Como o veículo escolhe a conversão a cada faixa olhando a árvore atual, o desvio acontece no caminho.
- **% com app** (`appPct`, padrão 60): os demais usam `zone_free`, cópia de fluxo livre em u16 (meio segundo). `t_free` (atraso) sempre usa a cópia livre.
- **Percepção** (`perception`, padrão 0,3): `stay_cost` soma a cada saída um erro determinístico `σ·ruído(motorista, via seguinte)·(penalidade + tempo da faixa seguinte)`; mesmo erro em todas as faixas da via, para não criar trocas de faixa. Perto do nó (< 50 m) o plano só muda por ganho > 30 s.
- **Antecipação de curva**: quem tem conversão escolhida freia antes da linha até `vturn` (`need = (v²−vturn²)/2d`, a partir de 0,6·b) em vez de frear até 9 m/s² dentro do cruzamento. Estatística `harshBrakes` (> 5 m/s², uma por episódio).
- **Desbloqueio** (`reroute_if_stuck`): saída que mais aproxima do destino em linha reta (retorno por último), em vez da primeira da lista.
- Troca de faixa: seta 1 s antes (`blink_to`), sem troca nos últimos 12 m (`LC_SOLID_ZONE`) salvo preso > 6 s (conta em `solidCrossings`, separado de `illegalMoves`), preferência de quem já está na faixa de destino; fusão sem placa = zíper. Bifurcação: `conn.rs::fork_lanes` (faixa da divisa vai aos dois ramos).
- Medido no Recife (`recife_behavior`, 4 zonas, 15 min, média de 3 sementes): sem rotas em tempo real → com 60% app: atraso total 55,2 → 50,2 veíc·h, fila de entrada 351 → 278, concluídos 479 → 485; passo 1,30 → 1,62 ms. Freadas bruscas 21,9 mil → 11,3 mil com a antecipação.
- **Atraso medido por conversão** (passo 3, F): ao entrar numa conversão, o veículo registra quanto passou do tempo de fluxo livre na faixa (`t_due`: fila, semáforo, brecha, lentidão) → média móvel por conversão (`conn_delay`). A cada árvore recalculada, `refresh_live_pen` faz penalidade = manobra (`turn_base`) + atraso medido, que vai perdendo peso para a espera típica (`junction_penalty`: semáforo 14 s, cruzamento 3 s) após 60 s sem amostra (constante de 5 min); o primeiro da fila conta o atraso que já acumulou. Entra só no `LaneRouter` (`set_pen`) — o peso da faixa voltou a ser o de fluxo livre, para não contar a lentidão duas vezes. A conversão imediata em `stay_cost` usa a penalidade fixa (senão a espera do próprio veículo inflava o custo da faixa dele e gerava trocas e infrações); a decisão de infringir compara em fluxo livre. Painel do veículo: `livePen`, `measuredDelay`.
- Medido (Recife, 4 zonas, 15 min, 3 sementes): sem rotas em tempo real → passo 2 (velocidade do trecho) → passo 3 (atraso por conversão): atraso total 55,2 → 50,2 → 39,3 veíc·h; fila de entrada 351 → 278 → 256; concluídos 479 → 485 → 488; passo +0,3 → +0,65 ms. Efeito colateral: cruzamentos de linha contínua ~2,2 mil → ~3,2 mil (quem muda de plano perto do nó e fica na fila da faixa errada) — R6/R8 da auditoria.
- **R8 — troca de faixa com ocupação**: custo de trocar para a faixa `l` × `lc_mul(l)` = 1 + 4·ocupação (veículos × 7 m / comprimento), na árvore (`LaneRouter::change_mul`, atualizado a cada árvore) e na decisão do veículo.
- **R6 — MOBIL** na troca voluntária (`consider_lane_change`): ganho próprio + 0,3 × (ganho do seguidor antigo + do novo) > 0,2 m/s², com viés de 0,3 m/s² para a direita (mais fácil) e contra a esquerda (CTB art. 29); novo seguidor não pode precisar frear > 4 m/s²; não troca para faixa que piora a rota em > 5 s. **Cooperação** na obrigatória: `mand` marca a troca de rota; a cada passo, `wants` lista quem está com seta; o primeiro atrás do ponto de entrada (70% dos motoristas, `COURTESY`) passa a tratá-lo como líder (`courtesy`) e abre o vão. Sem vão à frente, quem troca ajusta a velocidade ao veículo da faixa de destino (até −3 m/s²).
- Medido (Recife, 4 zonas, 15 min, 3 sementes, com rotas em tempo real): linha contínua ~3,2 mil → ~0,75 mil; conversões proibidas 11 → 2; freadas bruscas 12,7 mil → 11,8 mil; atraso total 39,3 → 44,5 veíc·h e fila de entrada 256 → 316 (ceder passagem custa tempo a quem cede — é o objetivo: realismo, não fluidez); passo ~1,7 ms.
- **Conexões padrão mais reais** (`conn.rs`, 2026-10-18):
  - "Em frente" mantém a posição (`target_index`: `t + min(primeira faixa, n_out − m)`): com a faixa da esquerda convertendo, as outras 3 de 4 vão a 1, 2, 3 (antes 0, 2, 3, pulando uma).
  - Sem `turn:lanes`, as faixas da ponta são compartilhadas: esquerda + em frente e direita + em frente, para qualquer nº de faixas. Antes, com 3 ou mais, eram exclusivas de conversão, e a avenida perdia uma faixa de "em frente" a cada cruzamento.
  - Bifurcação por capacidade (`fork_alloc`, substitui `fork_lanes`): o ramo principal (`pick_main`: mesma via por nome ou way, depois mais faixas, depois mais alinhado) fica com todas as faixas que couberem, cada uma na sua posição. Cada ramo lateral pega as faixas da sua ponta, compartilhando no máximo uma; se os ramos juntos têm até `m` faixas, os blocos são exclusivos e proporcionais. Exemplo: avenida de 4 com saída de 1 à direita → as 4 seguem e só a da direita também sai.
  - Convergência (`merge_side`): o ramal que também segue "em frente" para a mesma saída entra pela ponta do seu lado, decidido pela **posição do braço** em relação à principal (não pelo sentido da curva). Quando a saída tem faixas a mais, a principal encosta no lado oposto.
  - Teste `straight_connections_never_cross_or_skip_lanes`: 0 cruzamentos e 0 saltos, também no Recife com `PERF_OSM`.
- Medido (Recife, 4 zonas, 15 min, 3 sementes, rotas em tempo real):
  - linha contínua ~745 → ~52;
  - conversões proibidas 2 → 1;
  - atraso total 44,5 → 32,3 veíc·h;
  - concluídos 478 → 522;
  - freadas bruscas 11,8 mil → 13,2 mil (mais convívio nas faixas compartilhadas).
- **Revisão geral das conversões padrão** (2026-10-18, depois de achar erros em vários nós):
  - **Restrições do OSM antes da distribuição** (`node_conns`: `banned(a, b)`, `assign`). As faixas se dividem entre as manobras permitidas: com a direita proibida, as duas faixas viram à esquerda (antes a faixa da direita ficava sem saída). O padrão que existiria sem a restrição é gerado como `CONN_BLOCKED` (tracejado vermelho).
    - Restrição cujo `to` não chega ao nó é ignorada.
    - Restrição com `except=motorcar|motor_vehicle|vehicle` é ignorada.
    - `restriction:motorcar` é lido; aproximação só de ônibus não é afetada.
    - Se todas as saídas forem proibidas, a aproximação faz retorno na própria via (salvo `no_u_turn`).
  - **Corredor exclusivo de ônibus**: `psv|bus=yes|designated` com `motorcar|motor_vehicle|access=no`, ou `*:conditional=no @ …`, vira faixa de ônibus em todas as faixas. O horário não é avaliado. Exemplo: Conde da Boa Vista.
  - **Convergência**: a principal encosta no lado oposto a quem mais entra na saída, seja ramal "em frente" ou conversão. Exemplo: Augusto Rodrigues com Ladário de Sousa, em que a Augusto usa as faixas 1 e 2 e a Ladário a 0 e a 1.
  - **T sem "em frente"**, com 3 ou mais faixas e sem setas: divisão esquerda/direita proporcional às faixas das saídas.
  - **Verificador** `default_connections_are_realistic`: cruzamento entre faixas, faixa sem saída, saída "em frente" sem uso, seta sem manobra e estreitamentos. Com `PERF_OSM` e `AUDIT_SHOW=n` mostra exemplos. Recife: 0 cruzamentos; 2 faixas sem saída, ambas com `no_u_turn` explícito no OSM.
  - **Ferramentas**: `conn_dump` (`NODES=osm1,osm2`: faixas, conversões, bloqueadas e restrições) e `conn_gallery` (`GALLERY=arquivo.html`: galeria SVG de cruzamentos sorteados), ambos `--ignored`.
  - **Pendente**: estreitamento de faixas (442 no Recife, ex.: 4 → 2) junta as faixas em pares. Não há dado de qual faixa termina.
- **Saída pelas divisas (fusão sem fome)** (2026-10-18). Medido com `gateway_entry_flow` (`--ignored`; `PERF_OSM`, `GATE_DIR=out|in`, `GATE_RATE` veíc./h por portal, `STUCK_S`): rumo à BR-232, veículos ficavam 4 a 10 min parados em fusões (motivo 6). A regra "quem chega primeiro ao ponto" nunca deixava entrar quem estava parado diante de um fluxo contínuo. Agora (`may_enter`, ramo de convergência):
  - em movimento, entra quem chega primeiro, com folga de 1 s (`MERGE_GAP`), e a chegada de quem está parado usa a aceleração real (`t_reach`);
  - quem está parado há mais de 2 s ganha a vez de quem ainda pode frear com conforto, se a outra corrente estiver em fila (< 5 m/s, `MERGE_SLOW`): zíper, um a um;
  - em fluxo livre, ganha a vez só depois de 20 s de espera (`MERGE_COURTESY`); antes disso, espera uma brecha;
  - a cortesia vale dos dois lados: quem se aproxima também cede a quem espera há mais tempo na outra corrente.
- Resultado (3.000 veíc./h por portal, saída): parados > 60 s, de vários (até 10 min) para 0; > 30 s: 1. Simulação geral do Recife: atraso total ~31 → ~34 veíc·h, dentro do ruído entre sementes, porque ceder custa tempo a quem cede.
- `Sim::gate_report` / `entry_stats`: por zona de origem, espera na fila de entrada e tempo na faixa de nascimento.
- **Destino inalcançável dentro da zona** (2026-10-18): um veículo que chega a qualquer nó da zona de destino, sem caminho por dentro dela até o ponto sorteado, sai ali (`stay_cost` → `SINK`). Antes isso só valia para faixas inteiras dentro da zona. Nos portais da divisa, que cobrem as duas pistas, o ponto sorteado podia ser a ponta da outra pista: o veículo empacava no fim da via e virava "perdido". Medido na Visconde de Jequitinhonha (portal de 80 m, 600 veíc./h, 40 min): concluídos 10 → 195, perdidos 39 → 3, fila de 79 veículos no fim da via → 0. `gateway_entry_flow` aceita `GATE_NODES`, `GATE_R` e `GATE_MIN`.
