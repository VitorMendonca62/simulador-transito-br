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
- Estacionamento, ciclovias e calçadas não são desenhados; `service=driveway|parking_aisle` e `access=private` (serviço) são ignorados.
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

### Feito: edição de conversões (2026-10-16)
- Clique numa **curva tracejada** do nó selecionado (ou numa **seta** no painel) alterna permitido/proibido; hover realça a curva. "Restaurar nó" desfaz as edições daquele cruzamento.
- Dados: `connU32[c*5+4]` é alterado em memória; o original fica em `connOrig` (store.ts). Edições = diferença contra o original, chaveadas por `osmIdDoNó:faixaOrigem:faixaDestino` (`persistence.ts`), salvas em IndexedDB e incluídas no JSON de "Exportar cenário" (campo `connections`). Ids de faixa são determinísticos para o mesmo arquivo; o cenário só vale para o mesmo mapa.
- Limite atual: só alterna conexões que o núcleo já gerou; criar conexões novas entre faixas arbitrárias (como no TM:PE) ainda não existe.
- A flag `allowed` ainda vive no TS; quando a simulação (fase 3) entrar, o cenário precisa ser enviado ao núcleo.

### Próximas peças da fase 2
nº de faixas por trecho; criar/apagar vias; adicionar/remover semáforos; fluxo de entrada (OD).
