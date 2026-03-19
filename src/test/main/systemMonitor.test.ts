import { describe, expect, it } from 'vitest'
import { buildSysStats, normalizeDiskStats } from '../../main/services/systemMonitor'

const GIB = 1024 ** 3

describe('systemMonitor helpers', () => {
  it('keeps every disk and labels each entry separately', () => {
    const disks = normalizeDiskStats([
      { fs: 'C:', mount: 'C:', size: 512 * GIB, used: 256 * GIB, use: 50 },
      { fs: 'D:', mount: 'D:', size: 1024 * GIB, used: 200 * GIB, use: 19.5 },
    ])

    expect(disks).toMatchObject([
      { name: 'C:', filesystem: 'C:', mount: 'C:', use: '50.0', size: '512', used: '256' },
      { name: 'D:', filesystem: 'D:', mount: 'D:', use: '19.5', size: '1024', used: '200' },
    ])
    expect(new Set(disks.map((disk) => disk.id)).size).toBe(2)
  })

  it('builds the monitor payload with all disks, gpu, and sorted processes', () => {
    const stats = buildSysStats({
      load: { currentLoad: 12.34 },
      mem: { used: 8 * GIB, total: 16 * GIB },
      graphics: {
        controllers: [
          {
            vendor: 'NVIDIA',
            model: 'RTX 4070',
            utilizationGpu: 44,
            temperatureGpu: 66,
            memoryTotal: 12288,
            memoryUsed: 2048,
          },
        ],
      },
      disks: [
        { fs: '/dev/disk1s1', mount: '/', size: 500 * GIB, used: 250 * GIB, use: 50 },
        { fs: '/dev/disk2s1', mount: '/data', size: 1000 * GIB, used: 400 * GIB, use: 40 },
      ],
      processes: {
        list: [
          { name: 'helper', cpu: 5.1, mem: 1.5, pid: 3 },
          { name: 'renderer', cpu: 18.7, mem: 4.2, pid: 2 },
          { name: 'electron', cpu: 30.2, mem: 6.3, pid: 1 },
        ],
      },
    })

    expect(stats.cpu).toBe('12.3')
    expect(stats.ram).toBe('50.0')
    expect(stats.gpuName).toBe('RTX 4070')
    expect(stats.disks).toHaveLength(2)
    expect(stats.disks[0]).toMatchObject({ name: '/', filesystem: '/dev/disk1s1', mount: '/' })
    expect(stats.processes.map((process) => process.name)).toEqual(['electron', 'renderer', 'helper'])
  })
})
