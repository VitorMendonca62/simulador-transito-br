// Mede a simulação no navegador com o cenário de referência da meta de desempenho (docs/AUDITORIA_DESEMPENHO_2026-11.md):
// Recife com OD 2018 + semáforos da CTTU + linhas de ônibus, parâmetros padrão, velocidade pedida 30×.
//
//   npm run dev                       (em outro terminal; o roteiro importa os módulos do Vite: /src/...)
//   node scripts/perf/bench-browser.mjs [url] [segundos] [velocidade] [escala OD] [saida.json]
//
// Precisa do Playwright (global ou no projeto). PROF=n grava um perfil da thread principal do segundo n ao n+10.
// Resumo no fim: ritmo (s simulados por s real), ms por passo no worker, pior passo, tarefas longas da página.
import fs from 'node:fs'
import { execSync } from 'node:child_process'

const { chromium } = await import('playwright').catch(() => import(`${execSync('npm root -g').toString().trim()}/playwright/index.mjs`))
const [url = 'http://localhost:5173/', secs = '120', speed = '30', scale = '1', out = 'bench-browser.json'] = process.argv.slice(2)
const PROF = Number(process.env.PROF || 0)

const browser = await chromium.launch({ args: ['--enable-precise-memory-info', '--enable-unsafe-swiftshader'] })
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } })
page.on('console', (m) => /passo lento|erro|error/i.test(m.text()) && console.log('[console]', m.text().slice(0, 200)))
await page.goto(url)
await page.waitForTimeout(1500)

// intervalos entre quadros (rAF) e tarefas longas da thread principal
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
  new PerformanceObserver((l) => l.getEntries().forEach((e) => w.__long.push(e.duration))).observe({ type: 'longtask' })
})

const open = await page.evaluate(async (scale) => {
  const { useStore } = await import('/src/model/store.ts')
  const t = performance.now()
  await useStore.getState().loadRecife({ zones: true, signals: true, lines: true })
  const ms = performance.now() - t
  if (Number(scale) !== 1) useStore.getState().setOdScale(Number(scale))
  return { ms, zones: useStore.getState().zones.length, error: useStore.getState().error }
}, scale)
console.log(`abrir o Recife: ${open.ms.toFixed(0)} ms (${open.zones} zonas)${open.error ? ' ERRO ' + open.error : ''}`)
await page.waitForTimeout(2000)

const start = await page.evaluate(async (speed) => {
  const { useStore } = await import('/src/model/store.ts')
  const { perf } = await import('/src/engine/perf.ts')
  useStore.getState().simSpeed(Number(speed))
  const t = performance.now()
  await useStore.getState().simStart()
  Object.assign(window, { __perf: perf, __store: useStore })
  window.__long.length = window.__frames.length = 0
  return { ms: performance.now() - t, error: useStore.getState().error }
}, speed)
console.log(`iniciar a simulação: ${start.ms.toFixed(0)} ms${start.error ? ' ERRO ' + start.error : ''}`)

let cdp = null
if (PROF) {
  cdp = await page.context().newCDPSession(page)
  await cdp.send('Profiler.enable')
  await cdp.send('Profiler.setSamplingInterval', { interval: 200 })
}

const read = () =>
  page.evaluate(() => {
    const p = window.__perf
    const st = window.__store.getState().sim.stats
    const fr = window.__frames.splice(0)
    const lg = window.__long.splice(0)
    return {
      t: performance.now(),
      busy: p.simBusyMs,
      busyAt: p.simBusyAt ?? 0,
      ui: p.uiMs,
      renders: p.renders,
      wasmMB: p.wasmBytes / 1048576,
      heapMB: (performance.memory?.usedJSHeapSize ?? 0) / 1048576,
      stepMax: p.sim?.stepMax ?? 0,
      simTime: st?.time ?? 0,
      alive: st?.alive ?? 0,
      raf: fr.length,
      longMs: lg.reduce((a, b) => a + b, 0),
    }
  })

