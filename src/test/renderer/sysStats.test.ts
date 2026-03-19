import { describe, expect, it } from 'vitest'
import { normalizeSysStats } from '../../renderer/utils/sysStats'

describe('normalizeSysStats', () => {
  it('accepts the new disks payload safely', () => {
    const stats = normalizeSysStats({
      cpu: '12.3',
      ram: '45.6',
      ramText: '7.3 / 16.0 GB',
      gpuName: 'RTX 4070',
      gpu: { load: 33, temp: 60, memTotal: 12000, memUsed: 2400 },
      disks: [{ id: 'c-1', name: 'C:', filesystem: 'C:', mount: 'C:', use: '55.0', size: '512', used: '282' }],
      processes: [],
    })

    expect(stats.disks).toHaveLength(1)
    expect(stats.disks[0].name).toBe('C:')
  })

  it('falls back to the legacy disk payload without crashing', () => {
    const stats = normalizeSysStats({
      cpu: '12.3',
      ram: '45.6',
      ramText: '7.3 / 16.0 GB',
      gpuName: 'N/A',
      disk: { use: '66.0', size: '1000', used: '660' },
    })

    expect(stats.disks).toEqual([
      expect.objectContaining({ name: 'DISK', use: '66.0', size: '1000', used: '660' }),
    ])
    expect(stats.processes).toEqual([])
  })
})
