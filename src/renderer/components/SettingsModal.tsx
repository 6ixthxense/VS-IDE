import React, { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useUiStore } from '../store/uiStore'
import { useConfigStore } from '../store/configStore'

const ACCENT_COLORS = [
  { name: 'Default Blue', value: '#007acc', grad: 'linear-gradient(135deg, #007acc, #005a9e)' },
  { name: 'Emerald Green', value: '#4caf50', grad: 'linear-gradient(135deg, #4caf50, #2e7d32)' },
  { name: 'Royal Purple', value: '#9c27b0', grad: 'linear-gradient(135deg, #9c27b0, #6a1b9a)' },
  { name: 'Amber Orange', value: '#ff9800', grad: 'linear-gradient(135deg, #ff9800, #e65100)' },
  { name: 'Crimson Red', value: '#f44336', grad: 'linear-gradient(135deg, #f44336, #b71c1c)' }
]

export function SettingsModal() {
  const { showSettingsModal, toggleSettingsModal } = useUiStore()
  const { theme, setTheme, fontSize, setFontSize, fontFamily, setFontFamily } = useConfigStore()
  const [activeAccent, setActiveAccent] = useState('#007acc')

  useEffect(() => {
    const saved = localStorage.getItem('vs-monitor-accent')
    const savedGrad = localStorage.getItem('vs-monitor-accent-grad')
    if (saved) {
      setActiveAccent(saved)
      document.documentElement.style.setProperty('--acc', saved)
      if (savedGrad) document.documentElement.style.setProperty('--acc-grad', savedGrad)
    }
  }, [])

  const changeAccent = (color: string, grad: string) => {
    setActiveAccent(color)
    localStorage.setItem('vs-monitor-accent', color)
    localStorage.setItem('vs-monitor-accent-grad', grad)
    document.documentElement.style.setProperty('--acc', color)
    document.documentElement.style.setProperty('--acc-grad', grad)
  }

  if (!showSettingsModal) return null

  return createPortal(
    <div className="modal-overlay" onClick={toggleSettingsModal}>
      <div className="modal" onClick={e => e.stopPropagation()}>
        <h3><i className="fa-solid fa-gear"></i> Settings</h3>
        
        <div style={{ marginBottom: '20px' }}>
          <div style={{ fontSize: '12px', color: 'var(--txt-bright)', marginBottom: '8px' }}>
            Accent Color
          </div>
          <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
            {ACCENT_COLORS.map(c => (
              <button
                key={c.value}
                onClick={() => changeAccent(c.value, c.grad)}
                style={{
                  width: '24px',
                  height: '24px',
                  borderRadius: '50%',
                  backgroundColor: c.value,
                  border: activeAccent === c.value ? '2px solid #fff' : '1px solid rgba(255,255,255,0.2)',
                  cursor: 'pointer',
                  transform: activeAccent === c.value ? 'scale(1.1)' : 'none',
                  transition: 'transform 0.1s',
                  boxShadow: activeAccent === c.value ? `0 0 8px ${c.value}` : 'none'
                }}
                title={c.name}
              />
            ))}
          </div>
        </div>

        <div style={{ marginBottom: '20px' }}>
          <div style={{ fontSize: '12px', color: 'var(--txt-bright)', marginBottom: '8px' }}>Theme</div>
          <div style={{ display: 'flex', gap: '8px' }}>
            {['dark', 'light', 'amoled'].map(t => (
              <button
                key={t}
                onClick={() => setTheme(t as any)}
                className="btn-cancel"
                style={{ 
                  flex: 1, 
                  border: theme === t ? '1px solid var(--acc)' : 'none',
                  background: theme === t ? 'rgba(0, 122, 204, 0.1)' : 'rgba(255,255,255,0.05)',
                  textTransform: 'capitalize' 
                }}
              >
                {t}
              </button>
            ))}
          </div>
        </div>

        <div style={{ marginBottom: '20px', display: 'flex', gap: '16px' }}>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: '12px', color: 'var(--txt-bright)', marginBottom: '8px' }}>Font Size (px)</div>
            <input 
              type="number" 
              value={fontSize} 
              onChange={e => setFontSize(parseInt(e.target.value))}
              style={{ padding: '6px 10px' }}
            />
          </div>
          <div style={{ flex: 2 }}>
            <div style={{ fontSize: '12px', color: 'var(--txt-bright)', marginBottom: '8px' }}>Font Family</div>
            <input 
              value={fontFamily} 
              onChange={e => setFontFamily(e.target.value)}
              style={{ padding: '6px 10px' }}
            />
          </div>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', borderTop: '1px solid rgba(255,255,255,0.05)', paddingTop: '16px' }}>
          <div style={{ fontSize: '11px', color: 'var(--txt-dim)' }}>
            <i className="fa-solid fa-circle-info"></i> More settings coming soon...
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn-confirm" onClick={toggleSettingsModal}>Done</button>
        </div>
      </div>
    </div>,
    document.body
  )
}
