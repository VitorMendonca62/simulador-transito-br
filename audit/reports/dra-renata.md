# Relatório — Dra. Renata Calheiros (pediatra, plantonista de PS infantil, preceptora)

> Fontes usadas: snapshot em `audit/snapshot/` (page.md, page.html, img/) e capturas em `audit/screens/` (base e demo, passos 1 a 3). Do site ao vivo baixei só o CSS público, para confirmar se as marcações de pendência aparecem na tela. Não usei WebFetch: o snapshot já cobria o necessário. Não tive acesso ao app logado, então não testei outros pesos, outras drogas nem o texto que realmente vai para a área de transferência ao copiar.

## Primeira impressão (5 segundos)
"Coloque o peso da criança e copie a prescrição pronta." Já comecei desconfiada. A promessa é tirar a conta da minha mão, e eu procuro de onde sai a dose. Na página inteira não há uma referência bibliográfica nem data de revisão. A "equipe" que revisa não tem nome. Continuei rolando para conferir as contas, não por confiança.

## Nota geral: 5/10
As contas de aciclovir, adrenalina e cefepime batem. Mas a página não diz de onde vêm as doses nem quando foram revisadas, e as prescrições mostradas saem em mL, sem nome da droga e sem dose em mg. Isso eu não deixaria um residente meu copiar.

## Conferência das contas (refeitas por mim)

**Aciclovir FA 250 mg IV, 20 kg (hero e prints 1-hero / 1-recorte)**
- Dose: 10 mg/kg/dose × 20 kg = **200 mg/dose**. Confere.
- Diária: 200 mg × 3 (8/8 h) = **600 mg**. Confere.
- Reconstituição: 250 mg + 10 mL de AD = 25 mg/mL. 200 mg ÷ 25 mg/mL = **8 mL aspirados**. Confere.
- Diluição: 8 mL + 32 mL de SF 0,9% = **40 mL**. Concentração final: 200 mg ÷ 40 mL = **5 mg/mL**. Está dentro do limite usual de ≤ 7 mg/mL para infusão periférica. Infusão em 60 min também está correta.
- Ressalva clínica: 10 mg/kg/dose é a dose de varicela ou HSV fora do SNC. Para encefalite herpética fora do período neonatal, as referências trazem 10–15 mg/kg/dose (e alguns protocolos usam até 20). No neonato a dose é 20 mg/kg/dose. O print não mostra **para qual indicação** essa dose foi escolhida.

**Adrenalina IM, 20 kg (print 4-prescricao-copiada, anafilaxia)**
- 0,01 mg/kg × 20 kg = **0,2 mg**. Com ampola de 1 mg/mL, 0,2 mg = **0,2 mL**. Confere e fica abaixo do teto de 0,5 mg. Vasto lateral da coxa e repetição a cada 5–15 min estão corretos.
- Problemas: o print **não mostra a concentração da ampola**. A conta de 0,2 mL só vale para 1 mg/mL (1:1000). Na diluição de 0,1 mg/mL, a criança receberia 10 vezes menos. Além disso, o campo de referência escreve "0.01 mg/kg/dose" (com ponto) e os detalhes escrevem "0,01 mg/kg/dose" (com vírgula) na mesma tela.

**Cefepime FA 1 g IV, 20 kg (demo, passo 3)**
- A demo **não mostra a dose em mg nem a dose em mg/kg**. Tive que deduzir: 1 g ÷ 20 kg = **50 mg/kg/dose** de 8/8 h, que está correto para neutropenia febril (teto de 2 g/dose).
- 1 g + 10 mL de AD ≈ 100 mg/mL (nominal). 10 mL + 15 mL = **25 mL**, ou 1.000 mg ÷ 25 mL = **40 mg/mL**. Isso é exatamente o teto usual para infusão intermitente. Está certo, mas no limite. Infusão em 15–30 min é aceitável.
- Ponto a conferir na bula do fabricante: o pó de cefepime desloca volume. Em várias bulas, 1 g + 10 mL dá cerca de 11,4 mL (≈ 90 mg/mL). Se for o caso, "Administrar 10 mL" entrega cerca de 880 mg (≈ 44 mg/kg), não 1 g. Para frasco inteiro, o texto seguro é "administrar todo o conteúdo do FA".

