import { create } from 'zustand'
import {
  clearTerminalSession,
  readTerminalSession,
  writeTerminalSession,
} from '../utils/workbenchPersistence'

interface TerminalDescriptor {
  id: string
  cwd: string
}

interface TerminalState {
  workspaceRoot: string | null
  terminals: TerminalDescriptor[]
  activeId: string
  restoreForWorkspace: (rootPath: string | null) => void
  addTerminal: (cwd: string) => string
  removeTerminal: (id: string) => void
  setActiveId: (id: string) => void
  updateTerminalCwd: (id: string, cwd: string) => void
}

function defaultTerminalState(rootPath: string | null) {
  return {
    terminals: [{ id: 'default', cwd: rootPath ?? '' }],
    activeId: 'default',
  }
}

function persistTerminalState(rootPath: string | null, terminals: TerminalDescriptor[], activeId: string) {
  if (!rootPath) return
  writeTerminalSession(rootPath, { terminals, activeId })
}

export const useTerminalStore = create<TerminalState>((set) => ({
  workspaceRoot: null,
  ...defaultTerminalState(null),

  restoreForWorkspace: (rootPath) => {
    if (!rootPath) {
      set({ workspaceRoot: null, ...defaultTerminalState(null) })
      return
    }

    const snapshot = readTerminalSession(rootPath)
    if (!snapshot || snapshot.terminals.length === 0) {
      const nextState = defaultTerminalState(rootPath)
      set({ workspaceRoot: rootPath, ...nextState })
      persistTerminalState(rootPath, nextState.terminals, nextState.activeId)
      return
    }

    const uniqueTerminals = snapshot.terminals.filter((terminal, index, items) =>
      terminal.id &&
      items.findIndex((item) => item.id === terminal.id) === index
    )

    const terminals = uniqueTerminals.length > 0 ? uniqueTerminals : defaultTerminalState(rootPath).terminals
    const activeId = terminals.some((terminal) => terminal.id === snapshot.activeId)
      ? snapshot.activeId
      : terminals[0].id

    set({ workspaceRoot: rootPath, terminals, activeId })
    persistTerminalState(rootPath, terminals, activeId)
  },

  addTerminal: (cwd) => {
    const id = `term-${Date.now()}`
    set((state) => {
      const terminals = [...state.terminals, { id, cwd }]
      persistTerminalState(state.workspaceRoot, terminals, id)
      return { terminals, activeId: id }
    })
    return id
  },

  removeTerminal: (id) => {
    set((state) => {
      if (state.terminals.length === 1) return state

      const terminals = state.terminals.filter((terminal) => terminal.id !== id)
      const activeId = state.activeId === id ? terminals[Math.max(0, terminals.length - 1)]?.id ?? 'default' : state.activeId

      persistTerminalState(state.workspaceRoot, terminals, activeId)
      return { terminals, activeId }
    })
  },

  setActiveId: (id) => {
    set((state) => {
      if (!state.terminals.some((terminal) => terminal.id === id)) return state
      persistTerminalState(state.workspaceRoot, state.terminals, id)
      return { activeId: id }
    })
  },

  updateTerminalCwd: (id, cwd) => {
    set((state) => {
      const terminals = state.terminals.map((terminal) =>
        terminal.id === id ? { ...terminal, cwd } : terminal
      )

      if (terminals.every((terminal, index) => terminal === state.terminals[index])) {
        return state
      }

      persistTerminalState(state.workspaceRoot, terminals, state.activeId)
      return { terminals }
    })
  },
}))

export function clearTerminalWorkspaceSession(rootPath: string) {
  clearTerminalSession(rootPath)
}
