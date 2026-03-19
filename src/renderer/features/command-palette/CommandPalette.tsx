import React, { useEffect, useRef, useState } from 'react'
import type { ThemeName } from '../../config/appearance'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'
import { useUiStore } from '../../store/uiStore'
import { useConfigStore } from '../../store/configStore'
import { showErrorToast, showSuccessToast } from '../../store/feedbackStore'

interface Command {
  id: string
  label: string
  icon: string
  category: string
  description: string
  shortcut?: string
  keywords?: string[]
  disabled?: boolean
  action: () => void | Promise<void>
}

function filterAndSortCommands(commands: Command[], query: string) {
  const normalizedQuery = query.trim().toLowerCase()

  return commands
    .map((command) => {
      if (!normalizedQuery) {
        return { command, score: 0 }
      }

      const haystacks = [
        command.label.toLowerCase(),
        command.description.toLowerCase(),
        command.category.toLowerCase(),
        ...(command.keywords ?? []).map((keyword) => keyword.toLowerCase()),
      ]

      const matchIndex = haystacks.findIndex((value) => value.includes(normalizedQuery))
      if (matchIndex === -1) return null

      const score =
        command.label.toLowerCase().startsWith(normalizedQuery)
          ? 0
          : matchIndex

      return { command, score }
    })
    .filter((entry): entry is { command: Command; score: number } => Boolean(entry))
    .sort((left, right) => left.score - right.score || left.command.label.localeCompare(right.command.label))
    .map((entry) => entry.command)
}

