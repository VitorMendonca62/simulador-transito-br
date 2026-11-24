// Etapa 4 da auditoria de desempenho (docs/AUDITORIA_DESEMPENHO_2026-11.md, meta M8): fluidez do mapa com a simulação
// rodando. Abre o Recife (OD 2018 + CTTU + linhas), inicia a 30×, espera a rede encher e mede quatro fases de ~10 s:
// parado, arrastando, dando zoom e arrastando com a cidade inteira na tela.
//
// Precisa de uma máquina com placa de vídeo (num contêiner sem GPU o WebGL roda por software e os números não valem):
//
//   npm run dev                                    (em outro terminal)
//   npm i --no-save playwright && npx playwright install chromium
//   node scripts/perf/bench-desenho.mjs [url] [segundos de espera] [velocidade]
//
// O navegador abre visível (HEADLESS=1 esconde) e não deve ficar atrás de outra janela: navegadores reduzem o ritmo de
// quadros de janelas cobertas. CHROME=1 usa o Chrome instalado em vez do Chromium do Playwright.
// Meta: ≥ 55 fps e p95 do quadro ≤ 25 ms ao arrastar e ao dar zoom.
import fs from 'node:fs'
import { execSync } from 'node:child_process'

const { chromium } = await import('playwright').catch(() => import(`${execSync('npm root -g').toString().trim()}/playwright/index.mjs`))
const [url = 'http://localhost:5173/', waitArg = '60', speed = '30', out = 'bench-desenho.json'] = process.argv.slice(2)
const PHASE_MS = 10_000

const browser = await chromium.launch({
  headless: !!process.env.HEADLESS,
  channel: process.env.CHROME ? 'chrome' : undefined,
  args: ['--enable-precise-memory-info', '--disable-renderer-backgrounding', '--disable-background-timer-throttling'],
})
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } })
// sem o tour do primeiro acesso por cima do mapa
await page.addInitScript(() => localStorage.setItem('tour-done-v1', '1'))
await page.goto(url)
await page.waitForTimeout(1500)

const gl = await page.evaluate(() => {
  const c = document.createElement('canvas').getContext('webgl2')
  const ext = c?.getExtension('WEBGL_debug_renderer_info')
  return ext ? c.getParameter(ext.UNMASKED_RENDERER_WEBGL) : 'desconhecida'
})
console.log(`placa de vídeo (WebGL): ${gl}`)
if (/swiftshader|llvmpipe|software/i.test(gl)) console.log('ATENÇÃO: desenho por software; os números abaixo não representam uma máquina com GPU.')

// intervalos entre quadros (rAF) e tarefas longas
await page.evaluate(() => {
  const w = window
  w.__frames = []
  w.__long = []
  let last = performance.now()
  const loop = (t) => {
    w.__frames.push(t - last)
    last = t
    requestAnimationFrame(loop)
  }
  requestAnimationFrame(loop)
  try {
    new PerformanceObserver((l) => l.getEntries().forEach((e) => w.__long.push(e.duration))).observe({ type: 'longtask' })
  } catch {
    /* sem a API (Firefox, Safari) */
  }
})

await page.evaluate(async () => {
  const { useStore } = await import('/src/model/store.ts')
  await useStore.getState().loadRecife({ zones: true, signals: true, lines: true })
})
await page.evaluate(async (speed) => {
  const { useStore } = await import('/src/model/store.ts')
  const { perf } = await import('/src/engine/perf.ts')
  useStore.getState().simSpeed(Number(speed))
  await useStore.getState().simStart()
  Object.assign(window, { __perf: perf, __store: useStore })
}, speed)
console.log(`simulação a ${speed}×; esperando ${waitArg} s para a rede encher…`)
await page.waitForTimeout(Number(waitArg) * 1000)

const box = await page.locator('canvas').first().boundingBox()
if (!box) throw new Error('canvas do mapa não encontrado')
const cx = box.x + box.width / 2
const cy = box.y + box.height / 2

