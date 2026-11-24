# Relatório final — Auditoria da landing page do MedLogic Pro

- **URL auditada:** https://medlogic-three.vercel.app
- **Versão analisada:** snapshot de 24/11/2026, ~17h10 (BRT). Ver `snapshot/README.md`.
- **Personas:** 8 (Renata, Lucas, Júlia, Marina, Caio, Patrícia, Roberto, Bruno). 118 achados individuais → 32 achados consolidados.
- **Escopo:** só a landing (home, `/register`, `/login`, `/privacidade`). O app logado e o checkout não foram testados (o checkout ainda não existe).

---

## 1. Resumo executivo

A página é bem escrita, acessível (axe e Lighthouse sem violações) e tecnicamente sólida (SSR, CLS 0). A demo interativa é o ponto mais elogiado por todas as personas. As contas de dose mostradas fecham aritmeticamente (aciclovir, cefepime, adrenalina). **Ainda não está pronta para lançar.** Faltam peças estruturais, e não só ajustes de texto. O próprio HTML de produção traz 11 pendências internas ("[CONFIRMAR]"/"[NEED]"), ocultas por CSS. Uma delas confirma que os botões "Assinar" ainda não levam a nenhum checkout.

Os 3 maiores riscos:
1. **Segurança do paciente.** A "prescrição pronta" que se copia sai com a dose só em mL, sem nome do fármaco, dose em mg e concentração. Isso inclui a adrenalina, que é medicamento potencialmente perigoso. Além disso, nenhuma fonte, data de revisão ou canal de erro aparece na página.
2. **Regulatório.** "Copie a prescrição pronta" e "Calcular para o meu paciente" tendem a enquadrar o app como software dispositivo médico (ANVISA). A credencial da médica aparece colada ao preço. Faltam Termos de Uso, identificação completa do fornecedor e direito de arrependimento.
3. **Funil quebrado e sem medição.** O CTA principal pede pagamento, mas leva a um cadastro genérico que perde o plano escolhido e não tem checkout. Não há analytics, então não seria possível saber o que converte.

## 2. Placar das personas

| Persona | Lente | Nota | Veredito |
|---|---|---|---|
| Dra. Renata (plantonista sênior) | Credibilidade clínica | 5/10 | Conta grátis, para testar uma droga que já sabe de cabeça. Não assinaria nem recomendaria a residentes sem fontes, data de revisão e prescrição completa |
| Lucas (R1) | Conversão, mobile | 7,5/10 | Conta grátis. Não assina sem saber se funciona offline, se o teste pede cartão e de onde vêm as doses |
| Júlia (interna) | Preço, estudantes | 6/10 | Conta grátis e indicaria a demo à turma. Não assinaria o plano do rodízio (2 meses de Mensal custam menos) |
| Marina (copy/CRO) | Copy, funil | 7/10 | Conta grátis. Não assinaria de cara: faltam fontes, a resposta sobre offline e "sem cartão" |
| Caio (UX) | Visual, acessibilidade | 7/10 | Conta grátis. Corrigiria a demo mobile e o hero antes de divulgar |
| Patrícia (farmacêutica) | Segurança de dose | 4/10 | Conta grátis, só para auditar o banco de doses. **Não recomendaria** aos residentes da UTI |
| Roberto (compliance) | CFM, LGPD, CDC, ANVISA | 5/10 | **Não liberaria o lançamento** (≈ sairia). Bloqueios: ANVISA, credencial, Termos, fornecedor e arrependimento |
| Bruno (SEO/performance) | SEO, medição | 5/10 | Conta grátis. Não colocaria mídia paga sem medição, domínio definitivo e og:image |

**Média: 5,8/10.** Nenhuma persona assinaria hoje. Sete criariam conta grátis, e o compliance bloqueia.

## 3. Bloqueadores de lançamento

Todos os achados consolidados com severidade **Crítica** (detalhes na seção 4). Separados em dois grupos: os que são Críticos por natureza e os que viraram Críticos pela regra de recorrência (3 ou mais personas, sobe um nível).