const samples = []
let prev = await read()
for (let i = 0; i < Number(secs); i++) {
  if (PROF && i === PROF) await cdp.send('Profiler.start')
  await page.waitForTimeout(1000)
  if (PROF && i === PROF + 10) {
    const { profile } = await cdp.send('Profiler.stop')
    fs.writeFileSync(out.replace(/\.json$/, '') + '.cpuprofile', JSON.stringify(profile))
    const self = new Map()
    const byId = new Map(profile.nodes.map((n) => [n.id, n]))
    profile.samples.forEach((id, k) => {
      const f = byId.get(id).callFrame
      const key = `${f.functionName || '(anônima)'} ${f.url.split('/').pop().split('?')[0]}:${f.lineNumber}`
      self.set(key, (self.get(key) || 0) + (profile.timeDeltas[k] || 0) / 1000)
    })
    const tot = [...self.values()].reduce((a, b) => a + b, 0)
    console.log(`--- thread principal, 10 s (tempo próprio, ms; total ${tot.toFixed(0)}) ---`)
    for (const [k, v] of [...self].sort((a, b) => b[1] - a[1]).slice(0, 20)) console.log(v.toFixed(0).padStart(7), `${((100 * v) / tot).toFixed(1)}%`.padStart(6), k)
  }
  const s = await read()
  const dt = s.t - prev.t
  const row = {
    i,
    simTime: s.simTime,
    alive: s.alive,
    pace: ((s.simTime - prev.simTime) * 1000) / dt,
    // pelo relógio do worker (a mensagem dele chega fora de fase com esta amostragem)
    simPct: s.busyAt > prev.busyAt ? ((s.busy - prev.busy) / (s.busyAt - prev.busyAt)) * 100 : (samples.at(-1)?.simPct ?? 0),
    busyMs: s.busy - prev.busy,
    busyTot: s.busy,
    busyAtTot: s.busyAt,
    uiPct: ((s.ui - prev.ui) / dt) * 100,
    fps: ((s.renders - prev.renders) * 1000) / dt,
    raf: s.raf,
    longMs: s.longMs,
    stepMax: s.stepMax,
    wasmMB: s.wasmMB,
    heapMB: s.heapMB,
  }
  samples.push(row)
  if (i % 10 === 0)
    console.log(
      `t=${String(i).padStart(3)} s  sim ${row.simTime.toFixed(0)} s  vivos ${row.alive}  ritmo ${row.pace.toFixed(1)}×  worker ${row.simPct.toFixed(0)}%  pior passo ${row.stepMax.toFixed(1)} ms  rAF ${row.raf}/s  tarefas longas ${row.longMs.toFixed(0)} ms/s  WASM ${row.wasmMB.toFixed(0)} MB`,
    )
  prev = s
}

// resumo da 2ª metade (rede já cheia)
const tail = samples.slice(Math.floor(samples.length / 2))
const sum = (k) => tail.reduce((a, x) => a + x[k], 0)
const steps = (tail.reduce((a, x, k) => a + (k ? x.simTime - tail[k - 1].simTime : 0), 0) / 0.2) || 1
const res = {
  abrirMs: open.ms,
  iniciarMs: start.ms,
  vivos: sum('alive') / tail.length,
  ritmo: sum('pace') / tail.length,
  msPorPasso: tail.slice(1).reduce((a, x) => a + x.busyMs, 0) / steps,
  piorPasso: Math.max(...tail.map((x) => x.stepMax)),
  workerPct: ((tail.at(-1).busyTot - tail[0].busyTot) / (tail.at(-1).busyAtTot - tail[0].busyAtTot)) * 100,
  rafPorS: sum('raf') / tail.length,
  tarefasLongasMsPorS: sum('longMs') / tail.length,
  wasmMB: samples.at(-1).wasmMB,
}
console.log('resumo (2ª metade):', Object.fromEntries(Object.entries(res).map(([k, v]) => [k, Math.round(v * 100) / 100])))
fs.writeFileSync(out, JSON.stringify({ open, start, res, samples }, null, 1))
await browser.close()
