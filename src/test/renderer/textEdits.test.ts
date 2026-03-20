import { describe, expect, it } from 'vitest'
import { applyTextSpanEdits } from '../../renderer/utils/textEdits'

describe('applyTextSpanEdits', () => {
  it('applies multiple edits from the end of the document backwards', () => {
    expect(applyTextSpanEdits('alpha beta gamma', [
      { start: 0, length: 5, newText: 'omega' },
      { start: 11, length: 5, newText: 'delta' },
    ])).toBe('omega beta delta')
  })

  it('supports repeated replacements in the same document', () => {
    expect(applyTextSpanEdits('foo + foo + foo', [
      { start: 0, length: 3, newText: 'bar' },
      { start: 6, length: 3, newText: 'bar' },
      { start: 12, length: 3, newText: 'bar' },
    ])).toBe('bar + bar + bar')
  })
})
