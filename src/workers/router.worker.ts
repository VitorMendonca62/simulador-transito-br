/// <reference lib="webworker" />
// Worker auxiliar das rotas em tempo real: guarda uma cópia do roteador por faixa da simulação e calcula as árvores
// das zonas (Dijkstra reverso) que o worker da simulação pede. Assim o cálculo sai do passo da simulação.
import init, { RouterHandle } from '../../core/pkg/core.js'
import type { RouterIn, RouterOut } from '../model/types'

const ready = init()
let router: RouterHandle | null = null
let gen = -1

const post = (m: RouterOut, t: Transferable[] = []) => (self as unknown as Worker).postMessage(m, t)

self.onmessage = async (ev: MessageEvent<RouterIn>) => {
  const msg = ev.data
  try {
    await ready
    if (msg.type === 'graph') {
      router?.free()
      router = RouterHandle.from_bytes(msg.bytes)
      gen = msg.gen
      return
    }
    if (!router || msg.gen !== gen) return
    if (msg.pen && msg.cm) router.set_weights(msg.pen, msg.cm)
    for (const { zone, targets } of msg.zones) {
      const dist = router.tree(targets)
      post({ type: 'tree', gen, init: msg.init, zone, dist }, [dist.buffer as ArrayBuffer])
    }
  } catch (e) {
    post({ type: 'error', gen: msg.gen, message: e instanceof Error ? e.message : String(e) })
  }
}
