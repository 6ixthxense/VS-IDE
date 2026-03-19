import type { DiskStat, SysStats } from '@shared/types/ipc'

interface CurrentLoadLike {
  currentLoad: number
}

interface MemoryLike {
  used: number
  total: number
}

interface GraphicsControllerLike {
  vendor?: string | null
  model?: string | null
  utilizationGpu?: number | null
  temperatureGpu?: number | null
  memoryTotal?: number | null
  memoryUsed?: number | null
}

interface GraphicsLike {
  controllers: GraphicsControllerLike[]
}

interface FsSizeLike {
  fs?: string | null
  mount?: string | null
  size?: number | null
  used?: number | null
  use?: number | null
}

interface ProcessLike {
  name: string
  cpu: number
  mem: number
  pid: number
}

interface ProcessesLike {
  list: ProcessLike[]
}

const BYTES_PER_GIB = 1024 ** 3

const formatPercent = (value?: number | null) => (Number.isFinite(value ?? NaN) ? (value ?? 0).toFixed(1) : '0.0')
const formatGigabytes = (value?: number | null) => (((value ?? 0) / BYTES_PER_GIB)).toFixed(0)
const trimValue = (value?: string | null) => value?.trim() ?? ''

function createUniqueDiskId(baseId: string, seenIds: Set<string>) {
  let nextId = baseId || 'disk'
  let suffix = 2

  while (seenIds.has(nextId)) {
    nextId = `${baseId || 'disk'}-${suffix}`
    suffix += 1
  }

  seenIds.add(nextId)
  return nextId
}

export function pickActiveGpu(controllers: GraphicsControllerLike[]) {
  return (
    controllers.find((controller) => (
      controller.vendor?.toLowerCase().includes('nvidia') ||
      controller.model?.toLowerCase().includes('rtx')
    )) ?? controllers[0]
  )
}

export function normalizeDiskStats(disks: FsSizeLike[]): DiskStat[] {
  const seenIds = new Set<string>()

  return disks
    .filter((disk) => Number.isFinite(disk.size) && (disk.size ?? 0) > 0)
    .map((disk, index) => {
      const filesystem = trimValue(disk.fs)
      const mount = trimValue(disk.mount)
      const name = mount || filesystem || `Disk ${index + 1}`
      const idBase = [mount.toLowerCase(), filesystem.toLowerCase()].filter(Boolean).join('|') || `disk-${index + 1}`

      return {
        id: createUniqueDiskId(idBase, seenIds),
        name,
        filesystem,
        mount,
        use: formatPercent(disk.use),
        size: formatGigabytes(disk.size),
        used: formatGigabytes(disk.used),
      }
    })
}

export function buildSysStats({
  load,
  mem,
  graphics,
  disks,
  processes,
}: {
  load: CurrentLoadLike
  mem: MemoryLike
  graphics: GraphicsLike
  disks: FsSizeLike[]
  processes: ProcessesLike
}): SysStats {
  const activeGpu = pickActiveGpu(graphics.controllers)
  const ramPercent = mem.total > 0 ? (mem.used / mem.total) * 100 : 0

  return {
    cpu: formatPercent(load.currentLoad),
    ram: formatPercent(ramPercent),
    ramText: `${(mem.used / BYTES_PER_GIB).toFixed(1)} / ${(mem.total / BYTES_PER_GIB).toFixed(1)} GB`,
    gpuName: activeGpu?.model ?? 'N/A',
    gpu: activeGpu ? {
      load: activeGpu.utilizationGpu ?? 0,
      temp: activeGpu.temperatureGpu ?? 0,
      memTotal: activeGpu.memoryTotal ?? 0,
      memUsed: activeGpu.memoryUsed ?? 0,
    } : null,
    disks: normalizeDiskStats(disks),
    processes: [...processes.list]
      .sort((a, b) => b.cpu - a.cpu)
      .slice(0, 5)
      .map((process) => ({
        name: process.name,
        cpu: process.cpu.toFixed(1),
        mem: process.mem.toFixed(1),
        pid: process.pid,
      })),
  }
}
