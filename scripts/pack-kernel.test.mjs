import { expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { packKernel } from './pack-kernel.mjs'

test('kernel package round-trips binary bytes and records both hashes', () => {
  const bytes = Buffer.alloc(300_000)
  for (let i = 0; i < bytes.length; i++) bytes[i] = i % 251
  const build = { name: 'test-kernel', revision: 'test-revision' }
  const first = packKernel(bytes, build)
  const second = packKernel(bytes, build)
  expect(Bun.zstdDecompressSync(first.compressed)).toEqual(bytes)
  expect(first).toEqual(second)
  expect(first.compressed.length).toBeLessThan(bytes.length)
  expect(first.metadata.binary).toEqual({
    bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'),
  })
  expect(first.metadata.compression.sha256).toBe(
    createHash('sha256').update(first.compressed).digest('hex'),
  )
  expect(first.metadata.revision).toBe(build.revision)
})

test('empty kernels are rejected before packaging', () => {
  expect(() => packKernel(Buffer.alloc(0), {})).toThrow('empty kernel')
})
