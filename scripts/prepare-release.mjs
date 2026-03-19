import fs from 'fs'
import path from 'path'

const rootDir = process.cwd()
const packageJsonPath = path.join(rootDir, 'package.json')
const outputPath = path.join(rootDir, 'build', 'update-config.json')

const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'))
const updateUrl = process.env.VSMONITOR_UPDATE_URL || process.env.VS_MONITOR_UPDATE_URL || ''
const channel = process.env.VSMONITOR_UPDATE_CHANNEL || 'latest'

const payload = {
  version: packageJson.version,
  generatedAt: new Date().toISOString(),
  updateUrl,
  channel,
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true })
fs.writeFileSync(outputPath, `${JSON.stringify(payload, null, 2)}\n`, 'utf8')

if (updateUrl) {
  console.log(`Prepared bundled updater config for ${updateUrl} (${channel}).`)
} else {
  console.log('Prepared bundled updater config without an update URL. Packaged builds will skip auto-updates until one is provided.')
}
