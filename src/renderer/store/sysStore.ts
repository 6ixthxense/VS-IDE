import { create } from 'zustand'
import type { SysStats } from '@shared/types/ipc'

interface SysState {
  history: {
    cpu: number[]
    ram: number[]
    gpu: number[]
  }
  addStats: (stats: SysStats) => void
}

export const useSysStore = create<SysState>((set) => ({
  history: {
    cpu: [],
    ram: [],
    gpu: [],
  },
  addStats: (stats) => set((state) => {
    const maxSamples = 60
    return {
      history: {
        cpu: [...state.history.cpu, parseFloat(stats.cpu)].slice(-maxSamples),
        ram: [...state.history.ram, parseFloat(stats.ram)].slice(-maxSamples),
        gpu: [...state.history.gpu, stats.gpu?.load ?? 0].slice(-maxSamples),
      }
    }
  }),
}))
