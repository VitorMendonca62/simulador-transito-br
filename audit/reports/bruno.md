# Relatório — Bruno Okamoto (growth / SEO técnico / performance / medição)

## Primeira impressão (5 segundos)
O HTML é bem feito: SSR do Next.js, `lang="pt-BR"`, um único h1, FAQ indexável, `next/image` servindo WebP e CLS zero. Mas a página está pronta para ser lida e não para ser medida nem encontrada. Não há analytics, as UTMs se perdem no clique, `robots.txt` e `sitemap.xml` dão 404, não existe canonical e o link compartilhado no WhatsApp chega sem imagem. Eu continuaria rolando, mas não colocaria um real em mídia antes de corrigir isso.

## Nota geral: 5/10
A base técnica é boa (SSR, CLS 0, WebP, alts descritivos), mas medição, indexação e compartilhamento estão em branco, e o LCP mobile fica em 3,5–3,6 s.

## O que foi medido e o que foi estimado
| Item | Como | Status |
|---|---|---|
| Head, metas, headings, imagens, scripts | `audit/snapshot/page.html` + parse Python | Medido |
| Status HTTP de rotas e links | `curl` ao vivo em 24/11/2026 ~20:12 UTC | Medido |
| Lighthouse 13.6.0 mobile (3 execuções) e desktop (1) | `npx lighthouse` + Chromium headless, throttling *devtools* (RTT 150 ms, 1,6 Mbps, CPU 4x) | Medido em laboratório, a partir de um datacenter fora do Brasil (Vercel respondeu de `iad1`). Os números de campo (CrUX) não existem para este domínio |
| Cookies, localStorage e hosts de terceiros | Playwright 390x844 | Medido |
| Preservação de UTM | Playwright: home com `?utm_*&gclid` → clique no CTA | Medido |
| INP | Não há INP em laboratório. Proxy: Event Timing com toque real e CPU 4x | **Estimado** |
| Causa do tempo de *rendering* (grain SVG + backdrop-filter) | Leitura do CSS | **Hipótese**, não perfilado camada a camada |
| `medlogicpro.com.br` | curl e WebFetch bloqueados pelo proxy/DNS do ambiente | **Não verificado** |
| Código-fonte (`layout.tsx`, `next.config`) | Fora deste repositório | **Não inspecionado**: as sugestões indicam o arquivo provável |

Arquivo bruto: `/home/user/simulador-transito-br/audit/lighthouse-mobile.json` (1ª execução mobile).

### Métricas Lighthouse
| Execução | Score perf | FCP | LCP | TBT | CLS | Speed Index | TTI |
|---|---|---|---|---|---|---|---|
| Mobile #1 | 74 | 3,1 s | 3,6 s | 420 ms | 0 | 3,2 s | 5,6 s |
| Mobile #2 | 74 | 3,0 s | 3,5 s | 450 ms | 0 | 3,1 s | — |
| Mobile #3 | 77 | 3,0 s | 3,5 s | 350 ms | 0 | 3,1 s | — |
| Desktop | 100 | 0,4 s | 0,8 s | 10 ms | 0 | 0,6 s | — |

- **Elemento LCP (mobile):** print do hero `1-hero-aciclovir-prescricao` (WebP 640w, 37 KB).
  - TTFB: 679 ms
  - Atraso até a requisição: 115 ms
  - **Download: 2.758 ms**
  - Render: 46 ms
- **Causa do download lento:** a imagem disputa banda com 5 fontes pré-carregadas (119 KiB) e 15 scripts (365 KiB) que começam ao mesmo tempo. O preload da imagem não tem `fetchpriority="high"` (o próprio Lighthouse aponta).
- **Peso total:** 633 KiB em 38 requisições.
  - JS: 365 KiB, 15 arquivos
  - Fontes: 119 KiB, 5 arquivos
  - Imagens: 74 KiB
  - CSS: 22 KiB
  - HTML: 23 KiB comprimido (183 KB descomprimido, porque inclui o payload RSC)
