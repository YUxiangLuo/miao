import { test, expect } from 'bun:test'
import { mkdtempSync, writeFileSync, rmSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const shellTest = test.skipIf(process.platform === 'win32')
const preflight = readFileSync(new URL('../crates/miao-core/src/services/vps/common.sh', import.meta.url), 'utf8')

// Only the shared preflight runs here; all package/service commands are fakes.
// Never run provisioning paths or a real package manager on the test host.
function fixture({ manager, init = 'openrc', arch = 'x86_64', missing = false }) {
  const path = mkdtempSync(join(tmpdir(), 'miao-vps-preflight-'))
  writeFileSync(join(path, 'ca.pem'), 'test CA fixture')
  const log = join(path, 'calls')
  const script = (name, body) => writeFileSync(join(path, name), `#!/bin/sh\n${body}\n`, { mode: 0o700 })
  script('id', 'echo 0')
  script('uname', `case "$1" in -s) echo Linux;; -m) echo ${arch};; esac`)
  for (const cmd of ['curl', 'sha256sum', 'awk', 'grep', 'mktemp', 'install']) script(cmd, 'exit 0')
  if (!missing) script('openssl', 'exit 0')
  if (init === 'openrc') {
    script('rc-service', 'exit 0')
    script('rc-update', 'exit 0')
  }
  if (manager) script(manager, `printf '%s\\n' "$*" >> '${log}'\n/bin/ln -sf /bin/true '${path}/openssl'`)
  try {
    return {
      result: spawnSync('/bin/sh', ['-s'], { input: `${preflight.replaceAll('/etc/ssl/certs/ca-certificates.crt', join(path, 'ca.pem'))}\nprintf '%s:%s' "$MIAO_INIT" "$HYSTERIA_ARCH"`, env: { PATH: path }, encoding: 'utf8' }),
      calls: (() => { try { return readFileSync(log, 'utf8') } catch { return '' } })(),
    }
  } finally { rmSync(path, { recursive: true, force: true }) }
}

for (const [manager, args] of [['apt-get', 'install -y'], ['dnf', 'install -y'], ['yum', 'install -y'], ['apk', 'add --no-cache'], ['pacman', '-S --needed --noconfirm'], ['zypper', '--non-interactive install']]) {
  shellTest(`VPS preflight installs missing OpenSSL with ${manager}`, () => {
    const { result, calls } = fixture({ manager, missing: true })
    expect(result.status).toBe(0)
    expect(result.stdout).toBe('openrc:amd64')
    expect(calls).toContain(args)
    expect(calls).toContain('openssl')
    expect(calls).not.toContain('upgrade')
  })
}

shellTest('VPS preflight supports arm64 without installing existing tools', () => {
  const { result, calls } = fixture({ manager: 'apk', arch: 'aarch64' })
  expect(result.status).toBe(0)
  expect(result.stdout).toBe('openrc:arm64')
  expect(calls).toBe('')
})

shellTest('VPS preflight rejects unsupported architecture before installation', () => {
  const { result, calls } = fixture({ manager: 'apk', missing: true, arch: 'riscv64' })
  expect(result.status).not.toBe(0)
  expect(result.stderr).toContain('不支持的 CPU 架构')
  expect(calls).toBe('')
})

shellTest('VPS preflight rejects missing init before installation', () => {
  const { result, calls } = fixture({ manager: 'apk', missing: true, init: 'none' })
  expect(result.status).not.toBe(0)
  expect(result.stderr).toContain('systemd 或 OpenRC')
  expect(calls).toBe('')
})
