import './style.css'
import { setupProgress } from './progress.ts'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/tauri'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div>
    <span id="input"></span>
    <div id="progress"></div>
    <label>Log:</label>
    <textarea id="log" rows="5" readonly></textarea>
  </div>
`

const { setProgress } = setupProgress(document.querySelector<HTMLDivElement>('#progress')!)
void (async () => {
  const onJob = (event: { payload: unknown }) => {
    const payload = event.payload
    if(Array.isArray(payload) && payload.length === 2
        && typeof(payload[0]) === 'string' && typeof(payload[1]) === 'string') {
      const input_el = document.querySelector<HTMLSpanElement>('#input')
      if(input_el) input_el.textContent = `${payload[0]} -> ${payload[1]}`
      setProgress(0)
    }
  }
  const onPercent = (event: { payload: unknown }) => {
    if(typeof(event.payload) === 'string') {
      const n = parseInt(event.payload)
      if(Number.isFinite(n)) setProgress(n)
    }
  }
  const onLog = (event: { payload: unknown }) => {
    if(typeof(event.payload) === 'string') {
      const log_el = document.querySelector<HTMLTextAreaElement>('#log')
      if(log_el) {
        log_el.value += event.payload + '\n'
        log_el.scrollTop = log_el.scrollHeight
      }
    }
  }
  const onResult = (event: { payload: unknown }) => {
    const p = event.payload
    if(p && typeof(p) === 'object' && 'ok' in p && 'exit_code' in p) {
      const ok = (p as { ok: boolean }).ok
      const code = (p as { exit_code: number }).exit_code
      const err = (p as { error?: string }).error
      const log_el = document.querySelector<HTMLTextAreaElement>('#log')
      if(log_el) {
        const line = ok ? `[ok] exit ${code}` : `[error] exit ${code}${err ? ': ' + err : ''}`
        log_el.value += line + '\n'
        log_el.scrollTop = log_el.scrollHeight
      }
      if(!ok) setProgress(0)
    }
  }
  await Promise.all([
    listen('job', onJob),
    listen('percent', onPercent),
    listen('log', onLog),
    listen('result', onResult),
  ])
  const job = await invoke<[string, string] | null>('current_job')
  if(Array.isArray(job) && job.length === 2
      && typeof(job[0]) === 'string' && typeof(job[1]) === 'string') {
    const input_el = document.querySelector<HTMLSpanElement>('#input')
    if(input_el) input_el.textContent = `${job[0]} -> ${job[1]}`
  }
})()