- **Main thread (CPU 4x): 6,9 s no total.**
  - Rendering/paint: 2,3 s
  - Style/Layout: 1,0 s
  - Script: 1,0 s
  - O chunk `1upe53-127sm4.js` gasta 805 ms de scripting e gera 4 long tasks (307, 141, 83, 62 ms)
- **INP estimado:** o pior toque foi 184 ms, em "SIM" na demo (troca de passo). FAQ: 48–80 ms. Fica abaixo do limite de 200 ms, mas no limite.
- Render-blocking: 2 CSS, economia estimada de 120 ms. Imagens maiores que o exibido: 39 KiB. Legacy JS (polyfills de `Array.prototype.at/flat`): 14 KiB.
- O Lighthouse também registrou 9 *prefetches* RSC de `/register` e `/login` durante a carga. Eles vêm do `<Link>` do Next e competem com a banda logo após o LCP.

## Achados
| ID | Seção | Severidade | Problema | Evidência | Sugestão concreta |
|----|-------|-----------|----------|-----------|-------------------|
| BRU-01 | Global / medição | Crítica | Não existe nenhuma medição. Não há analytics, pixel nem eventos de conversão, então é impossível saber quantos visitantes clicam em CTA, iniciam cadastro ou assinam. Isso bloqueia qualquer lançamento com mídia ou parcerias. | `page.html`: 0 ocorrências de `gtag`, `googletagmanager`, `dataLayer`, `fbq`, `_vercel/insights`, `posthog`, `plausible`, `clarity`. O Playwright registrou um único host de rede: `medlogic-three.vercel.app`. Nenhum cookie e localStorage vazio. | Instalar já `@vercel/analytics` e `@vercel/speed-insights` no `app/layout.tsx` (cookieless, sem banner obrigatório). Depois, GA4 via GTM com Consent Mode v2 (`analytics_storage: 'denied'` por padrão) e os eventos do plano abaixo. |
| BRU-02 | Todos os CTAs → `/register` | Alta | Os UTMs e o `gclid` se perdem no primeiro clique, e a escolha do plano também. Os 13 CTAs de assinatura/cadastro apontam para `/register` puro. A página de cadastro não menciona plano, preço nem trial. Quem clicou em "Assinar o anual" cai num formulário genérico. | Teste: abrir `/?utm_source=whatsapp&utm_medium=social&utm_campaign=teste&gclid=abc` e clicar em "Assinar o plano do rodízio" levou a `https://medlogic-three.vercel.app/register`, sem query, com `document.referrer` vazio. `/register` mostra só "Criar conta / Preencha seus dados para começar / Nome completo / E-mail / Senha". | Gerar os hrefs com o plano e repassar a query atual: `/register?plano=rodizio&origem=hero` (mensal/rodizio/anual/gratis). Guardar os `utm_*`/`gclid`/`fbclid` em `sessionStorage` na primeira visita (ou num cookie first-party de 30 dias, após consentimento) e enviá-los no POST de cadastro. Em `/register`, exibir "Plano do rodízio · R$ 199,50 a cada 6 meses · 7 dias de acesso completo" conforme o `?plano`. |
| BRU-03 | `<head>` / domínio | Alta | Não há canonical, a landing está em `*.vercel.app` e o próprio FAQ cita outro domínio. Se o definitivo for `medlogicpro.com.br`, o Google pode indexar a URL da Vercel, e a autoridade e os links compartilhados ficam no domínio errado. | `page.html`: 0 ocorrências de `canonical`. FAQ "Em quais aparelhos funciona?": "No navegador, em medlogicpro.com.br". Não foi possível checar `medlogicpro.com.br` (bloqueado no ambiente). | Definir o domínio oficial e apontá-lo para este projeto na Vercel, com redirect 308 de `medlogic-three.vercel.app` para o domínio oficial (Domains → Redirect). No `layout.tsx`: `metadataBase: new URL('https://medlogicpro.com.br')` e `alternates: { canonical: '/' }`. Se o app web ficar em `medlogicpro.com.br`, servir a landing na raiz e o app em `/app` ou `app.medlogicpro.com.br`. |
| BRU-04 | Compartilhamento (WhatsApp) | Alta | O preview no WhatsApp sai só com texto, sem imagem. Médico compartilha por WhatsApp, e um link sem miniatura perde clique. | Não há `og:image`, `og:url`, `og:site_name` nem `twitter:image`. `twitter:card` = `summary`. `/opengraph-image` → 404. | Criar `app/opengraph-image.png` (ou `.tsx` com `ImageResponse`) de **1200x630**, com menos de 300 KB (o WhatsApp ignora imagens grandes). Conteúdo: o print do aciclovir + "Peso da criança → prescrição pronta" + o logo. Adicionar `og:url`, `og:site_name`, `og:image:width/height/alt` e `twitter:card=summary_large_image` (bloco abaixo). |
| BRU-05 | Indexação | Alta | `robots.txt` e `sitemap.xml` não existem. As páginas de app (`/login`, `/register`, `/forgot-password`) são indexáveis e têm title/description genéricos, iguais entre si. | `/robots.txt` → 404, `/sitemap.xml` → 404. `/login` e `/register`: `<title>MedLogic Pro</title>`, description "Plataforma clínica para profissionais de saúde". | Criar `app/robots.ts` (allow `/`, disallow `/login`, `/register`, `/forgot-password`, `/app`, com sitemap) e `app/sitemap.ts` (`/` e `/privacidade`). Nas rotas de auth: `export const metadata = { robots: { index: false, follow: true } }`. Em `/privacidade`, dar uma description própria. |
| BRU-06 | Performance / LCP mobile | Média | LCP mobile de 3,5–3,6 s nas 3 execuções, acima da meta de 2,5 s. Desses, 2,76 s são o download de uma imagem de 37 KB que divide a banda com fontes e JS. | Lighthouse: `lcp-breakdown` resourceLoadDuration 2.758 ms. `lcp-discovery-insight`: "fetchpriority=high should be applied" = false. O preload da hero começa junto com 5 woff2 e 15 chunks JS. | No `<Image>` do hero, usar `priority` + `fetchPriority="high"` (Next 15+: `preload` + `fetchPriority="high"`). Ajustar `sizes="(min-width:1024px) 300px, 62vw"` e `quality={70}` (o Lighthouse aponta 18,7 KB de sobra nessa imagem). Reduzir os preloads de fonte (BRU-07). Meta: LCP ≤ 2,5 s no mesmo perfil. |
| BRU-07 | Performance / fontes | Média | São 4 famílias (Inter, Instrument Sans, Schibsted Grotesk, DM Mono) e 5 woff2 pré-carregados (119 KiB), um deles de 48 KiB. Todos competem com o LCP. | `<head>`: 5 `<link rel="preload" as="font">`. CSS: `font-family` Inter, Instrument Sans, Schibsted Grotesk e DM Mono, todas com `font-display:swap`. | Ficar com 2 famílias (texto + títulos). DM Mono só nos números do card: `preload: false`. Em `next/font`, `subsets: ['latin']` e só os pesos usados. Se a Inter vier do layout raiz do app e não for usada na landing, tirá-la dessa rota. |
| BRU-08 | Performance / main thread | Média | O trabalho de main thread é alto para uma landing estática: 6,9 s com CPU 4x, sendo 2,3 s de *rendering*. TBT de 350–450 ms, um chunk com 805 ms de scripting, INP estimado de 184 ms no toque da demo. | `mainthread-work-breakdown`: paintCompositeRender 2.282 ms. `bootup-time`: `1upe53-127sm4.js` 805 ms. CSS: textura `--grain` (SVG `feTurbulence` com `numOctaves='3'`) + `backdrop-filter: blur(8px)` no masthead fixo (a causa é hipótese). | Trocar a textura `--grain` por um PNG/WebP de 180x180 pré-renderizado, ou removê-la no mobile (`@media (max-width:768px)`). Testar o masthead sem `backdrop-filter`, usando fundo opaco a 96%. Carregar a demo interativa com `next/dynamic` só quando `#demonstracao` entra na viewport. Configurar `browserslist` moderno para cortar 14 KiB de polyfills. |
| BRU-09 | Links / fluxo | Média | A página promete "Disponível em iPhone, Android e web" 2 vezes e o FAQ cita os apps, mas não há link para App Store nem Google Play. "medlogicpro.com.br" aparece só como texto. Não há Termos de Uso, mesmo com cobrança recorrente. | Nenhum `href` contendo `apps.apple` ou `play.google`. `/termos` e `/termos-de-uso` → 404. O rodapé tem só Privacidade, Entrar e Criar conta. | Adicionar os badges oficiais (App Store / Google Play) ao lado de "Disponível em iPhone, Android e web", com UTMs de loja (`?pt=…&ct=landing` / `&referrer=utm_source%3Dlanding`). Transformar "medlogicpro.com.br" em link. Publicar `/termos` e linkar no rodapé e no cadastro. |
| BRU-10 | Dados estruturados | Média | Não há JSON-LD. A página perde a chance de rich results de FAQ e preço, e de deixar a entidade explícita para o Google. | `page.html`: 0 ocorrências de `application/ld+json`. | Inserir o JSON-LD sugerido abaixo (`Organization` + `WebSite` + `SoftwareApplication` com `Offer` + `FAQPage`) via `<script type="application/ld+json">` no `page.tsx`. Não usar `aggregateRating` sem avaliações reais. |
| BRU-11 | SEO on-page | Média | O texto não tem termos que um médico digita na busca. "calculadora" aparece 0 vezes, "prescrição pediátrica" 0 vezes e "dose pediátrica" só 1 vez. Os h2 começam com numeração ("1.Encontre…", "2.Veja…"), que vai junto para o snippet. | Contagem no texto indexável (1.592 palavras): calculadora 0, dose pediátrica 1, prescrição pediátrica 0, pelo peso 3, diluição 2. Headings extraídos: `h2 "1.Encontre o algoritmo pelo cenário do plantão"`. | Title: "Calculadora de dose pediátrica pelo peso, com prescrição pronta \| MedLogic Pro" (66 caracteres). Subtítulo do hero: "Calculadora de dose pediátrica com prescrição pronta pelo peso, volume a aspirar e diluição". Marcar o número do h2 como `aria-hidden` ou movê-lo para fora do h2. Criar depois uma página indexável por medicamento líder (ex.: "/doses/aciclovir": "Diluição de aciclovir em pediatria"), com o conteúdo de amostra que já existe. |
| BRU-12 | Cadastro (`/register`) | Média | O formulário não ajuda o preenchimento automático no celular e não tem link para privacidade nem para termos no momento da coleta de dados. | Playwright: inputs `["text","", …,"Dr. João Silva",null]`, `["email",…,null]`, `["password",…,"Mínimo 6 caracteres",null]`, sem `name` e sem `autocomplete`. O único link da página é "Entrar". | `autocomplete="name"`, `autocomplete="email"` e `inputmode="email"`, `autocomplete="new-password"`, além de `name` em cada campo. Abaixo do botão: "Ao criar a conta você concorda com os [Termos de Uso] e a [Política de Privacidade]". |
| BRU-13 | LGPD × medição | Média | A política de privacidade não cobre cookies, analytics nem o processador de pagamento por cartão. Hoje isso é coerente, porque não há cookie, mas fica desatualizado assim que o BRU-01 for feito. | `/privacidade` (28/07/2026): "Não utilizamos seus dados para publicidade". Não há menção a cookies, analytics nem pagamentos, e cita "SharedPreferences" (termo de app Android). Playwright: 0 cookies na landing. | Junto com o BRU-01, adicionar à política a seção "Cookies e medição de audiência" (ferramentas, finalidade, base legal, retenção) e citar o gateway de pagamento como operador. Banner de consentimento só se entrar GA4/Meta Pixel (Vercel Analytics dispensa). Alinhar com a persona de compliance. |
| BRU-14 | PWA / ícones | Baixa | Não há manifest nem `apple-touch-icon`. Quem salva a página na tela inicial do iPhone fica com um ícone genérico. O favicon é um `.ico` de 25 KB. | `/manifest.webmanifest`, `/manifest.json` e `/apple-touch-icon.png` → 404. `/favicon.ico` → 200, 25.931 B. | Criar `app/apple-icon.png` (180x180), `app/icon.png` (32x32 / 192x192) e `app/manifest.ts` com `name`, `short_name: "MedLogic"`, `theme_color` e `lang: "pt-BR"`. |
| BRU-15 | Imagens | Baixa | As capturas são servidas em 640w quando o mobile exibe cerca de 451 px. O Lighthouse estima 39 KiB de sobra. Os originais são PNG de 1170x2532. | `image-delivery-insight`: "640x1385 for displayed 412x892" (`5-lista-algoritmos`, 21 KB a mais) e "451x976" (hero, 18,7 KB a mais). | `sizes` mais preciso (`(min-width:1024px) 270px, 60vw`) e `images.formats: ['image/avif','image/webp']` no `next.config` (hoje serve só WebP, o que já é bom). |

