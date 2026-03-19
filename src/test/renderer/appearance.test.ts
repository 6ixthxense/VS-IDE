import { describe, expect, it } from 'vitest'
import {
  DEFAULT_APPEARANCE,
  resolveAccentOption,
  sanitizeEditorFontSize,
  sanitizeFontFamily,
  sanitizeTabSize,
  sanitizeTerminalFontSize,
} from '../../renderer/config/appearance'

describe('appearance config helpers', () => {
  it('falls back to the default accent when the value is unknown', () => {
    expect(resolveAccentOption('#not-found').value).toBe(DEFAULT_APPEARANCE.accent)
  })

  it('clamps font sizes into supported ranges', () => {
    expect(sanitizeEditorFontSize(8)).toBe(12)
    expect(sanitizeEditorFontSize(40)).toBe(24)
    expect(sanitizeTerminalFontSize(4)).toBe(10)
    expect(sanitizeTerminalFontSize(30)).toBe(22)
    expect(sanitizeTabSize(1)).toBe(2)
    expect(sanitizeTabSize(9)).toBe(8)
  })

  it('keeps the default font stack when the custom value is blank', () => {
    expect(sanitizeFontFamily('   ')).toBe(DEFAULT_APPEARANCE.fontFamily)
  })
})