### 3a. Críticos por natureza (segurança, legal, funil inexistente)
| # | Bloqueador | IDs de origem |
|---|---|---|
| C01 | Prescrição copiável só em volume (mL), sem nome do fármaco, dose em mg e concentração. Inclui adrenalina IM sem "1 mg/mL" | REN-02, REN-07, PAT-01, PAT-02, PAT-10, LUC-01 |
| C02 | Demo do cefepime sem dose em mg, mg/kg e concentração. "10 mL = 1 g" depende do deslocamento de volume do pó (não verificável) | PAT-04, REN-05, REN-10, LUC-01, JUL-09, MAR-08 |
| C03 | Aciclovir exibido sem indicação. 10 mg/kg/dose é cerca de metade do esquema de encefalite e imunodeprimidos | PAT-03, REN-06 |
| C04 | Sem referências bibliográficas, data de revisão, equipe nomeada nem canal para reportar erro de dose. A linha "Fontes" é um placeholder oculto | REN-01, REN-03, PAT-06, PAT-08, LUC-04, MAR-03, JUL-08 |
| C05 | Risco de enquadramento como SaMD (ANVISA RDC 657/2022). "Prescrição pronta" contradiz "não substitui a prescrição médica" e incentiva uso acrítico por iniciantes e estudantes | ROB-01, ROB-07, ROB-08, REN-08, PAT-07 |
| C06 | Credencial e foto da Dra. Karla entre o preço e o CTA, funcionando como endosso comercial (CFM 2.336/2023). A própria equipe deixou pendente "conferir com o jurídico" | ROB-02 |
| C07 | Sem Termos de Uso (/termos dá 404) e sem aceite no cadastro. Rodapé sem CNPJ, endereço e canal de atendimento (Decreto 7.962/2013) | ROB-03, ROB-04, MAR-07, REN-15, LUC-15, BRU-09, BRU-12 |
| C08 | Não há checkout. Todos os 13 CTAs vão para o mesmo `/register`, sem plano, preço ou trial, e perdem UTM e plano | MAR-06, BRU-02, LUC-03, LUC-08, JUL-11, ROB-06 + pendência no HTML |
| C09 | Nenhuma medição (analytics ou eventos de conversão) | BRU-01 |
| C10 | Cobertura invisível: "+50 algoritmos" sem lista, e o print mostra só Anafilaxia | REN-04, CAI-04 |

### 3b. Críticos por recorrência (Alta citada por 3 ou mais personas)
| # | Bloqueador | IDs de origem |
|---|---|---|
| C11 | CTA primário pede pagamento a tráfego frio, e o teste de 7 dias fica como secundário. Não diz se pede cartão | MAR-01, MAR-02, LUC-05, LUC-07, JUL-11, CAI-12 |
| C12 | "1 mês grátis" e "2 meses grátis" são descontos cobrados à vista. Rodízio e anual custam o mesmo por mês (R$ 33,25), e a página não mostra o equivalente mensal | MAR-04, JUL-05, LUC-06, ROB-14 |
| C13 | "Disponível em iPhone, Android e web" sem links para as lojas. "Cancele no app" e "mesmo preço no app" não valem para compras pela App Store ou Google Play | ROB-06, ROB-09, BRU-09, CAI-09, JUL-13, LUC-10, MAR-07 |
| C14 | Caso fixo da demo: ITU seguindo o ramo que sai do algoritmo (neutropenia febril), com dose de 8/8 h, quando na ITU a referência usual é 12/12 h. Critério "< 28 dias" de um lado e linha "< 2 meses" do outro | PAT-05, REN-11, MAR-08, LUC-13 |
| C15 | Gotas/min e mL/h prometidos sem nenhum exemplo, fator de gotejamento ou unidade | PAT-09, REN-13, CAI-10 |
| C16 | Demo mobile: o botão de ação fica fora da tela, a rolagem fica presa no mockup e o header cobre o topo | CAI-02, LUC-12, REN-14 |
| C17 | Domínio provisório (`*.vercel.app`) enquanto o FAQ cita `medlogicpro.com.br`. Sem canonical, robots.txt e sitemap.xml | BRU-03, BRU-05, JUL-07, MAR-14 |
| C18 | Link sem imagem de prévia no WhatsApp: falta `og:image` e o `twitter:card` é `summary` | BRU-04, JUL-06, LUC-14 |

> Observação do orquestrador: C17 e C18 só são Críticos pela regra de recorrência. Sem tráfego pago, eles não arriscam pacientes nem geram risco legal. São, porém, *quick wins* de menos de 1 dia.

### 3c. Placeholders
Nenhum placeholder aparece **na tela**. O HTML servido contém **11 marcações** (`data-pendencia`: 7× CONFIRMAR, 4× NEED), ocultas por CSS e legíveis no código-fonte:
- destino do checkout dos botões "Assinar" (3×);
- "a criação de conta pede cartão?" (2×);
- "credencial perto do botão: conferir com o jurídico pelas regras de publicidade médica do CFM";
- links das lojas (2×);
- "4 a 6 algoritmos reais em destaque";
- "prints da lista de algoritmos e da bula com a especialista";
- "referências que o conteúdo segue (SBP, AAP, Ministério da Saúde…)".

