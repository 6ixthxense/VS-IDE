import type { RunCodePayload, SearchOptions } from '../shared/types/ipc'

function requireString(value: unknown, label: string, options?: { trim?: boolean; allowEmpty?: boolean }) {
  if (typeof value !== 'string') {
    throw new TypeError(`${label} must be a string`)
  }

  const nextValue = options?.trim ? value.trim() : value
  if (!options?.allowEmpty && nextValue.length === 0) {
    throw new TypeError(`${label} must not be empty`)
  }

  return nextValue
}

function normalizePattern(value: unknown) {
  if (value == null) return undefined
  return requireString(value, 'Search pattern', { trim: true, allowEmpty: true })
}

export function requireCallback<T>(value: unknown, label: string): (payload: T) => void {
  if (typeof value !== 'function') {
    throw new TypeError(`${label} must be a function`)
  }

  return value as (payload: T) => void
}

export function requirePath(value: unknown, label = 'Path') {
  return requireString(value, label)
}

export function requireIdentifier(value: unknown, label: string) {
  return requireString(value, label, { trim: true })
}

export function requireMaybeEmptyString(value: unknown, label: string) {
  return requireString(value, label, { allowEmpty: true })
}

export function requireOptionalPath(value: unknown, label = 'Path') {
  if (value == null) return undefined
  return requirePath(value, label)
}

export function normalizeSearchOptions(options?: Partial<SearchOptions>): Partial<SearchOptions> | undefined {
  if (!options) return undefined

  return {
    caseSensitive: Boolean(options.caseSensitive),
    wholeWord: Boolean(options.wholeWord),
    useRegex: Boolean(options.useRegex),
    includePattern: normalizePattern(options.includePattern),
    excludePattern: normalizePattern(options.excludePattern),
  }
}

export function normalizeRunCodePayload(payload: RunCodePayload): RunCodePayload {
  const terminalId = requireIdentifier(payload?.terminalId, 'Terminal id')
  const code = requireString(payload?.code, 'Code', { allowEmpty: true })
  const language = payload?.language

  if (language !== 'javascript' && language !== 'python') {
    throw new TypeError('Language must be either "javascript" or "python"')
  }

  return { terminalId, code, language }
}
