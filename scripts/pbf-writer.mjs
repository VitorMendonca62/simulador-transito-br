import { deflateSync } from 'node:zlib'

// Escritor mínimo de .osm.pbf (DenseNodes + ways + relations; blob de dados com zlib, como nos arquivos reais).
// Usado só para gerar o exemplo e validar o caminho PBF do núcleo.

const varint = (n) => {
  let v = BigInt(n)
  if (v < 0n) v += 1n << 64n
  const out = []
  while (v > 0x7fn) {
    out.push(Number(v & 0x7fn) | 0x80)
    v >>= 7n
  }
  out.push(Number(v))
  return Buffer.from(out)
}
const zz = (n) => {
  const v = BigInt(n)
  return v >= 0n ? v << 1n : ((-v) << 1n) - 1n
}
const tag = (field, wire) => varint((field << 3) | wire)
const lenField = (field, buf) => Buffer.concat([tag(field, 2), varint(buf.length), buf])
const intField = (field, n) => Buffer.concat([tag(field, 0), varint(n)])
const packed = (field, nums, signed = false) =>
  lenField(field, Buffer.concat(nums.map((n) => varint(signed ? zz(n) : n))))

function frame(type, body, compress = false) {
  const blob = compress
    ? Buffer.concat([intField(2, body.length), lenField(3, deflateSync(body))])
    : Buffer.concat([lenField(1, body), intField(2, body.length)])
  const header = Buffer.concat([lenField(1, Buffer.from(type)), intField(3, blob.length)])
  const len = Buffer.alloc(4)
  len.writeUInt32BE(header.length)
  return Buffer.concat([len, header, blob])
}

export function encodePbf({ nodes, ways, rels }) {
  const strings = ['']
  const sid = (s) => {
    let i = strings.indexOf(String(s))
    if (i < 0) i = strings.push(String(s)) - 1
    return i
  }
  const E7 = (deg) => Math.round(deg * 1e7)

  // DenseNodes (com delta)
  let pid = 0, plat = 0, plon = 0
  const ids = [], lats = [], lons = [], kv = []
  for (const n of nodes) {
    ids.push(n.id - pid); pid = n.id
    const la = E7(n.lat), lo = E7(n.lon)
    lats.push(la - plat); plat = la
    lons.push(lo - plon); plon = lo
    for (const [k, v] of Object.entries(n.tags)) kv.push(sid(k), sid(v))
    kv.push(0)
  }
  const dense = Buffer.concat([packed(1, ids, true), packed(8, lats, true), packed(9, lons, true), packed(10, kv)])
  const nodeGroup = lenField(2, dense)

  const wayMsgs = ways.map((w) => {
    const keys = Object.keys(w.tags)
    let prev = 0
    const refs = w.refs.map((r) => { const d = r - prev; prev = r; return d })
    return lenField(3, Buffer.concat([
      intField(1, w.id),
      packed(2, keys.map(sid)),
      packed(3, keys.map((k) => sid(w.tags[k]))),
      packed(8, refs, true),
    ]))
  })
  const relMsgs = rels.map((r) => {
    const keys = Object.keys(r.tags)
    let prev = 0
    const mem = r.members.map((m) => { const d = m[1] - prev; prev = m[1]; return d })
    return lenField(4, Buffer.concat([
      intField(1, r.id),
      packed(2, keys.map(sid)),
      packed(3, keys.map((k) => sid(r.tags[k]))),
      packed(8, r.members.map((m) => sid(m[2]))),
      packed(9, mem, true),
      packed(10, r.members.map((m) => ({ node: 0, way: 1, relation: 2 })[m[0]])),
    ]))
  })

  const groups = [nodeGroup, Buffer.concat([...wayMsgs, ...relMsgs])]
  const table = Buffer.concat(strings.map((s) => lenField(1, Buffer.from(s))))
  const block = Buffer.concat([lenField(1, table), ...groups.map((g) => lenField(2, g))])
  const headerBlock = Buffer.concat([lenField(4, Buffer.from('OsmSchema-V0.6')), lenField(4, Buffer.from('DenseNodes'))])
  return Buffer.concat([frame('OSMHeader', headerBlock), frame('OSMData', block, true)])
}
