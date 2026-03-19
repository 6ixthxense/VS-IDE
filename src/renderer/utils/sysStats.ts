import type { DiskStat, SysStats } from '@shared/types/ipc'

type LegacyDiskStat = {
  use?: string
  size?: string
  used?: string
} | null

type SysStatsPayload = Partial<SysStats> & {
  disk?: LegacyDiskStat
  disks?: DiskStat[] | null
}

function normalizeDiskName(index: number, disk?: Partial<DiskStat>) {
  if (disk?.name?.trim()) return disk.name.trim()
  if (disk?.mount?.trim()) return disk.mount.trim()
  if (disk?.filesystem?.trim()) return disk.filesystem.trim()
  return `Disk ${index + 1}`
}

function normalizeDiskEntry(index: number, disk?: Partial<DiskStat> | null): DiskStat {
  const name = normalizeDiskName(index, disk)

  return {
    id: disk?.id?.trim() || `${name.toLowerCase()}-${index + 1}`,
    name,
    filesystem: disk?.filesystem?.trim() ?? '',
    mount: disk?.mount?.trim() ?? '',
    use: disk?.use ?? '0.0',
    size: disk?.size ?? '0',
    used: disk?.used ?? '0',
  }
}

export function normalizeSysStats(stats: SysStatsPayload): SysStats {
  const disks = Array.isArray(stats.disks) && stats.disks.length > 0
    ? stats.disks.map((disk, index) => normalizeDiskEntry(index, disk))
    : stats.disk
      ? [normalizeDiskEntry(0, {
          id: 'legacy-disk-1',
          name: 'DISK',
          use: stats.disk.use,
          size: stats.disk.size,
          used: stats.disk.used,
        })]
      : []

  return {
    cpu: stats.cpu ?? '0.0',
    ram: stats.ram ?? '0.0',
    ramText: stats.ramText ?? '0.0 / 0.0 GB',
    gpuName: stats.gpuName ?? 'N/A',
    gpu: stats.gpu ?? null,
    disks,
    processes: Array.isArray(stats.processes) ? stats.processes : [],
  }
}