Elas não são bloqueadoras por estarem visíveis. O bloqueio está nas **decisões** que elas registram (C04, C06, C08, C10, C11, C13), que continuam em aberto. As personas divergiram na contagem (9, 10, 14); a contagem acima é a minha, feita direto no `page.html`.

## 4. Achados consolidados

Regra de severidade: parte da maior severidade entre os IDs de origem e sobe um nível (▲) quando 3 ou mais personas citam o mesmo problema.

| ID | Achado | Seção | Severidade | Personas | IDs de origem |
|---|---|---|---|---|---|
| C01 | Prescrição copiável só em mL, sem nome, mg e concentração; notação decimal mista (0.01/0,01) | Hero, prints, demo | Crítica | 3 | REN-02, REN-07, PAT-01, PAT-02, PAT-10, LUC-01 |
| C02 | Demo do cefepime sem mg, mg/kg e concentração; deslocamento de volume | #demonstracao | Crítica ▲ | 5 | PAT-04, REN-05, REN-10, LUC-01, JUL-09, MAR-08 |
| C03 | Aciclovir sem indicação, com dose possivelmente subterapêutica para encefalite | Hero | Crítica | 2 | PAT-03, REN-06 |
| C04 | Sem fontes, data de revisão, equipe ou canal de erro; FAQ "Posso confiar" insuficiente | #quem-esta-por-tras, FAQ | Crítica | 6 | REN-01, REN-03, PAT-06, PAT-08, LUC-04, MAR-03, JUL-08 |
| C05 | Enquadramento SaMD; "prescrição pronta" × disclaimer; uso acrítico por iniciantes | Hero, #para-quem, demo | Crítica | 3 | ROB-01, ROB-07, ROB-08, REN-08, PAT-07 |
| C06 | Credencial médica junto ao preço e CTA; "14 anos" × residência concluída em 2013 | Hero | Crítica | 1 | ROB-02 |
| C07 | Sem Termos, aceite, CNPJ, endereço ou contato | Rodapé, /register | Crítica | 5 | ROB-03, ROB-04, MAR-07, REN-15, LUC-15, BRU-09, BRU-12 |
| C08 | Sem checkout; CTAs genéricos perdem plano e UTM; "Calcular para o meu paciente" leva a cadastro | Todos os CTAs | Crítica ▲ | 5 | MAR-06, BRU-02, LUC-03, LUC-08, JUL-11, ROB-06 |
| C09 | Sem analytics nem eventos | Global | Crítica | 1 | BRU-01 |
| C10 | Cobertura invisível (+50 algoritmos sem lista; print só com Anafilaxia) | #catalogo | Crítica | 2 | REN-04, CAI-04 |
| C11 | CTA primário pago × trial; cartão não esclarecido | Hero, header, CTA final | Crítica ▲ | 4 | MAR-01, MAR-02, LUC-05, LUC-07, JUL-11, CAI-12 |
| C12 | "Meses grátis" enganosos; rodízio = anual por mês; sem preço por mês | #planos | Crítica ▲ | 4 | MAR-04, JUL-05, LUC-06, ROB-14 |
| C13 | Apps sem links de loja; promessas de cancelamento e preço no app | Hero, CTA final, FAQ | Crítica ▲ | 7 | ROB-06, ROB-09, BRU-09, CAI-09, JUL-13, LUC-10, MAR-07 |
| C14 | Caso da demo clinicamente incoerente (ITU → neutropenia febril; 8/8 h) | #demonstracao | Crítica ▲ | 4 | PAT-05, REN-11, MAR-08, LUC-13 |
| C15 | Gotas e mL/h sem exemplo, fator ou unidade | #diferencial | Crítica ▲ | 3 | PAT-09, REN-13, CAI-10 |
| C16 | Demo mobile com rolagem presa, ação fora da tela e header por cima | #demonstracao (mobile) | Crítica ▲ | 3 | CAI-02, LUC-12, REN-14 |
| C17 | Domínio provisório; sem canonical, robots e sitemap; /login e /register indexáveis | `<head>`, infra | Crítica ▲ | 4 | BRU-03, BRU-05, JUL-07, MAR-14 |
| C18 | Sem og:image | `<head>` | Crítica ▲ | 3 | BRU-04, JUL-06, LUC-14 |
| C19 | Direito de arrependimento ausente; sem aviso antes da renovação | #planos, FAQ | Alta | 2 | ROB-05, JUL-10 |
| C20 | Público inconsistente: landing fala em "estudantes", a política diz "profissionais adultos"; como comprovar | #para-quem, /privacidade | Alta ▲ | 3 | JUL-04, ROB-13, ROB-08 |
| C21 | "Plano do rodízio" não cabe no rodízio (~2 meses); sem desconto de estudante, indicação ou Pix | #planos | Alta | 2 | JUL-01, JUL-02, JUL-12, MAR-11 |
| C22 | Gratuito sem algoritmos; "1 medicamento" sem dizer qual | #planos | Alta | 2 | JUL-03, LUC-09 |
| C23 | Objeções sem resposta: offline, atualização, "já uso o Whitebook" | Página toda, FAQ | Alta | 3* | LUC-02, MAR-05, MAR-15, REN-12 |
| C24 | Escores de adulto (TFG-MDRD, HOMA) num produto pediátrico | #catalogo | Alta | 2 | REN-09, MAR-10 |
| C25 | "Dados no Brasil" × Vercel iad1 (EUA); política sem DPO, base legal, cookies e pagamento | #quem-esta-por-tras, /privacidade | Alta | 2 | ROB-10, ROB-11, ROB-12, BRU-13 |
| C26 | Sem navegação no mobile (13.206 px de página) | Header mobile | Alta | 1 | CAI-01 |
| C27 | Print do hero ilegível no mobile (texto de 6–10 px); aviso de conferência longe do resultado | Hero mobile | Alta ▲ | 3 | CAI-03, PAT-14, PAT-07 |
| C28 | Ambiguidades de preparo: "AD", faixa de tempo, diluente alternativo; concentração no limite máximo; estabilidade | Hero, demo | Média | 1 | PAT-11, PAT-12, PAT-13 |
| C29 | Performance mobile: LCP 3,5 s, 4 famílias de fonte, TBT 350–450 ms, imagem sem fetchpriority | Global | Média | 2 | BRU-06, BRU-07, BRU-08, BRU-15, CAI-14 |
| C30 | Planos no mobile ocupam ~3 telas | #planos mobile | Média | 2 | CAI-06, LUC-06 |
| C31 | SEO on-page e JSON-LD: sem "calculadora"/"dose pediátrica", h2 numerados, sem FAQPage; sem manifest/apple-touch-icon | Global | Média | 1 | BRU-10, BRU-11, BRU-14 |
| C32 | Polimento visual e de copy: chips que parecem botão, demo desktop com vazio, aciclovir repetido 3×, headings do mockup, linhas longas, Voltar/Recomeçar desabilitados, bloco "Privacidade" fora de contexto, mockup do CTA final vazio, preço antes do valor no hero, cena "PA sozinho" enterrada, login social | Várias | Média/Baixa | 4 | CAI-05, CAI-07, CAI-10, CAI-11, CAI-13, CAI-15, MAR-09, MAR-12, MAR-13, LUC-08, LUC-11 |

