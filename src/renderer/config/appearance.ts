export type ThemeName = 'dark' | 'light' | 'amoled'

export interface ThemeOption {
  id: ThemeName
  label: string
  description: string
  preview: {
    frame: string
    panel: string
    surface: string
    accent: string
    text: string
  }
}

export interface AccentOption {
  id: string
  name: string
  description: string
  value: string
  gradient: string
}

export interface AppearanceConfig {
  theme: ThemeName
  accent: string
  accentGradient: string
  fontSize: number
  terminalFontSize: number
  fontFamily: string
  wordWrap: boolean
  lineNumbers: boolean
  autoSave: boolean
  tabSize: number
}

export const THEME_OPTIONS: ThemeOption[] = [
  {
    id: 'dark',
    label: 'Night Shift',
    description: 'Balanced contrast for long coding sessions.',
    preview: {
      frame: '#1e1e1f',
      panel: '#252526',
      surface: '#121314',
      accent: '#0a84ff',
      text: '#f3f4f6',
    },
  },
  {
    id: 'light',
    label: 'Daylight',
    description: 'Bright panels with softer borders and cleaner text.',
    preview: {
      frame: '#e7edf3',
      panel: '#f7fafc',
      surface: '#ffffff',
      accent: '#2563eb',
      text: '#111827',
    },
  },
  {
    id: 'amoled',
    label: 'OLED Black',
    description: 'Pure black chrome for high-contrast setups.',
    preview: {
      frame: '#020202',
      panel: '#080808',
      surface: '#000000',
      accent: '#22c55e',
      text: '#fafafa',
    },
  },
]

export const ACCENT_OPTIONS: AccentOption[] = [
  {
    id: 'ocean',
    name: 'Ocean Blue',
    description: 'Clean and familiar with a VS-style feel.',
    value: '#007acc',
    gradient: 'linear-gradient(135deg, #007acc, #005a9e)',
  },
  {
    id: 'jade',
    name: 'Jade',
    description: 'Fresh and calm with clearer success states.',
    value: '#12b981',
    gradient: 'linear-gradient(135deg, #12b981, #0f766e)',
  },
  {
    id: 'sunset',
    name: 'Sunset',
    description: 'Warm highlight for tabs, buttons, and focus rings.',
    value: '#f97316',
    gradient: 'linear-gradient(135deg, #f97316, #ea580c)',
  },
  {
    id: 'rose',
    name: 'Rose',
    description: 'A punchier accent that still reads well on dark UIs.',
    value: '#e11d48',
    gradient: 'linear-gradient(135deg, #e11d48, #be123c)',
  },
  {
    id: 'indigo',
    name: 'Indigo',
    description: 'Cool and deeper than the default blue.',
    value: '#6366f1',
    gradient: 'linear-gradient(135deg, #6366f1, #4338ca)',
  },
]

export const FONT_PRESETS = [
  {
    label: 'Cascadia Code',
    value: "'Cascadia Code', 'Fira Code', Consolas, monospace",
  },
  {
    label: 'JetBrains Mono',
    value: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace",
  },
  {
    label: 'IBM Plex Mono',
    value: "'IBM Plex Mono', 'Cascadia Code', Consolas, monospace",
  },
  {
    label: 'Fira Code',
    value: "'Fira Code', 'Cascadia Code', Consolas, monospace",
  },
]

export const DEFAULT_APPEARANCE: AppearanceConfig = {
  theme: 'dark',
  accent: ACCENT_OPTIONS[0].value,
  accentGradient: ACCENT_OPTIONS[0].gradient,
  fontSize: 14,
  terminalFontSize: 12,
  fontFamily: FONT_PRESETS[0].value,
  wordWrap: true,
  lineNumbers: true,
  autoSave: false,
  tabSize: 4,
}

function clampNumber(value: number, fallback: number, min: number, max: number) {
  if (!Number.isFinite(value)) return fallback
  return Math.min(max, Math.max(min, Math.round(value)))
}

function isThemeName(value: string | null): value is ThemeName {
  return value === 'dark' || value === 'light' || value === 'amoled'
}

export function sanitizeEditorFontSize(value: number) {
  return clampNumber(value, DEFAULT_APPEARANCE.fontSize, 12, 24)
}

export function sanitizeTerminalFontSize(value: number) {
  return clampNumber(value, DEFAULT_APPEARANCE.terminalFontSize, 10, 22)
}

export function sanitizeFontFamily(value: string) {
  const trimmed = value.trim()
  return trimmed || DEFAULT_APPEARANCE.fontFamily
}

export function sanitizeTabSize(value: number) {
  return clampNumber(value, DEFAULT_APPEARANCE.tabSize, 2, 8)
}

function readBooleanSetting(key: string, fallback: boolean) {
  if (typeof window === 'undefined') return fallback

  const value = window.localStorage.getItem(key)
  if (value == null) return fallback
  return value === 'true'
}

export function resolveAccentOption(value: string | null | undefined) {
  return ACCENT_OPTIONS.find((accent) => accent.value === value) ?? ACCENT_OPTIONS[0]
}

export function getInitialAppearanceConfig(): AppearanceConfig {
  if (typeof window === 'undefined') {
    return DEFAULT_APPEARANCE
  }

  const storedTheme = window.localStorage.getItem('theme')
  const storedAccent = window.localStorage.getItem('accent') || window.localStorage.getItem('vs-monitor-accent')
  const storedFontSize = Number.parseInt(window.localStorage.getItem('fontSize') || '', 10)
  const storedTerminalFontSize = Number.parseInt(window.localStorage.getItem('terminalFontSize') || '', 10)
  const storedFontFamily = window.localStorage.getItem('fontFamily') || DEFAULT_APPEARANCE.fontFamily
  const storedTabSize = Number.parseInt(window.localStorage.getItem('tabSize') || '', 10)
  const accentOption = resolveAccentOption(storedAccent)

  return {
    theme: isThemeName(storedTheme) ? storedTheme : DEFAULT_APPEARANCE.theme,
    accent: accentOption.value,
    accentGradient: accentOption.gradient,
    fontSize: sanitizeEditorFontSize(storedFontSize),
    terminalFontSize: sanitizeTerminalFontSize(storedTerminalFontSize),
    fontFamily: sanitizeFontFamily(storedFontFamily),
    wordWrap: readBooleanSetting('wordWrap', DEFAULT_APPEARANCE.wordWrap),
    lineNumbers: readBooleanSetting('lineNumbers', DEFAULT_APPEARANCE.lineNumbers),
    autoSave: readBooleanSetting('autoSave', DEFAULT_APPEARANCE.autoSave),
    tabSize: sanitizeTabSize(storedTabSize),
  }
}

export function applyAppearanceConfig(config: AppearanceConfig) {
  if (typeof document === 'undefined') return

  const root = document.documentElement
  root.setAttribute('data-theme', config.theme)
  root.style.setProperty('--acc', config.accent)
  root.style.setProperty('--acc-grad', config.accentGradient)
  root.style.setProperty('--editor-font-family', config.fontFamily)
  root.style.setProperty('--editor-font-size', `${config.fontSize}px`)
  root.style.setProperty('--terminal-font-size', `${config.terminalFontSize}px`)
}