## O que está funcionando (não mexer)
- **SSR completo:** todo o texto, inclusive as respostas do FAQ (`<details>` no HTML), está no HTML inicial. Nada de conteúdo preso em imagem: o card do hero (aciclovir, 200 mg/dose, 8 mL, 32 mL, 40 mL) é texto HTML.
- **Marcação básica:** `lang="pt-BR"`, `og:locale=pt_BR`, um único `h1` ("Coloque o peso da criança e copie a prescrição pronta.") e hierarquia h2 → h3 coerente por seção.
- **Acessibilidade e SEO de imagem:** os alts são descritivos e trazem os números do cálculo (ex.: "Dose de referência usada: 10 mg/kg/dose…"). A miniatura decorativa da Dra. Karla tem `alt=""`, como deve ser.
- **Imagens e layout:** `next/image` com WebP, `width`/`height` declarados, `loading="lazy"` fora da dobra e LCP sem lazy. **CLS = 0** em todas as execuções.
- **Entrega:** `font-display: swap`, HSTS com preload, cache de CDN da Vercel (`x-vercel-cache: HIT`, página pré-renderizada) e desktop com 100 de performance.
- **Âncoras e links:** todas as âncoras do menu existem (`#conteudo`, `#demonstracao`, `#planos`, `#perguntas`, `#quem-esta-por-tras`). Não há `href="#"`, 404 nem placeholder entre os links da página. O "Pular para o conteúdo" está implementado.
- **Title atual:** "MedLogic Pro | Dose pediátrica por peso e prescrição pronta", com 58 caracteres, já com termo de busca.