\* C23 já é Alta na origem; com 3 personas subiria para Crítica, mas mantive Alta porque "offline" é uma pergunta sobre o produto, não um defeito da página. A promoção depende da resposta (ver seção 8).

## 5. Matriz de prioridade

| ID | Impacto | Esforço | Observação |
|---|---|---|---|
| C01 | Alto | Médio | Muda o template de saída do app e os prints |
| C02 | Alto | Baixo | Mostrar "1.000 mg (50 mg/kg/dose) · 100 mg/mL" no passo 3 |
| C03 | Alto | Baixo | Rótulo de indicação no card |
| C04 | Alto | Médio | Exige levantar as referências reais |
| C05 | Alto | Alto | Parecer regulatório + revisão de copy |
| C06 | Alto | Baixo | Mover o selo |
| C07 | Alto | Médio | Termos (jurídico) + rodapé |
| C08 | Alto | Alto | Integrar checkout e repassar plano/UTM |
| C09 | Alto | Baixo | GA4/Plausible + eventos |
| C10 | Alto | Baixo | Lista de 4–6 algoritmos + link para a lista completa |
| C11 | Alto | Baixo | Trocar a hierarquia dos CTAs |
| C12 | Alto | Baixo | Reescrever os selos de preço |
| C13 | Alto | Baixo/Médio | Links das lojas, ou retirar a frase até publicar |
| C14 | Alto | Baixo | Trocar o caso da demo |
| C15 | Médio | Médio | Criar exemplo e print |
| C16 | Alto | Médio | Ajuste de scroll/sticky |
| C17 | Médio | Baixo | Domínio + metadados |
| C18 | Médio | Baixo | 1 imagem 1200×630 |
| C19 | Alto | Baixo | Texto + processo de reembolso |
| C20 | Alto | Baixo | Alinhar texto e política (decisão do dono) |
| C21 | Médio | Médio | Decisão comercial |
| C22 | Médio | Baixo | Especificar o "1 medicamento" |
| C23 | Alto | Baixo | FAQ, depende da resposta |
| C24 | Médio | Baixo | Trocar exemplos por Schwartz etc. |
| C25 | Alto | Médio | Política + ajuste da frase |
| C26 | Médio | Baixo | Menu ou âncoras |
| C27 | Médio | Baixo | Recorte só da prescrição |
| C28 | Médio | Médio | App + prints |
| C29 | Médio | Médio | Fontes + fetchpriority |
| C30 | Baixo | Médio | — |
| C31 | Baixo | Baixo | — |
| C32 | Baixo | Baixo/Médio | — |

