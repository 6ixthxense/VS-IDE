import React, { useEffect, useMemo, useState } from 'react'
import type { TaskDefinition, TaskRun } from '@shared/types/ipc'
import { useTasksStore } from '../../store/tasksStore'
import { useWorkspaceStore } from '../../store/workspaceStore'

function formatTaskTitle(task: TaskDefinition) {
  if (task.source === 'package-script') {
    return task.label
  }

  return task.label.length > 40 ? `${task.label.slice(0, 37)}...` : task.label
}

function relativeTime(timestamp?: string) {
  if (!timestamp) return ''

  const value = Date.parse(timestamp)
  if (!Number.isFinite(value)) return ''

  const diffMinutes = Math.max(0, Math.round((Date.now() - value) / 60000))
  if (diffMinutes < 1) return 'just now'
  if (diffMinutes === 1) return '1 min ago'
  if (diffMinutes < 60) return `${diffMinutes} min ago`

  const diffHours = Math.round(diffMinutes / 60)
  return diffHours === 1 ? '1 hour ago' : `${diffHours} hours ago`
}

function buildCustomTask(rootPath: string, command: string): TaskDefinition {
  const trimmed = command.trim()
  const label = trimmed.length > 48 ? `${trimmed.slice(0, 45)}...` : trimmed

  return {
    id: `custom:${trimmed}`,
    label,
    command: trimmed,
    args: [],
    source: 'custom',
    cwd: rootPath,
    detail: 'Custom command',
    shell: true,
  }
}

function TaskCard({
  task,
  onRun,
}: {
  task: TaskDefinition
  onRun: (task: TaskDefinition) => void | Promise<void>
}) {
  return (
    <div className="task-card">
      <div className="task-card-copy">
        <strong>{formatTaskTitle(task)}</strong>
        <span>{task.detail || task.command}</span>
      </div>
      <div className="task-card-actions">
        <span className={`task-source-pill ${task.source === 'custom' ? 'custom' : ''}`}>
          {task.source === 'package-script' ? 'npm script' : 'custom'}
        </span>
        <button type="button" className="git-preview-btn primary" onClick={() => { void onRun(task) }}>
          <i className="fa-solid fa-play"></i>
          <span>Run</span>
        </button>
      </div>
    </div>
  )
}

function TaskRunCard({
  run,
  onStop,
  onRetry,
}: {
  run: TaskRun
  onStop: (runId: string) => void | Promise<void>
  onRetry: (runId: string) => void | Promise<void>
}) {
  return (
    <div className="task-run-card">
      <div className="task-card-copy">
        <strong>{run.label}</strong>
        <span>{run.command}</span>
      </div>
      <div className="task-run-meta">
        <span className={`task-status-pill ${run.status}`}>{run.status}</span>
        <span>{relativeTime(run.finishedAt || run.startedAt)}</span>
        {run.status === 'running' ? (
          <button type="button" className="git-preview-btn danger" onClick={() => { void onStop(run.runId) }}>
            <i className="fa-solid fa-stop"></i>
            <span>Stop</span>
          </button>
        ) : (
          <button type="button" className="git-preview-btn" onClick={() => { void onRetry(run.runId) }}>
            <i className="fa-solid fa-rotate-right"></i>
            <span>Retry</span>
          </button>
        )}
      </div>
    </div>
  )
}

