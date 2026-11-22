import { afterEach, expect, test } from 'bun:test'
import { mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { packKernel, writePackedKernel } from '../lib/pack-kernel.mjs'
import { customizationHash, readSource } from '../sing-box/source.mjs'
import {
  assembleRelease,
  formatReleaseSummary,
  recordRules,
  RELEASE_ASSETS,
  stageLinuxRelease,
  stageWindowsRelease,
  validateResources,
  verifyRelease,
  WINDOWS_INSTALLER_ASSET,
} from '../lib/release-artifacts.mjs'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const commit = 'a'.repeat(40)

function makePeStub(machine = 0x014c) {
  const pe = Buffer.alloc(128)
  pe.write('MZ', 0, 'ascii')
  pe.writeUInt32LE(64, 0x3c)
  pe.set([0x50, 0x45, 0x00, 0x00], 64)
  pe.writeUInt16LE(machine, 68)
  return pe
}

function fixture({ geoip = 'b'.repeat(40), direct = 'c'.repeat(40) } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'harbor-release-test-'))
  roots.push(root)
  mkdirSync(join(root, 'embedded'))
  writeFileSync(join(root, 'Cargo.toml'), '[workspace.package]\nversion = "1.2.3"\n')
  for (const [target, triple, machine] of [['amd64', 'x86_64', 62], ['arm64', 'aarch64', 183]]) {
    writePackedKernel(join(root, 'embedded', `sing-box-${target}`), packKernel(Buffer.from('isolated-kernel-fixture'), {
      name: 'harbor-kernel', target, ...readSource(), customization_sha256: customizationHash(),
    }))
    const directory = join(root, 'target', `${triple}-unknown-linux-musl`, 'release')
    mkdirSync(directory, { recursive: true })
    const elf = Buffer.alloc(64)
    elf.set([0x7f, 0x45, 0x4c, 0x46, 2, 1])
    elf.writeUInt16LE(machine, 18)
    writeFileSync(join(directory, 'harbor-cli'), elf)
  }
  writePackedKernel(join(root, 'embedded', 'sing-box-windows-amd64.exe'), packKernel(Buffer.from('isolated-win-kernel-fixture'), {
    name: 'harbor-kernel', target: 'windows-amd64', ...readSource(), customization_sha256: customizationHash(),
  }))
  const nsisDir = join(root, 'target', 'release', 'bundle', 'nsis')
  mkdirSync(nsisDir, { recursive: true })
  writeFileSync(join(nsisDir, 'Harbor_1.2.3_x64-setup.exe'), makePeStub(0x014c))
  writeFileSync(join(root, 'embedded', 'geoip-cn.srs'), 'geoip-fixture')
  writeFileSync(join(root, 'embedded', 'geosite-geolocation-cn.srs'), 'geosite-fixture')
  recordRules(join(root, 'embedded'), geoip, direct)
  return { root, stage: join(root, 'dist', 'v1.2.3') }
}

test('stages Linux and Windows independently, assembles v3 manifest and verifies all 6 release assets', () => {
  const { root, stage } = fixture()
  stageLinuxRelease(root, stage, commit, 'v1.2.3')
  stageWindowsRelease(root, stage, commit, 'v1.2.3')
  assembleRelease(stage, commit, 'v1.2.3')
  verifyRelease(stage, commit, 'v1.2.3')

  expect(readdirSync(stage).sort()).toEqual([
    'harbor-linux-amd64', 'harbor-linux-amd64.sha256',
    'harbor-linux-arm64', 'harbor-linux-arm64.sha256',
    'harbor-windows-amd64-setup.exe', 'harbor-windows-amd64-setup.exe.sha256',
    'release-manifest.json',
  ])
  if (Bun.which('sha256sum')) {
    for (const asset of RELEASE_ASSETS) {
      const result = Bun.spawnSync(['sha256sum', '-c', `${asset}.sha256`], {
        cwd: stage, stdout: 'pipe', stderr: 'pipe',
      })
      expect(result.exitCode).toBe(0)
    }
  }
  const manifest = JSON.parse(readFileSync(join(stage, 'release-manifest.json'), 'utf8'))
  expect(manifest.schema_version).toBe(3)
  expect(manifest.kernels.amd64.revision).toBe(readSource().revision)
  expect(manifest.kernels['windows-amd64'].revision).toBe(readSource().revision)
  expect(manifest.rules.sources).toEqual({ 'sing-geoip': 'b'.repeat(40), 'v2ray-rules-dat': 'c'.repeat(40) })

  const summary = formatReleaseSummary(stage, 'https://github.com/YUxiangLuo/miao/releases/tag/v1.2.3')
  expect(summary).toContain('## Harbor v1.2.3')
  expect(summary).toContain(WINDOWS_INSTALLER_ASSET)
  expect(summary).toContain('https://github.com/YUxiangLuo/miao/releases/tag/v1.2.3')

  expect(() => verifyRelease(stage, 'd'.repeat(40), 'v1.2.3')).toThrow('HEAD/version')
  expect(() => verifyRelease(stage, commit, 'v1.2.4')).toThrow('HEAD/version')
  const binary = readFileSync(join(stage, 'harbor-linux-arm64'))
  binary[63] ^= 1
  writeFileSync(join(stage, 'harbor-linux-arm64'), binary)
  expect(() => verifyRelease(stage, commit, 'v1.2.3')).toThrow('Staged asset changed')
})