### Quick wins (alto impacto, baixo esforço)
1. C02, mostrar mg, mg/kg e concentração na demo
2. C03, indicação no card do aciclovir
3. C06, tirar a credencial da área de preço
4. C14, trocar o caso da demo por ITU típica
5. C11, "Testar 7 dias grátis" como CTA primário + "sem cartão" (se for verdade)
6. C12, preço por mês e "6 pelo preço de 5"
7. C19, texto de arrependimento e aviso de renovação
8. C20, alinhar o público entre landing e política
9. C09, instalar analytics e eventos
10. C10, destacar 4–6 algoritmos reais
11. C13, links das lojas ou retirar a promessa
12. C23, FAQ: offline, atualização, diferença para outros apps

### Projetos (alto impacto, esforço médio ou alto)
1. C01, novo formato da prescrição copiável (app + prints)
2. C04, bloco "Fontes e revisão" com referências reais e canal de erro
3. C05, parecer regulatório (SaMD) e revisão de copy
4. C07, Termos de Uso + aceite + rodapé completo
5. C08, checkout real com plano e UTM preservados
6. C16, demo mobile
7. C25, política de privacidade e local de processamento

### Polimento (o resto)
C15, C17, C18, C21, C22, C24, C26, C27, C28, C29, C30, C31, C32. Entre eles, C17 e C18 são quase gratuitos e devem sair junto com os quick wins.

## 6. Conflitos entre personas

Critério de desempate: **segurança do paciente > conformidade regulatória > clareza > conversão > estética.**

| # | Conflito | Solução que atende os dois lados | Prioridade |
|---|---|---|---|
| 1 | **Headline "copie a prescrição pronta"**. Marina e Lucas: é a promessa que converte. Patrícia, Renata e Roberto: incentiva uso acrítico e contradiz o disclaimer (C05) | Manter a velocidade e acrescentar a conferência: "Informe o peso e confira a prescrição montada, com a conta à vista." CTA final: "sugestão de prescrição para você conferir" | Patrícia/Roberto (segurança e regulatório) |
| 2 | **CTA primário**. Marina quer "Testar 7 dias grátis" (mais agressivo em conversão). Roberto pede cuidado com "grátis" em valores já descontados | Usar "grátis" só para o trial real ("7 dias grátis, sem cartão", se for verdade). Nos planos, trocar "1 mês grátis" por "6 meses pelo preço de 5" | Roberto no "grátis" dos planos; Marina na hierarquia dos CTAs (não há conflito real) |
| 3 | **Disclaimers**. Lucas e Caio gostam de avisos curtos e de um hero enxuto. Patrícia quer o aviso colado ao resultado | Uma linha curta *dentro* do card de resultado ("Confira dose, concentração e indicação antes de prescrever"), retirando o aviso solto do hero. No total, não aumenta o texto | Patrícia (segurança) |
| 4 | **Credencial da Dra. Karla**. Marina, Lucas e Renata dizem que é a principal prova de confiança. Roberto: endosso comercial ao lado do preço | Manter nome, CRM e RQE em `#quem-esta-por-tras` e um link discreto no hero ("Conteúdo clínico revisado — ver responsável e fontes"), sem foto e fora da área de preço | Roberto (regulatório) |
| 5 | **Estudantes como público**. Júlia quer desconto de estudante e o internato em destaque. Roberto: oferecer "copiar prescrição" a quem não pode prescrever é risco | Para estudantes, vender o **raciocínio** (algoritmo + conta aberta + bula), não a prescrição; incluir "uso educacional sob supervisão" no card do internato; alinhar a política de privacidade | Roberto/Patrícia. O desconto de estudante é decisão comercial e não conflita |
| 6 | **Prova social**. Marina quer número de usuários e depoimentos. Roberto: depoimento sobre resultado clínico é sensível pelo CFM | Usar prova de processo: referências, data de revisão, número de revisores, nota nas lojas. Depoimentos só sobre usabilidade e tempo, nunca sobre desfecho clínico | Roberto |
| 7 | **Hero mobile**. Caio quer cortar o print longo (performance e legibilidade). Bruno: o print é o LCP. Lucas: o card do aciclovir dá confiança | Um recorte só da prescrição e do preparo (menor, legível e mais leve para o LCP), com `fetchpriority="high"` | Caio/Bruno (clareza), sem perda de conversão |
| 8 | **Analytics**. Bruno quer medição completa. Roberto: a política de privacidade não prevê cookies | Instalar analytics sem cookies (Plausible/Umami ou GA4 em modo consentimento) e atualizar a política **no mesmo deploy** | Roberto define o "como"; Bruno define o "o quê" |

