import { afterEach, expect, test } from 'bun:test'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { packKernel, writePackedKernel } from './pack-kernel.mjs'
import { customizationHash, readSource } from './sing-box/source.mjs'
import { recordRules, stageRelease, validateResources, verifyRelease } from './release-artifacts.mjs'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const commit = 'a'.repeat(40)
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'miao-release-test-'))
  roots.push(root)
  mkdirSync(join(root, 'embedded'))
  writeFileSync(join(root, 'Cargo.toml'), '[workspace.package]\nversion = "1.2.3"\n')
  for (const [target, triple, machine] of [['amd64', 'x86_64', 62], ['arm64', 'aarch64', 183]]) {
    writePackedKernel(join(root, 'embedded', `sing-box-${target}`), packKernel(Buffer.from('isolated-kernel-fixture'), {
      name: 'miao-kernel', target, ...readSource(), customization_sha256: customizationHash(),
    }))
    const directory = join(root, 'target', `${triple}-unknown-linux-musl`, 'release')
    mkdirSync(directory, { recursive: true })
    const elf = Buffer.alloc(64)
    elf.set([0x7f, 0x45, 0x4c, 0x46, 2, 1])
    elf.writeUInt16LE(machine, 18)
    writeFileSync(join(directory, 'miao-rust'), elf)
  }
  writeFileSync(join(root, 'embedded', 'geoip-cn.srs'), 'geoip-fixture')
  writeFileSync(join(root, 'embedded', 'geosite-geolocation-cn.srs'), 'geosite-fixture')
  recordRules(join(root, 'embedded'), 'b'.repeat(40), 'c'.repeat(40))
  return { root, stage: join(root, 'dist', 'v1.2.3') }
}

test('release binds both architectures, sources and checksums to one commit/version', () => {
  const { root, stage } = fixture()
  stageRelease(root, stage, commit, 'v1.2.3')
  verifyRelease(stage, commit, 'v1.2.3')
  expect(readFileSync(join(stage, 'miao-embedded-sources.txt'), 'utf8')).toContain(`miao v1.2.3 ${commit}`)
  expect(() => verifyRelease(stage, 'd'.repeat(40), 'v1.2.3')).toThrow('HEAD/version')
  expect(() => verifyRelease(stage, commit, 'v1.2.4')).toThrow('HEAD/version')
  writeFileSync(join(stage, 'miao-rust-linux-arm64'), 'tampered')
  expect(() => verifyRelease(stage, commit, 'v1.2.3')).toThrow('Staged asset changed')
})

test('staging rejects the wrong architecture instead of packaging a native binary twice', () => {
  const { root, stage } = fixture()
  const amd64 = readFileSync(join(root, 'target/x86_64-unknown-linux-musl/release/miao-rust'))
  writeFileSync(join(root, 'target/aarch64-unknown-linux-musl/release/miao-rust'), amd64)
  expect(() => stageRelease(root, stage, commit, 'v1.2.3')).toThrow('Expected arm64 ELF')
})

test('resource reuse rejects test stubs, changed customization and damaged bundles', () => {
  const { root } = fixture()
  const path = join(root, 'embedded/sing-box-amd64.meta.json')
  const original = JSON.parse(readFileSync(path, 'utf8'))
  for (const override of [{ test_stub: true }, { customization_sha256: 'stale' }, { revision: 'd'.repeat(40) }]) {
    writeFileSync(path, JSON.stringify({ ...original, ...override }))
    expect(() => validateResources(root, 'amd64')).toThrow()
  }
  writeFileSync(path, JSON.stringify(original))
  writeFileSync(join(root, 'embedded/sing-box-amd64.zst'), 'damaged')
  expect(() => validateResources(root, 'amd64')).toThrow('checksum mismatch')
})

test('resource reuse rejects stale rule contents and unpinned source refs', () => {
  const { root } = fixture()
  expect(() => recordRules(join(root, 'embedded'), 'rule-set', 'release')).toThrow('pinned commits')
  writeFileSync(join(root, 'embedded/geoip-cn.srs'), '')
  expect(() => validateResources(root, 'amd64')).toThrow('Rule checksum mismatch')
})