test('publishing rejects missing, incorrect or malformed checksum files for any platform asset', () => {
  const { root, stage } = fixture()
  stageLinuxRelease(root, stage, commit, 'v1.2.3')
  stageWindowsRelease(root, stage, commit, 'v1.2.3')
  assembleRelease(stage, commit, 'v1.2.3')
  for (const name of RELEASE_ASSETS) {
    const path = join(stage, `${name}.sha256`)
    const original = readFileSync(path, 'utf8')
    for (const content of [
      `${'0'.repeat(64)}  ${name}\n`,
      original.replace(name, 'other-binary'),
      `${original}${original}`,
      '',
    ]) {
      writeFileSync(path, content)
      expect(() => verifyRelease(stage, commit, 'v1.2.3')).toThrow('Staged checksum changed')
    }
    rmSync(path)
    expect(() => verifyRelease(stage, commit, 'v1.2.3')).toThrow()
    writeFileSync(path, original)
  }
  expect(() => verifyRelease(stage, commit, 'v1.2.3')).not.toThrow()
})

test('staging rejects wrong ELF architectures and non-PE Windows installers', () => {
  const { root, stage } = fixture()
  const amd64 = readFileSync(join(root, 'target/x86_64-unknown-linux-musl/release/harbor-cli'))
  writeFileSync(join(root, 'target/aarch64-unknown-linux-musl/release/harbor-cli'), amd64)
  expect(() => stageLinuxRelease(root, stage, commit, 'v1.2.3')).toThrow('Expected arm64 ELF')

  writeFileSync(join(root, 'target/release/bundle/nsis/Harbor_1.2.3_x64-setup.exe'), amd64)
  expect(() => stageWindowsRelease(root, stage, commit, 'v1.2.3')).toThrow('Expected Windows PE installer')
})

test('assembly rejects mismatched geo-rule source commits between Linux and Windows builds', () => {
  const linux = fixture({ geoip: 'b'.repeat(40), direct: 'c'.repeat(40) })
  const windows = fixture({ geoip: 'e'.repeat(40), direct: 'c'.repeat(40) })
  stageLinuxRelease(linux.root, linux.stage, commit, 'v1.2.3')
  stageWindowsRelease(windows.root, linux.stage, commit, 'v1.2.3')
  expect(() => assembleRelease(linux.stage, commit, 'v1.2.3')).toThrow('Rule sources differ')
})

test('verification requires a complete schema v3 manifest, including all kernels and rule provenance', () => {
  const { root, stage } = fixture()
  stageLinuxRelease(root, stage, commit, 'v1.2.3')
  stageWindowsRelease(root, stage, commit, 'v1.2.3')
  assembleRelease(stage, commit, 'v1.2.3')
  const path = join(stage, 'release-manifest.json')
  const original = JSON.parse(readFileSync(path, 'utf8'))
  for (const mutate of [
    manifest => { manifest.schema_version = 2 },
    manifest => { delete manifest.files[WINDOWS_INSTALLER_ASSET] },
    manifest => { manifest.kernels['windows-amd64'].revision = 'stale' },
    manifest => { manifest.kernels.arm64.revision = 'stale' },
    manifest => { delete manifest.rules.files['geoip-cn.srs'] },
  ]) {
    const manifest = structuredClone(original)
    mutate(manifest)
    writeFileSync(path, JSON.stringify(manifest))
    expect(() => verifyRelease(stage, commit, 'v1.2.3')).toThrow()
  }
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
  expect(() => validateResources(root, 'windows-amd64')).not.toThrow()
  expect(() => recordRules(join(root, 'embedded'), 'rule-set', 'release')).toThrow('pinned commits')
  writeFileSync(join(root, 'embedded/geoip-cn.srs'), '')
  expect(() => validateResources(root, 'amd64')).toThrow('Rule checksum mismatch')
})
