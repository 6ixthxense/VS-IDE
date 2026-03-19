import { create } from 'zustand'
import {
  DEFAULT_APPEARANCE,
  getInitialAppearanceConfig,
  resolveAccentOption,
  sanitizeEditorFontSize,
  sanitizeFontFamily,
  sanitizeTabSize,
  sanitizeTerminalFontSize,
  type ThemeName,
} from '../config/appearance'

interface ConfigState {
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
  setTheme: (theme: ThemeName) => void
  setAccent: (accent: string) => void
  setFontSize: (size: number) => void
  setTerminalFontSize: (size: number) => void
  setFontFamily: (family: string) => void
  setWordWrap: (enabled: boolean) => void
  setLineNumbers: (enabled: boolean) => void
  setAutoSave: (enabled: boolean) => void
  setTabSize: (size: number) => void
  resetAppearance: () => void
}

const persistAppearance = (
  state: Pick<ConfigState, 'theme' | 'accent' | 'fontSize' | 'terminalFontSize' | 'fontFamily' | 'wordWrap' | 'lineNumbers' | 'autoSave' | 'tabSize'>
) => {
  if (typeof window === 'undefined') return
  window.localStorage.setItem('theme', state.theme)
  window.localStorage.setItem('accent', state.accent)
  window.localStorage.setItem('fontSize', state.fontSize.toString())
  window.localStorage.setItem('terminalFontSize', state.terminalFontSize.toString())
  window.localStorage.setItem('fontFamily', state.fontFamily)
  window.localStorage.setItem('wordWrap', state.wordWrap.toString())
  window.localStorage.setItem('lineNumbers', state.lineNumbers.toString())
  window.localStorage.setItem('autoSave', state.autoSave.toString())
  window.localStorage.setItem('tabSize', state.tabSize.toString())
}

const initialAppearance = getInitialAppearanceConfig()

export const useConfigStore = create<ConfigState>((set) => ({
  ...initialAppearance,

  setTheme: (theme) => set((state) => {
    const nextState = { ...state, theme }
    persistAppearance(nextState)
    return { theme }
  }),

  setAccent: (accent) => set((state) => {
    const nextAccent = resolveAccentOption(accent)
    const nextState = {
      ...state,
      accent: nextAccent.value,
      accentGradient: nextAccent.gradient,
    }
    persistAppearance(nextState)
    return {
      accent: nextAccent.value,
      accentGradient: nextAccent.gradient,
    }
  }),

  setFontSize: (fontSize) => set((state) => {
    const nextFontSize = sanitizeEditorFontSize(fontSize)
    const nextState = { ...state, fontSize: nextFontSize }
    persistAppearance(nextState)
    return { fontSize: nextFontSize }
  }),

  setTerminalFontSize: (terminalFontSize) => set((state) => {
    const nextTerminalFontSize = sanitizeTerminalFontSize(terminalFontSize)
    const nextState = { ...state, terminalFontSize: nextTerminalFontSize }
    persistAppearance(nextState)
    return { terminalFontSize: nextTerminalFontSize }
  }),

  setFontFamily: (fontFamily) => set((state) => {
    const nextFontFamily = sanitizeFontFamily(fontFamily)
    const nextState = { ...state, fontFamily: nextFontFamily }
    persistAppearance(nextState)
    return { fontFamily: nextFontFamily }
  }),

  setWordWrap: (wordWrap) => set((state) => {
    const nextState = { ...state, wordWrap }
    persistAppearance(nextState)
    return { wordWrap }
  }),

  setLineNumbers: (lineNumbers) => set((state) => {
    const nextState = { ...state, lineNumbers }
    persistAppearance(nextState)
    return { lineNumbers }
  }),

  setAutoSave: (autoSave) => set((state) => {
    const nextState = { ...state, autoSave }
    persistAppearance(nextState)
    return { autoSave }
  }),

  setTabSize: (tabSize) => set((state) => {
    const nextTabSize = sanitizeTabSize(tabSize)
    const nextState = { ...state, tabSize: nextTabSize }
    persistAppearance(nextState)
    return { tabSize: nextTabSize }
  }),

  resetAppearance: () => set(() => {
    persistAppearance(DEFAULT_APPEARANCE)
    return DEFAULT_APPEARANCE
  }),
}))