## 7. Plano de ação (10 primeiras mudanças, em ordem)

> O código-fonte da landing não está neste repositório. "Onde" indica a seção e o ID da página (Next.js na Vercel). Os textos sugeridos estão prontos para usar; os marcados com ⚖️ precisam passar pelo jurídico antes.

1. **Prescrição copiável com dose em massa.** *(C01)*
   - **Onde:** app (template do texto copiado), prints do hero (`1-hero-aciclovir-prescricao.png`) e `4-prescricao-copiada.png`, card do hero, demo passo 3.
   - **Especificação:** o texto começa pelo fármaco e pela dose em mg; o volume aparece só como instrução de preparo. Exemplo: *"Cefepime 1.000 mg (50 mg/kg/dose) IV de 8/8 h. Preparo: reconstituir FA 1 g com 10 mL de água para injetáveis (100 mg/mL); aspirar 10 mL e diluir em 15 mL de SF 0,9% (40 mg/mL); infundir em 30 min."* Adrenalina: *"Adrenalina (epinefrina) 1 mg/mL — 0,2 mg (0,01 mg/kg) = 0,2 mL IM…"*. Usar vírgula decimal em toda a interface. Trocar "AD" por "água para injetáveis".
   - **IDs:** REN-02, REN-07, PAT-01, PAT-02, PAT-10, PAT-11, LUC-01.
2. **Demo e card do hero com a conta à vista e um caso coerente.** *(C02, C03, C14)*
   - **Onde:** `#demonstracao` (caso fixo e passo 3) e card do hero.
   - **Demo:** trocar o caso para o ramo "Sepse/urosepse → Ceftriaxona IV" (ITU típica). Se mantiver o cefepime, rotular: "Esta dose segue o protocolo de neutropenia febril". No passo 3, mostrar "Dose: X mg (Y mg/kg/dose) · concentração após reconstituição: Z mg/mL".
   - **Card do aciclovir:** acrescentar "Indicação: <quadro> · Referência: 10 mg/kg/dose 8/8 h (<fonte, ano>) · ajustar pela função renal".
   - **Antes de publicar:** conferir com a Dra. Karla e com um farmacêutico o deslocamento de volume do cefepime.
   - **IDs:** PAT-03, PAT-04, PAT-05, REN-05, REN-06, REN-10, REN-11, LUC-01, LUC-13, JUL-09, MAR-08.
3. **Bloco "Fontes e revisão".** *(C04)*
   - **Onde:** `#quem-esta-por-tras` (substitui a linha oculta "Fontes [NEED]") e FAQ "Posso confiar na dose calculada?".
   - **Texto:** "Referências: bulas ANVISA, <SBP…>, <protocolos…>. Última revisão do conteúdo: mm/aaaa. Revisamos a cada N meses e sempre que uma bula muda. Revisão por: <nomes/funções, incluindo farmacêutico se houver>. Encontrou um erro de dose? <link/e-mail> — respondemos em até X dias úteis."
   - **No FAQ:** acrescentar o que o app **não** considera (função renal, < 28 dias, obesidade) e dizer que a dose máxima é **sinalizada** quando aplicada.
   - **IDs:** REN-01, REN-03, PAT-06, PAT-08, LUC-04, MAR-03, JUL-08.
4. **Tirar a credencial da área de preço.** *(C06)* ⚖️
   - **Onde:** selo do hero entre o preço e o CTA.
   - **Mudança:** remover foto, CRM e RQE do hero. No lugar, o link "Conteúdo clínico revisado · ver responsável e fontes" → `#quem-esta-por-tras`.
   - **Texto da seção:** "Responsável técnica pelo conteúdo clínico: Dra. Karla Araújo, pediatra, CRM-PE 18734, RQE 13315."
   - **Também:** confirmar o "mais de 14 anos" ou trocar por "desde 20XX".
   - **IDs:** ROB-02.