const snap = () =>
  page.evaluate(() => {
    const p = window.__perf
    return {
      t: performance.now(),
      renders: p.renders,
      ui: p.uiMs,
      gpu: p.gpuMs,
      gpuOk: p.gpuSupported,
      simTime: window.__store.getState().sim.stats?.time ?? 0,
      alive: window.__store.getState().sim.stats?.alive ?? 0,
      frames: window.__frames.splice(0),
      long: window.__long.splice(0),
    }
  })

const pct = (a, q) => (a.length ? a[Math.min(a.length - 1, Math.floor(q * a.length))] : 0)

async function phase(name, act) {
  await snap() // zera os acumuladores de quadros
  const a = await snap()
  const t0 = Date.now()
  let k = 0
  while (Date.now() - t0 < PHASE_MS) {
    await act(k++)
  }
  const b = await snap()
  const dt = (b.t - a.t) / 1000
  const frames = b.frames.sort((x, y) => x - y)
  const renders = b.renders - a.renders
  const r = {
    fase: name,
    fps: frames.length / dt,
    p50: pct(frames, 0.5),
    p95: pct(frames, 0.95),
    p99: pct(frames, 0.99),
    acima33: frames.filter((f) => f > 33.4).length / Math.max(1, frames.length),
    desenhos: renders / dt,
    jsMsPorDesenho: renders ? (b.ui - a.ui) / renders : 0,
    gpuMsPorDesenho: b.gpuOk && renders ? (b.gpu - a.gpu) / renders : null,
    tarefasLongasMsPorS: b.long.reduce((x, y) => x + y, 0) / dt,
    ritmo: (b.simTime - a.simTime) / dt,
    veiculos: b.alive,
  }
  console.log(
    `${name.padEnd(22)} ${r.fps.toFixed(0).padStart(4)} fps  quadro p50 ${r.p50.toFixed(1)} / p95 ${r.p95.toFixed(1)} / p99 ${r.p99.toFixed(1)} ms  ` +
      `>33 ms ${(100 * r.acima33).toFixed(1)}%  desenhos ${r.desenhos.toFixed(0)}/s  JS ${r.jsMsPorDesenho.toFixed(1)} ms  ` +
      `GPU ${r.gpuMsPorDesenho == null ? 'n/d' : r.gpuMsPorDesenho.toFixed(1) + ' ms'}  tarefas longas ${r.tarefasLongasMsPorS.toFixed(0)} ms/s  ritmo ${r.ritmo.toFixed(1)}×`,
  )
  return r
}

// arrasto em círculo, ~60 eventos por segundo
async function dragCircle(radius) {
  await page.mouse.move(cx + radius, cy)
  await page.mouse.down()
  const t0 = Date.now()
  while (Date.now() - t0 < PHASE_MS) {
    const a = ((Date.now() - t0) / 2000) * 2 * Math.PI
    await page.mouse.move(cx + radius * Math.cos(a), cy + radius * Math.sin(a))
    await page.waitForTimeout(16)
  }
  await page.mouse.up()
}

const res = []
res.push(await phase('parado', () => page.waitForTimeout(250)))
res.push(
  await phase('arrastando', async () => {
    await dragCircle(250)
  }),
)
res.push(
  await phase('zoom', async (k) => {
    await page.mouse.move(cx, cy)
    // 20 passos para dentro, 20 para fora
    await page.mouse.wheel(0, Math.floor(k / 20) % 2 ? 120 : -120)
    await page.waitForTimeout(50)
  }),
)
// cidade inteira na tela: bastante zoom para fora, depois arrasta
await page.mouse.move(cx, cy)
for (let i = 0; i < 30; i++) {
  await page.mouse.wheel(0, 240)
  await page.waitForTimeout(30)
}
await page.waitForTimeout(1000)
res.push(
  await phase('arrastando afastado', async () => {
    await dragCircle(150)
  }),
)

const ok = res.filter((r) => r.fase !== 'parado').every((r) => r.fps >= 55 && r.p95 <= 25)
console.log(`\nmeta M8 (≥ 55 fps e p95 ≤ 25 ms ao arrastar e no zoom): ${ok ? 'BATIDA' : 'NÃO batida'}`)
fs.writeFileSync(out, JSON.stringify({ gl, speed: Number(speed), res }, null, 1))
console.log(`detalhes em ${out}`)
await browser.close()
