# Relatório — Marina Duarte (copy e CRO)

> Fontes: snapshot em `audit/snapshot/` (page.md, page.html, raw.json, privacidade.html, img/) e capturas em `audit/screens/` (base-desktop, base-mobile, demo-passo1–3). Coleta de 2026-11-24. O site ao vivo não precisou ser consultado: o snapshot cobriu todas as seções, o FAQ aberto e os três passos da demo. A concorrência foi pesquisada com WebSearch (3 buscas). O código-fonte da landing não está neste repositório, então as referências de "onde mudar" usam a seção ou o âncora (`#planos` etc.).
>
> Limitações registradas:
> - Não consegui confirmar se o `/register` pede cartão, porque não criei conta.
> - A imagem do mockup do CTA final aparece vazia em `base-desktop.png`. O HTML usa `loading="lazy"`, então pode ser só um artefato da captura (ver MAR-13).
> - Os dados de concorrentes vêm das listagens das lojas e de páginas públicas, sem teste dos apps.

## Primeira impressão (5 segundos)
Em 5 segundos eu entendo o que é (dose pediátrica pelo peso com a prescrição pronta), para quem é ("Para médicos e estudantes de medicina") e vejo um resultado real no card do aciclovir. A página é sóbria, sem cara de infoproduto, o que conta pontos com médico. O que me trava é o botão principal: ele me pede para pagar R$ 199,50 antes de eu saber se o app presta. O teste de 7 dias, que é o melhor argumento de entrada, aparece só como nota de rodapé ao lado. Eu continuaria rolando, mas iria direto em "Criar conta grátis".

## Nota geral: 7/10
A proposta de valor, a demo e a transparência de preço e cancelamento estão acima da média de healthtech. Perdem pontos o CTA principal, que empurra a assinatura em vez do teste, a prova social quase nula e objeções centrais que ficam sem resposta: fontes das doses, offline e "já uso o Whitebook".

## Mapa da página

| # | Seção (âncora) | Objetivo da seção | Cumpre? | Comentário |
|---|---|---|---|---|
| 1 | Header | Navegação e CTA persistente | Parcial | O CTA "Assinar" é a única ação de destaque. Falta a ação de baixo risco (teste grátis). |
| 2 | Hero | Teste dos 5 s: o que é, para quem, por que agora | Sim (com ressalva) | Headline concreta e card com números reais. A ressalva é o CTA de pagamento como primário e o preço antes de mostrar valor. |
| 3 | `#problema` | Criar identificação com a dor | Sim | "Na hora do sufoco..." e "a fila andando" são bons, com a linguagem do plantão. |
| 4 | `#catalogo` | Mostrar amplitude (o que tem dentro) | Parcial | Os números (+50/+120/+85) funcionam, mas os exemplos de escores são de adulto ("TFG-MDRD", "HOMA") num produto pediátrico. |
| 5 | `#demonstracao` | Provar o mecanismo (algoritmo → dose → prescrição) | Parcial | A demo interativa é o melhor ativo da página. O caso escolhido (neutropenia febril dentro de ITU) é raro, e a dose em mg/mg/kg do cefepime não aparece. |
| 6 | `#diferencial` | Diferenciar de calculadora/bulário | Parcial | O diferencial (preparo, gotas, mL/h) aparece, mas não é contrastado com a alternativa ("o que você usa hoje"). |
| 7 | `#para-quem` | Segmentar o público e mostrar fit | Sim | Três perfis claros. A linha "não pede nem guarda dados de pacientes" está bem colocada. |
| 8 | `#quem-esta-por-tras` | Autoridade e confiança | Parcial | A fundadora é sólida (CRM/RQE, IMIP, 14 anos), mas é a única prova. "A equipe" não tem nome nem fontes, e o bloco "Privacidade" está fora de lugar aqui. |
| 9 | `#planos` | Converter: escolha de plano | Parcial | Tabela clara. O "2 meses grátis" do anual sugere vantagem que não existe (mesmo preço/mês do rodízio), e os CTAs perdem o plano escolhido (todos vão para `/register`). |
| 10 | `#perguntas` | Remover objeções finais | Parcial | Boas respostas sobre cobrança e cancelamento. Faltam offline, fontes/atualização, diferença para outros apps e "precisa de cartão?". |
| 11 | CTA final | Fechamento | Parcial | Headline boa. O CTA repete o pagamento, e o mockup aparece vazio na captura. |
| 12 | Footer | Confiança institucional | Não | Sem Termos de Uso, CNPJ, contato ou links das lojas. "Opex Consultoria Ltda." não conversa com a marca. |