5. **Ajustar a copy de risco (título, para-quem, CTA final).** *(C05)* ⚖️
   - **Onde:** H1, `<title>`, meta description, `#para-quem`, CTA final.
   - **H1:** "Informe o peso e confira a prescrição montada, com a conta à vista."
   - **Card "Anos de plantão":** trocar "sem você refazer a conta" por "com cada etapa da conta visível".
   - **Card "No internato":** acrescentar "para estudar o raciocínio, sob supervisão".
   - **Demo:** trocar "Calcular para o meu paciente" por "Testar o cálculo no app".
   - **Parecer:** contratar o parecer regulatório de enquadramento SaMD em paralelo.
   - **IDs:** ROB-01, ROB-07, ROB-08, REN-08, PAT-07.
6. **Base jurídica do e-commerce.** *(C07, C19)* ⚖️
   - **Termos de Uso:** criar `/termos`, com aceite obrigatório (checkbox) e link para `/privacidade` no `/register`.
   - **Rodapé:** "Opex Consultoria Ltda. · CNPJ XX.XXX.XXX/0001-XX · <endereço> · contato@… · Termos · Privacidade".
   - **Arrependimento** (em `#planos` e no FAQ "Como cancelo?"): "Se assinar e desistir em até 7 dias, devolvemos o valor integral. Peça em <canal> ou em Conta > Assinatura."
   - **Renovação:** "Avisamos por e-mail X dias antes de cada renovação." Só publicar se for implementado.
   - **IDs:** ROB-03, ROB-04, ROB-05, MAR-07, REN-15, LUC-15, BRU-09, BRU-12, JUL-10.
7. **Hierarquia de CTAs e copy de preço.** *(C11, C12)*
   - **CTAs:**
     - Hero, header e CTA final: primário "Testar 7 dias grátis" (acrescentar "sem cartão" se o cadastro não pedir cartão), secundário "Ver planos".
     - Header no mobile: "Testar grátis".
     - "Assinar o plano do rodízio" fica como primário só dentro de `#planos`.
   - **Selos de preço:**
     - Rodízio: "R$ 33,25/mês · 6 meses pelo preço de 5".
     - Anual: "R$ 33,25/mês · 12 meses pelo preço de 10". Melhor ainda: dar ao anual um preço por mês menor, para que ele seja de fato a opção mais vantajosa.
   - **Hero:** retirar o preço.
   - **IDs:** MAR-01, MAR-02, MAR-04, MAR-09, LUC-05, LUC-06, LUC-07, JUL-05, JUL-11, CAI-12, ROB-14.
8. **Funil: checkout, plano/UTM e medição.** *(C08, C09)*
   - **Links dos CTAs:** passar para `/register?plano=rodizio|anual|mensal|gratis`, preservando `utm_*`/`gclid`.
   - **`/register`:** mostrar o plano escolhido, o preço e os 7 dias.
   - **Checkout:** ligar ao checkout real assim que existir. Até lá, os botões de plano dizem "Começar com 7 dias grátis".
   - **Medição:**
     - Instalar analytics sem cookies.
     - Eventos: `cta_click{plano,posicao}`, `demo_step{1,2,3}`, `register_start`, `register_complete`, `checkout_start`, `purchase`.
     - Atualizar `/privacidade` no mesmo deploy.
   - **IDs:** MAR-06, BRU-01, BRU-02, BRU-13, LUC-03, LUC-08, JUL-11, ROB-06.
9. **Lojas e cobertura.** *(C13, C10)*
   - **Lojas:** colocar os selos com links para a App Store e o Google Play no hero e no CTA final. Se os apps ainda não estiverem publicados, trocar por "Na web hoje; apps para iPhone e Android em breve".
   - **Pagamento nas lojas:** ajustar "cancele no app" e "mesmo preço no app" conforme a cobrança aconteça pelas lojas. Avisar no FAQ que assinaturas da App Store e do Google Play se cancelam na loja.
   - **`#catalogo`:** listar 4–6 algoritmos reais (ex.: bronquiolite, asma, sepse, crise convulsiva, ITU, anafilaxia), com o link "ver os +50", e trocar o print da lista por um com vários algoritmos.
   - **Escores:** trocar os exemplos por escores pediátricos (Schwartz em vez de MDRD).
   - **IDs:** ROB-06, ROB-09, BRU-09, CAI-04, CAI-09, JUL-13, LUC-10, MAR-07, REN-04, REN-09, MAR-10.
