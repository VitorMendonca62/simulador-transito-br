# Formato padrão de relatório (referência — NÃO é um subagente)

Todas as personas entregam o relatório neste formato, para o orquestrador conseguir consolidar.
Este arquivo começa com `_` e não tem frontmatter, então o Claude Code não o carrega como agente.

```markdown
# Relatório — <Nome da persona>

## Primeira impressão (5 segundos)
<2–3 frases, na voz da persona: o que entendeu, o que sentiu, se continuaria rolando a página>

## Nota geral: X/10
<1 frase justificando>

## Achados
| ID | Seção | Severidade | Problema | Evidência | Sugestão concreta |
|----|-------|-----------|----------|-----------|-------------------|
| <PREFIXO>-01 | Hero | Crítica/Alta/Média/Baixa | ... | trecho exato da página ou descrição do print | texto/ajuste pronto para aplicar |

Severidade:
- **Crítica** — bloqueia o lançamento, gera risco ao paciente, risco legal ou quebra a conversão.
- **Alta** — faz uma parte relevante do público desistir.
- **Média** — atrito perceptível, mas não impeditivo.
- **Baixa** — polimento.

## O que está funcionando (não mexer)
- ...

## Perguntas que ficaram sem resposta
- <dúvidas que a persona teria e que a página não responde>

## Veredito
<Assinaria / Criaria conta grátis / Sairia da página — e por quê, em 1–2 frases>
```

Regras gerais:
- Cite sempre a evidência (texto literal entre aspas ou seção + elemento). Achado sem evidência não entra.
- Sugestões precisam ser aplicáveis: reescreva a frase, indique o elemento, diga o valor.
- No máximo 15 achados, ordenados por severidade.
- Placeholders visíveis ("[definir]", "lista a confirmar" etc.) são sempre severidade Crítica.
