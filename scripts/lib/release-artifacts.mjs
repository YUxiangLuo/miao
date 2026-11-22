import { createHash } from 'node:crypto'
import { copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { customizationHash, readSource } from '../sing-box/source.mjs'

const linuxTargets = { amd64: ['x86_64-unknown-linux-musl', 62], arm64: ['aarch64-unknown-linux-musl', 183] }
const kernelArtifacts = { amd64: 'sing-box-amd64', arm64: 'sing-box-arm64', 'windows-amd64': 'sing-box-windows-amd64.exe' }
const ruleFiles = ['geoip-cn.srs', 'geosite-geolocation-cn.srs']
const windowsPeMachines = new Set([0x014c, 0x8664])

export const WINDOWS_INSTALLER_ASSET = 'harbor-windows-amd64-setup.exe'
export const LINUX_ASSETS = Object.keys(linuxTargets).map(target => `harbor-linux-${target}`)
export const RELEASE_ASSETS = [...LINUX_ASSETS, WINDOWS_INSTALLER_ASSET]
export const RELEASE_UPLOAD_FILES = RELEASE_ASSETS.flatMap(name => [name, `${name}.sha256`])

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
  ensure(Object.hasOwn(kernelArtifacts, target), `Unsupported target: ${target}`)
  ensure(metadata?.schema_version === 1 && metadata.name === 'harbor-kernel' && !metadata.test_stub, 'Real kernel required')
  ensure(metadata.target === target, 'Kernel target mismatch')
  for (const [key, value] of Object.entries(readSource())) {
    ensure(JSON.stringify(metadata[key]) === JSON.stringify(value), `Stale kernel ${key}`)
  }
  ensure(metadata.customization_sha256 === customizationHash(), 'Stale kernel customization')
}

