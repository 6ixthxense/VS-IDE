import { describe, expect, it } from 'vitest'
import { normalizeRunCodePayload, normalizeSearchOptions, requireCallback, requireIdentifier, requirePath } from '../../preload/validation'

describe('preload validation helpers', () => {
  it('normalizes search flags and trims include/exclude patterns', () => {
    expect(normalizeSearchOptions({
      caseSensitive: 1 as unknown as boolean,
      wholeWord: 0 as unknown as boolean,
      useRegex: true,
      includePattern: ' src/**/*  ',
      excludePattern: ' dist/* ',
    })).toEqual({
      caseSensitive: true,
      wholeWord: false,
      useRegex: true,
      includePattern: 'src/**/*',
      excludePattern: 'dist/*',
    })
  })

  it('rejects invalid callbacks and blank identifiers', () => {
    expect(() => requireCallback(null, 'Listener')).toThrow('Listener must be a function')
    expect(() => requireIdentifier('   ', 'Terminal id')).toThrow('Terminal id must not be empty')
  })

  it('requires non-empty paths and valid run payloads', () => {
    expect(() => requirePath('', 'File path')).toThrow('File path must not be empty')

    expect(normalizeRunCodePayload({
      terminalId: '  default  ',
      code: 'print("ok")',
      language: 'python',
    })).toEqual({
      terminalId: 'default',
      code: 'print("ok")',
      language: 'python',
    })

    expect(() => normalizeRunCodePayload({
      terminalId: 'default',
      code: 'echo test',
      language: 'bash' as never,
    })).toThrow('Language must be either "javascript" or "python"')
  })
})
