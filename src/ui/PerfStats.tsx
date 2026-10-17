import { useEffect, useRef } from 'react'
import { perf } from '../engine/perf'
import { watchWorkerPerf } from '../model/importer'

const mb = (bytes: number) => `${(bytes / 1048576).toFixed(0)} MB`

/**
 * Taxa de quadros, CPU, GPU e RAM na barra de status. Atualiza o DOM direto uma vez por segundo
 * (sem passar pelo React, para o próprio indicador não custar re-renders).
 */
export function PerfStats() {
  const fps = useRef<HTMLSpanElement>(null)
  const cpu = useRef<HTMLSpanElement>(null)
  const gpu = useRef<HTMLSpanElement>(null)
  const ram = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    watchWorkerPerf()
    let last = { t: performance.now(), renders: perf.renders, ui: perf.uiMs, gpu: perf.gpuMs, sim: perf.simBusyMs }
    const id = window.setInterval(() => {
      const now = performance.now()
      const dt = now - last.t
      const cur = { t: now, renders: perf.renders, ui: perf.uiMs, gpu: perf.gpuMs, sim: perf.simBusyMs }
      const renders = cur.renders - last.renders
      const set = (el: HTMLSpanElement | null, text: string) => {
        if (el && el.textContent !== text) el.textContent = text
      }
      const rate = (renders * 1000) / dt
      set(fps.current, renders === 0 ? 'parado' : `${rate.toFixed(0)} fps`)
      const pct = (ms: number) => `${Math.min(100, (ms / dt) * 100).toFixed(0)}%`
      set(cpu.current, `CPU tela ${pct(cur.ui - last.ui)} · sim ${pct(cur.sim - last.sim)}`)
      if (perf.gpuSupported) set(gpu.current, renders === 0 ? 'GPU —' : `GPU ${pct(cur.gpu - last.gpu)} (${((cur.gpu - last.gpu) / renders).toFixed(1)} ms/quadro)`)
      else set(gpu.current, 'GPU n/d')
      const heap = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize
      set(ram.current, `RAM ${heap != null ? mb(heap) : 'n/d'}${perf.wasmBytes ? ` + ${mb(perf.wasmBytes)} WASM` : ''}`)
      last = cur
    }, 1000)
    return () => window.clearInterval(id)
  }, [])

  return (
    <>
      <span ref={fps} title="Quadros por segundo realmente desenhados. O mapa só redesenha quando algo muda; parado = nada a desenhar.">
        —
      </span>
      <span ref={cpu} title="CPU tela: parte do tempo em que a thread principal esteve no laço de desenho e no tratamento dos quadros da simulação. CPU sim: tempo que o worker passou calculando a simulação (100% = não acompanha o tempo real).">
        CPU —
      </span>
      <span ref={gpu} title="Tempo de GPU gasto desenhando o mapa (timer de GPU do navegador). n/d = o navegador não oferece essa medição.">
        GPU —
      </span>
      <span ref={ram} title="Memória: heap do JavaScript da página (só Chromium informa) + memória linear do núcleo WASM no worker.">
        RAM —
      </span>
    </>
  )
}