export function TaskRunnerPanel() {
  const rootPath = useWorkspaceStore((state) => state.rootPath)
  const definitions = useTasksStore((state) => state.definitions)
  const recentTasks = useTasksStore((state) => state.recentTasks)
  const runs = useTasksStore((state) => state.runs)
  const loadingDefinitions = useTasksStore((state) => state.loadingDefinitions)
  const loadDefinitions = useTasksStore((state) => state.loadDefinitions)
  const runTask = useTasksStore((state) => state.runTask)
  const stopTask = useTasksStore((state) => state.stopTask)
  const retryTask = useTasksStore((state) => state.retryTask)
  const [customCommand, setCustomCommand] = useState('')

  useEffect(() => {
    if (rootPath && definitions.length === 0 && !loadingDefinitions) {
      void loadDefinitions()
    }
  }, [definitions.length, loadDefinitions, loadingDefinitions, rootPath])

  const recentCustomTasks = useMemo(
    () => recentTasks.filter((task) => task.source === 'custom'),
    [recentTasks]
  )

  const handleRunCustomTask = async () => {
    if (!rootPath || !customCommand.trim()) return
    await runTask(buildCustomTask(rootPath, customCommand))
    setCustomCommand('')
  }

  return (
    <div className="tasks-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">TASKS</span>
        <div className="sidebar-actions">
          <button onClick={() => { void loadDefinitions() }} title="Refresh Tasks" disabled={!rootPath || loadingDefinitions}>
            <i className={`fa-solid ${loadingDefinitions ? 'fa-spinner fa-spin' : 'fa-rotate-right'}`}></i>
          </button>
        </div>
      </div>

      {!rootPath ? (
        <div className="sidebar-empty">
          <div className="panel-empty-card">
            <span className="panel-empty-eyebrow">Task Runner</span>
            <strong>Open a workspace to discover tasks</strong>
            <p>Run package scripts and one-off commands through the integrated terminal without leaving the IDE.</p>
          </div>
        </div>
      ) : (
        <>
          <div className="tasks-toolbar">
            <label className="search-field">
              <span className="search-field-icon">
                <i className="fa-solid fa-terminal"></i>
              </span>
              <input
                className="search-input"
                placeholder="Custom command, e.g. npm run lint or python main.py"
                value={customCommand}
                onChange={(event) => setCustomCommand(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault()
                    void handleRunCustomTask()
                  }
                }}
              />
            </label>
            <button
              type="button"
              className="search-btn search-btn-primary"
              disabled={!customCommand.trim()}
              onClick={() => { void handleRunCustomTask() }}
            >
              <i className="fa-solid fa-play"></i>
              <span>Run Custom</span>
            </button>
          </div>

          <div className="tasks-results">
            <section className="tasks-section">
              <div className="tasks-section-head">
                <strong>Package Scripts</strong>
                <span>{definitions.length} found</span>
              </div>
              <div className="tasks-list">
                {definitions.length === 0 ? (
                  <div className="database-empty-inline">
                    <i className="fa-solid fa-box-open"></i>
                    <span>No `package.json` scripts found in this workspace yet.</span>
                  </div>
                ) : (
                  definitions.map((task) => (
                    <TaskCard key={task.id} task={task} onRun={runTask} />
                  ))
                )}
              </div>
            </section>

            {recentTasks.length > 0 && (
              <section className="tasks-section">
                <div className="tasks-section-head">
                  <strong>Recent Tasks</strong>
                  <span>{recentTasks.length}</span>
                </div>
                <div className="tasks-list">
                  {recentTasks.map((task) => (
                    <TaskCard key={`recent-${task.id}`} task={task} onRun={runTask} />
                  ))}
                </div>
              </section>
            )}

            {recentCustomTasks.length > 0 && (
              <section className="tasks-section">
                <div className="tasks-section-head">
                  <strong>Custom History</strong>
                  <span>{recentCustomTasks.length}</span>
                </div>
                <div className="tasks-list">
                  {recentCustomTasks.map((task) => (
                    <TaskCard key={`custom-${task.id}`} task={task} onRun={runTask} />
                  ))}
                </div>
              </section>
            )}

            <section className="tasks-section">
              <div className="tasks-section-head">
                <strong>Recent Runs</strong>
                <span>{runs.length}</span>
              </div>
              <div className="tasks-list">
                {runs.length === 0 ? (
                  <div className="database-empty-inline">
                    <i className="fa-solid fa-clock-rotate-left"></i>
                    <span>Run a task and its status will show up here.</span>
                  </div>
                ) : (
                  runs.map((run) => (
                    <TaskRunCard key={run.runId} run={run} onStop={stopTask} onRetry={retryTask} />
                  ))
                )}
              </div>
            </section>
          </div>
        </>
      )}
    </div>
  )
}