## Tabela de links
Checagem em 24/11/2026 ~20:12 UTC (curl e Playwright).

| Texto | Destino | Status |
|---|---|---|
| Pular para o conteúdo | `#conteudo` | âncora existe |
| Demonstração (menu) | `#demonstracao` | âncora existe |
| Planos (menu) | `#planos` | âncora existe |
| Perguntas (menu) | `#perguntas` | âncora existe |
| MedLogic Pro (logo) | `/` | 200 |
| Entrar (header) | `/login` | 200 |
| Assinar (header) | `/register` | 200 (sem plano e sem UTM) |
| Assinar o plano do rodízio (hero) | `/register` | 200 (sem plano e sem UTM) |
| Criar conta grátis (hero) | `/register` | 200 |
| Selo "Conteúdo coordenado pela Dra. Karla Araújo…" | `#quem-esta-por-tras` | âncora existe |
| Calcular para o meu paciente (demo) | `/register` | 200 |
| Criar conta grátis / Assinar o mensal / Assinar o plano do rodízio / Assinar o anual (planos, versão desktop e versão mobile, 8 links) | `/register` | 200 (todos idênticos) |
| Assinar o plano do rodízio (CTA final) | `/register` | 200 |
| Criar conta grátis (CTA final) | `/register` | 200 |
| Privacidade (rodapé) | `/privacidade` | 200 |
| Entrar (rodapé) | `/login` | 200 |
| Criar conta (rodapé) | `/register` | 200 |
| (em `/login`) Esqueceu sua senha? | `/forgot-password` | 200 |
| (em `/privacidade`) e-mail | `mailto:contato@opextecnologia.com.br` | não testável |
| "medlogicpro.com.br" (FAQ) | texto, não é link | domínio não verificado (bloqueado no ambiente) |
| App Store / Google Play | inexistentes | — |
| Termos de Uso | inexistente (`/termos` → 404) | — |
| `/robots.txt` · `/sitemap.xml` | — | 404 · 404 |