export function CommandPalette() {
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  const showCommandPalette = useUiStore((state) => state.showCommandPalette)
  const toggleCommandPalette = useUiStore((state) => state.toggleCommandPalette)
  const toggleTerminal = useUiStore((state) => state.toggleTerminal)
  const toggleSysMonitor = useUiStore((state) => state.toggleSysMonitor)
  const toggleSidebar = useUiStore((state) => state.toggleSidebar)
  const toggleDiagnosticsModal = useUiStore((state) => state.toggleDiagnosticsModal)
  const showSidebarView = useUiStore((state) => state.showSidebarView)
  const appInfo = useUiStore((state) => state.appInfo)
  const updateStatus = useUiStore((state) => state.updateStatus)

  const rootPath = useWorkspaceStore((state) => state.rootPath)
  const openFolder = useWorkspaceStore((state) => state.openFolder)
  const refreshTree = useWorkspaceStore((state) => state.refreshTree)

  const saveTab = useEditorStore((state) => state.saveTab)
  const getActiveTab = useEditorStore((state) => state.getActiveTab)
  const activeGroupId = useEditorStore((state) => state.activeGroupId)
  const splitGroup = useEditorStore((state) => state.splitGroup)
  const closedTabs = useEditorStore((state) => state.closedTabs)
  const reopenClosedTab = useEditorStore((state) => state.reopenClosedTab)

  const theme = useConfigStore((state) => state.theme)
  const setTheme = useConfigStore((state) => state.setTheme)

  const activeTab = getActiveTab()

  const closePalette = () => useUiStore.setState({ showCommandPalette: false })

  const revealSidebarView = (view: 'explorer' | 'search' | 'git') => {
    showSidebarView(view)
  }

  const openSettings = () => {
    useUiStore.setState({ showSettingsModal: true })
  }

  const openDiagnostics = () => {
    toggleDiagnosticsModal()
  }

  const openQuickOpen = () => {
    useUiStore.setState({ showQuickOpen: true })
  }

  const syncGitAction = async (direction: 'pull' | 'push') => {
    if (!rootPath) return

    const result = direction === 'pull'
      ? await window.electronAPI.gitPull(rootPath)
      : await window.electronAPI.gitPush(rootPath)

    if (!result.success) {
      showErrorToast(
        result.error || `${direction === 'pull' ? 'Pull' : 'Push'} failed.`,
        `${direction === 'pull' ? 'Pull' : 'Push'} failed`,
        result.details
      )
      return
    }

    if (direction === 'pull') {
      await refreshTree()
    }

    showSuccessToast(
      direction === 'pull' ? 'Workspace is up to date.' : 'Remote updated successfully.',
      direction === 'pull' ? 'Pull complete' : 'Push complete'
    )
  }

  const setAppTheme = (nextTheme: ThemeName) => {
    setTheme(nextTheme)
    showSuccessToast(`Theme switched to ${nextTheme}.`, 'Appearance updated')
  }

  const checkForUpdates = async () => {
    const result = await window.electronAPI.checkForUpdates()
    if (!result.success) {
      showErrorToast(result.error || 'Unable to check for updates.', 'Update check failed', result.details)
      return
    }

    showSuccessToast('Update check started in the background.', 'Checking for updates')
  }

  const installUpdate = async () => {
    const result = await window.electronAPI.installUpdate()
    if (!result.success) {
      showErrorToast(result.error || 'Unable to install the downloaded update.', 'Install update failed', result.details)
      return
    }

    showSuccessToast('The app will restart to finish installing the update.', 'Installing update')
  }

  const commands: Command[] = [
    {
      id: 'open-folder',
      label: 'Open Folder',
      icon: 'fa-solid fa-folder-open',
      category: 'Workspace',
      description: 'Pick a workspace folder and refresh the tree.',
      shortcut: 'Ctrl+O',
      keywords: ['workspace', 'project', 'directory'],
      action: async () => { await openFolder() },
    },
    {
      id: 'quick-open',
      label: 'Quick Open',
      icon: 'fa-solid fa-file-magnifying-glass',
      category: 'Workspace',
      description: 'Jump to any file in the current workspace.',
      shortcut: 'Ctrl+P',
      keywords: ['files', 'open file'],
      disabled: !rootPath,
      action: () => { openQuickOpen() },
    },
    {
      id: 'refresh-workspace',
      label: 'Refresh Workspace',
      icon: 'fa-solid fa-rotate',
      category: 'Workspace',
      description: 'Reload the file tree and current git status.',
      keywords: ['reload', 'tree', 'refresh'],
      disabled: !rootPath,
      action: async () => { await refreshTree() },
    },
    {
      id: 'save-file',
      label: 'Save File',
      icon: 'fa-solid fa-floppy-disk',
      category: 'Editor',
      description: 'Write the active editor tab to disk.',
      shortcut: 'Ctrl+S',
      keywords: ['write', 'persist'],
      disabled: !activeTab,
      action: async () => {
        if (activeTab) {
          await saveTab(activeTab.id)
        }
      },
    },
    {
      id: 'split-editor',
      label: 'Split Editor Right',
      icon: 'fa-solid fa-columns',
      category: 'Editor',
      description: 'Duplicate the active tab into a second editor pane.',
      keywords: ['layout', 'pane', 'group'],
      disabled: !activeTab,
      action: () => {
        splitGroup(activeGroupId, 'horizontal')
      },
    },
    {
      id: 'reopen-closed-tab',
      label: 'Reopen Closed Tab',
      icon: 'fa-solid fa-clock-rotate-left',
      category: 'Editor',
      description: 'Restore the most recently closed tab into the active group.',
      shortcut: 'Ctrl+Shift+T',
      keywords: ['undo close', 'restore tab'],
      disabled: closedTabs.length === 0,
      action: () => {
        reopenClosedTab(activeGroupId)
      },
    },
    {
      id: 'show-explorer',
      label: 'Show Explorer',
      icon: 'fa-solid fa-folder-tree',
      category: 'Navigation',
      description: 'Reveal the explorer sidebar and file tree.',
      keywords: ['sidebar', 'files'],
      action: () => { revealSidebarView('explorer') },
    },
    {
      id: 'show-search',
      label: 'Show Search',
      icon: 'fa-solid fa-magnifying-glass',
      category: 'Navigation',
      description: 'Open the search panel with filters and replace tools.',
      keywords: ['find in files', 'replace'],
      action: () => { revealSidebarView('search') },
    },
    {
      id: 'show-source-control',
      label: 'Show Source Control',
      icon: 'fa-solid fa-code-branch',
      category: 'Navigation',
      description: 'Jump straight to staged changes and diff preview.',
      keywords: ['git', 'scm', 'diff'],
      action: () => { revealSidebarView('git') },
    },
    {
      id: 'toggle-sidebar',
      label: 'Toggle Sidebar',
      icon: 'fa-solid fa-panels-left',
      category: 'View',
      description: 'Show or hide the left sidebar.',
      shortcut: 'Ctrl+B',
      keywords: ['layout', 'navigation'],
      action: () => { toggleSidebar() },
    },
    {
      id: 'toggle-terminal',
      label: 'Toggle Terminal',
      icon: 'fa-solid fa-terminal',
      category: 'View',
      description: 'Show or hide the integrated terminal.',
      shortcut: 'Ctrl+`',
      keywords: ['console', 'shell'],
      action: () => { toggleTerminal() },
    },
    {
      id: 'toggle-sysmonitor',
      label: 'Toggle System Monitor',
      icon: 'fa-solid fa-chart-line',
      category: 'View',
      description: 'Show or hide live CPU, RAM, GPU, and disk stats.',
      keywords: ['performance', 'stats'],
      action: () => { toggleSysMonitor() },
    },
    {
      id: 'open-settings',
      label: 'Open Settings',
      icon: 'fa-solid fa-gear',
      category: 'View',
      description: 'Adjust appearance, typography, and editor behavior.',
      keywords: ['preferences', 'theme'],
      action: () => { openSettings() },
    },
    {
      id: 'open-diagnostics',
      label: 'Open Diagnostics',
      icon: 'fa-solid fa-stethoscope',
      category: 'Application',
      description: 'Inspect logs, update wiring, and recent runtime errors.',
      keywords: ['logs', 'health', 'errors', 'diagnostics'],
      action: () => { openDiagnostics() },
    },
    {
      id: 'check-for-updates',
      label: 'Check for Updates',
      icon: 'fa-solid fa-cloud-arrow-down',
      category: 'Application',
      description: appInfo?.updateConfigured
        ? 'Look for a newer packaged release.'
        : 'Update checks are enabled only in packaged builds with an update URL.',
      keywords: ['release', 'upgrade', 'download'],
      disabled: !appInfo?.updateConfigured,
      action: async () => { await checkForUpdates() },
    },
    {
      id: 'install-downloaded-update',
      label: 'Install Downloaded Update',
      icon: 'fa-solid fa-rotate',
      category: 'Application',
      description: updateStatus?.state === 'downloaded'
        ? 'Restart the app and finish installing the downloaded release.'
        : 'A downloaded update needs to be ready first.',
      keywords: ['restart', 'upgrade', 'apply update'],
      disabled: updateStatus?.state !== 'downloaded',
      action: async () => { await installUpdate() },
    },
    {
      id: 'theme-dark',
      label: 'Theme: Night Shift',
      icon: 'fa-solid fa-moon',
      category: 'Appearance',
      description: theme === 'dark' ? 'Currently active theme.' : 'Balanced contrast for longer coding sessions.',
      keywords: ['dark', 'theme'],
      action: () => { setAppTheme('dark') },
    },
    {
      id: 'theme-light',
      label: 'Theme: Daylight',
      icon: 'fa-solid fa-sun',
      category: 'Appearance',
      description: theme === 'light' ? 'Currently active theme.' : 'Brighter chrome and cleaner reading surfaces.',
      keywords: ['light', 'theme'],
      action: () => { setAppTheme('light') },
    },
    {
      id: 'theme-amoled',
      label: 'Theme: OLED Black',
      icon: 'fa-solid fa-circle-half-stroke',
      category: 'Appearance',
      description: theme === 'amoled' ? 'Currently active theme.' : 'Pure black panels for higher contrast setups.',
      keywords: ['amoled', 'theme', 'black'],
      action: () => { setAppTheme('amoled') },
    },
    {
      id: 'git-pull',
      label: 'Git Pull',
      icon: 'fa-solid fa-arrow-down',
      category: 'Source Control',
      description: 'Fetch and integrate the latest upstream changes.',
      keywords: ['remote', 'sync', 'update'],
      disabled: !rootPath,
      action: async () => { await syncGitAction('pull') },
    },
    {
      id: 'git-push',
      label: 'Git Push',
      icon: 'fa-solid fa-arrow-up',
      category: 'Source Control',
      description: 'Send your latest commits to the remote branch.',
      keywords: ['remote', 'publish'],
      disabled: !rootPath,
      action: async () => { await syncGitAction('push') },
    },
  ]

  const filtered = filterAndSortCommands(commands, query)

  useEffect(() => {
    if (showCommandPalette) {
      setQuery('')
      setSelectedIndex(0)
      window.setTimeout(() => inputRef.current?.focus(), 40)
    }
  }, [showCommandPalette])

  useEffect(() => {
    setSelectedIndex((current) => {
      if (filtered.length === 0) return 0
      return Math.min(current, filtered.length - 1)
    })
  }, [filtered])

  const executeCommand = async (command: Command | undefined) => {
    if (!command || command.disabled) return
    closePalette()
    await command.action()
  }

  if (!showCommandPalette) return null

  return (
    <div className="palette-overlay" onClick={toggleCommandPalette}>
      <div className="palette" onClick={(event) => event.stopPropagation()}>
        <div className="palette-head">
          <input
            ref={inputRef}
            className="palette-input"
            placeholder="Type a command, theme, or workspace action..."
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setSelectedIndex(0)
            }}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                toggleCommandPalette()
                return
              }

              if (event.key === 'ArrowDown') {
                event.preventDefault()
                if (filtered.length > 0) {
                  setSelectedIndex((current) => (current + 1) % filtered.length)
                }
                return
              }

              if (event.key === 'ArrowUp') {
                event.preventDefault()
                if (filtered.length > 0) {
                  setSelectedIndex((current) => (current - 1 + filtered.length) % filtered.length)
                }
                return
              }

              if (event.key === 'Enter') {
                event.preventDefault()
                void executeCommand(filtered[selectedIndex])
              }
            }}
          />
          <div className="palette-meta">
            <span>{filtered.length} command{filtered.length === 1 ? '' : 's'}</span>
            <span>Arrow keys to move</span>
            <span>Enter to run</span>
          </div>
        </div>

        <div className="palette-list">
          {filtered.map((command, index) => (
            <button
              key={command.id}
              type="button"
              className={`palette-item ${index === selectedIndex ? 'active' : ''} ${command.disabled ? 'disabled' : ''}`}
              onMouseEnter={() => setSelectedIndex(index)}
              onClick={() => { void executeCommand(command) }}
              disabled={command.disabled}
            >
              <span className="palette-icon">
                <i className={command.icon}></i>
              </span>
              <span className="palette-info">
                <span className="palette-row">
                  <span className="palette-label">{command.label}</span>
                  <span className="palette-tag">{command.category}</span>
                </span>
                <span className="palette-sub">
                  {command.disabled ? 'Unavailable until a workspace or active tab is ready.' : command.description}
                </span>
              </span>
              {command.shortcut && <kbd className="palette-shortcut">{command.shortcut}</kbd>}
            </button>
          ))}
          {filtered.length === 0 && (
            <div className="palette-empty">
              <i className="fa-solid fa-sparkles"></i>
              <strong>No matching commands</strong>
              <span>Try “theme”, “git”, “sidebar”, or “open folder”.</span>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
