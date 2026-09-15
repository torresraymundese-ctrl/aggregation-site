const TOKEN_KEY = 'token'
const WORKSPACE_KEY = 'nexus_workspace'

export function getAuthToken() {
  try {
    const token = window.sessionStorage.getItem(TOKEN_KEY)
    if (token) return token

    const oldToken = window.localStorage.getItem(TOKEN_KEY)
    if (oldToken) {
      window.sessionStorage.setItem(TOKEN_KEY, oldToken)
      window.localStorage.removeItem(TOKEN_KEY)
      return oldToken
    }
  } catch {
    return ''
  }
  return ''
}

export function setAuthToken(token) {
  try {
    window.sessionStorage.setItem(TOKEN_KEY, token)
    window.localStorage.removeItem(TOKEN_KEY)
  } catch {
    return
  }
}

export function clearAuthToken() {
  try {
    window.sessionStorage.removeItem(TOKEN_KEY)
    window.localStorage.removeItem(TOKEN_KEY)
    window.sessionStorage.removeItem(WORKSPACE_KEY)
  } catch {
    return
  }
}

export function getSelectedWorkspace() {
  try {
    return window.sessionStorage.getItem(WORKSPACE_KEY) || ''
  } catch {
    return ''
  }
}

export function setSelectedWorkspace(uid) {
  try {
    if (uid) window.sessionStorage.setItem(WORKSPACE_KEY, uid)
    else window.sessionStorage.removeItem(WORKSPACE_KEY)
  } catch {
    return
  }
}

export function clearSelectedWorkspace() {
  setSelectedWorkspace('')
}

export function authHeaders(extra = {}) {
  const workspace = getSelectedWorkspace()
  return {
    Authorization: `Bearer ${getAuthToken()}`,
    ...(workspace ? { 'X-Nexus-Workspace': workspace } : {}),
    ...extra,
  }
}
