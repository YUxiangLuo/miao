import { appendFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { packKernel, writePackedKernel } from './pack-kernel.mjs'

const directory = fileURLToPath(new URL('../embedded/', import.meta.url))
const [, , createdList] = process.argv
mkdirSync(directory, { recursive: true })

function record(path) {
  if (createdList) appendFileSync(createdList, `${path}\n`)
}

// Only prepare missing assets. Never turn an existing real kernel into a stub.
for (const name of ['sing-box-amd64', 'sing-box-arm64', 'sing-box-windows-amd64.exe']) {
  const base = `${directory}${name}`
  const compressedExists = existsSync(`${base}.zst`)
  const metadataExists = existsSync(`${base}.meta.json`)
  if (compressedExists && metadataExists) continue
  if (compressedExists || metadataExists) {
    throw new Error(`Incomplete embedded kernel ${name}; rebuild embedded assets`)
  }
  record(`${base}.zst`)
  record(`${base}.meta.json`)
  writePackedKernel(base, packKernel(
    Buffer.from('#!/bin/sh\necho "inert test-only kernel" >&2\nexit 1\n'),
    { name: 'inert-test-kernel', test_stub: true },
  ))
}

for (const name of ['geoip-cn.srs', 'geosite-geolocation-cn.srs']) {
  const path = `${directory}${name}`
  if (existsSync(path)) continue
  record(path)
  writeFileSync(path, '')
}