## Bloco `<head>` sugerido
O domínio abaixo é suposto: trocar pelo definitivo. No App Router, o ideal é gerar isto via `export const metadata` no `app/layout.tsx` / `app/page.tsx`. O HTML resultante deve ficar assim:

```html
<html lang="pt-BR">
<head>
  <title>Calculadora de dose pediátrica pelo peso, com prescrição pronta | MedLogic Pro</title>
  <meta name="description" content="Informe o peso da criança e copie a prescrição pronta: dose, volume a aspirar, diluição e intervalo. +50 algoritmos pediátricos, +120 medicamentos e bulário em português. 7 dias grátis.">
  <link rel="canonical" href="https://medlogicpro.com.br/">
  <meta name="robots" content="index, follow, max-image-preview:large">
  <meta name="theme-color" content="#0e1624">

  <meta property="og:type" content="website">
  <meta property="og:site_name" content="MedLogic Pro">
  <meta property="og:locale" content="pt_BR">
  <meta property="og:url" content="https://medlogicpro.com.br/">
  <meta property="og:title" content="MedLogic Pro: peso da criança → prescrição pronta">
  <meta property="og:description" content="Dose pediátrica pelo peso com volume a aspirar e diluição, ligada ao algoritmo e ao bulário. 7 dias de acesso completo.">
  <meta property="og:image" content="https://medlogicpro.com.br/og/medlogic-1200x630.jpg">
  <meta property="og:image:width" content="1200">
  <meta property="og:image:height" content="630">
  <meta property="og:image:alt" content="Tela do MedLogic Pro com prescrição de aciclovir para criança de 20 kg: 200 mg/dose, aspirar 8 mL, diluir em 32 mL.">

  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content="MedLogic Pro: peso da criança → prescrição pronta">
  <meta name="twitter:description" content="Dose pediátrica pelo peso com volume a aspirar e diluição. 7 dias de acesso completo.">
  <meta name="twitter:image" content="https://medlogicpro.com.br/og/medlogic-1200x630.jpg">

  <link rel="icon" href="/icon.png" type="image/png" sizes="32x32">
  <link rel="apple-touch-icon" href="/apple-icon.png">
  <link rel="manifest" href="/manifest.webmanifest">
</head>
```

