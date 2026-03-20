import { describe, expect, it } from 'vitest'
import type { ProblemEntry } from '@shared/types/ipc'
import { countProblemSeverity, getProblemsTimestampLabel, mergeProblemEntries } from '../../renderer/utils/problems'

function createEntry(overrides: Partial<ProblemEntry> = {}): ProblemEntry {
  return {
    id: 'problem',
    filePath: 'C:/repo/src/index.ts',
    line: 1,
    column: 1,
    message: 'Example problem',
    source: 'typescript',
    severity: 'error',
    ...overrides,
  }
}

describe('mergeProblemEntries', () => {
  it('replaces scanned TypeScript entries with live TypeScript diagnostics', () => {
    const scanEntries = [
      createEntry({ id: 'scan-ts', message: 'Old ts issue', source: 'typescript' }),
      createEntry({ id: 'scan-eslint', source: 'eslint', message: 'Lint issue' }),
    ]
    const liveEntries = [
      createEntry({ id: 'live-ts', message: 'Live ts issue', source: 'typescript' }),
    ]

    expect(mergeProblemEntries(scanEntries, liveEntries)).toEqual([
      scanEntries[1],
      liveEntries[0],
    ])
  })

  it('keeps scanned entries intact when no live diagnostics exist', () => {
    const scanEntries = [
      createEntry({ id: 'scan-ts', message: 'TS issue' }),
      createEntry({ id: 'scan-eslint', source: 'eslint', message: 'Lint issue', severity: 'warning' }),
    ]

    expect(mergeProblemEntries(scanEntries, [])).toEqual([
      scanEntries[0],
      scanEntries[1],
    ])
  })
})

describe('problem helpers', () => {
  it('counts severities from a merged set', () => {
    const entries = [
      createEntry({ severity: 'error' }),
      createEntry({ id: 'warning', severity: 'warning', line: 2 }),
    ]

    expect(countProblemSeverity(entries, 'error')).toBe(1)
    expect(countProblemSeverity(entries, 'warning')).toBe(1)
  })

  it('shows Live when only live diagnostics exist', () => {
    expect(getProblemsTimestampLabel(null, [createEntry()])).toBe('Live')
  })
})
