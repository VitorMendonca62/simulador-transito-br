import { useEffect, useRef } from 'react'
import { perf, watchLongTasks } from '../engine/perf'
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
  const sim = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    watchWorkerPerf()
    watchLongTasks()
    let last = { t: performance.now(), renders: perf.renders, ui: perf.uiMs, long: perf.longMs, gpu: perf.gpuMs }
    // ocupação do worker pelo relógio dele: a mensagem chega 1×/s, mas fora de fase com este intervalo (antes dava 150 %)
    let simLast = { busy: perf.simBusyMs, at: perf.simBusyAt }
    let simPct = 0
    const id = window.setInterval(() => {
      const now = performance.now()
      const dt = now - last.t
      const cur = { t: now, renders: perf.renders, ui: perf.uiMs, long: perf.longMs, gpu: perf.gpuMs }
      if (perf.simBusyAt > simLast.at) {
        simPct = (perf.simBusyMs - simLast.busy) / (perf.simBusyAt - simLast.at)
        simLast = { busy: perf.simBusyMs, at: perf.simBusyAt }
      }
      const renders = cur.renders - last.renders
      const set = (el: HTMLSpanElement | null, text: string) => {
        if (el && el.textContent !== text) el.textContent = text
      }
      const rate = (renders * 1000) / dt
      set(fps.current, renders === 0 ? 'parado' : `${rate.toFixed(0)} fps`)
      const pct = (ms: number) => `${Math.min(100, (ms / dt) * 100).toFixed(0)}%`
      // tela: o maior entre o JS do app e as tarefas longas (que incluem o desenho nativo); são sobrepostos, não somam
      set(cpu.current, `CPU tela ${pct(Math.max(cur.ui - last.ui, cur.long - last.long))} · sim ${Math.min(100, simPct * 100).toFixed(0)}%`)
      if (perf.gpuSupported) set(gpu.current, renders === 0 ? 'GPU —' : `GPU ${pct(cur.gpu - last.gpu)} (${((cur.gpu - last.gpu) / renders).toFixed(1)} ms/quadro)`)
      else set(gpu.current, 'GPU n/d')
      const heap = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize
      set(ram.current, `RAM ${heap != null ? mb(heap) : 'n/d'}${perf.wasmBytes ? ` + ${mb(perf.wasmBytes)} WASM` : ''}`)
      const s = perf.sim
      if (sim.current) sim.current.hidden = !s
      if (s) {
        const behind = s.pace < s.speed * 0.9 ? ` · ritmo ${s.pace.toFixed(1)}× de ${s.speed}×` : ''
        set(sim.current, `passo ${s.stepMax.toFixed(0)} ms · espera rotas ${s.waitMs.toFixed(0)} ms · quadros ${s.gapMax.toFixed(0)} ms${behind}`)
      }
      last = cur
    }, 1000)
    return () => window.clearInterval(id)
  }, [])

  return (
    <>
      <span ref={fps} title="Quadros por segundo realmente desenhados. O mapa só redesenha quando algo muda; parado = nada a desenhar.">
        —
      </span>
      <span ref={cpu} title="CPU tela: parte do tempo em que a thread principal esteve ocupada: o laço de desenho e o tratamento dos quadros da simulação ou, se for maior, as tarefas longas (acima de 50 ms, inclusive o desenho feito pelo próprio navegador; medidas só no Chrome/Edge). CPU sim: tempo que o worker passou calculando a simulação (100% = não acompanha o tempo real).">
        CPU —
      </span>
      <span ref={gpu} title="Tempo de GPU gasto desenhando o mapa (timer de GPU do navegador). n/d = o navegador não oferece essa medição.">
        GPU —
      </span>
      <span ref={ram} title="Memória: heap do JavaScript da página (só Chromium informa) + memória linear do núcleo WASM no worker.">
        RAM —
      </span>
      <span ref={sim} hidden title="Simulação no último segundo: pior passo do núcleo, tempo parado esperando as rotas dos workers auxiliares e maior intervalo entre quadros enviados à tela (acima de ~150 ms os carros dão uma parada). Ritmo aparece quando a simulação não acompanha a velocidade pedida.">
        passo —
      </span>
    </>
  )
}
