import type { SearchOptions, SearchResult } from '@shared/types/ipc'

const SEARCH_HISTORY_KEY = 'searchHistory'
const MAX_SEARCH_HISTORY = 10

export interface SearchResultGroup {
  path: string
  name: string
  relativePath: string
  matches: SearchResult[]
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function normalizePath(value: string) {
  return value.replace(/\\/g, '/')
}

export function readSearchHistory() {
  if (typeof window === 'undefined') return [] as string[]

  try {
    const raw = window.localStorage.getItem(SEARCH_HISTORY_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === 'string') : []
  } catch {
    return []
  }
}

export function persistSearchHistory(items: string[]) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(SEARCH_HISTORY_KEY, JSON.stringify(items))
}

export function touchSearchHistory(items: string[], query: string) {
  const normalizedQuery = query.trim()
  if (!normalizedQuery) return items
  return [normalizedQuery, ...items.filter((item) => item !== normalizedQuery)].slice(0, MAX_SEARCH_HISTORY)
}

export function groupSearchResults(results: SearchResult[], rootPath: string | null) {
  const groups = new Map<string, SearchResultGroup>()

  for (const result of results) {
    if (!groups.has(result.path)) {
      const relativePath = rootPath && result.path.startsWith(rootPath)
        ? normalizePath(result.path.slice(rootPath.length).replace(/^[\\/]/, ''))
        : normalizePath(result.path)

      groups.set(result.path, {
        path: result.path,
        name: result.name,
        relativePath,
        matches: [],
      })
    }

    groups.get(result.path)!.matches.push(result)
  }

  return Array.from(groups.values())
}

export function buildClientSearchRegExp(query: string, options: Partial<SearchOptions>) {
  const source = options.useRegex ? query : escapeRegExp(query)
  const wholeWordSource = options.wholeWord ? `\\b(?:${source})\\b` : source
  const flags = options.caseSensitive ? 'g' : 'gi'

  return new RegExp(wholeWordSource, flags)
}

export function buildReplacePreview(
  matchLine: string,
  query: string,
  replacement: string,
  options: Partial<SearchOptions>
) {
  if (!query.trim() || !replacement) return ''

  try {
    const searchRegExp = buildClientSearchRegExp(query, options)
    const preview = matchLine.replace(searchRegExp, replacement)
    return preview === matchLine ? '' : preview
  } catch {
    return ''
  }
}