Equivalente em Next.js:

```ts
export const metadata: Metadata = {
  metadataBase: new URL('https://medlogicpro.com.br'),
  title: 'Calculadora de dose pediátrica pelo peso, com prescrição pronta | MedLogic Pro',
  description: 'Informe o peso da criança e copie a prescrição pronta: dose, volume a aspirar, diluição e intervalo. +50 algoritmos pediátricos, +120 medicamentos e bulário em português. 7 dias grátis.',
  alternates: { canonical: '/' },
  openGraph: { type: 'website', siteName: 'MedLogic Pro', locale: 'pt_BR', url: '/',
    images: [{ url: '/og/medlogic-1200x630.jpg', width: 1200, height: 630, alt: '...' }] },
  twitter: { card: 'summary_large_image' },
};
```

Para a imagem OG no WhatsApp: JPG ou PNG, 1200x630, **com menos de 300 KB**, texto grande centralizado (o WhatsApp corta as bordas em miniatura quadrada) e URL absoluta em HTTPS.

## JSON-LD sugerido
Preços conforme a página. CNPJ conforme `/privacidade`. Revisar o domínio e as URLs das lojas.

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "Organization",
      "@id": "https://medlogicpro.com.br/#org",
      "name": "MedLogic Pro",
      "legalName": "Opex Consultoria Ltda",
      "taxID": "53.858.521/0001-40",
      "url": "https://medlogicpro.com.br/",
      "logo": "https://medlogicpro.com.br/icon.png",
      "email": "contato@opextecnologia.com.br"
    },
    {
      "@type": "WebSite",
      "@id": "https://medlogicpro.com.br/#site",
      "url": "https://medlogicpro.com.br/",
      "name": "MedLogic Pro",
      "inLanguage": "pt-BR",
      "publisher": { "@id": "https://medlogicpro.com.br/#org" }
    },
    {
      "@type": "SoftwareApplication",
      "@id": "https://medlogicpro.com.br/#app",
      "name": "MedLogic Pro",
      "applicationCategory": "MedicalApplication",
      "operatingSystem": "iOS, Android, Web",
      "inLanguage": "pt-BR",
      "description": "Calculadora de dose pediátrica pelo peso com prescrição pronta, volume a aspirar e diluição, ligada a +50 algoritmos pediátricos e ao bulário.",
      "audience": { "@type": "MedicalAudience", "audienceType": "Médicos e estudantes de medicina" },
      "publisher": { "@id": "https://medlogicpro.com.br/#org" },
      "offers": [
        { "@type": "Offer", "name": "Gratuito", "price": "0", "priceCurrency": "BRL" },
        { "@type": "Offer", "name": "Mensal", "price": "39.90", "priceCurrency": "BRL",
          "priceSpecification": { "@type": "UnitPriceSpecification", "price": "39.90", "priceCurrency": "BRL", "billingDuration": "P1M", "unitCode": "MON" } },
        { "@type": "Offer", "name": "Plano do rodízio", "price": "199.50", "priceCurrency": "BRL",
          "priceSpecification": { "@type": "UnitPriceSpecification", "price": "199.50", "priceCurrency": "BRL", "billingDuration": "P6M" } },
        { "@type": "Offer", "name": "Anual", "price": "399.00", "priceCurrency": "BRL",
          "priceSpecification": { "@type": "UnitPriceSpecification", "price": "399.00", "priceCurrency": "BRL", "billingDuration": "P1Y", "unitCode": "ANN" } }
      ]
    },
    {
      "@type": "FAQPage",
      "mainEntity": [
        { "@type": "Question", "name": "Posso testar antes de assinar?",
          "acceptedAnswer": { "@type": "Answer", "text": "Sim. Toda conta nova tem 7 dias de acesso completo e depois passa para o Gratuito." } },
        { "@type": "Question", "name": "Quem faz o conteúdo?",
          "acceptedAnswer": { "@type": "Answer", "text": "O conteúdo clínico é coordenado pela Dra. Karla Araújo, pediatra (CRM-PE 18734 · RQE 13315), e revisado pela equipe antes de cada publicação." } },
        { "@type": "Question", "name": "Preciso informar dados do paciente?",
          "acceptedAnswer": { "@type": "Answer", "text": "Não. Para calcular a dose você informa o peso." } },
        { "@type": "Question", "name": "Como cancelo?",
          "acceptedAnswer": { "@type": "Answer", "text": "Você cancela quando quiser, direto no site ou no app. Quem cancela passa para o Gratuito." } }
      ]
    }
  ]
}
</script>
```

O exemplo traz 4 perguntas. Incluir as 10 do FAQ com o texto **idêntico** ao visível. O Google limita rich results de FAQ a sites de saúde/governo com autoridade, então o ganho principal aqui é semântico.

## Plano de eventos de analytics (mínimo para o funil)
**Ferramentas recomendadas:**
- Vercel Web Analytics + Speed Insights desde o dia 1: sem cookies, dá INP/LCP reais de campo.
- GA4 via GTM com Consent Mode v2, se houver mídia paga (Google/Meta).
- A conversão de assinatura deve ser enviada pelo servidor (webhook do gateway → GA4 Measurement Protocol / Meta CAPI), não pelo navegador.

| # | Evento | Quando dispara | Parâmetros |
|---|---|---|---|
| 1 | `page_view` | Carga da landing | `utm_source`, `utm_medium`, `utm_campaign`, `utm_content`, `gclid`/`fbclid`, `referrer` |
| 2 | `cta_click` | Clique em qualquer CTA de cadastro/assinatura | `cta_id` (`hero_rodizio`, `hero_gratis`, `planos_mensal`, `planos_rodizio`, `planos_anual`, `planos_gratis`, `demo_calcular`, `final_rodizio`, `final_gratis`, `header_assinar`, `footer_criar`), `plano`, `secao` |
| 3 | `demo_step` | Avanço na demo (passo 1→2→3) e `demo_complete` | `step` (1, 2, 3) |
| 4 | `faq_open` | Abertura de `<details>` do FAQ | `pergunta` |
| 5 | `scroll_section` | Seção entrou 50% na viewport (`#planos`, `#perguntas`) | `secao` |
| 6 | `app_store_click` | Clique nos badges (após o BRU-09) | `loja` (`ios`, `android`) |
| 7 | `sign_up_start` | Primeiro foco num campo de `/register` | `plano`, UTMs persistidos |
| 8 | `sign_up` | Conta criada (servidor) | `method: email`, `plano`, UTMs, `user_id` (hash) |
| 9 | `trial_start` | Início dos 7 dias de acesso completo | `plano_intencao` |
| 10 | `begin_checkout` | Abertura do pagamento | `plano`, `value`, `currency: BRL` |
| 11 | `purchase` | Pagamento confirmado (webhook do gateway) | `transaction_id`, `plano`, `value` (39.90 / 199.50 / 399.00), `currency: BRL` |
| 12 | `trial_to_paid` / `subscription_cancel` | Lado servidor | `plano`, `dias_desde_cadastro` |

