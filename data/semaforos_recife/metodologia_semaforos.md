# Semáforos do Recife: metodologia

Base georreferenciada dos semáforos do Recife para o simulador. Os números de cada execução ficam em
[`relatorio_semaforos.md`](relatorio_semaforos.md). O script é
[`scripts/semaforos_recife/gerar_semaforos.py`](../../scripts/semaforos_recife/gerar_semaforos.py).

```bash
python3 -m venv .venv && .venv/bin/pip install -r scripts/semaforos_recife/requirements.txt
.venv/bin/python scripts/semaforos_recife/gerar_semaforos.py            # usa o cache em data/semaforos_recife/_brutos
.venv/bin/python scripts/semaforos_recife/gerar_semaforos.py --atualizar-osm   # refaz as consultas Overpass
```

## Classes de informação

Cada coluna pertence a uma destas classes. Elas nunca se misturam numa mesma coluna.

| classe | o que é | colunas |
|---|---|---|
| **dado oficial** | publicado pela CTTU/Prefeitura | `id_oficial`, `latitude`, `longitude`, `endereco_original`, `via_principal`, `tipo_controle*`, `funcionamento_descricao`, `sinal_sonoro_*`, `horario_sinal_sonoro`, `sinalizador_ciclista`, `iluminacao_suspensa_pedestre`, `data_atualizacao_fonte`, todas as `orig_*` |
| **derivado do oficial** | o dado oficial reclassificado ou cruzado com outra base oficial | `via_secundaria`, `ponto_referencia`, `intersecao`, `tipo_local`, `tipo_semaforo`, `bairro`/`cod_bairro` (associação espacial com os limites oficiais) |
| **complementar do OSM** | OpenStreetMap, ODbL | `via_osm_*`, `dist_via_*_osm_m`, `osm_id_correspondente`, `dist_osm_m`, tudo em `comparacao_cttu_osm.csv` |
| **inferência geográfica** | regra deste script | `intersection_id` e grupos de `intersecoes_semaforizadas.csv` (quando `qtd_semaforos > 1`), `confianca_localizacao`, `fonte = ambos` |
| **parâmetro sintético** | nenhum é gerado | os tempos e fases que faltam estão listados em `parametros_simulacao_pendentes.csv` |

## Fontes

### Primária: CTTU, [Localização dos Semáforos](https://dados.recife.pe.gov.br/dataset/localizacao-dos-semaforos)

- Responsável: Autarquia de Trânsito e Transporte Urbano do Recife (CTTU). Publicação: EMPREL.
- Licença: ODbL. Atualização declarada: "Semestral" no CKAN e "anual" no dicionário JSON (as duas fontes divergem).
- Conjunto criado em 2026-02-27. Recursos atualizados em 2026-03-02. Esta última data vai em `data_atualizacao_fonte`. Ela é a data de publicação no portal, não a do levantamento em campo.