Sobre o tamanho: 11 blocos é adequado para uma assinatura de R$ 199,50 vendida a um público cético. O problema não é o comprimento, e sim a ordem: a prova (autoridade, números de uso) vem tarde e fraca, e as objeções ficam escondidas no acordeão. Sugestão de ordem: Hero → Problema → Demo → Diferencial (com contraste) → Catálogo → Para quem → Autoridade + fontes + prova social → Planos → FAQ → CTA final. Na prática, a demo sobe para antes do catálogo, porque o mecanismo vende mais que a lista.

## Achados

| ID | Seção | Severidade | Problema | Evidência | Sugestão concreta |
|----|-------|-----------|----------|-----------|-------------------|
| MAR-01 | Hero, Planos, CTA final, Header | Alta | O CTA primário pede pagamento imediato de tráfego frio, apesar de existir um teste de 7 dias que é o caminho natural. O próprio texto diz que o plano pago é "cobrado na hora da assinatura". A ação de baixo risco fica como botão secundário (contorno). | "Assinar o plano do rodízio" (botão escuro, hero e CTA final); "Criar conta grátis" (contorno); header "Assinar"; "Os planos pagos são cobrados na hora da assinatura" | Inverter a hierarquia no hero, no header e no CTA final. Primário: **"Testar 7 dias grátis"**. Secundário (link de texto): "Ver planos". Header: "Testar grátis". Manter "Assinar o plano do rodízio" como primário só dentro de `#planos`. |
| MAR-02 | Hero, CTAs | Alta | O texto do CTA não diz o próximo passo nem o risco: não informa se pede cartão, se cobra ao fim dos 7 dias ou o que acontece depois. A página só explica isso em `#planos` e no FAQ. | "Criar conta grátis" + "Toda conta nova tem 7 dias de acesso completo." (sem menção a cartão) | Microcopy sob o botão: "7 dias de acesso completo. Sem cartão. Depois, segue no plano gratuito." (confirmar com produto se de fato não pede cartão; se pedir, dizer "Cobrança só se você assinar"). Adicionar a pergunta "Preciso cadastrar cartão para testar?" no FAQ. |
| MAR-03 | `#quem-esta-por-tras`, geral | Alta | A prova social se resume à coordenadora. Não há número de usuários, depoimentos, nota nas lojas, instituições nem fontes bibliográficas. "Revisado pela equipe" não diz quem é a equipe. Para médico, a pergunta "de onde vem essa dose?" vem antes de "quem usa". | "Cada medicamento, bula, algoritmo e ferramenta é revisado pela equipe antes de ser publicado." Única pessoa citada: Dra. Karla. Nenhum selo de loja ou número de usuários. | (1) Bloco **"De onde vêm as doses"** com as referências usadas (ex.: diretrizes da SBP, bulas da Anvisa, compêndios pediátricos — listar as reais). (2) Nomear os revisores com CRM/RQE. (3) Mostrar "Última revisão do conteúdo: mês/ano". (4) Quando houver, nota nas lojas e número de contas, com dado real e data. Depoimentos de médicos **passam por compliance** (publicidade médica, Res. CFM 2.336/2023) antes de entrar. |
| MAR-04 | Planos (copy de preço) | Alta | Ancoragem enganosa sem intenção: rodízio e anual saem pelo mesmo preço/mês (R$ 199,50/6 = R$ 399,00/12 = R$ 33,25). Mesmo assim, "2 meses grátis" faz o anual parecer melhor, e "1 mês grátis" se confunde com o teste de 7 dias (o usuário pode achar que ganha 1 mês de uso antes de pagar). O FAQ precisa esclarecer: "já estão descontados no valor". | "1 mês grátis / R$ 199,50 a cada 6 meses"; "2 meses grátis / R$ 399,00 por ano"; hero: "Plano do rodízio: R$ 199,50 a cada 6 meses · 1 mês grátis" | Trocar o selo por equivalente mensal e desconto: **"R$ 33,25/mês · 6 meses pelo preço de 5"** e **"R$ 33,25/mês · 12 meses pelo preço de 10"**. Ou diferenciar de fato o anual (ex.: preço/mês menor) para justificar a ancoragem. **Revisão de compliance/CDC** para o uso de "grátis" em valor já descontado. |
| MAR-05 | Objeções (página toda) | Alta | Três objeções decisivas para plantonista não são respondidas: funciona offline (pronto-socorro com sinal ruim)? Como é a atualização (data, frequência, errata)? Por que trocar o app que já uso? | Não há ocorrência de "offline", "atualizado em" nem comparação com alternativas em page.md/page.html. FAQ "Em quais aparelhos funciona?" fala só de plataformas. | Novas perguntas no FAQ: "Funciona sem internet?", "Com que frequência o conteúdo é atualizado?" e "Já uso outro app. O que muda?" (resposta focada no fluxo algoritmo → dose → preparo, sem citar concorrente pelo nome — **compliance/CONAR** para publicidade comparativa). |
| MAR-06 | Planos → `/register` | Alta | Todos os botões de plano levam ao mesmo `/register` sem parâmetro, então a escolha feita na tabela se perde. Quem clicou "Assinar o anual" cai num cadastro genérico. Isso quebra a continuidade e impede medir a conversão por plano. | Tabela de links: "Assinar o mensal / Assinar o anual (planos, 2x cada) → /register"; "Todos os CTAs de plano vão para o mesmo /register (sem parâmetro de plano)" | Passar `?plano=mensal|rodizio|anual|gratis` e `utm_content=<posição do CTA>`. Na tela de cadastro, repetir o plano e o preço escolhidos. |
| MAR-07 | Footer | Alta | Falta base institucional mínima para uma assinatura recorrente: link de Termos de Uso, CNPJ (está só em /privacidade), canal de contato e links/selos das lojas. Além disso, a razão social "Opex Consultoria Ltda." não tem relação aparente com o produto. | Footer: "Operado por Opex Consultoria Ltda." + links "Privacidade / Entrar / Criar conta". CNPJ 53.858.521/0001-40 só em privacidade.html. "Disponível em iPhone, Android e web." sem links. | Footer: "MedLogic Pro é operado por Opex Consultoria Ltda. · CNPJ 53.858.521/0001-40 · contato@… · Termos de Uso · Privacidade". Adicionar os selos App Store / Google Play com link no hero e no CTA final. **Compliance/jurídico**: Termos com regras de renovação e cancelamento. |
| MAR-08 | Demo `#demonstracao` | Média | O caso da demo é pouco representativo e não mostra o cálculo: "Imunossupressão + neutropenia febril" dentro de ITU, cuja conduta é "seguir protocolo de neutropenia febril". O passo 3 mostra a prescrição do cefepime sem a dose em mg nem em mg/kg, ao contrário do hero (aciclovir, "10 mg/kg/dose"). Para o médico cético, ver a dose de referência é o que gera confiança. | Caixa fixa: "Quadro: Imunossupressão + neutropenia febril · Cefepime FA de 1 g IV"; passo 3 mostra só "Reconstituir 1 FA + 10 mL de AD. Administrar 10 mL + 15 mL..." | Usar um caso de alta frequência no plantão (ex.: ITU em criança previamente hígida → cefalotina, ou anafilaxia → adrenalina, que já tem prints). No passo 3, mostrar "Dose de referência: 50 mg/kg/dose · 1.000 mg/dose" acima da prescrição. **Validar com a persona clínica.** |
| MAR-09 | Hero | Média | O preço aparece no hero antes de qualquer prova de valor. Ao mesmo tempo, o maior diferencial (algoritmo do quadro ligado à dose) fica só na subheadline. A headline vende o "output", que concorrentes também prometem. | Bloco "Plano do rodízio: R$ 199,50 a cada 6 meses / 1 mês grátis" logo abaixo dos CTAs | Trocar o bloco de preço do hero por um ponto de confiança ("Conteúdo coordenado por pediatra · CRM-PE 18734 RQE 13315 · Revisado em <mês/ano>") e deixar o preço em `#planos`. Ver as headlines alternativas em "Reescritas". |
| MAR-10 | `#catalogo` | Média | Os exemplos de escores são de adulto num produto que se posiciona como pediátrico. "TFG-MDRD" é fórmula de adulto. Para um pediatra, isso é sinal de conteúdo genérico. | "+85 como Clearance de Creatinina, TFG-MDRD e HOMA" | Trocar por exemplos pediátricos que existam no app (ex.: "TFG de Schwartz, Glasgow pediátrico, escore de desconforto respiratório"), se disponíveis. Se não, "inclui ferramentas de uso geral, como Clearance de Creatinina". **Confirmar a lista com o clínico.** |
| MAR-11 | Planos / nome do plano | Média | O nome "Plano do rodízio" fala com o interno, mas não com o residente nem com o plantonista de anos, dois dos três perfis de `#para-quem`. Para eles, "rodízio" não significa nada, e o CTA principal da página inteira carrega esse nome. | "Assinar o plano do rodízio" (4 ocorrências); `#para-quem` lista "Começando a dar plantão", "No internato" e "Anos de plantão" | Renomear para **"Semestral"** com o subtítulo "ideal para o rodízio de pediatria". Assim o nome fica autoexplicativo e o gancho do internato continua. |
| MAR-12 | `#quem-esta-por-tras` | Média | O bloco "Privacidade — Seus dados ficam no Brasil, conforme a LGPD" está dentro da seção da coordenadora, fora de contexto. Ele dilui a autoridade e esconde um argumento útil. | Seção 8: "Privacidade / Seus dados ficam no Brasil, conforme a LGPD." | Mover para `#para-quem`, junto de "O app não pede nem guarda dados de pacientes", ou para o FAQ "Preciso informar dados do paciente?". **Compliance**: confirmar se "ficam no Brasil" é verdadeiro para toda a infraestrutura (Vercel e lojas). |
| MAR-13 | CTA final | Média | Na captura, o mockup do celular ao lado do CTA final aparece vazio. Se isso ocorrer de fato para o usuário (lazy load não disparando), o fechamento perde o visual. O texto do CTA também repete o pagamento. | `base-desktop.png`, seção final: celular em branco; HTML do `4-prescricao-copiada` com `loading="lazy"` | Verificar no navegador real. Se confirmar, usar `priority`/eager nessa imagem. CTA final: "Testar 7 dias grátis" (primário) + "Ver planos" (secundário). |
| MAR-14 | Microcopy / consistência | Baixa | Há inconsistências pequenas. O domínio citado no FAQ ("medlogicpro.com.br") difere do domínio auditado (vercel.app). "+50/+120" com "+" à esquerda soa a promo. O título "Escolha por quanto tempo você quer o MedLogic Pro no bolso" fala de duração, não de valor. | FAQ "Em quais aparelhos funciona?"; "+50 algoritmos", "+120 bulas"; headline de `#planos` | Publicar no domínio próprio antes do lançamento. Usar "Mais de 50 algoritmos". Ver a nova headline de preços em "Reescritas". Não há placeholders nem erros de português relevantes. |
| MAR-15 | Comunicação vs. concorrência | Baixa | A página não posiciona o produto contra a alternativa real. O médico chega comparando com Afya Whitebook (que tem seção pediátrica com doses e calculadoras), MEDCode (calculadora em gotas/mL, também com 7 dias grátis), PediaTools e calculadoras web gratuitas (Sanar). O diferencial existe (algoritmo → prescrição → preparo num toque, só pediatria), mas não é dito como contraste. | `#diferencial` lista recursos sem referência ao "como você faz hoje"; `#problema` cita "PDF da residência... tabela... bula em outro app" | Fechar `#diferencial` com a linha: "Calculadoras dão a dose. Bulários dão a bula. Aqui o algoritmo do quadro termina na prescrição com o preparo — sem trocar de app." Sem nomear marcas (**compliance/CONAR**). |

