import React from 'react'
import { useEditorStore } from '../store/editorStore'
import type { Tab } from '@shared/types/file'

function getFileIconClass(name: string): string {
  if (!name) return 'fa-regular fa-file'
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  const icons: Record<string, string> = {
    ts: 'fa-solid fa-circle-chevron-right',
    tsx: 'fa-brands fa-react',
    js: 'fa-brands fa-js',
    jsx: 'fa-brands fa-react',
    json: 'fa-solid fa-code',
    html: 'fa-brands fa-html5',
    css: 'fa-brands fa-css3-alt',
    py: 'fa-brands fa-python',
    md: 'fa-solid fa-file-lines',
    sh: 'fa-solid fa-terminal'
  }
  return icons[ext] ?? 'fa-regular fa-file'
}

function getFileIconColor(name: string): string {
  if (!name) return '#a0a0a0'
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  const colors: Record<string, string> = {
    ts: '#007acc', tsx: '#00d8ff', js: '#f7df1e', jsx: '#00d8ff',
    json: '#ffb300', html: '#e44d26', css: '#244ff2', py: '#3776ab',
    md: '#03a9f4', sh: '#4caf50'
  }
  return colors[ext] ?? '#a0a0a0'
}

export function Tabs({ groupId }: { groupId: string }) {
  const tabs = useEditorStore(s => s.tabs)
  const group = useEditorStore(s => s.groups.find(g => g.id === groupId))
  const closeTab = useEditorStore(s => s.closeTab)
  const setActiveGroupId = useEditorStore(s => s.setActiveGroupId)
  const setActiveTab = useEditorStore(s => s.setActiveTab)

  if (!group || group.tabIds.length === 0) return null

  const groupTabs = group.tabIds
    .map(id => tabs.find(t => t.id === id))
    .filter(Boolean) as Tab[]

  return (
    <div className="tabs-bar" onClick={() => setActiveGroupId(groupId)}>
      {groupTabs.map(tab => {
        const isDirty = tab.content !== tab.savedContent
        const isActive = tab.id === group.activeTabId
        return (
          <div
            key={tab.id}
            className={`tab ${isActive ? 'tab-active' : ''}`}
            onClick={() => {
              setActiveTab(groupId, tab.id)
            }}
          >
            <span className="tab-name">
              <i
                className={getFileIconClass(tab.name)}
                style={{ color: getFileIconColor(tab.name), marginRight: '6px', fontSize: '13px' }}
              />
              {isDirty && <span className="tab-dirty" style={{ color: '#ff9900', marginRight: '4px' }}>●</span>}
              {tab.recoveryState && (
                <span
                  className="tab-recovery-indicator"
                  title={tab.recoveryMessage}
                  style={{ color: tab.recoveryState === 'conflict' ? '#ff6b6b' : '#4fc3f7', marginRight: '4px' }}
                >
                  {tab.recoveryState === 'conflict' ? '!' : '↺'}
                </span>
              )}
              {tab.name}
            </span>
            <button
              className="tab-close"
              onClick={e => { e.stopPropagation(); closeTab(tab.id, groupId) }}
              title="Close"
            >
              ×
            </button>
          </div>
        )
      })}
    </div>
  )
}
