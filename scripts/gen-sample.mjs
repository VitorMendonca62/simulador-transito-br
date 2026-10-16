// Gera samples/sample.osm: malha sintética (coordenadas reais de SP) para demonstrar
// semáforos, rotatória, faixa de ônibus, turn:lanes, restrição de conversão e travessias.
import { writeFileSync } from 'node:fs'
import { encodePbf } from './pbf-writer.mjs'

const LAT0 = -23.5614
const LON0 = -46.6559
const M = 111319.49
const nodes = new Map() // key -> {id, lat, lon, tags}
const ways = []
const rels = []
let nid = 1000
let wid = 5000

const node = (x, y, tags = {}) => {
  const key = `${x.toFixed(2)},${y.toFixed(2)}`
  let n = nodes.get(key)
  if (!n) {
    n = { id: nid++, lat: LAT0 + y / M, lon: LON0 + x / (M * Math.cos((LAT0 * Math.PI) / 180)), tags: {} }
    nodes.set(key, n)
  }
  Object.assign(n.tags, tags)
  return n
}
const way = (tags, pts) => {
  const w = { id: wid++, tags, refs: pts.map(([x, y]) => node(x, y).id) }
  ways.push(w)
  return w
}
const tagNode = (x, y, tags) => node(x, y, tags)

// Avenida Brasil (primary, 2+2) dividida em x=-200 para a restrição
const avW = way({ highway: 'primary', name: 'Avenida Brasil', lanes: '4' }, [[-400, 0], [-300, 0], [-200, 0]])
way({ highway: 'primary', name: 'Avenida Brasil', lanes: '4' }, [[-200, 0], [-100, 0], [0, 0], [100, 0], [200, 0], [300, 0], [400, 0]])

// Rua das Flores (residential 1+1) com T em (-200,-100)
const floresN = way({ highway: 'residential', name: 'Rua das Flores' }, [[-200, 0], [-200, 150], [-200, 300]])
way({ highway: 'residential', name: 'Rua das Flores' }, [[-200, -300], [-200, -100], [-200, 0]])
way({ highway: 'residential', name: 'Rua do Parque' }, [[-400, -100], [-300, -100], [-200, -100]])

// Rua Central (secondary): 3 faixas para norte (turn:lanes) + 2 para sul, faixa de ônibus à direita
way(
  { highway: 'secondary', name: 'Rua Central', 'lanes:forward': '3', 'lanes:backward': '2', lanes: '5',
    'turn:lanes:forward': 'left|through|through;right', 'busway:right': 'lane' },
  [[0, -300], [0, 0]],
)
way({ highway: 'secondary', name: 'Rua Central', lanes: '4', 'busway:right': 'lane' }, [[0, 0], [0, 150], [0, 300]])

// Rua do Comércio (tertiary) passa pela rotatória e cruza a avenida
way({ highway: 'tertiary', name: 'Rua do Comércio' }, [[200, -300], [200, -168]])
way({ highway: 'tertiary', name: 'Rua do Comércio' }, [[200, -132], [200, 0], [200, 150], [200, 300]])
way({ highway: 'residential', name: 'Rua das Palmeiras' }, [[100, -150], [182, -150]])
way({ highway: 'residential', name: 'Rua das Palmeiras' }, [[218, -150], [320, -150]])

// Rotatória (anti-horária, mão direita)
const R = 18
const ring = []
for (let i = 0; i < 12; i++) {
  const a = (i * Math.PI * 2) / 12
  ring.push([200 + R * Math.cos(a), -150 + R * Math.sin(a)])
}
ring.push(ring[0])
way({ highway: 'tertiary', junction: 'roundabout', name: 'Praça da Rotatória', oneway: 'yes' }, ring)

// Rua de sentido único com 2 faixas
way({ highway: 'secondary', name: 'Rua Sentido Único', oneway: 'yes', lanes: '2' },
  [[-400, 150], [-200, 150], [0, 150], [200, 150], [400, 150]])

// Rua curva ligando à Sentido Único
const curve = []
for (let i = 0; i <= 10; i++) {
  const t = (i / 10) * (Math.PI / 2)
  curve.push([-400 + 160 * (1 - Math.cos(t)), 150 + 130 * Math.sin(t)])
}
way({ highway: 'residential', name: 'Rua Curva' }, [[-400, 150], ...curve.slice(1)])

// Semáforos e travessias
for (const [x, y] of [[0, 0], [200, 0], [0, 150]]) tagNode(x, y, { highway: 'traffic_signals' })
tagNode(100, 0, { highway: 'crossing', crossing: 'zebra' })
tagNode(-100, 0, { highway: 'crossing', crossing: 'zebra' })

// Restrição: vindo da Avenida (oeste) em (-200,0) é proibido virar à esquerda para Rua das Flores (norte)
rels.push({ id: 9000, tags: { type: 'restriction', restriction: 'no_left_turn' },
  members: [['way', avW.id, 'from'], ['node', node(-200, 0).id, 'via'], ['way', floresN.id, 'to']] })

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;')
const tg = (t, ind) => Object.entries(t).map(([k, v]) => `${ind}<tag k="${esc(k)}" v="${esc(v)}"/>`).join('\n')
let out = `<?xml version="1.0" encoding="UTF-8"?>\n<osm version="0.6" generator="gen-sample">\n`
for (const n of nodes.values()) {
  const t = tg(n.tags, '   ')
  out += t ? ` <node id="${n.id}" lat="${n.lat.toFixed(7)}" lon="${n.lon.toFixed(7)}">\n${t}\n </node>\n` : ` <node id="${n.id}" lat="${n.lat.toFixed(7)}" lon="${n.lon.toFixed(7)}"/>\n`
}
for (const w of ways) out += ` <way id="${w.id}">\n${w.refs.map((r) => `  <nd ref="${r}"/>`).join('\n')}\n${tg(w.tags, '  ')}\n </way>\n`
for (const r of rels) out += ` <relation id="${r.id}">\n${r.members.map((m) => `  <member type="${m[0]}" ref="${m[1]}" role="${m[2]}"/>`).join('\n')}\n${tg(r.tags, '  ')}\n </relation>\n`
out += '</osm>\n'
writeFileSync(new URL('../samples/sample.osm', import.meta.url), out)
console.log(`samples/sample.osm: ${nodes.size} nós, ${ways.length} vias`)

writeFileSync(new URL('../samples/sample.pbf', import.meta.url), encodePbf({ nodes: [...nodes.values()], ways, rels }))
console.log('samples/sample.pbf gerado')
