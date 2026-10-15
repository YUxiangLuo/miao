import { createHash } from 'node:crypto'
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { customizationHash, readSource } from '../sing-box/source.mjs'

const targets = { amd64: ['x86_64-unknown-linux-musl', 62], arm64: ['aarch64-unknown-linux-musl', 183] }
const ruleFiles = ['geoip-cn.srs', 'geosite-geolocation-cn.srs']
const assetNames = Object.keys(targets).map(target => `miao-rust-linux-${target}`)
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')
const checksumLine = (name, digest) => `${digest}  ${name}\n`
const json = path => JSON.parse(readFileSync(path, 'utf8'))
const writeJson = (path, data) => writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`)
const ensure = (condition, message) => { if (!condition) throw new Error(message) }

export function version(root = '.') {
  const value = Bun.TOML.parse(readFileSync(join(root, 'Cargo.toml'), 'utf8')).workspace.package.version
  ensure(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value), 'Expected workspace version X.Y.Z')
  return value
}

function validateKernelMetadata(metadata, target) {
  ensure(Object.hasOwn(targets, target), `Unsupported target: ${target}`)
  ensure(metadata.schema_version === 1 && metadata.name === 'miao-kernel' && !metadata.test_stub, 'Real kernel required')
  ensure(metadata.target === target, 'Kernel target mismatch')
  for (const [key, value] of Object.entries(readSource())) {
    ensure(JSON.stringify(metadata[key]) === JSON.stringify(value), `Stale kernel ${key}`)
  }
  ensure(metadata.customization_sha256 === customizationHash(), 'Stale kernel customization')
}

export function validateKernel(directory, target) {
  const base = join(directory, `sing-box-${target}`)
  const metadata = json(`${base}.meta.json`)
  validateKernelMetadata(metadata, target)
  const bytes = readFileSync(`${base}.zst`)
  ensure(bytes.length === metadata.compression.bytes && sha256(bytes) === metadata.compression.sha256, 'Kernel package checksum mismatch')
  return metadata
}

export function recordRules(directory, geoip, direct) {
  ensure([geoip, direct].every(ref => /^[a-f0-9]{40}$/.test(ref)), 'Rule sources must be pinned commits')
  writeJson(join(directory, 'rules.meta.json'), {
    sources: { 'sing-geoip': geoip, 'v2ray-rules-dat': direct },
    files: Object.fromEntries(ruleFiles.map(name => [name, sha256(readFileSync(join(directory, name)))])),
  })
}

function validateRuleMetadata(rules) {
  ensure(['sing-geoip', 'v2ray-rules-dat'].every(name => /^[a-f0-9]{40}$/.test(rules.sources?.[name])), 'Missing rule source commits')
  ensure(ruleFiles.every(name => /^[a-f0-9]{64}$/.test(rules.files?.[name])), 'Missing rule checksums')
}

function validateRules(directory) {
  const rules = json(join(directory, 'rules.meta.json'))
  validateRuleMetadata(rules)
  for (const name of ruleFiles) {
    const bytes = readFileSync(join(directory, name))
    ensure(bytes.length > 0 && sha256(bytes) === rules.files[name], `Rule checksum mismatch: ${name}`)
  }
  return rules
}

export function validateResources(root, target) {
  const directory = join(root, 'embedded')
  validateKernel(directory, target)
  return validateRules(directory)
}

function validateBinary(path, target) {
  const bytes = readFileSync(path)
  ensure(bytes.length >= 64 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
    && bytes[4] === 2 && bytes[5] === 1 && bytes.readUInt16LE(18) === targets[target][1], `Expected ${target} ELF binary: ${path}`)
  return bytes
}

export function stageRelease(root, directory, commit, tag) {
  ensure(/^[a-f0-9]{40}$/.test(commit) && tag === `v${version(root)}`, 'Release identity mismatch')
  const rules = validateRules(join(root, 'embedded'))
  const kernels = {}, files = {}
  mkdirSync(directory, { recursive: true })
  for (const [target, [triple]] of Object.entries(targets)) {
    kernels[target] = validateKernel(join(root, 'embedded'), target)
    const binary = join(root, 'target', triple, 'release', 'miao-rust')
    const bytes = validateBinary(binary, target)
    const name = `miao-rust-linux-${target}`
    copyFileSync(binary, join(directory, name))
    files[name] = { bytes: bytes.length, sha256: sha256(bytes) }
    writeFileSync(join(directory, `${name}.sha256`), checksumLine(name, files[name].sha256))
  }
  writeJson(join(directory, 'release-manifest.json'), {
    schema_version: 2, commit, tag, files, kernels, rules,
  })
  verifyRelease(directory, commit, tag)
}

export function verifyRelease(directory, commit, tag) {
  const manifest = json(join(directory, 'release-manifest.json'))
  ensure(manifest.schema_version === 2 && manifest.commit === commit && manifest.tag === tag, 'Staged release does not match HEAD/version or format; rebuild it')
  ensure(JSON.stringify(Object.keys(manifest.files).sort()) === JSON.stringify([...assetNames].sort()), 'Incomplete release manifest')
  validateRuleMetadata(manifest.rules)
  for (const target of Object.keys(targets)) {
    const name = `miao-rust-linux-${target}`
    validateKernelMetadata(manifest.kernels[target], target)
    const bytes = validateBinary(join(directory, name), target)
    ensure(bytes.length === manifest.files[name].bytes && sha256(bytes) === manifest.files[name].sha256, `Staged asset changed: ${name}`)
    ensure(readFileSync(join(directory, `${name}.sha256`), 'utf8') === checksumLine(name, manifest.files[name].sha256), `Staged checksum changed: ${name}.sha256`)
  }
}

if (import.meta.main) {
  try {
    const [command, ...args] = process.argv.slice(2)
    switch (command) {
      case 'resources': validateResources('.', args[0]); break
      case 'rules': recordRules(...args); break
      case 'stage': stageRelease('.', ...args); break
      case 'verify': verifyRelease(...args); break
      case 'version': {
        if (args.length === 0) { console.log(version()); break }
        ensure(args.length === 1 && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(args[0]), 'Expected version X.Y.Z')
        const current = version()
        ensure(current !== args[0], `Already at ${current}`)
        const manifest = readFileSync('Cargo.toml', 'utf8').replace(/(\[workspace\.package\][\s\S]*?^version = ")[^"]+("$)/m, `$1${args[0]}$2`)
        ensure(Bun.TOML.parse(manifest).workspace.package.version === args[0], 'Workspace version update failed')
        writeFileSync('Cargo.toml', manifest)
        break
      }
      default: throw new Error(`Unknown artifact command: ${command}`)
    }
  } catch (error) {
    console.error(`artifacts: ${error.message}`)
    process.exitCode = 1
  }
}
