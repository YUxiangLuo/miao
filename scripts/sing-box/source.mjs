import { createHash } from 'node:crypto'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const directory = dirname(fileURLToPath(import.meta.url))

export function readSource() {
  const source = JSON.parse(readFileSync(join(directory, 'source.json'), 'utf8'))
  const validNames = values => Array.isArray(values) && values.length > 0
    && new Set(values).size === values.length
    && values.every(value => typeof value === 'string' && /^[a-z0-9_]+$/.test(value))
  if (!/^https:\/\/github\.com\/[^\s]+\.git$/.test(source.repository)
    || !/^[0-9a-f]{40}$/.test(source.revision)
    || !/^1\.\d+\.\d+$/.test(source.go_version)
    || !/^[0-9A-Za-z.+-]+$/.test(source.version)
    || !/^[a-z0-9-]+$/.test(source.profile)
    || !Array.isArray(source.build_tags)
    || source.build_tags.length === 0
    || source.build_tags.some(tag => !/^[a-z0-9_]+$/.test(tag))
    || !validNames(source.node_protocols)
    || !validNames(source.dns_transports)
    || source.tun_stack !== 'go') {
    throw new Error('Invalid pinned kernel source in scripts/sing-box/source.json')
  }
  return source
}

// Includes the patch, command preparation and regression tests. Paths are
// relative, so a checkout location or build timestamp cannot change this hash.
export function customizationHash() {
  const files = []
  function visit(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name)
      if (entry.isDirectory()) visit(path)
      else if (/\.(json|mjs|go|patch)$/.test(entry.name)) files.push(path)
    }
  }
  visit(directory)
  const hash = createHash('sha256')
  for (const path of files.sort()) {
    hash.update(relative(directory, path).replaceAll('\\', '/')).update('\0')
    // Git may check out CRLF on Windows; all customization inputs are text.
    hash.update(readFileSync(path, 'utf8').replaceAll('\r\n', '\n')).update('\0')
  }
  return hash.digest('hex')
}

if (import.meta.main) {
  const source = readSource()
  const field = process.argv[2]
  if (!field) console.log(JSON.stringify(source, null, 2))
  else if (Object.hasOwn(source, field)) {
    console.log(Array.isArray(source[field]) ? source[field].join(',') : source[field])
  } else {
    throw new Error(`Unknown kernel source field: ${field}`)
  }
}
