import { reactive } from 'vue'

let nextId = 1

export const notices = reactive([])

export function notify(message, type = 'info') {
  const text = String(message || '').trim()
  if (!text) return
  const id = nextId++
  notices.push({ id, message: text, type })
  window.setTimeout(() => dismissNotice(id), 3600)
}

export function notifySuccess(message) {
  notify(message, 'success')
}

export function notifyError(message) {
  notify(message, 'error')
}

export function dismissNotice(id) {
  const index = notices.findIndex((item) => item.id === id)
  if (index >= 0) notices.splice(index, 1)
}