## Mapa de objeções

| Objeção | Onde é respondida hoje | Como melhorar |
|---|---|---|
| 1. Confiabilidade ("posso confiar nessa dose? de onde vem?") | FAQ "Posso confiar na dose calculada?"; seção da coordenadora; "revisado pela equipe" | Bloco "De onde vêm as doses" com referências reais, revisores nomeados e data da última revisão. Mostrar a dose de referência (mg/kg) também na demo (MAR-03, MAR-08). |
| 2. Preço ("R$ 199,50 vale?") | `#planos`; hero mostra preço | Equivalente mensal (R$ 33,25/mês) e comparação com a alternativa ("menos que um plantão de 1 hora"; validar o tom). Sair do hero (MAR-04, MAR-09). |
| 3. Offline (sinal ruim no PS) | **Não respondida** | FAQ "Funciona sem internet?" com resposta honesta. Se funcionar, levar para o hero como bullet. |
| 4. Atualização | Parcial: "Todo conteúdo novo entra na sua assinatura, sem custo extra." | Informar a cadência ("revisão trimestral"), data visível e canal para reportar erro. |
| 5. Responsabilidade legal | Parcial: faixa "Ferramenta de apoio... Não substitui a prescrição médica."; FAQ "A dose final é sempre decisão do médico." | Manter. Acrescentar "Termos de Uso" no footer e explicar o papel do app (apoio à conferência). **Compliance** deve validar se "copie a prescrição pronta" pede contrapeso ("para conferir e assinar"). |
| 6. Cancelamento | Bem respondida: `#planos`, FAQ "Como cancelo?", CTA final "Cancele quando quiser, no site ou no app" | Não mexer. Só acrescentar "sem multa" se for verdade. |
| 7. Curva de aprendizado | Demo interativa de 3 passos | Boa. Trocar por um caso mais comum (MAR-08) e indicar o tempo ("do algoritmo à prescrição em 3 toques"). |
| 8. "Já uso outro app" | **Não respondida**; só indireta em `#problema` | Linha de contraste em `#diferencial` (MAR-15) e pergunta no FAQ. |
| Extra: "Precisa de cartão para testar?" | **Não respondida** | Microcopy sob o CTA e item no FAQ (MAR-02). |
| Extra: "Quem está por trás da empresa?" | Só "Opex Consultoria Ltda." | CNPJ, contato e Termos no footer (MAR-07). |

