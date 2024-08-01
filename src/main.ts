import './style.css'
import { setupProgress } from './progress.ts'
import { listen } from '@tauri-apps/api/event'

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div>
    <span id="input"></span>
    <div id="progress"></div>
    <label>Log:</label>
    <textarea id="log" rows="5" readonly></textarea>
  </div>
`

const { setProgress } = setupProgress(document.querySelector<HTMLDivElement>('#progress')!)
await listen('percent', (event) => {
  if(typeof(event.payload) === 'string') {
    setProgress(parseInt(event.payload))
  }
})
