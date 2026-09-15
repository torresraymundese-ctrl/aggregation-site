export class ApiError extends Error {
  constructor(message, { code = 'API_ERROR', status = 0 } = {}) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
  }
}

export async function requestJson(input, init) {
  let response
  try {
    response = await fetch(input, init)
  } catch {
    throw new ApiError('Network request failed', { code: 'NETWORK_ERROR' })
  }

  const body = await response.text()
  let data = null

  if (body.trim()) {
    try {
      data = JSON.parse(body)
    } catch {
      throw new ApiError('Server returned invalid JSON', {
        code: 'INVALID_JSON_RESPONSE',
        status: response.status,
      })
    }
  }

  if (!response.ok) {
    throw new ApiError(data?.error?.message || data?.message || `Request failed with HTTP ${response.status}`, {
      code: data?.error?.code || data?.code || `HTTP_${response.status}`,
      status: response.status,
    })
  }

  if (!data) {
    throw new ApiError('Server returned an empty response', {
      code: 'EMPTY_RESPONSE',
      status: response.status,
    })
  }

  return data
}

export function apiErrorMessage(error, fallback) {
  if (error instanceof ApiError && !['NETWORK_ERROR', 'EMPTY_RESPONSE', 'INVALID_JSON_RESPONSE'].includes(error.code)) {
    return error.message
  }
  return fallback
}
