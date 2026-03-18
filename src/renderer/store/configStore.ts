import { create } from 'zustand'

interface ConfigState {
  theme: 'dark' | 'light' | 'amoled'
  fontSize: number
  fontFamily: string
  setTheme: (theme: 'dark' | 'light' | 'amoled') => void
  setFontSize: (size: number) => void
  setFontFamily: (family: string) => void
}

export const useConfigStore = create<ConfigState>((set) => ({
  theme: (localStorage.getItem('theme') as any) || 'dark',
  fontSize: parseInt(localStorage.getItem('fontSize') || '14'),
  fontFamily: localStorage.getItem('fontFamily') || "'Cascadia Code', 'Fira Code', Consolas, monospace",
  
  setTheme: (theme) => {
    set({ theme })
    localStorage.setItem('theme', theme)
    document.documentElement.setAttribute('data-theme', theme)
  },
  setFontSize: (fontSize) => {
    set({ fontSize })
    localStorage.setItem('fontSize', fontSize.toString())
  },
  setFontFamily: (fontFamily) => {
    set({ fontFamily })
    localStorage.setItem('fontFamily', fontFamily)
  },
}))
