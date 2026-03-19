export interface ParsedQuickOpenQuery {
  fileQuery: string
  lineNumber?: number
}

export interface QuickOpenItem {
  path: string
  relativePath: string
  fileName: string
  isRecent: boolean
  score: number
  fileNameMatches: number[]
  pathMatches: number[]
}

export function getRelativeQuickOpenPath(filePath: string, rootPath: string | null) {
  if (!rootPath || !filePath.startsWith(rootPath)) {
    return filePath.replace(/\\/g, '/')
  }

  return filePath.slice(rootPath.length).replace(/^[\\/]/, '').replace(/\\/g, '/')
}

export function parseQuickOpenQuery(rawQuery: string): ParsedQuickOpenQuery {
  const trimmed = rawQuery.trim()
  if (!trimmed) {
    return { fileQuery: '' }
  }

  const lineMatch = trimmed.match(/^(.*?)(?::(\d+))$/)
  if (!lineMatch) {
    return { fileQuery: trimmed }
  }

  const [, rawFileQuery, rawLineNumber] = lineMatch
  const lineNumber = Number.parseInt(rawLineNumber, 10)

  return {
    fileQuery: rawFileQuery.trim(),
    lineNumber: Number.isFinite(lineNumber) && lineNumber > 0 ? lineNumber : undefined,
  }
}

export function findFuzzyMatch(text: string, query: string) {
  const normalizedText = text.toLowerCase()
  const normalizedQuery = query.trim().toLowerCase()

  if (!normalizedQuery) {
    return { score: 0, indexes: [] as number[] }
  }

  const indexes: number[] = []
  let queryIndex = 0

  for (let textIndex = 0; textIndex < normalizedText.length; textIndex += 1) {
    if (normalizedText[textIndex] === normalizedQuery[queryIndex]) {
      indexes.push(textIndex)
      queryIndex += 1
      if (queryIndex === normalizedQuery.length) {
        break
      }
    }
  }

  if (queryIndex !== normalizedQuery.length) {
    return null
  }

  const gaps = indexes.reduce((total, index, current) => {
    if (current === 0) return total
    return total + Math.max(0, index - indexes[current - 1] - 1)
  }, 0)

  const startsWith = normalizedText.startsWith(normalizedQuery)
  const includes = normalizedText.includes(normalizedQuery)
  const score =
    (startsWith ? 140 : 0) +
    (includes ? 60 : 0) +
    Math.max(0, 80 - indexes[0] * 2) +
    Math.max(0, 100 - gaps * 8) +
    Math.max(0, 40 - Math.max(0, normalizedText.length - normalizedQuery.length))

  return { score, indexes }
}

export function buildQuickOpenItems(
  files: string[],
  rootPath: string | null,
  rawQuery: string,
  recentFiles: string[],
  limit = 12
) {
  const parsed = parseQuickOpenQuery(rawQuery)
  const recentWeights = new Map(recentFiles.map((filePath, index) => [filePath, recentFiles.length - index]))
  const uniqueFiles = Array.from(new Set(files))

  const items = uniqueFiles
    .map<QuickOpenItem | null>((filePath) => {
      const relativePath = getRelativeQuickOpenPath(filePath, rootPath)
      const fileName = relativePath.split('/').pop() || relativePath
      const fileNameMatch = findFuzzyMatch(fileName, parsed.fileQuery)
      const pathMatch = fileNameMatch ? null : findFuzzyMatch(relativePath, parsed.fileQuery)

      if (parsed.fileQuery && !fileNameMatch && !pathMatch) {
        return null
      }

      const recentBoost = (recentWeights.get(filePath) ?? 0) * 12
      const matchScore = fileNameMatch
        ? fileNameMatch.score + 220
        : pathMatch
          ? pathMatch.score + 80
          : 40

      return {
        path: filePath,
        relativePath,
        fileName,
        isRecent: recentWeights.has(filePath),
        score: matchScore + recentBoost - Math.min(relativePath.length, 120),
        fileNameMatches: fileNameMatch?.indexes ?? [],
        pathMatches: pathMatch?.indexes ?? [],
      }
    })
    .filter((item): item is QuickOpenItem => Boolean(item))
    .sort((left, right) => {
      if (right.score !== left.score) return right.score - left.score
      if (left.isRecent !== right.isRecent) return left.isRecent ? -1 : 1
      return left.relativePath.localeCompare(right.relativePath)
    })

  return {
    parsed,
    items: items.slice(0, limit),
  }
}