export function validateKernel(directory, target) {
  ensure(Object.hasOwn(kernelArtifacts, target), `Unsupported target: ${target}`)
  const base = join(directory, kernelArtifacts[target])
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
  ensure(['sing-geoip', 'v2ray-rules-dat'].every(name => /^[a-f0-9]{40}$/.test(rules?.sources?.[name])), 'Missing rule source commits')
  ensure(ruleFiles.every(name => /^[a-f0-9]{64}$/.test(rules?.files?.[name])), 'Missing rule checksums')
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

function validateElfBinary(path, target) {
  const bytes = readFileSync(path)
  ensure(bytes.length >= 64 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
    && bytes[4] === 2 && bytes[5] === 1 && bytes.readUInt16LE(18) === linuxTargets[target][1], `Expected ${target} ELF binary: ${path}`)
  return bytes
}

function validateWindowsInstaller(path) {
  const bytes = readFileSync(path)
  ensure(bytes.length >= 64 && bytes[0] === 0x4d && bytes[1] === 0x5a, `Expected Windows PE installer: ${path}`)
  const peOffset = bytes.readUInt32LE(0x3c)
  ensure(peOffset + 6 <= bytes.length
    && bytes.subarray(peOffset, peOffset + 4).equals(Buffer.from([0x50, 0x45, 0x00, 0x00]))
    && windowsPeMachines.has(bytes.readUInt16LE(peOffset + 4)), `Expected Windows PE installer: ${path}`)
  return bytes
}

function validateStagedAsset(directory, name, entry) {
  const assetPath = join(directory, name)
  const bytes = name === WINDOWS_INSTALLER_ASSET
    ? validateWindowsInstaller(assetPath)
    : validateElfBinary(assetPath, name.replace('harbor-linux-', ''))
  ensure(bytes.length === entry?.bytes && sha256(bytes) === entry?.sha256, `Staged asset changed: ${name}`)
  ensure(readFileSync(join(directory, `${name}.sha256`), 'utf8') === checksumLine(name, entry.sha256), `Staged checksum changed: ${name}.sha256`)
}

function resolveWindowsInstallerPath(root, explicitPath) {
  if (explicitPath) return explicitPath
  const nsisDir = join(root, 'target', 'release', 'bundle', 'nsis')
  const installers = readdirSync(nsisDir).filter(name => name.endsWith('.exe')).sort()
  ensure(installers.length === 1, `Expected 1 NSIS installer under ${nsisDir}, found ${installers.length}`)
  return join(nsisDir, installers[0])
}

export function stageLinuxRelease(root, directory, commit, tag) {
  ensure(/^[a-f0-9]{40}$/.test(commit) && tag === `v${version(root)}`, 'Release identity mismatch')
  const rules = validateRules(join(root, 'embedded'))
  const kernels = {}, files = {}
  mkdirSync(directory, { recursive: true })
  for (const [target, [triple]] of Object.entries(linuxTargets)) {
    kernels[target] = validateKernel(join(root, 'embedded'), target)
    const binary = join(root, 'target', triple, 'release', 'harbor-cli')
    const bytes = validateElfBinary(binary, target)
    const name = `harbor-linux-${target}`
    copyFileSync(binary, join(directory, name))
    files[name] = { bytes: bytes.length, sha256: sha256(bytes) }
    writeFileSync(join(directory, `${name}.sha256`), checksumLine(name, files[name].sha256))
  }
  writeJson(join(directory, 'linux-manifest.json'), {
    schema_version: 3, platform: 'linux', commit, tag, files, kernels, rules,
  })
  verifyPartial(directory, 'linux', commit, tag)
}

export function stageWindowsRelease(root, directory, commit, tag, installerPath) {
  ensure(/^[a-f0-9]{40}$/.test(commit) && tag === `v${version(root)}`, 'Release identity mismatch')
  const rules = validateRules(join(root, 'embedded'))
  const kernels = { 'windows-amd64': validateKernel(join(root, 'embedded'), 'windows-amd64') }
  const sourcePath = resolveWindowsInstallerPath(root, installerPath)
  const bytes = validateWindowsInstaller(sourcePath)
  mkdirSync(directory, { recursive: true })
  copyFileSync(sourcePath, join(directory, WINDOWS_INSTALLER_ASSET))
  const files = { [WINDOWS_INSTALLER_ASSET]: { bytes: bytes.length, sha256: sha256(bytes) } }
  writeFileSync(join(directory, `${WINDOWS_INSTALLER_ASSET}.sha256`), checksumLine(WINDOWS_INSTALLER_ASSET, files[WINDOWS_INSTALLER_ASSET].sha256))
  writeJson(join(directory, 'windows-manifest.json'), {
    schema_version: 3, platform: 'windows', commit, tag, files, kernels, rules,
  })
  verifyPartial(directory, 'windows', commit, tag)
}

function verifyPartial(directory, platform, commit, tag) {
  const manifest = json(join(directory, `${platform}-manifest.json`))
  ensure(manifest.schema_version === 3 && manifest.platform === platform && manifest.commit === commit && manifest.tag === tag,
    `Staged ${platform} release does not match HEAD/version or format; rebuild it`)
  validateRuleMetadata(manifest.rules)
  const expectedAssets = platform === 'linux' ? LINUX_ASSETS : [WINDOWS_INSTALLER_ASSET]
  const expectedKernels = platform === 'linux' ? Object.keys(linuxTargets) : ['windows-amd64']
  ensure(JSON.stringify(Object.keys(manifest.files).sort()) === JSON.stringify([...expectedAssets].sort()), `Incomplete ${platform} manifest`)
  for (const target of expectedKernels) validateKernelMetadata(manifest.kernels[target], target)
  for (const name of expectedAssets) validateStagedAsset(directory, name, manifest.files[name])
  return manifest
}

export function assembleRelease(directory, commit, tag) {
  const linux = verifyPartial(directory, 'linux', commit, tag)
  const windows = verifyPartial(directory, 'windows', commit, tag)
  ensure(JSON.stringify(linux.rules.sources) === JSON.stringify(windows.rules.sources),
    'Rule sources differ between Linux and Windows builds; rebuild with pinned refs')
  writeJson(join(directory, 'release-manifest.json'), {
    schema_version: 3,
    commit,
    tag,
    files: { ...linux.files, ...windows.files },
    kernels: { ...linux.kernels, ...windows.kernels },
    rules: linux.rules,
  })
  rmSync(join(directory, 'linux-manifest.json'))
  rmSync(join(directory, 'windows-manifest.json'))
  verifyRelease(directory, commit, tag)
}

export function verifyRelease(directory, commit, tag) {
  const manifest = json(join(directory, 'release-manifest.json'))
  ensure(manifest.schema_version === 3 && manifest.commit === commit && manifest.tag === tag,
    'Staged release does not match HEAD/version or format; rebuild it')
  ensure(JSON.stringify(Object.keys(manifest.files).sort()) === JSON.stringify([...RELEASE_ASSETS].sort()), 'Incomplete release manifest')
  validateRuleMetadata(manifest.rules)
  for (const target of Object.keys(kernelArtifacts)) {
    validateKernelMetadata(manifest.kernels[target], target)
  }
  for (const name of RELEASE_ASSETS) {
    validateStagedAsset(directory, name, manifest.files[name])
  }
  return manifest
}

export function formatReleaseSummary(directory, releaseUrl = '') {
  const manifest = json(join(directory, 'release-manifest.json'))
  const mb = bytes => `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  const lines = [
    `## Harbor ${manifest.tag}`,
    '',
    `- **Commit**: \`${manifest.commit}\``,
    `- **sing-box Kernel**: \`${manifest.kernels.amd64.version}\` (\`${manifest.kernels.amd64.revision.slice(0, 12)}\`)`,
    `- **Geo Rules**: \`sing-geoip@${manifest.rules.sources['sing-geoip'].slice(0, 12)}\`, \`v2ray-rules-dat@${manifest.rules.sources['v2ray-rules-dat'].slice(0, 12)}\``,
  ]
  if (releaseUrl) lines.push(`- **Release**: ${releaseUrl}`)
  lines.push('', '| Asset | Size | SHA-256 |', '| --- | ---: | --- |')
  for (const name of RELEASE_ASSETS) {
    const info = manifest.files[name]
    lines.push(`| \`${name}\` | ${mb(info.bytes)} | \`${info.sha256}\` |`)
  }
  return `${lines.join('\n')}\n`
}

if (import.meta.main) {
  try {
    const [command, ...args] = process.argv.slice(2)
    switch (command) {
      case 'resources': validateResources('.', args[0]); break
      case 'rules': recordRules(...args); break
      case 'stage-linux': stageLinuxRelease('.', ...args); break
      case 'stage-windows': stageWindowsRelease('.', ...args); break
      case 'assemble': assembleRelease(...args); break
      case 'verify': verifyRelease(...args); break
      case 'assets': console.log(RELEASE_UPLOAD_FILES.join('\n')); break
      case 'summary': process.stdout.write(formatReleaseSummary(...args)); break
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
