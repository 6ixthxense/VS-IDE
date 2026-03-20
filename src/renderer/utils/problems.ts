import type { ProblemEntry, ProblemScanResult, ProblemSeverity } from '@shared/types/ipc'

function buildProblemKey(entry: ProblemEntry) {
  return [
    entry.filePath,
    entry.line,
    entry.column,
    entry.source,
    entry.severity,
    entry.code ?? '',
    entry.message,
  ].join(':')
}

function compareProblemSeverity(left: ProblemSeverity, right: ProblemSeverity) {
  if (left === right) return 0
  return left === 'error' ? -1 : 1
}

export function mergeProblemEntries(scanEntries: ProblemEntry[], liveEntries: ProblemEntry[]) {
  const baseEntries = liveEntries.length > 0
    ? scanEntries.filter((entry) => entry.source !== 'typescript')
    : scanEntries
  const mergedEntries = [...baseEntries, ...liveEntries]
  const dedupedEntries = new Map<string, ProblemEntry>()

  for (const entry of mergedEntries) {
    dedupedEntries.set(buildProblemKey(entry), entry)
  }

  return [...dedupedEntries.values()].sort((left, right) =>
    compareProblemSeverity(left.severity, right.severity)
    || left.filePath.localeCompare(right.filePath)
    || left.line - right.line
    || left.column - right.column
    || left.message.localeCompare(right.message)
  )
}

export function countProblemSeverity(entries: ProblemEntry[], severity: ProblemSeverity) {
  return entries.filter((entry) => entry.severity === severity).length
}

export function getProblemsTimestampLabel(scanResult: ProblemScanResult | null, liveEntries: ProblemEntry[]) {
  if (scanResult) {
    return new Date(scanResult.scannedAt).toLocaleTimeString()
  }

  return liveEntries.length > 0 ? 'Live' : 'Never'
}
