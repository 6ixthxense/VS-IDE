import React, { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useUiStore } from '../store/uiStore'
import { useConfigStore } from '../store/configStore'
import {
  ACCENT_OPTIONS,
  FONT_PRESETS,
  THEME_OPTIONS,
  resolveAccentOption,
} from '../config/appearance'

export function SettingsModal() {
  const { showSettingsModal, toggleSettingsModal } = useUiStore()
  const {
    theme,
    accent,
    setTheme,
    setAccent,
    fontSize,
    setFontSize,
    terminalFontSize,
    setTerminalFontSize,
    fontFamily,
    setFontFamily,
    wordWrap,
    setWordWrap,
    lineNumbers,
    setLineNumbers,
    autoSave,
    setAutoSave,
    tabSize,
    setTabSize,
    resetAppearance,
  } = useConfigStore()

  useEffect(() => {
    if (!showSettingsModal) return

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        toggleSettingsModal()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [showSettingsModal, toggleSettingsModal])

  if (!showSettingsModal) return null

  const activeAccent = resolveAccentOption(accent)
  const activeTheme = THEME_OPTIONS.find((option) => option.id === theme) ?? THEME_OPTIONS[0]

  return createPortal(
    <div className="modal-overlay settings-overlay" onClick={toggleSettingsModal}>
      <div className="modal settings-modal" onClick={(event) => event.stopPropagation()}>
        <div className="settings-hero">
          <div>
            <div className="settings-eyebrow">
              <i className="fa-solid fa-sliders"></i> Appearance Settings
            </div>
            <h3>Make the workspace feel polished and easier to use.</h3>
            <p>
              Theme, accent, and typography update live, so you can tune the look without guessing.
            </p>
          </div>
          <div className="settings-hero-badge" style={{ backgroundImage: activeAccent.gradient }}>
            <span>{activeTheme.label}</span>
            <strong>{activeAccent.name}</strong>
          </div>
        </div>

        <div className="settings-body">
          <div className="settings-grid">
            <section className="settings-card settings-card-wide">
              <div className="settings-section-head">
                <div>
                  <h4>Theme</h4>
                  <p>Choose the overall surface style for panels, editors, and chrome.</p>
                </div>
              </div>

              <div className="settings-theme-grid">
                {THEME_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className={`settings-theme-option ${theme === option.id ? 'active' : ''}`}
                    onClick={() => setTheme(option.id)}
                  >
                    <div
                      className="theme-preview"
                      style={{
                        '--preview-frame': option.preview.frame,
                        '--preview-panel': option.preview.panel,
                        '--preview-surface': option.preview.surface,
                        '--preview-accent': option.preview.accent,
                        '--preview-text': option.preview.text,
                      } as React.CSSProperties}
                    >
                      <div className="theme-preview-top" />
                      <div className="theme-preview-body">
                        <div className="theme-preview-sidebar" />
                        <div className="theme-preview-editor">
                          <span />
                          <span />
                          <span />
                        </div>
                      </div>
                    </div>
                    <div className="settings-option-copy">
                      <strong>{option.label}</strong>
                      <span>{option.description}</span>
                    </div>
                  </button>
                ))}
              </div>
            </section>

            <section className="settings-card">
              <div className="settings-section-head">
                <div>
                  <h4>Accent</h4>
                  <p>Use one highlight color consistently across actions and focus states.</p>
                </div>
              </div>

              <div className="settings-accent-grid">
                {ACCENT_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className={`settings-accent-option ${accent === option.value ? 'active' : ''}`}
                    onClick={() => setAccent(option.value)}
                  >
                    <span className="settings-accent-swatch" style={{ backgroundImage: option.gradient }} />
                    <span className="settings-option-copy">
                      <strong>{option.name}</strong>
                      <span>{option.description}</span>
                    </span>
                  </button>
                ))}
              </div>
            </section>

            <section className="settings-card">
              <div className="settings-section-head">
                <div>
                  <h4>Typography</h4>
                  <p>Dial in editor and terminal readability without touching the rest of the UI.</p>
                </div>
              </div>

              <div className="settings-font-presets">
                {FONT_PRESETS.map((preset) => (
                  <button
                    key={preset.label}
                    type="button"
                    className={`settings-pill ${fontFamily === preset.value ? 'active' : ''}`}
                    onClick={() => setFontFamily(preset.value)}
                  >
                    {preset.label}
                  </button>
                ))}
              </div>

              <label className="settings-field">
                <div className="settings-field-head">
                  <span>Editor font size</span>
                  <strong>{fontSize}px</strong>
                </div>
                <input
                  type="range"
                  min="12"
                  max="24"
                  step="1"
                  value={fontSize}
                  onChange={(event) => setFontSize(Number.parseInt(event.target.value, 10))}
                />
              </label>

              <label className="settings-field">
                <div className="settings-field-head">
                  <span>Terminal font size</span>
                  <strong>{terminalFontSize}px</strong>
                </div>
                <input
                  type="range"
                  min="10"
                  max="22"
                  step="1"
                  value={terminalFontSize}
                  onChange={(event) => setTerminalFontSize(Number.parseInt(event.target.value, 10))}
                />
              </label>

              <label className="settings-field">
                <div className="settings-field-head">
                  <span>Custom editor font stack</span>
                </div>
                <input
                  className="settings-text-input"
                  value={fontFamily}
                  onChange={(event) => setFontFamily(event.target.value)}
                  placeholder="'Cascadia Code', 'Fira Code', Consolas, monospace"
                />
              </label>
            </section>

            <section className="settings-card">
              <div className="settings-section-head">
                <div>
                  <h4>Editor Behavior</h4>
                  <p>Control how the editor wraps, saves, and renders code structure.</p>
                </div>
              </div>

              <div className="settings-toggle-grid">
                <button
                  type="button"
                  className={`settings-toggle-btn ${wordWrap ? 'active' : ''}`}
                  onClick={() => setWordWrap(!wordWrap)}
                >
                  <strong>Word Wrap</strong>
                  <span>{wordWrap ? 'Long lines wrap inside the editor.' : 'Long lines stay on one row.'}</span>
                </button>
                <button
                  type="button"
                  className={`settings-toggle-btn ${lineNumbers ? 'active' : ''}`}
                  onClick={() => setLineNumbers(!lineNumbers)}
                >
                  <strong>Line Numbers</strong>
                  <span>{lineNumbers ? 'Gutter numbers are visible.' : 'A cleaner view without gutters.'}</span>
                </button>
                <button
                  type="button"
                  className={`settings-toggle-btn ${autoSave ? 'active' : ''}`}
                  onClick={() => setAutoSave(!autoSave)}
                >
                  <strong>Auto Save</strong>
                  <span>{autoSave ? 'Files save shortly after edits.' : 'Save happens only when you trigger it.'}</span>
                </button>
              </div>

              <label className="settings-field">
                <div className="settings-field-head">
                  <span>Tab size</span>
                  <strong>{tabSize} spaces</strong>
                </div>
                <input
                  type="range"
                  min="2"
                  max="8"
                  step="1"
                  value={tabSize}
                  onChange={(event) => setTabSize(Number.parseInt(event.target.value, 10))}
                />
              </label>
            </section>

            <section className="settings-card settings-preview-card">
              <div className="settings-section-head">
                <div>
                  <h4>Live Preview</h4>
                  <p>Quick check for contrast, spacing, and code readability.</p>
                </div>
              </div>

              <div className="settings-preview-window">
                <div className="settings-preview-titlebar">
                  <span>VS-Monitor IDE</span>
                  <span className="settings-preview-chip">LIVE</span>
                </div>
                <div className="settings-preview-main">
                  <div className="settings-preview-sidebar">
                    <span />
                    <span />
                    <span />
                  </div>
                  <div className="settings-preview-panel">
                    <div className="settings-preview-tab">
                      <span className="settings-preview-dot" />
                      <span>app.tsx</span>
                    </div>
                    <div className="settings-preview-code" style={{ fontFamily, fontSize }}>
                      <span>{'const cpu = stats.currentLoad'}</span>
                      <span>{'if (cpu > 80) alert("High load")'}</span>
                      <span>{'return <Dashboard accent="live" />'}</span>
                    </div>
                    <div className="settings-preview-terminal" style={{ fontFamily, fontSize: terminalFontSize }}>
                      <span>{'> npm run dev'}</span>
                      <span>{'ready in 423ms'}</span>
                    </div>
                  </div>
                </div>
              </div>
            </section>
          </div>

          <div className="settings-footer">
            <div className="settings-note">
              <i className="fa-solid fa-circle-info"></i>
              <span>
                Active now: {activeTheme.label} with {activeAccent.name}. Changes are saved automatically.
              </span>
            </div>
            <div className="modal-actions">
              <button type="button" className="btn-cancel" onClick={resetAppearance}>
                Reset Defaults
              </button>
              <button type="button" className="btn-confirm" onClick={toggleSettingsModal}>
                Done
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>,
    document.body
  )
}