Conclusão: nenhuma conta mostrada está errada. O risco está no que **falta** na tela: indicação, concentração da apresentação, dose em mg e nome da droga na prescrição.

## Achados
| ID | Seção | Severidade | Problema | Evidência | Sugestão concreta |
|----|-------|-----------|----------|-----------|-------------------|
| REN-01 | #quem-esta-por-tras | Crítica | Não há nenhuma referência bibliográfica na página. A linha "Fontes" existe, mas está escondida e é só um placeholder no HTML servido. | No HTML: `<dt>Fontes</dt>` seguido de "[ NEED : referências que o conteúdo segue (SBP, AAP, Ministério da Saúde…). ]", dentro da classe `soRevisao`, que o CSS esconde (`:not([data-revisao]) .soRevisao{display:none}`). Na tela pública não aparece fonte nenhuma. | Preencher e exibir: "Fontes: Harriet Lane Handbook (23ª ed.), Lexicomp Pediatric & Neonatal Dosage Handbook, bulas ANVISA (Bulário Eletrônico), Documentos Científicos e Tratado de Pediatria da SBP, Red Book AAP 2024." Listar só o que for verdade. Remover do HTML de produção todos os spans `data-pendencia` (há 9: links das lojas, algoritmos em destaque, prints "com a especialista", checkout, cartão, CFM). |
| REN-02 | Hero, print aciclovir; print adrenalina; demo passo 3 | Crítica | A "prescrição pronta" que se copia sai em mL, sem nome do medicamento, sem dose em mg e sem concentração. Colada no prontuário, "Administrar 0,2 mL IM" é a receita do erro de 10 vezes. | "Reconstituir 1 FA + 10 mL de AD. Administrar 8 mL + 32 mL de SF 0,9% IV…"; "Administrar 0,2 mL IM, com intervalo a cada 5-15 minutos…"; "Administrar 10 mL + 15 mL de SG 5%…" | Mostrar nos prints um texto copiado completo: "Aciclovir 200 mg (10 mg/kg/dose) IV 8/8 h — Reconstituir FA 250 mg com 10 mL AD (25 mg/mL); aspirar 8 mL + 32 mL SF 0,9% (5 mg/mL); infundir em 60 min." Para adrenalina: "Adrenalina 1 mg/mL — 0,2 mg (0,2 mL) IM no vasto lateral…". Se o app já faz isso, trocar os prints. |
| REN-03 | #quem-esta-por-tras / FAQ "Quem faz o conteúdo?" | Crítica | O processo de revisão é anônimo e não tem data. Não sei quem é "a equipe", quantos revisores há, se existe farmacêutico clínico, nem quando cada droga foi revisada. | "Cada medicamento, bula, algoritmo e ferramenta é revisado pela equipe antes de ser publicado." Não há data de revisão em lugar nenhum. | Nomear a equipe (nome, CRM/CRF, função) e descrever o processo: "Cada dose é conferida por dois revisores (pediatra + farmacêutico clínico) contra as fontes listadas; cada ficha mostra a data da última revisão." Adicionar: "Última revisão geral do conteúdo: mês/ano." |
| REN-04 | #catalogo | Crítica | A cobertura é invisível. "+50 algoritmos" sem lista, e o placeholder de destaques está escondido. O único print mostra Anafilaxia, Artrite Séptica e Candidemia. Não sei se tem bronquiolite, asma, sepse, crise convulsiva, CAD ou desidratação. | Filtros "Respiratório / Infeccioso / Emergência / Neurológico / Outros" sem conteúdo; no HTML: "[ NEED : 4 a 6 algoritmos reais em destaque, cada um com uma linha de descrição. ]" | Abaixo dos filtros, listar os algoritmos por categoria (texto, não print): "Respiratório: bronquiolite, crise de asma, laringite… Emergência: anafilaxia, crise convulsiva/estado de mal, sepse, CAD…". Incluir link "ver lista completa dos 50+". Fazer o mesmo para as 120 drogas. |
| REN-05 | Demo, passo 3 | Alta | A demo esconde exatamente o que eu confiro: dose em mg e mg/kg. O FAQ promete o contrário. | O passo 3 mostra só a prescrição em mL. O FAQ diz: "Junto da prescrição aparecem a dose de referência, a dose por administração e os volumes de preparo, para você conferir." | Incluir no passo 3 o card "Detalhes do cálculo": dose de referência 50 mg/kg/dose · dose 1.000 mg · dose máxima 2 g/dose · concentração final 40 mg/mL. |
| REN-06 | Hero, card do aciclovir | Alta | A dose aparece sem indicação e sem faixa etária. 10 mg/kg/dose não é a dose de encefalite herpética para muita gente, nem a do neonato. Plantonista experiente estranha na hora. | "Aciclovir FA 250 mg IV · 20 kg · 200 mg/dose" e "Dose de referência usada 10 mg/kg/dose" | Acrescentar a indicação no card: "Aciclovir — varicela em imunodeprimido, >3 meses: 10 mg/kg/dose 8/8 h". Ou trocar o exemplo por uma indicação inequívoca. Mostrar também que o app pede ou exibe a indicação. |
| REN-07 | Print da adrenalina | Alta | A concentração da apresentação não aparece, e o separador decimal está misturado na mesma tela. Ponto e vírgula juntos num app de dose é bandeira vermelha. | "Referência: 0.01 mg/kg/dose" (campo) x "0,01 mg/kg/dose" (detalhes); não há "1 mg/mL" no print. | Padronizar vírgula em todo o app e mostrar "Apresentação: ampola 1 mg/mL (1:1000)" acima da prescrição. Refazer o print. |
| REN-08 | Hero (título) / #diferencial | Alta | "Copie a prescrição pronta" vende a ideia de não pensar. O papel do médico só aparece como rodapé jurídico. | "Coloque o peso da criança e copie a prescrição pronta." / "Ferramenta de apoio ao profissional de saúde. Não substitui a prescrição médica." | Título: "Do peso à prescrição montada, com cada conta à vista para você conferir." Trocar o aviso jurídico por: "Você confere dose, concentração e volume na mesma tela antes de copiar. A decisão é sua; a conta, a gente mostra." |
| REN-09 | #catalogo (escores) | Alta | Os escores usados como exemplo são de adulto. TFG-MDRD não é validada em criança (em pediatria se usa Schwartz) e HOMA não é ferramenta de PS. Para pediatra, isso derruba a credibilidade de "Pediatria". | "+85 como Clearance de Creatinina, TFG-MDRD e HOMA" | Trocar por exemplos pediátricos reais, se existirem: "Schwartz (TFG), Glasgow pediátrico, Westley (crupe), PRAM (asma), superfície corporal, Holliday-Segar." Se o app não tem Schwartz, incluir antes de lançar. |
| REN-10 | Demo, cefepime | Média | "Administrar 10 mL" depois de reconstituir com 10 mL ignora o deslocamento de volume do pó. Em várias bulas, 1 g + 10 mL dá ≈ 11,4 mL (≈ 90 mg/mL). | "Reconstituir 1 FA + 10 mL de AD. Administrar 10 mL + 15 mL…" | Conferir com a bula ANVISA do produto. Se for frasco inteiro, escrever "Administrar todo o conteúdo reconstituído (1 g)". Se for fração, usar a concentração real pós-reconstituição. |
| REN-11 | Demo, passos 1 e 2 | Média | O algoritmo de ITU mostrado é incoerente e o caso escolhido é estranho. O critério de internação diz "< 28 dias", mas o tratamento hospitalar traz linha para "< 2 meses". E o caso da demo é neutropenia febril dentro de ITU, que no meu PS vai para o protocolo de onco, não para o de ITU. | Passo 1: "Idade < 28 dias;". Passo 2: "Idade < 2 meses → Aminoglicosídeo ± Ampicilina"; "Imunossupressão + neutropenia febril → … Cefepime". | Pedir revisão da Dra. Karla no algoritmo (alinhar o critério com a faixa de < 2–3 meses, como fazem SBP e AAP). Trocar o caso da demo por algo de rotina de PS (ex.: ITU febril em lactente de 8 meses → ceftriaxona 50 mg/kg/dia), mostrando a conta inteira. |
| REN-12 | #para-quem, card "Anos de plantão" | Média | O card para o experiente é genérico e não prova ganho de tempo. Não há comparação com o fluxo que eu já tenho, que é a dose de cabeça e o app só para diluição. | "Você já sabe a conduta. O app entrega a prescrição e os volumes de preparo sem você refazer a conta." | "Você já sabe a dose. O que toma tempo é a diluição da droga que você usa duas vezes por ano: concentração máxima, diluente compatível, tempo de infusão e mL/h da bomba, numa tela, com a fonte ao lado." Mostrar um exemplo de infusão contínua (ex.: midazolam ou adrenalina em mcg/kg/min → mL/h). |
| REN-13 | #diferencial | Média | As promessas de "gotas" e "infusão contínua em mL/h" não têm nenhum exemplo. Infusão contínua é justamente onde eu mais usaria o app e onde mais se erra. | "Em gotas — o número de gotas"; "Infusão contínua — a velocidade em mL/h para a bomba" | Adicionar um print real: "Adrenalina 0,1 mcg/kg/min, 20 kg, solução X mg em Y mL → Z mL/h", com a conta visível. |
| REN-14 | Demo (mobile) | Baixa | O header fixo cobre o topo do celular simulado nos passos 2 e 3. Some justamente a linha "Ver no bulário" e parte do texto do algoritmo. | `screens/demo-passo2-mobile.png` e `demo-passo3-mobile.png`: a barra "MedLogic Pro / Entrar / Assinar" sobrepõe o mockup. | Somar `scroll-margin-top` igual à altura do header na âncora da demo, ou deixar a moldura do mockup começar abaixo do header. |
| REN-15 | Rodapé / geral | Baixa | Não há link de Termos de Uso, contato nem CNPJ. Para uma ferramenta de dose, quero saber a quem reportar erro. | Rodapé: "Operado por Opex Consultoria Ltda." e só "Privacidade". | Adicionar Termos de Uso, CNPJ e um canal "Reportar erro de dose" (e-mail ou formulário), com prazo de resposta. |

