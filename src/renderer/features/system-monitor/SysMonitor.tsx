import type { SysStats } from '@shared/types/ipc'
import { useSysStore } from '../../store/sysStore'

interface SysMonitorProps {
  stats: SysStats | null
}

export function SysMonitor({ stats }: SysMonitorProps) {
  const history = useSysStore(s => s.history)
  const cpuPercent = parseFloat(stats?.cpu ?? '0')
  const ramPercent = parseFloat(stats?.ram ?? '0')
  const disks = stats?.disks ?? []

  const getStatusColor = (percent: number) => {
    if (percent > 85) return 'var(--danger)'
    if (percent > 70) return 'var(--warning)'
    return 'var(--acc)'
  }

  const renderChart = (data: number[], color: string) => {
    if (data.length < 2) return null
    const width = 200
    const height = 40
    const max = 100
    const points = data.map((v, i) => {
      const x = (i / (60 - 1)) * width
      const y = height - (v / max) * height
      return `${x},${y}`
    }).join(' ')

    return (
      <svg width="100%" height={height} viewBox={`0 0 ${width} ${height}`} style={{ marginTop: '8px', overflow: 'visible' }}>
        <polyline
          fill="none"
          stroke={color}
          strokeWidth="1.5"
          strokeLinejoin="round"
          points={points}
        />
        <path
          d={`M ${points} L ${width},${height} L 0,${height} Z`}
          fill={`url(#grad-${color.replace(/[^a-z]/g, '')})`}
          opacity="0.2"
        />
        <defs>
          <linearGradient id={`grad-${color.replace(/[^a-z]/g, '')}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} />
            <stop offset="100%" stopColor="transparent" />
          </linearGradient>
        </defs>
      </svg>
    )
  }

  return (
    <div className="sys-monitor">
      <div className="sys-header">
        <i className="fa-solid fa-gauge-high"></i> SYSTEM MONITOR
      </div>
      
      <div className="sys-card" style={{ '--border-g': getStatusColor(cpuPercent) } as any}>
        <div className="sys-row">
          <div className="sys-title">
            <i className="fa-solid fa-microchip" style={{ color: '#0088ff' }}></i>
            <span>CPU</span>
          </div>
          <span className="sys-value">{stats?.cpu ?? '0'}%</span>
        </div>
        <div className="disk-bar">
          <div className="disk-fill cpu-fill" style={{ width: `${cpuPercent}%` }} />
        </div>
        {renderChart(history.cpu, '#0088ff')}
      </div>

      <div className="sys-card" style={{ '--border-g': getStatusColor(ramPercent) } as any}>
        <div className="sys-row">
          <div className="sys-title">
            <i className="fa-solid fa-memory" style={{ color: '#00cc66' }}></i>
            <span>RAM</span>
          </div>
          <span className="sys-value">{stats?.ramText ?? '0 / 0 GB'}</span>
        </div>
        <div className="disk-bar">
          <div className="disk-fill ram-fill" style={{ width: `${ramPercent}%` }} />
        </div>
        {renderChart(history.ram, '#00cc66')}
      </div>

      {disks.length > 0 && (
        <>
          <div className="sys-header">
            <i className="fa-solid fa-hard-drive"></i> STORAGE
          </div>
          {disks.map((disk) => {
            const diskPercent = parseFloat(disk.use)
            const diskSecondary = disk.filesystem && disk.filesystem !== disk.name
              ? disk.filesystem
              : disk.mount && disk.mount !== disk.name
                ? disk.mount
                : ''

            return (
              <div key={disk.id} className="sys-card" style={{ '--border-g': getStatusColor(diskPercent) } as any}>
                <div className="sys-row">
                  <div className="sys-title-block">
                    <div className="sys-title">
                      <i className="fa-solid fa-hard-drive" style={{ color: '#ff9900' }}></i>
                      <span>{disk.name}</span>
                    </div>
                    {diskSecondary && <div className="sys-subtitle">{diskSecondary}</div>}
                  </div>
                  <span className="sys-value">{disk.use}%</span>
                </div>
                <div className="disk-bar">
                  <div className="disk-fill disk-fill-bar" style={{ width: `${diskPercent}%` }} />
                </div>
                <div className="sys-meta">
                  <span>{disk.used} / {disk.size} GB</span>
                  {diskSecondary && <span>{diskSecondary}</span>}
                </div>
              </div>
            )
          })}
        </>
      )}

      {stats?.gpu ? (
        <div className="sys-card" style={{ '--border-g': getStatusColor(stats.gpu.load) } as any}>
          <div className="sys-row">
            <div className="sys-title">
              <i className="fa-solid fa-cube" style={{ color: '#9c27b0' }}></i>
              <span>{stats.gpuName !== 'N/A' ? stats.gpuName : 'GPU'}</span>
            </div>
            <span className="sys-value">{stats.gpu.load}%</span>
          </div>
          <div className="disk-bar">
            <div className="disk-fill" style={{ width: `${stats.gpu.load}%`, background: 'linear-gradient(90deg, #9c27b0, #ba68c8)' }} />
          </div>
          {renderChart(history.gpu, '#9c27b0')}
          <div style={{ fontSize: '11px', color: 'var(--txt-dim)', marginTop: '4px', display: 'flex', justifyContent: 'space-between' }}>
            <span><i className="fa-solid fa-temperature-three-quarters"></i> {stats.gpu.temp}°C</span>
            <span>VRAM: {stats.gpu.memUsed} / {stats.gpu.memTotal} MB</span>
          </div>
        </div>
      ) : (
        <div className="sys-gpu">
          <i className="fa-solid fa-cube" style={{ color: '#9c27b0' }}></i>
          <span>{stats?.gpuName ?? 'N/A'}</span>
        </div>
      )}

      {stats?.processes && stats.processes.length > 0 && (
        <>
          <div className="sys-header" style={{ marginTop: '16px' }}>
            <i className="fa-solid fa-list-check"></i> TOP PROCESSES
          </div>
          <div className="sys-card">
            {stats.processes.map(p => (
              <div key={p.pid} style={{ display: 'flex', justifyContent: 'space-between', fontSize: '11px', marginBottom: '4px', color: 'var(--txt)' }}>
                <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: '120px' }}>{p.name}</span>
                <span>
                  <span style={{ color: parseFloat(p.cpu) > 10 ? 'var(--warning)' : 'var(--txt-dim)' }}>{p.cpu}% CPU</span>
                </span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  )
}
