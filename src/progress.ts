export function setupProgress(element: HTMLDivElement) {
  let percent = 0
  const progress_bar = document.createElement('div')
  progress_bar.id = 'progress-bar'
  element.appendChild(progress_bar)

  const setProgress = (new_percent: number) => {
    if(new_percent < 0 && new_percent > 100) {
      return
    }
    percent = new_percent
    progress_bar.style.width = `${percent}%`
  }

  return { setProgress }
}