## O que está funcionando (não mexer)
- As contas mostradas estão certas: aciclovir 200 mg / 8 mL / 32 mL / 40 mL (5 mg/mL), adrenalina 0,2 mg = 0,2 mL e cefepime 50 mg/kg em 40 mg/mL.
- O card "Detalhes do cálculo" do print do aciclovir mostra dose de referência, dose diária, número de administrações e volumes. É exatamente o que eu quero conferir e deve aparecer na demo.
- O alerta "Aplicar IM no músculo vasto lateral da coxa" no print da adrenalina é útil para residente.
- A ligação entre bula e algoritmo ("Retornar ao algoritmo · Anafilaxia") resolve uma troca de tela real.
- O CRM e o RQE da coordenadora estão visíveis e a formação (UPE/IMIP) é verificável.
- "O app não pede nem guarda dados de pacientes" é dito de forma clara e curta.
- O plano anual de R$ 399 está dentro do que eu pago sem pensar, desde que mostre as fontes.

## Perguntas que ficaram sem resposta
- Quais referências cada dose segue (Harriet Lane? Lexicomp? bula ANVISA? SBP?) e o app mostra a fonte em cada ficha?
- Quando cada medicamento foi revisado pela última vez, e quem revisou, além da coordenadora?
- O texto copiado inclui o nome da droga, a dose em mg e a concentração, ou só os mL?
- O app pede a indicação ou a idade? Neonato e criança maior recebem a mesma referência?
- Como o app trata o deslocamento de volume na reconstituição e a concentração máxima de cada droga?
- Quais são os 50+ algoritmos e as 120 drogas? Tem bronquiolite, asma, sepse, estado de mal epiléptico, CAD e desidratação?
- Tem infusão contínua de drogas vasoativas e de sedação, em mcg/kg/min, com a conta da bomba?
- Para onde reporto um erro de dose, e em quanto tempo ele é corrigido?

## Veredito
Eu criaria conta grátis para testar uma droga que eu sei de cabeça e ver se a conta bate. Não assinaria enquanto a página não mostrar as fontes, a data de revisão e uma prescrição copiada com nome, mg e concentração. Para residente, eu não recomendaria do jeito que está.
