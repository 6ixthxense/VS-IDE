import fs from 'fs'
import http from 'http'
import path from 'path'

const relativeDir = process.argv[2] || 'dist'
const port = Number.parseInt(process.env.PORT || '5500', 10)
const rootDir = path.resolve(process.cwd(), relativeDir)

if (!fs.existsSync(rootDir)) {
  console.error(`Release directory not found: ${rootDir}`)
  process.exit(1)
}

const contentTypes = {
  '.yml': 'text/yaml; charset=utf-8',
  '.yaml': 'text/yaml; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.exe': 'application/vnd.microsoft.portable-executable',
  '.blockmap': 'application/octet-stream',
}

const server = http.createServer((request, response) => {
  const requestPath = decodeURIComponent(new URL(request.url || '/', `http://${request.headers.host}`).pathname)
  const normalizedPath = requestPath === '/' ? '/latest.yml' : requestPath
  const filePath = path.resolve(rootDir, `.${normalizedPath}`)

  if (!filePath.startsWith(rootDir)) {
    response.writeHead(403)
    response.end('Forbidden')
    return
  }

  if (!fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
    response.writeHead(404)
    response.end('Not Found')
    return
  }

  response.writeHead(200, {
    'Content-Type': contentTypes[path.extname(filePath).toLowerCase()] || 'application/octet-stream',
    'Cache-Control': 'no-cache',
  })
  fs.createReadStream(filePath).pipe(response)
})

server.listen(port, () => {
  console.log(`Serving ${rootDir} at http://127.0.0.1:${port}`)
  console.log('Point VSMONITOR_UPDATE_URL at this server to smoke-test packaged auto-updates.')
})
