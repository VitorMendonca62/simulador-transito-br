# Fluxos OD de carro, Recife/RMR: metadados e limitações

Gerado por `scripts/od_recife/gerar_od.py`. Para reproduzir:

```bash
pip install -r scripts/od_recife/requirements.txt
python scripts/od_recife/gerar_od.py
```

Os brutos vão para `data/od_recife/_brutos/` (fora do git). Contagens, totais e os 15 maiores fluxos da última execução ficam em `estatisticas_execucao.md`.

**Todos os números são de 2018.** Não existe base OD de 2021 com origem e destino publicada (ver seção 1).

---

## 1. Inventário das fontes

| Arquivo | Onde | Data | O que contém | Usado? |
|---|---|---|---|---|
| `pesquisa-origem-destino-metropolitana-2018.csv` (170 MB) | [Portal de Dados Abertos](https://dados.recife.pe.gov.br/dataset/e2213899-a081-49e5-8b05-7b27e856847d/resource/0ceadbca-6581-4b1f-8269-d5e94fd92a17/download/pesquisa-origem-destino-metropolitana-2018.csv) | coleta out/2017–2019; publicado em 06/03/2026 | **Microdados**: 167.308 linhas (115.160 respondentes + 52.148 filhos), com `ZONA_RES`, `ZONA_TRAB`, `ZONA_EDU`, modo, horários e `EXPANSAO` | **Sim, é a fonte principal** |
| `zonas-de-trafego-pesquisa-od-metropolitana-2018.zip` (na verdade um RAR) | [Portal](https://dados.recife.pe.gov.br/dataset/e2213899-a081-49e5-8b05-7b27e856847d/resource/e0aad253-82f0-4241-8d7f-6abd3e3ff011/download/zonas-de-trafego-pesquisa-od-metropolitana-2018.zip) | 2017/2018 | Shapefile com as **253 zonas** (`CODIGOZONA`, `MUNICIPIO`, EPSG:31985) e um PDF de metodologia | Sim |
| `zonas-de-trafego-...geojson` | Portal | — | Só as 108 zonas do Recife, sem município | Não (o shapefile está completo) |
| `metadados-legenda-das-respostas.pdf` | [Portal](https://dados.recife.pe.gov.br/dataset/e2213899-a081-49e5-8b05-7b27e856847d/resource/f63f5a6d-a285-45aa-8bda-34490f30cbbc/download/metadados-legenda-das-respostas.pdf) | — | Legenda de sexo, renda, faixa etária e respostas sim/não. Não cobre modo nem zona | Consulta |
| `sobre-a-pesquisa-origem-destino.pdf` | [Portal](https://dados.recife.pe.gov.br/dataset/e2213899-a081-49e5-8b05-7b27e856847d/resource/4ed00f48-ceec-4a88-a921-7206831cdc43/download/sobre-a-pesquisa-origem-destino.pdf) | — | Metodologia: pesquisa online feita no destino (empresas, matrícula escolar), expansão pela população | Consulta |
| `bairros-do-recife.geojson` | [Portal, área urbana](https://dados.recife.pe.gov.br/dataset/1702c764-ec7a-4ef0-9861-8acd0e40c819/resource/5c67ce14-1799-40c4-a37c-9daa04d1761c/download/bairros-do-recife.geojson) | 2019 | 94 bairros oficiais (`EBAIRRNOMEOF`) | Sim |
| Matrizes OD 2018, Carro, Trabalho/Educação, por zona (`.xlsx`) | [Pasta da CTTU no Drive](https://drive.google.com/drive/folders/1nxKGgadvO5iO9rCzUbhIBGyLqEpISXiA) | 20/10/2020 | Matriz residência → destino, soma da expansão, **Carro = dirigindo + passageiro**, só a ida | **Só para validar** (seção 7) |
| Matriz OD 2018 por bairro, Trabalho, Carro | mesma pasta, subpasta "Por bairro" | 20/10/2020 | Idem, por bairro, a partir do endereço declarado | Não usada; fica como alternativa oficial por bairro |
| Matrizes da página do [ICPS](https://icps.recife.pe.gov.br/matrizes-origem-destino/) | ICPS | jun/2020 | Trabalho e Educação × {todos, público, particular}. "Particular" mistura carro, moto, táxi, Uber, a pé e bicicleta | Não: os botões "Clique Aqui" estão sem link e o recorte não isola carro |
| `BANCO DE DADOS OD 2018 - CODIFICADO - Nov-2020.xlsx` | [CTTU, link "Pesquisa OD Metropolitana 2018"](https://docs.google.com/uc?export=download&id=1ieLmzB9TBOyqijw0myGj_YZJX2QzCVJd) | nov/2020 | Mesmos microdados com respostas codificadas, **sem fator de expansão e sem zona de educação**, mais dicionário de códigos e população por zona | Não: não serve para expandir |
| Relatório Final OD Recife 2021 (PDF, 18 MB) | [CTTU](https://cttu.recife.pe.gov.br/sites/default/files/2025-12/Relat%C3%B3rio%20Final%20-%20Pesquisa%20Origem%20Destino%20Recife%20%28OD%202021%29.pdf) | atualizado em dez/2024 | Só gráficos e percentuais por modo/motivo/horário. **Não tem matriz OD nem microdados** | Contexto (seção 8) |
| Zonas de Tráfego OD 2021 (RAR → KML) | [CTTU](https://docs.google.com/uc?export=download&id=1KBOAx_MuRJy9Sd2iNOwMaOoFGAYE6w0Y) | 2021 | 256 polígonos e centroides, sem atributos de demanda | Não (não há dados de 2021 para associar) |

**O que tem matriz ou microdados de fato:** só o CSV de microdados de 2018 do Portal. As matrizes xlsx de 2020 também são matrizes reais, mas só cobrem a ida, juntam motorista e passageiro e usam uma versão anterior da expansão e do zoneamento.

## 2. Como origem e destino estão representados

- **Unidade espacial:** zona de tráfego (1–253), em `ZONA_RES`, `ZONA_TRAB` e `ZONA_EDU`. `X`/`x` quer dizer "não geocodificado" e a linha é descartada para aquele motivo. Não há coordenadas. Há bairro e CEP declarados em texto livre, mas eles não foram usados.
- **Cobertura:** 15 municípios da RMR. O Recife tem 108 zonas.
- **Formato:** uma linha por pessoa, com uma viagem habitual por motivo. Não é diário de viagens.
- **Motivos com zona de destino:** trabalho (principal), educação (do respondente) e escola dos filhos (linhas `-FILHO`).
- **Motivos sem zona de destino:** outro trabalho, compras, consulta médica e serviços. Há modo, mas não destino, então **"outros motivos" não entra na matriz**. Para escala, as pessoas que vão de carro dirigindo nesses motivos estão em `estatisticas_execucao.md` (compras ≈ 91 mil, serviços ≈ 34 mil, consulta ≈ 26 mil, outro trabalho ≈ 16 mil).

## 3. Modos que dá para separar

O campo de modo principal permite separar estes modos (todos estão nos arquivos):

| `modo` | respostas originais |
|---|---|
| `carro_dirigindo` | Carro (dirigindo) |
| `carro_passageiro` | Carro (como passageiro). Para filhos, "dirigindo" também cai aqui (ver seção 5) |
| `taxi` | Táxi |
| `aplicativo` | Uber |
| `motocicleta` | Motocicleta (não separa piloto de garupa) |
| `mototaxi` | Mototáxi |
| `transporte_publico` | só ônibus (1 ou 2+ linhas), ônibus+metrô, só metrô, bicicleta+ônibus(+metrô), VLT, barco |
| `a_pe`, `bicicleta`, `fretado`, `transporte_escolar`, `caminhao`, `outro` | respectivos |

`fluxos_carro_origem_destino.csv` traz os 6 modos individuais motorizados. Os totais de **todos os modos**, inclusive transporte público, estão em `totais_por_modo_motivo.csv`.

## 4. O que os números representam

- **`fluxo_pessoas`** = soma de `EXPANSAO`. É uma **estimativa expandida** do número de pessoas da população que fazem aquele deslocamento habitualmente. Não é contagem observada. A soma de `EXPANSAO` dá 4.069.376, quase igual à população estimada da RMR em 2019 (4.079.575). Cada linha representa, portanto, X habitantes.
- **Pessoas, viagens e veículos.** Cada pessoa gera uma perna de ida e, em geral, uma de volta (seção 5). Então `dia_util` = pessoas-viagem num dia útil típico (ida + volta). Os picos contam pessoas cuja perna parte dentro da janela.
- **`fluxo_veiculos_estimado`** só é preenchido para `carro_dirigindo`, e é igual a `fluxo_pessoas`. **Premissa: um condutor = um veículo.** Isso decorre da definição do modo e não é uma taxa de ocupação: cada pessoa que dirige é um carro na via, e os passageiros (`carro_passageiro`) vão dentro de carros que já foram contados ou cujo motorista não está na amostra. A pesquisa **não pergunta a ocupação do veículo**, por isso fica **nulo** para passageiro, táxi, aplicativo, moto e mototáxi. Se o simulador quiser converter esses modos, terá de declarar cenários próprios (por exemplo: táxi/app = 1 veículo por viagem, ocupação 1,0–1,5), sempre rotulados como hipótese.
- **`fluxo_pessoas_ajustado_frequencia`** = Σ `EXPANSAO` × min(vezes por semana, 5) / 5. É a média esperada num dia útil, considerando quem vai só 2 ou 3 vezes por semana. Os filhos não têm frequência e entram com fator 1.
- **`n_respostas`** = número de respostas por trás do fluxo. **Fluxos com menos de ~10 respostas são instáveis**: uma única resposta com fator 733 vira "733 pessoas".

## 5. Filtros e regras aplicadas

1. Descartadas 1.987 linhas com `EXPANSAO` = `x`.
2. A zona de origem e a de destino precisam existir no shapefile (1–253).
3. Modo normalizado: maiúsculas, sem acento. As 20 respostas distintas foram todas mapeadas.
4. **Origem da ida** vem de "De onde você sai": residência → `ZONA_RES`; escola → `ZONA_EDU` (para trabalho); trabalho → `ZONA_TRAB` (para educação). Respostas como "outro", estágio ou curso, sem zona, usam `ZONA_RES` como aproximação (1,7% das pernas de carro dirigindo).
5. **Volta** = destino → `ZONA_RES`, partindo no horário de término da atividade. **É uma premissa:** a pesquisa não pergunta para onde a pessoa vai depois. Quem encadeia trabalho → escola (ou o contrário) não ganha volta da primeira atividade, porque a ida da segunda substitui essa volta.
6. **Horário de partida:**
   - ida = início da atividade − ponto médio da classe de duração (por exemplo, "entre 30 e 45 min" = 37,5 min);
   - volta = término da atividade;
   - 00:00 e horas acima de 23 são tratadas como não respondidas.
   99,4% das pernas de carro dirigindo têm horário.
7. **Períodos** (pela hora de partida da perna):

   | período | janela |
   |---|---|
   | `pico_manha` | 06:00–08:59 |
   | `pico_tarde` | 17:00–19:59 |
   | `dia_util` | todas as pernas, inclusive as sem horário |

   Carro dirigindo: 37% das pessoas-viagem no pico da manhã e 33% no da tarde. A CTTU 2021 aponta 7h–8h e 17h–18h como as horas de maior saída de carro. Para recortar outros horários, use `perfil_horario_carro.csv`.
8. **Motivos:**

   | motivo | o que inclui |
   |---|---|
   | `trabalho` | trabalho principal |
   | `educacao` | estudo do respondente |
   | `educacao_dependente` | filhos indo à escola, residência ↔ escola, sem horário (por isso ficam só em `dia_util`) |
   | `todos_motivos_disponiveis` | soma dos três |

   **O motivo "outros" não existe na matriz** (ver seção 2).
9. **Filhos:** criança não dirige. A resposta "carro (dirigindo)" para o filho foi lida como "o responsável dirige com a criança" e classificada como `carro_passageiro`. O veículo pode já estar contado na viagem de trabalho do responsável, ou não estar (outro adulto, ou responsável fora da amostra). Por isso `fluxo_veiculos_estimado` fica nulo.

## 6. Zonas de tráfego → bairros

As zonas **não são bairros**: há zonas que cobrem vários bairros (a 124 pega Graças, Derby, Espinheiro…) e bairros divididos em várias zonas (Boa Viagem tem 71, 72, 74, 77 e outras).

- Foi feita interseção geométrica em EPSG:31985 entre as 253 zonas e os 94 bairros. O resultado está em `zona_bairro_correspondencia.csv`, com o percentual da zona dentro do bairro, o percentual do bairro dentro da zona e o bairro principal.
- Lascas abaixo de 0,5% da área da zona foram descartadas (limites desalinhados entre as duas camadas).
- **`*_bairro_estimado` nos fluxos e resumos por zona:** bairro de **maior sobreposição de área**. O código da zona original é mantido sempre ao lado.
- **Arquivos `*_bairro.csv`:** **divisão proporcional por área**, fluxo(bairro_o, bairro_d) = Σ fluxo(z_o, z_d) · área(z_o∩bairro_o)/área(z_o) · área(z_d∩bairro_d)/área(z_d). Nas zonas do Recife os pesos são renormalizados entre os bairros. Nas de outros municípios, a parte fora do Recife vira "MUNICÍPIO (fora do Recife)".
  - A divisão por área supõe que a demanda está espalhada de forma uniforme dentro da zona, o que não acontece em zonas com parques, rios ou áreas industriais.
  - A planilha da CTTU traz população por setor censitário e zona (2010). Com ela daria para fazer a divisão por população, o que **não foi feito aqui**.
- O bairro chamado "Recife" é o Bairro do Recife (Recife Antigo), não o município.

## 7. Validação contra as matrizes oficiais de 2020

Comparação da ida residência → destino, carro dirigindo + passageiro, usando o mesmo recorte das matrizes da CTTU (`validacao_matriz_oficial.csv`):

| motivo | pessoas (oficial 2020) | pessoas (este CSV) | corr. por par OD | corr. total por origem | corr. total por destino |
|---|---:|---:|---:|---:|---:|
| trabalho | 193.782 | 180.140 | 0,86 | 0,99 | 0,97 |
| educação | 139.400 | 95.110 | 0,85 | 0,97 | 0,94 |

No trabalho, os dados batem bem: 7% abaixo e mesma distribuição espacial. Na educação, o CSV atual tem 32% menos, porque mais registros estão com `ZONA_EDU` não geocodificada (`x`). **A educação deste CSV está provavelmente subestimada.** Para o motivo educação, vale considerar a matriz oficial ou aplicar um fator de correção declarado. As diferenças vêm de versões distintas da expansão e do zoneamento: as matrizes são de out/2020 e o CSV foi republicado em 2026.

## 8. Limitações

- **Ano:** 2018 é o dado OD mais recente com origem e destino. A OD Recife 2021 (CTTU) só publicou relatório agregado, sem matriz nem microdados.
- **Pandemia:** a OD 2021 mostra que entre usuários de carro o trabalho híbrido passou de 2% para 11%. A demanda de 2018 pode estar acima da atual nos dias de home office.
- **Método:** pesquisa online feita no destino (empresas grandes, matrícula escolar, ações de campo). A expansão é por zona de residência e faixa de renda (Censo 2010 projetado). Viagens fora desses canais ficam sub-representadas: autônomos, informais, lazer, compras.
- **Viagem habitual, não diário:** cada pessoa declara uma viagem por motivo. Paradas intermediárias e viagens não habituais não aparecem.
- **Sem motivos "outros" com destino:** a matriz é só trabalho + educação. Os motivos sem destino somam ≈ 168 mil pessoas de carro dirigindo, contra 151 mil indo ao trabalho.
- **Volta à residência** é premissa. Os horários dos picos são estimados por classe de duração.
- **Fluxos intrazonais** (origem = destino) são grandes no topo do ranking. Num simulador de rede, a maior parte deles não usa vias arteriais.
- **Pares com poucas respostas** têm variância alta: filtre por `n_respostas`. Exemplo: zona 2 (Itamaracá) ↔ 78 (Ibura), 733 pessoas com **1 resposta**.
- **Lotes suspeitos:** 63 das 71 respostas de carro dirigindo da zona 87 (Areias) para a 69 (Imbiribeira) dizem "início 9h, fim 6h". Parece um lote de uma empresa (turno noturno 21h–6h digitado como 9h?). Com o dado como está, a ida cai no pico da manhã e a volta às 6h também. Nada foi corrigido: o par 87↔69 aparece no topo do ranking por causa desse lote.
- **Veículos** só para carro dirigindo. Os demais modos ficam nulos de propósito.

## 9. Arquivos de saída e colunas

### `fluxos_carro_origem_destino.csv`

Formato longo, uma linha por (par OD, período, motivo, modo).

| coluna | definição |
|---|---|
| `origem_zona`, `destino_zona` | zona de tráfego 2018 (1–253) |
| `origem_bairro_estimado`, `destino_bairro_estimado` | bairro de maior sobreposição, ou "MUNICÍPIO (fora do Recife)" |
| `periodo` | `pico_manha`, `pico_tarde`, `dia_util` |
| `motivo` | `trabalho`, `educacao`, `educacao_dependente`, `todos_motivos_disponiveis` |
| `modo` | `carro_dirigindo`, `carro_passageiro`, `taxi`, `aplicativo`, `motocicleta`, `mototaxi` |
| `fluxo_pessoas` | pessoas-viagem (soma de `EXPANSAO`) |
| `fluxo_veiculos_estimado` | = `fluxo_pessoas` se `carro_dirigindo`; nulo nos demais |
| `fonte`, `ano` | OD Metropolitana 2018 (ICPS/Grande Recife); 2018 |
| `fluxo_pessoas_ajustado_frequencia` | idem, ponderado pela frequência semanal (seção 4) |
| `n_respostas` | respostas da amostra por trás do fluxo |

### `resumo_por_origem.csv` (zona) e `resumo_por_origem_bairro.csv` (bairro, divisão por área)

Carro dirigindo, trabalho + educação, um bloco por `periodo`.

| coluna | definição |
|---|---|
| `origem`, `origem_municipio`, `origem_bairro_estimado` | zona de origem e seus rótulos |
| `total_saidas_carro` | veículos que saem da origem no período (= pessoas que dirigem) |
| `destino_k`, `destino_k_bairro`, `fluxo_k` | os 10 maiores destinos (k = 1…10) |
| `percentual_dos_top_3`, `percentual_dos_top_10` | parcela do total que vai para os 3 e os 10 maiores destinos |
| `n_respostas` | respostas por trás do total (só no arquivo por zona) |

### Matrizes

- `matriz_od_carros.csv`: linhas = zona de origem, colunas = zona de destino, valor = veículos de carro dirigindo por dia útil, trabalho + educação.
- `_pico_manha` e `_pico_tarde`: mesmo formato, só o período.
- `matriz_od_carros_bairro.csv`: o mesmo por bairro, com divisão por área.

### Arquivos de apoio

- `perfil_horario_carro.csv`: pessoas de carro dirigindo por hora de partida, motivo e sentido. Serve de perfil horário no simulador.
- `totais_por_modo_motivo.csv`: totais de todos os modos.
- `zona_bairro_correspondencia.csv`: interseções zona × bairro.
- `validacao_matriz_oficial.csv`: tabela da seção 7.
- `estatisticas_execucao.md`: contagens e rankings da última execução.

## 10. Observado e estimado

- **Observado (resposta declarada):** zona de residência, de trabalho e de estudo (geocodificadas pela prefeitura a partir do endereço), modo, horários de início e fim, classe de duração e frequência.
- **Estimado pela prefeitura:** `EXPANSAO`, a partir da população por zona e renda.
- **Estimado neste trabalho:**
  - a perna de volta e seu destino;
  - o horário de partida da ida;
  - o bairro (sobreposição ou divisão por área);
  - os veículos (um condutor = um veículo);
  - o ajuste por frequência.
