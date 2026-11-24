// Passo da simulação em WASM fora do navegador (sem disputa com a tela), no cenário de referência da meta de
// desempenho (docs/AUDITORIA_DESEMPENHO_2026-11.md). As rotas rodam no próprio passo, como no `perf_sim` nativo.
//
//   wasm-pack build core --target nodejs --release --out-dir <pasta>
//   gunzip -c public/data/mapa-recife.osm.gz > recife.osm
//   node scripts/perf/bench-wasm.cjs <pasta do pkg> recife.osm simulacao.json [minutos]
//
// simulacao.json: "Baixar JSON da simulação" no painel de depuração, com o Recife aberto (OD 2018 + CTTU + linhas).
// Fixe o núcleo (taskset -c 2) e compare médias de várias rodadas: a variação entre rodadas passa de 10 %.
const fs = require('fs')
const path = require('path')
const [pkg, osm, simJson, minArg] = process.argv.slice(2)
if (!pkg || !osm || !simJson) {
  console.error('uso: node bench-wasm.cjs <pkg nodejs> <recife.osm> <simulacao.json> [minutos]')
  process.exit(1)
}
const core = require(path.resolve(pkg, 'core.js'))
const data = path.join(__dirname, '../../public/data/')
const mins = Number(minArg || 20)

let t = performance.now()
const net = core.import_osm_full(fs.readFileSync(osm), fs.readFileSync(data + 'semaforos_cttu.json', 'utf8'), fs.readFileSync(data + 'onibus_recife.json', 'utf8'))
console.log(`import_osm_full ${(performance.now() - t).toFixed(0)} ms`)
t = performance.now()
net.sim_init(fs.readFileSync(simJson, 'utf8'))
console.log(`sim_init ${(performance.now() - t).toFixed(0)} ms`)

const rows = []
for (let m = 1; m <= mins; m++) {
  let t0 = performance.now()
  let worst = 0
  for (let k = 0; k < 300; k++) {
    const a = performance.now()
    net.sim_step(1)
    worst = Math.max(worst, performance.now() - a)
  }
  const step = (performance.now() - t0) / 300
  t0 = performance.now()
  for (let k = 0; k < 10; k++) net.sim_snapshot()
  const snap = (performance.now() - t0) / 10
  const alive = JSON.parse(net.sim_stats()).alive
  rows.push({ step, worst, snap, alive })
  console.log(`min ${String(m).padStart(2)}: vivos ${String(alive).padStart(6)}  passo ${step.toFixed(2)} ms  pior ${worst.toFixed(1)} ms  snapshot ${snap.toFixed(2)} ms`)
}
const tail = rows.slice(Math.floor(rows.length / 2))
const avg = (k) => tail.reduce((a, r) => a + r[k], 0) / tail.length
console.log(`2ª metade: passo médio ${avg('step').toFixed(2)} ms, pior ${Math.max(...tail.map((r) => r.worst)).toFixed(1)} ms, ${avg('alive').toFixed(0)} veículos`)