## Reescritas

**Headline do hero (3 opções para teste)**
1. *Atual (controle):* "Coloque o peso da criança e copie a prescrição pronta." É forte e concreta, e passa no teste dos 5 s. Tem dois pontos fracos: não diz que parte do algoritmo do quadro (o diferencial) e "pronta" pode soar como dispensa de conferência (**compliance**).
2. "Do algoritmo do quadro à prescrição pediátrica pelo peso, com o preparo já calculado." Lidera pelo diferencial.
3. "Informe o peso. A prescrição sai com a dose, quanto aspirar e quanto diluir." Lidera pelo output tangível e mantém o tom direto.
4. "A conta da dose pediátrica, feita e mostrada para você conferir." Lidera pela segurança e conversa com o médico cético. Validar com **compliance**.

**Subheadline**
"Escolha o quadro, siga o fluxograma e toque no medicamento: a prescrição vem com dose por peso, intervalo, volume a aspirar e diluição — e a conta aparece para você conferir. Mais de 50 algoritmos pediátricos, em português, coordenados por pediatra."

**CTA principal (2 opções)**
- A: **"Testar 7 dias grátis"**, com a microcopy "Acesso completo. Sem cartão*. Depois, segue no plano gratuito." (*confirmar).
- B: **"Abrir meu primeiro algoritmo"**, com a microcopy "7 dias de acesso completo ao criar a conta." Comunica o próximo passo concreto.

