import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { customizationHash, readSource } from './sing-box/source.mjs'

const compressionLevel = 10
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')

export function packKernel(bytes, build) {
  if (bytes.length === 0) throw new Error('Cannot package an empty kernel')
  const compressed = Bun.zstdCompressSync(bytes, { level: compressionLevel })
  return {
    compressed,
    metadata: {
      schema_version: 1,
      ...build,
      binary: { bytes: bytes.length, sha256: sha256(bytes) },
      compression: {
        format: 'zstd', level: compressionLevel, bytes: compressed.length,
        sha256: sha256(compressed), bun_version: Bun.version,
      },
    },
  }
}

export function writePackedKernel(base, packed) {
  writeFileSync(`${base}.zst`, packed.compressed)
  writeFileSync(`${base}.meta.json`, `${JSON.stringify(packed.metadata, null, 2)}\n`)
}

if (import.meta.main) {
  const [, , path, target] = process.argv
  if (!path || !['amd64', 'arm64', 'windows-amd64'].includes(target)) {
    throw new Error('Usage: bun pack-kernel.mjs <binary> <amd64|arm64|windows-amd64>')
  }
  const source = readSource()
  const packed = packKernel(readFileSync(path), {
    name: 'miao-kernel',
    target,
    ...source,
    command: './cmd/miao-kernel',
    cgo_enabled: false,
    trimpath: true,
    ldflags: `-s -w -buildid= -X github.com/sagernet/sing-box/constant.Version=${source.version}`,
    customization_sha256: customizationHash(),
  })
  writePackedKernel(path, packed)
  console.log(`Packed ${target}: ${packed.metadata.binary.bytes} -> ${packed.compressed.length} bytes`)
}