| recurso | arquivo em `_brutos/` |
|---|---|
| [Semáforos do Recife (CSV)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/03995e83-91a3-4719-bedd-488886f103a1/download/semaforos-do-recife.csv) | `semaforos-do-recife.csv` (712 registros) |
| [Equipados para deficientes visuais (CSV)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/d163e8ff-dc1f-410d-81d5-164ea97dc447/download/semaforos-equipados-para-deficientes-visuais.csv) | `semaforos-deficientes-visuais.csv` (67) |
| [Sinalizador para ciclistas (CSV)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/922cc31a-5171-48c6-9764-c94d3fe5aa3d/download/semaforos-equipados-com-sinalizador-para-ciclistas.csv) | `semaforos-ciclistas.csv` (47) |
| [Iluminação suspensa para pedestres (CSV)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/a769697f-ade1-41ec-ae53-16bb2007ffd6/download/semaforos-com-iluminacao-suspensa-para-pedestres.csv) | `semaforos-iluminacao-pedestres.csv` (37) |
| [Dicionário de dados (PDF)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/2ea42f23-f405-4fdc-b06e-d70af7bbb375/download/dicionario-de-dados-dos-semaforos.pdf) e [(JSON)](https://dados.recife.pe.gov.br/dataset/289c2371-31f4-49ed-9226-059de68d2bae/resource/195dd8e0-a78e-436b-a862-b057834b9884/download/dicionario-de-dados-dos-semaforos.json) | idem |
| Dicionários/metadados dos três recortes (JSON) | idem |
| Metadados CKAN (`package_show`) | `metadados_ckan.json` |

### Limites de bairro (oficial)

Os 94 polígonos de [Bairros do Recife](https://dados.recife.pe.gov.br/dataset/bairros) (`bairros.geojson`, EPSG:4326, nome em `EBAIRRNOMEOF`) são a mesma base usada em `data/od_recife`. Não existe uma camada própria de limite municipal no portal. O município é tratado como a união dos bairros.

### Complementar: OpenStreetMap (Overpass API)

- Semáforos: nós `highway=traffic_signals` e `crossing=traffic_signals` dentro da área administrativa do Recife (`admin_level=8`, `IBGE:GEOCODIGO=2611606`). Resultado: 1.376 nós.
- Vias: ways `highway` veiculares com geometria. Resultado: 22.566 ways. Servem só para conferir nomes e posições.
- O timestamp do snapshot fica registrado no relatório.

**Atenção:** 988 dos 1.376 nós do OSM têm `source=Dados Abertos Prefeitura do Recife` e `ref=<código CTTU>`. Eles vieram de um import de 2014 desta mesma base. Quando um ponto oficial coincide com um nó importado, isso é concordância entre duas versões da mesma fonte, não validação independente. A comparação marca esses casos em `osm_importado_base_prefeitura`.

O Google Maps não foi usado como fonte.

## Etapa 1: auditoria do CSV oficial

### Colunas e dicionário

| coluna no CSV | significado (dicionário) | observação |
|---|---|---|
| `semaforo` | código do semáforo (3 caracteres, com zeros à esquerda: `001`…`712`) | sequência completa e sem repetição. Nos recortes, a coluna se chama `semafororo` e o código vem sem zeros |
| `localizacao1` | endereço onde está instalado | → `via_principal` |
| `localizacao2` | via do cruzamento **ou** ponto de referência | separado em `via_secundaria` ou `ponto_referencia` (ver Etapa 2) |
| `bairro` | bairro informado pela CTTU | preservado em `orig_bairro`. O `bairro` final vem da associação espacial |
| `latitude`, `longitude` | graus decimais | o CRS não é declarado. Os valores só fazem sentido como WGS84/SIRGAS 2000 geográfico, tratado como EPSG:4326 (a diferença entre os dois é submétrica) |
| `tipo` | tecnologia: `GIT` = Gerenciador Inteligente de Trânsito, `E` = Eletrônico (CES), `M` = Eletromecânico, `E/E` = Eletroeletrônico | valores encontrados: `E/Git` 557, `E` 137, `E/DIG` 15, `E/GIt` 3 (normalizado para `E/GIT`). **`E/DIG` não está no dicionário.** Não aparece nenhum `M` nem `E/E` |
| `funcionamento` | destinação: Ocasional/Ped, Veic.C/Ped., Veic.C/Ped.Ocas, Veic.C/Ped.Paral, Veic.C/Laço Det | aparecem também `Veicular` e `Pedestre`, que não estão listados |
| `Unnamed: 8` | **não documentada** | `1` em 626 linhas, vazia em 86. O significado é desconhecido. Fica em `orig_Unnamed: 8` e não é interpretada |

O PDF do dicionário usa nomes diferentes do CSV: `codigo`, `localizacao`, `funcionamento` para a tecnologia e `utilizacao` para a destinação. Ele também cita `sinalsorono` e `sinalizadorciclista`, que não existem no CSV. Esses dois atributos aparecem como recortes separados. Os rótulos do PDF ainda dizem "Postos de Saúde", um resíduo de modelo. Neste trabalho vale o dicionário JSON, que bate com o CSV.

### Validações

| verificação | resultado |
|---|---|
| total de registros | 712 |
| coordenadas ausentes | 0 |
| coordenada malformada | 1. O `693` tem latitude `-8.060979,` (vírgula final). Foi corrigida na leitura e anotada em `observacoes` |
| fora da faixa plausível ou do município | 0. O `091` fica 1,6 m fora dos polígonos (borda do rio) e recebe o bairro mais próximo |
| IDs duplicados / coordenadas idênticas / pares < 3 m | 0 / 0 / 0 |
| endereço textual idêntico | `336`/`617` e `515`/`590`. São pares em sentidos opostos (19 m e outra via) e foram mantidos |
| bairro publicado ≠ bairro espacial | 195. Destes, 26 usam um nome que não é bairro oficial (ex.: "Vila do Ipsep", "Brasilit", "Porta Larga"), 98 ficam a até 100 m do bairro publicado (borda) e 71 a mais de 100 m. Grafias equivalentes ("STO AMARO", "BAIRRO DO RECIFE") não contam como divergência |
| recortes x base principal | todos os IDs dos recortes existem na base. **`255` (Cais da Alfândega):** a base principal põe o ponto na Madalena, 4,1 km longe do endereço. O recorte de deficientes visuais traz uma coordenada coerente com o Cais da Alfândega. A coordenada oficial principal foi mantida, a alternativa vai em `observacoes` e a confiança fica `baixa`. Outras diferenças de 6 a 23 m: `017`, `196`, `342`, `118`, `150`, `137`, `309` |
| recorte de iluminação | as 37 longitudes usam vírgula decimal. Foram convertidas |

## Etapa 2: normalização

Saída: `semaforos_recife_normalizados.csv` (712 linhas) e `.geojson` (EPSG:4326). Todas as colunas originais seguem junto com o prefixo `orig_`.

- **`via_principal`** = `localizacao1`, com ajuste só de espaçamento e pontuação. As abreviações oficiais ("Av.", "Gov.") ficam como estão.
- **`via_secundaria`**: recebe `localizacao2` só quando o texto começa por um tipo de logradouro (Rua, Av., Estrada, Travessa, Praça, Largo, Ponte, Cais, Viaduto, BR-…) e não tem marcador de referência ("Em frente", "Próx.", "Estação", "nº", "Sentido", …). Fora disso, o texto vai para `ponto_referencia` e `tipo_local = travessia/meio de quadra`. A classificação lê só o texto publicado. A topologia nunca é usada para deduzir um cruzamento.
- **`intersecao`**: `Via A × Via B` quando há via secundária. Sem ela, `Via A (referência)`.
- **`bairro`**: *spatial join* `within` com os polígonos oficiais. Fora deles, o bairro mais próximo a até 200 m.
- **`tipo_semaforo`**, a partir de `funcionamento`:
  - `Veicular` e `Veic.C/Laço Det.` → **veicular**
  - `Ocasional/Ped.` e `Pedestre` → **pedestre**
  - `Veic.C/Ped*` → **misto**
- **`tipo_controle`**: o código `tipo` expandido segundo o dicionário.
- **`fonte`**: `ambos` quando existe nó OSM a ≤ 15 m, senão `CTTU`. Isso não muda a posição, que é sempre a oficial.
- **`confianca_localizacao`**: compara os nomes oficiais com as vias do OSM num raio de 60 m. Antes da comparação, abreviações são expandidas, acentos, tipo de logradouro e preposições são removidos, as variantes Morais/Moraes, Góis/Góes e Manoel/Manuel são igualadas, e números viram extenso ("21 de Abril" = "Vinte e Um de Abril").
  - **alta**: a via principal está a ≤ 30 m e, se houver via secundária, ela está a ≤ 50 m.
  - **média**: as vias foram achadas, mas mais longe, ou a via secundária não foi achada, ou a coordenada precisou de correção.
  - **baixa**: a via principal não aparece no OSM a até 60 m, ou um recorte oficial dá uma posição a mais de 100 m.
  - A confiança mede a coerência entre endereço e coordenada. Ela não garante que o semáforo exista.

## Etapa 3: agrupamento por interseção

Os grupos são as componentes conexas de um grafo em que dois semáforos se ligam quando:

1. estão a até **25 m** um do outro, ou
2. estão a até **60 m** e citam o mesmo par de vias (via principal e via secundária, em qualquer ordem). Isso cobre grandes avenidas com pista dupla e canteiro.

`intersection_id = INT-<menor código CTTU do grupo>`, estável entre execuções. O centro é o centroide dos membros, e o bairro sai do centro. Um grupo é marcado `revisar_manual` quando tem 5 ou mais semáforos, diâmetro acima de 80 m ou mais de dois pares de vias distintos.

Resultado: 712 semáforos formam 680 locais (523 cruzamentos e 157 travessias/meio de quadra). Só 32 grupos têm 2 semáforos e nenhum tem mais. Os 32 foram conferidos à mão e todos são coerentes: as duas travessias de uma estação de BRT (Cid/Sub e Sub/Cid), pista principal e pista local, ou uma travessia de pedestre colada ao cruzamento ("Ao lado Semáforo Nº 279"). Nenhum foi desfeito. A base indica que cada código CTTU corresponde a um controlador por local, e não a um grupo focal.

## Etapa 4: parâmetros para simulação

Foram verificados o dataset, os dicionários e uma busca no catálogo do portal por "semáforo", "tempo de ciclo" e "CTTU". A busca achou só datasets sem tempos semafóricos: chamados, fiscalização, velocidade, fluxo amostral, faixas de ônibus.

| item | classificação | o que existe |
|---|---|---|
| tempo de ciclo | indisponível publicamente | — |
| verde / amarelo / vermelho | indisponível publicamente | — |
| número de fases | inferência possível, mas não confirmada | `funcionamento` indica se existe grupo de pedestre ("paralelo", "ocasional") |
| grupos semafóricos | indisponível publicamente | — |
| coordenação / onda verde | indisponível publicamente | só reportagens |
| plano por horário | indisponível publicamente | apenas o horário do **sinal sonoro** (67 semáforos, ex.: `06:00/20:00h`) |
| prioridade para pedestre | inferência possível, mas não confirmada (onde há grupo de pedestre) | "Ocasional" sugere acionamento por demanda (botoeira), mas o dicionário não diz isso |
| prioridade para ônibus / emergência | indisponível publicamente | — |
| semáforo adaptativo / inteligente | inferência possível, mas não confirmada (`E/Git`) | integração ao GIT (central) não equivale a operação adaptativa |
| detecção veicular | disponível na fonte (1 semáforo, `Veic.C/Laço Det.`) | só a existência do laço |
| estado operacional em tempo real | indisponível publicamente | — |
| sentidos e movimentos | inferência possível, mas não confirmada | `oneway`/`turn:lanes` do OSM, como dado complementar |

Contexto não oficial, que não entra na base: reportagens de 2024 citam cerca de 717 semáforos, cerca de 200 com controladores capazes de operar em tempo real e um piloto adaptativo na Av. Antônio de Góes e na Av. Abdias de Carvalho. Nenhum desses números aparece nos dados abertos.

`parametros_simulacao_pendentes.csv` traz uma linha por interseção e dado necessário (680 × 13), com disponibilidade, fonte verificada e recomendação. Para usar no simulador:

1. **Pedido via LAI à CTTU**: planos semafóricos por controlador (ciclo, estágios, grupos, entreverdes, defasagens, tabela horária), lista de controladores em tempo real ou adaptativos, significado de `E/DIG` e da coluna sem nome.
2. **Coleta em campo** (cronometragem) nos cruzamentos de interesse do cenário.
3. Enquanto isso, **cenário sintético explicitamente rotulado** (no simulador, `Signal.phases` padrão de 2 fases por eixo, marcado como sintético), nunca apresentado como oficial.

## Etapa 5: comparação com o OSM

`comparacao_cttu_osm.csv`: para cada semáforo oficial, o nó OSM mais próximo (qualquer tag de semáforo) e a classificação:

- **correspondência provável** (≤ 15 m): 482
- **possível divergência** (15–40 m): 140
- **sem correspondência** (> 40 m): 90

Colunas extras:

- `qtd_osm_ate_40m`: o OSM costuma mapear um nó por aproximação ou travessia, então um ponto CTTU pode ter vários nós por perto.
- `osm_ref_igual_id`, `osm_importado_base_prefeitura`.
- `dist_no_osm_mesmo_ref_m`: distância até o nó importado com o mesmo código. São 573 oficiais com mediana de 7,6 m, e 29 deles a mais de 40 m, o que indica que o ponto mudou entre 2014 e 2026 ou que o OSM foi editado.

Outros **139 nós OSM** não têm semáforo oficial a até 40 m. Eles entram como `classificacao = somente OSM`, com `osm_lat`/`osm_lon`, mas **não** vão para a base oficial. Podem ser semáforos que a CTTU não publica, que foram desativados ou que estão mapeados de forma errada.

## Uso no simulador

O script também grava `public/data/semaforos_cttu.json` (`id`, `lat`, `lon`, `cruzamento`, `tipo`, `nome`). No app, o botão **Importar semáforos da CTTU** (modo Semáforo) põe `signalSource: "cttu"` nas edições. A escolha é salva no autosave, nos cenários A/B e no arquivo de cenário. O worker então remonta a rede com `import_osm_signals` (`core/src/extsig.rs`). Se algum ponto cai na área do mapa, os semáforos do OSM são descartados e cada ponto CTTU é encaixado como descrito abaixo. **Voltar aos semáforos do OSM** desfaz a troca. Cada mapa novo começa com os do OSM.

- **cruzamento**: marca o nó de interseção mais próximo (até 50 m) e todos os outros a até 30 m, para cobrir a pista dupla;
- **travessia/meio de quadra**: marca um nó em cada pista paralela a até 30 m. Usa um vértice existente se houver um a até 6 m; senão cria um nó sintético com id `200<<40 + código·64 + j`, estável entre importações.

A posição usada é a oficial. Só no `255` vale a do recorte de deficientes visuais, porque a coordenada principal cai a 4 km do endereço. Nos dois recortes do Recife testados, 706 dos 712 pontos foram encaixados (1.325 a 1.328 nós). Sobram 6 por recorte, e a lista muda um pouco entre eles:

- `270`, `471` e `570` dividem o nó com outro controlador do mesmo grupo;
- `441` e `529` estão em vias que não existem no OSM;
- `488` e `571` estão em vias que não são de carro.

Fases e tempos continuam sendo o padrão sintético do simulador (eixos, 30/3/2 s). Cada nó marcado tem fases próprias. Os nós de um mesmo controlador CTTU ainda não são coordenados entre si.

## Limitações

- O formato `Via A × Via B` vem do texto oficial. A via secundária só é reconhecida por padrão textual. Referências do tipo "Rua X (Pista Leste)" ficam como via.
- A conferência com o OSM depende da qualidade dos nomes no OSM, e os pontos importados enviesam a comparação para cima.
- Não há sentido de circulação, aproximações nem posição de linha de retenção na fonte oficial. O ponto CTTU é um ponto por controlador, não por grupo focal.
- A data de atualização é a da publicação no portal.