**Headline da seção de preços**
"Comece com 7 dias completos. Depois, escolha o plano." Subtítulo: "Mensal R$ 39,90 · Semestral e Anual saem por R$ 33,25/mês. Cancele quando quiser, no site ou no app."

**Headline final**
"No próximo plantão de pediatria, a conta da dose já vem feita — e à vista para você conferir." Subtítulo: "Crie a conta e use tudo por 7 dias." CTA: "Testar 7 dias grátis" + link "Ver planos".

## O que está funcionando (não mexer)
- O card do aciclovir no hero, com números reais (200 mg/dose, 8 mL, 32 mL, 40 mL), mostra o produto em vez de prometer.
- A dor em `#problema` tem a linguagem certa do plantão ("a criança na sua frente e a fila andando"), sem dramatização.
- A demo interativa em 3 passos: "É o algoritmo de verdade" é a melhor peça de convencimento da página.
- A transparência de cobrança, renovação e cancelamento em `#planos`, no FAQ e no CTA final. "Mesmo preço no site e no app" é um ótimo detalhe.
- O tom sóbrio, sem superlativos nem contagem regressiva, adequado a público médico e área regulada.
- O disclaimer visível no hero e no footer, e "O app não pede nem guarda dados de pacientes".
- A credencial completa da coordenadora (CRM + RQE + IMIP + tempo de emergência).

