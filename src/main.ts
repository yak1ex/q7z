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
      setProgress(parseInt(event.payload))
    }
  }
  await Promise.all([
    listen('job', onJob),
    listen('percent', onPercent),
  ])
  const job = await invoke<[string, string] | null>('current_job')
  if(Array.isArray(job) && job.length === 2
      && typeof(job[0]) === 'string' && typeof(job[1]) === 'string') {
    const input_el = document.querySelector<HTMLSpanElement>('#input')
    if(input_el) input_el.textContent = `${job[0]} -> ${job[1]}`
  }
})()
