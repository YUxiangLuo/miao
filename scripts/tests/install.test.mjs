import { afterEach, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const name = 'harbor-linux-amd64'
const binary = 'downloaded fixture binary'
const digest = `sha256:${createHash('sha256').update(binary).digest('hex')}`
const shellTest = test.skipIf(process.platform === 'win32')

function download(assets) {
  const root = mkdtempSync(join(tmpdir(), 'harbor-install-test-'))
  roots.push(root)
  writeFileSync(join(root, 'metadata'), JSON.stringify({ assets }))
  writeFileSync(join(root, 'binary'), binary)
  writeFileSync(join(root, 'calls'), '')
  const result = Bun.spawnSync(['bash', '-c', `
source "$INSTALL_SCRIPT"
download() {
  printf '%s\\n' "$1" >> "$FIXTURE/calls"
  case "$1" in
    "$RELEASE_API") cp "$FIXTURE/metadata" "$2" ;;
    https://example.com/v1.2.3/harbor) cp "$FIXTURE/binary" "$2" ;;
    *) return 1 ;;
  esac
}
download_release_binary harbor-linux-amd64 "$FIXTURE" "$FIXTURE/candidate"
`], {
    env: { ...process.env, INSTALL_SCRIPT: fileURLToPath(new URL('../../install.sh', import.meta.url)), FIXTURE: root },
    stdout: 'pipe', stderr: 'pipe',
  })
  return { status: result.exitCode, calls: readFileSync(join(root, 'calls'), 'utf8').trim().split('\n'), root }
}

shellTest('installer downloads a pinned release URL and verifies its GitHub digest without sidecar assets', () => {
  const result = download([{ name, browser_download_url: 'https://example.com/v1.2.3/harbor', digest }])
  expect(result.status).toBe(0)
  expect(result.calls).toHaveLength(2)
  expect(result.calls[1]).toBe('https://example.com/v1.2.3/harbor')
  expect(readFileSync(join(result.root, 'candidate'), 'utf8')).toBe(binary)
})

shellTest('installer uses the binary digest when checksum sidecars are also present', () => {
  const result = download([
    { name, browser_download_url: 'https://example.com/v1.2.3/harbor', digest },
    { name: `${name}.sha256`, browser_download_url: 'https://example.com/v1.2.3/harbor.sha256', digest: null },
  ])
  expect(result.status).toBe(0)
  expect(result.calls).toHaveLength(2)
  expect(result.calls[1]).toBe('https://example.com/v1.2.3/harbor')
})

shellTest('installer rejects missing, duplicate or invalid digests before downloading a binary', () => {
  const asset = { name, browser_download_url: 'https://example.com/v1.2.3/harbor', digest }
  for (const assets of [[], [asset, asset], [{ ...asset, digest: null }], [{ ...asset, digest: 'sha256:bad' }], [{ ...asset, digest: digest.replace('sha256', 'sha512') }]]) {
    const result = download(assets)
    expect(result.status).not.toBe(0)
    expect(result.calls).toHaveLength(1)
  }
})

shellTest('installer refuses a binary whose SHA256 differs from release metadata', () => {
  const result = download([{ name, browser_download_url: 'https://example.com/v1.2.3/harbor', digest: `sha256:${'a'.repeat(64)}` }])
  expect(result.status).not.toBe(0)
  expect(result.calls).toHaveLength(2)
})

shellTest('installation and removal help and argument errors are handled before privilege checks', () => {
  const root = mkdtempSync(join(tmpdir(), 'harbor-entry-test-'))
  roots.push(root)
  const log = join(root, 'privilege-checks')
  writeFileSync(log, '')
  // Force a non-root identity even when the test runner is root. Accidental
  // fallthrough must stop before any install/remove operation can run.
  writeFileSync(join(root, 'id'), '#!/bin/sh\nprintf "checked\\n" >> "$ENTRY_LOG"\necho 1\n', { mode: 0o755 })
  for (const script of ['install.sh', 'remove.sh']) {
    const path = fileURLToPath(new URL(`../../${script}`, import.meta.url))
    for (const [args, status] of [[['--help'], 0], [['-h'], 0], [['--unknown'], 1], [['--yes', '--unknown'], 1]]) {
      const result = Bun.spawnSync(['bash', path, ...args], {
        env: { ...process.env, PATH: `${root}:/usr/bin:/bin`, ENTRY_LOG: log },
        stdout: 'pipe', stderr: 'pipe',
      })
      expect(result.exitCode).toBe(status)
      expect(`${result.stdout}${result.stderr}`).toContain(script)
    }
  }
  expect(readFileSync(log, 'utf8')).toBe('')
})

shellTest('removal clears interrupted upgrade state so a fresh installation is not blocked', () => {
  const root = mkdtempSync(join(tmpdir(), 'harbor-remove-test-'))
  roots.push(root)
  const commands = join(root, 'commands')
  mkdirSync(commands)
  // All service, process and network commands are fake; every removal path
  // points inside this fixture, even when the test runner happens to be root.
  for (const [name, body] of Object.entries({
    id: 'echo 0',
    systemctl: '[[ "$1" == daemon-reload ]]',
    pgrep: 'exit 1',
    pkill: 'exit 1',
    ip: 'exit 1',
  })) writeFileSync(join(commands, name), `#!/bin/bash\n${body}\n`, { mode: 0o755 })
  const paths = {
    '/usr/local/bin/harbor': join(root, 'harbor'),
    '/etc/harbor': join(root, 'config'),
    '/tmp/harbor-sing-box': join(root, 'runtime'),
    '/etc/systemd/system/harbor.service': join(root, 'harbor.service'),
  }
  let script = readFileSync(new URL('../../remove.sh', import.meta.url), 'utf8')
  for (const [from, to] of Object.entries(paths)) script = script.replaceAll(from, to)
  const scriptPath = join(root, 'remove.sh')
  writeFileSync(scriptPath, script)
  const binaries = ['', '.bak', '.new', '.failed', '.upgrade-pending.json', '.upgrade-pending.json.tmp']
    .map(suffix => join(root, `harbor${suffix}`))
  for (const path of [...binaries, paths['/etc/systemd/system/harbor.service'], join(root, 'keep')]) writeFileSync(path, 'fixture')
  for (const path of [paths['/etc/harbor'], paths['/tmp/harbor-sing-box']]) {
    mkdirSync(path)
    writeFileSync(join(path, 'data'), 'fixture')
  }
  const result = Bun.spawnSync(['bash', scriptPath, '-y'], {
    env: { ...process.env, PATH: `${commands}:/usr/bin:/bin` },
    stdout: 'pipe', stderr: 'pipe',
  })
  expect(result.exitCode).toBe(0)
  for (const path of [...binaries, ...Object.values(paths)]) expect(existsSync(path)).toBe(false)
  expect(readFileSync(join(root, 'keep'), 'utf8')).toBe('fixture')
  const pending = Bun.spawnSync(['bash', '-c', `
source "$INSTALL_SCRIPT"
SELF_UPGRADE_BACKUP="$FIXTURE/harbor.bak"
SELF_UPGRADE_PENDING="$FIXTURE/harbor.upgrade-pending.json"
self_upgrade_pending
`], {
    env: { ...process.env, INSTALL_SCRIPT: fileURLToPath(new URL('../../install.sh', import.meta.url)), FIXTURE: root },
    stdout: 'pipe', stderr: 'pipe',
  })
  expect(pending.exitCode).toBe(1)
})