## Perguntas que ficaram sem resposta
- Preciso cadastrar cartão para os 7 dias? O que acontece no dia 8?
- Funciona sem internet?
- Quais são as referências das doses? Quem é "a equipe" que revisa? Quando foi a última revisão?
- Se eu encontrar um erro de dose, como reporto e em quanto tempo corrigem?
- Por que o anual não é mais barato por mês que o semestral?
- Onde baixo o app (links das lojas)? Qual é o site oficial, vercel.app ou medlogicpro.com.br?
- Quantos médicos usam? Qual a nota nas lojas?
- Quem é a Opex Consultoria e como falo com a empresa?

## Hipóteses de teste A/B (priorizadas)

| # | Hipótese | Variação | Métrica primária | Impacto esperado |
|---|---|---|---|---|
| 1 | Trocar o primário de "Assinar o plano do rodízio" para "Testar 7 dias grátis" aumenta as contas criadas sem reduzir a receita em 30 dias. | Hero + header + CTA final | Cadastros/visitante; secundária: assinaturas pagas em D+30 | Alto: maior alavanca da página |
| 2 | Microcopy "Sem cartão" (se verdadeiro) sob o CTA reduz a ansiedade e aumenta o clique. | Com vs. sem microcopy | CTR do CTA principal | Médio-alto |
| 3 | Bloco "De onde vêm as doses" (referências + revisores + data) antes de `#planos` aumenta a conversão do teste em pagamento. | Com vs. sem bloco | Conversão teste→pago; tempo na página | Médio-alto |
| 4 | Mostrar o equivalente mensal (R$ 33,25/mês) em vez de "1 mês grátis/2 meses grátis" aumenta a escolha de planos longos e reduz a confusão com o teste. | Selo de preço em `#planos` | Mix de planos; tickets de suporte sobre cobrança | Médio (**compliance**) |
| 5 | Headline com o diferencial ("Do algoritmo do quadro à prescrição...") supera a atual em rolagem até a demo. | Headline 2 vs. controle | Scroll até `#demonstracao`; CTR do hero | Médio |
| 6 | Demo com caso de alta frequência e dose em mg/kg visível aumenta o clique em "Calcular para o meu paciente". | Caso ITU hígido/anafilaxia vs. neutropenia febril | CTR do CTA da demo | Médio |
| 7 | Remover o preço do hero (trocando pelo selo de revisão clínica) não reduz cadastros e aumenta a rolagem. | Com vs. sem bloco de preço | Cadastros; scroll até `#planos` | Baixo-médio |