10. **Demo e hero no mobile + metadados.** *(C16, C27, C26, C17, C18)*
    - **Demo:**
      - Limitar `.telaApp` a ~520 px.
      - Rolar até a ação a cada passo (`scrollIntoView({block:'nearest'})` dentro do contêiner).
      - Faixa de instrução `position: sticky`.
      - Header sem cobrir o mockup.
    - **Hero mobile:** usar só o recorte da prescrição, com `fetchpriority="high"`.
    - **Navegação mobile:** menu simples (Demonstração · Planos · Perguntas).
    - **`<head>`:**
      - `og:image` 1200×630, `og:url`, `og:site_name`, `twitter:card=summary_large_image`.
      - `canonical` para o domínio definitivo.
      - `robots.txt` + `sitemap.xml`.
      - `noindex` em `/login` e `/register`.
    - **IDs:** CAI-01, CAI-02, CAI-03, LUC-12, REN-14, PAT-14, BRU-03, BRU-04, BRU-05, BRU-06, JUL-06, JUL-07, LUC-14, MAR-14.

**Depois dos 10:** retirar as 11 marcações `data-pendencia` do build de produção (renderizar só com uma flag de ambiente). Depois, tratar C15 (exemplo de gotas e mL/h), C20–C25, C28–C32.

## 8. Perguntas para o dono do produto

1. **O cadastro e o trial pedem cartão?** O `/register` atual não pede, mas a página não diz. Isso define se o CTA pode dizer "sem cartão".
2. **Como será a cobrança?** Checkout próprio no site (qual gateway?) ou compra pelas lojas (App Store/Google Play)? Isso muda as promessas "cancele no app", "mesmo preço" e "cartão de débito".
3. **Os apps já estão publicados nas lojas?** Se não, quando? Até lá, retiramos "Disponível em iPhone, Android"?
4. **Qual é o domínio definitivo?** O FAQ cita `medlogicpro.com.br`; a página está em `medlogic-three.vercel.app`.
5. **O app funciona offline?** Total, parcial (doses em cache) ou não?
6. **Quais são as referências do conteúdo** (bulas ANVISA, SBP, AAP, Harriet Lane, protocolos do IMIP…)? **Quem é "a equipe" de revisão** (nomes ou funções, se há farmacêutico)? Qual a data da última revisão?
7. **Estudantes podem assinar?** A política de privacidade diz "exclusivamente profissionais de saúde adultos". Haverá comprovação? Haverá preço de estudante, de turma ou de indicação, ou Pix?
8. **O anual deve ser mais barato por mês que o rodízio?** Hoje os dois saem por R$ 33,25. E o nome "Plano do rodízio" continua, já que o rodízio de pediatria dura ~2 meses?
9. **No plano Gratuito, qual é "1 medicamento"?** Fixo, à escolha do usuário ou rotativo? Vale liberar alguns algoritmos?
10. **Haverá aviso por e-mail antes da renovação automática?** E qual será o processo de reembolso do arrependimento de 7 dias?
11. **Qual é o vínculo formal da Dra. Karla com a Opex** (sócia, contratada, responsável técnica)? O "mais de 14 anos" está certo, se a residência foi concluída em 2013?
12. **O app guarda algo no servidor** (logs de cálculo, peso, medicamento)? Onde ficam a API e o banco (para sustentar "seus dados ficam no Brasil")?
13. **Pretendem pedir o parecer regulatório (ANVISA/SaMD)** antes do lançamento?
14. **O cefepime da apresentação usada** foi validado com o volume final real após a reconstituição (deslocamento do pó)?

## 9. Anexos

**Relatórios individuais**
- [Dra. Renata — plantonista sênior](reports/dra-renata.md)
- [Lucas — residente R1](reports/lucas.md)
- [Júlia — interna](reports/julia.md)
- [Marina — copy e conversão](reports/marina.md)
- [Caio — design e UX](reports/caio.md)
- [Patrícia — farmacêutica, segurança](reports/patricia.md)
- [Roberto — compliance e jurídico](reports/roberto.md)
- [Bruno — SEO e performance](reports/bruno.md)

**Material de apoio**
- `snapshot/`: `page.html`, `page.md`, `privacidade.html`, `img/`, `README.md`.
- `screens/`: capturas base, passos da demo e as capturas extras de Lucas (`lucas-*`) e de Caio (`caio-*`).
- `lighthouse-mobile.json`: execução do Lighthouse mobile (Bruno).

**Limitações gerais da auditoria**
- O app logado e o checkout não foram testados.
- O Lighthouse rodou de um servidor fora do Brasil (dados de laboratório).
- As faixas de dose foram checadas por buscas na web, sem acesso direto ao Bulário ANVISA nem aos sites dos fabricantes.
- O site do CFM estava inacessível pela rede do ambiente; a Resolução 2.336/2023 foi lida por resumos.
- A análise jurídica é triagem de risco, não parecer.