**Funil a acompanhar:** `page_view` → `cta_click` → `sign_up_start` → `sign_up` → `begin_checkout` → `purchase`. Dois cortes principais:
- `utm_source` (WhatsApp, Instagram, indicação de preceptor)
- `cta_id` (qual seção converte)

**Requisito técnico:** os UTMs precisam sobreviver de `page_view` até `sign_up`. Hoje eles se perdem no primeiro clique (BRU-02).

**LGPD:**
- Vercel Analytics não grava cookie e pode rodar sem banner, com menção na política.
- GA4 ou Meta Pixel exigem banner com opção "Recusar" de mesmo peso e Consent Mode com `denied` por padrão.
- Não enviar peso, medicamento consultado nem qualquer dado clínico como parâmetro de evento.

## Perguntas que ficaram sem resposta
- O domínio definitivo é `medlogicpro.com.br`? O que ele serve hoje: o app, outra landing, nada? Não deu para verificar daqui.
- Depois de `/register`, como o usuário chega ao pagamento do plano que escolheu? O fluxo não foi testado, porque não criei conta.
- Qual gateway de pagamento é usado? Ele aceita webhook para conversão server-side?
- Onde estão os apps nas lojas? Quais são as URLs?
- Existe Search Console ou alguma propriedade de analytics configurada fora do código?

## Veredito
Criaria conta grátis, porque o produto parece sólido e a página técnica tem boa base. Mas não liberaria o lançamento com tráfego pago ou parceria antes de três itens:
- Medição + persistência de UTM/plano (BRU-01/02)
- Domínio definitivo com canonical, robots e sitemap (BRU-03/05)
- `og:image` para o WhatsApp (BRU-04)

Juntos são cerca de 1–2 dias de trabalho em Next.js. Performance mobile (LCP 3,5 s → ≤ 2,5 s) vem logo em seguida.

## Limitações registradas
- Lighthouse rodou com throttling *devtools* a partir de servidor fora do Brasil. É um valor de laboratório, não de campo. O TTFB mobile de ~560–680 ms inclui a latência simulada (562 ms por requisição).
- INP é estimado (Event Timing com toque sintético e CPU 4x). Não há dados CrUX para o domínio.
- `medlogicpro.com.br` e as lojas de app ficaram inacessíveis (proxy/DNS do ambiente).
- O código-fonte não está neste repositório: os arquivos citados (`app/layout.tsx`, `app/robots.ts`, `next.config`) são os caminhos convencionais do App Router.
- Fluxo pós-cadastro e checkout não testados.