## Concorrência (WebSearch, 3 buscas)
- **Afya Whitebook (antigo PebMed):** o líder em conteúdo médico generalista, com "mais de 150 mil usuários". Tem seção pediátrica com doses por peso/idade, calculadoras de infusão contínua e escores. Na App Store aparecem planos de R$ 69,90 (periodicidade não clara). Vantagem dele: marca e prova social. O MedLogic ganha em foco e fluxo: algoritmo → prescrição → preparo, só pediatria, mais barato por mês. A página precisa dizer isso.
- **MEDCode:** "conduta e bula", calculadoras pediátricas em gotas/mL e versão gratuita de 7 dias, ou seja, a mesma oferta de entrada. O teste de 7 dias não é diferencial, então não deve ser o argumento central (apenas o caminho de entrada).
- **PediaTools (R$ 14,90), Pedi STAT EMS, calculadoras web gratuitas (Sanar, Omni):** resolvem a conta isolada. Daí o risco de o médico achar que "calculadora existe de graça". A comunicação do MedLogic deve evitar se apresentar como "calculadora" e insistir no algoritmo ligado à prescrição com preparo.
- **Conclusão:** na concorrência encontrada, nenhum produto comunica de forma explícita "algoritmo do quadro → dose → volume a aspirar/diluente → prescrição copiável" em pediatria. O diferencial é real e ainda subcomunicado (MAR-15). Não nomear concorrentes na página (**compliance/CONAR**).

Fontes:
- [Afya Whitebook — App Store](https://apps.apple.com/BR/app/id638521744)
- [Nova seção de Pediatria do Whitebook — Afya](https://portal.afya.com.br/saude/conheca-tudo-da-nova-sessao-de-pediatria-do-whitebook)
- [Whitebook: mais de 150 mil usuários — Afya](https://portal.afya.com.br/saude/whitebook-mais-de-150-mil-usuarios-e-crescendo)
- [MEDCode — App Store](https://apps.apple.com/br/app/medcode-conduta-e-bula/id1296653822)
- [PediaTools — App Store](https://apps.apple.com/br/app/pediatools/id1661526326?l=en-GB)
- [Pedi STAT EMS — App Store](https://apps.apple.com/br/app/pedi-stat-ems/id1595805495)
- [Cálculo de Dose — Sanar](https://sanarmed.com/ferramentas-medicas/calculo-de-dose/)

## Itens que precisam passar pela persona de compliance
MAR-03 (depoimentos e publicidade médica), MAR-04 ("grátis" em valor já descontado), MAR-05 e MAR-15 (publicidade comparativa), MAR-07 (Termos/renovação), MAR-12 ("dados ficam no Brasil"), headline atual e alternativa 4 ("prescrição pronta" / "conferir"), hipótese de teste 4.

## Veredito
Criaria conta grátis, não assinaria de cara. O produto me convenceu na demo, mas antes de pagar R$ 199,50 eu precisaria saber de onde vêm as doses, se funciona offline e se o teste pede cartão, e a página ainda não responde nenhuma das três.
