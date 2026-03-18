import React from 'react'
import { useUiStore } from '../store/uiStore'

export function ActivityBar() {
  const { activeSidebarView, setActiveSidebarView, showSidebar, toggleSidebar, toggleSysMonitor, toggleSettingsModal } = useUiStore()

  const handleTabClick = (view: 'explorer' | 'search' | 'git') => {
    if (activeSidebarView === view) {
      toggleSidebar()
    } else {
      setActiveSidebarView(view)
      if (!showSidebar) toggleSidebar()
    }
  }

  return (
    <div className="activity-bar">
      <div className="activity-top">
        <button 
          className={`activity-btn ${activeSidebarView === 'explorer' && showSidebar ? 'active' : ''}`}
          onClick={() => handleTabClick('explorer')}
          title="Explorer"
        >
          <i className="fa-solid fa-folder"></i>
        </button>
        <button 
          className={`activity-btn ${activeSidebarView === 'search' && showSidebar ? 'active' : ''}`}
          onClick={() => handleTabClick('search')}
          title="Search"
        >
          <i className="fa-solid fa-magnifying-glass"></i>
        </button>
        <button 
          className={`activity-btn ${activeSidebarView === 'git' && showSidebar ? 'active' : ''}`}
          onClick={() => handleTabClick('git')}
          title="Source Control"
        >
          <i className="fa-solid fa-code-branch"></i>
        </button>
        <button className="activity-btn" onClick={toggleSysMonitor} title="Toggle System Monitor">
          <i className="fa-solid fa-chart-line"></i>
        </button>
      </div>

      <div className="activity-bottom">
        <button className="activity-btn" onClick={toggleSettingsModal} title="Settings">
          <i className="fa-solid fa-gear"></i>
        </button>
      </div>
    </div>
  )
}
