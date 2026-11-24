# Snapshot da auditoria — MedLogic Pro

- **URL auditada:** https://medlogic-three.vercel.app
- **Data/hora da coleta:** 2026-11-24, ~20:09–20:12 UTC (17:09–17:12 BRT)
- **Ferramentas:** curl (HTML), Playwright + Chromium headless (render, capturas, FAQ expandido, demo clicada).

## Arquivos
| Arquivo | Conteúdo |
|---|---|
| `page.html` | HTML servido pela home (Next.js, SSR) |
| `page.md` | Resumo seção por seção: textos, CTAs, destinos dos links, FAQ, estados da demo, imagens e alts |
| `raw.json` | Extração bruta (metas, seções, links, botões, imagens) |
| `privacidade.html` | HTML de /privacidade |
| `img/` | Imagens originais da página (prints do app e foto da Dra. Karla) |
| `../screens/base-mobile.png` | Página inteira, 390x844 |
| `../screens/base-desktop.png` | Página inteira, 1440x900 |
| `../screens/demo-passo{1,2,3}-mobile.png` | Seção de demonstração em cada passo (390x844) |

## Código-fonte
O código-fonte da landing **não está neste repositório** (`simulador-transito-br`, que é um simulador de trânsito em Rust/WASM). Pelo HTML, o site é um app **Next.js** hospedado na Vercel (`/_next/static/...`, `next/image`). Componentes, textos e imagens precisam ser localizados no repositório próprio do MedLogic Pro; nesta auditoria as referências de "onde mudar" são por seção/ID da página (`#problema`, `#catalogo`, `#demonstracao`, `#diferencial`, `#para-quem`, `#quem-esta-por-tras`, `#planos`, `#perguntas`).

## Limitações
- Rede do ambiente liberada só para `medlogic-three.vercel.app`; outros hosts (ex.: `medlogicpro.com.br`, lojas de app) podem estar inacessíveis via curl. WebSearch/WebFetch dos subagentes podem funcionar por outro caminho.
- Lighthouse não foi executado na coleta (cada persona registra se rodou algo).
