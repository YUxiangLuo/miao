import { test, expect } from 'bun:test'
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const preflight = readFileSync(new URL('../../crates/miao-core/src/services/vps/common.sh', import.meta.url), 'utf8')
const provision = readFileSync(new URL('../../crates/miao-core/src/services/vps/provision.sh', import.meta.url), 'utf8')
const probe = readFileSync(new URL('../../crates/miao-core/src/services/vps/probe.sh', import.meta.url), 'utf8')
const legacyConfig = 'listen: :543\nauth:\n  type: password\n  password: old-secret\nobfs:\n  type: salamander\n  salamander:\n    password: old-obfs\n'

test('legacy probe requests migration without overwriting config or restarting', () => {
  const root = mkdtempSync(join(tmpdir(), 'miao-vps-probe-'))
  const config = legacyConfig
  try {
    writeFileSync(join(root, 'config.yaml'), config)
    writeFileSync(join(root, 'server.crt'), 'certificate')
    writeFileSync(join(root, 'server.key'), 'key')
    const result = spawnSync('/bin/sh', ['-s'], {
      input: `set -eu\nFALLBACK_OBFS_PASSWORD=new-obfs\nopenssl() { echo 'subject=CN = miao-hysteria'; }\nmiao_start_checked() { touch '${root}/restarted'; exit 1; }\n` + probe.replaceAll('/etc/hysteria', root),
      encoding: 'utf8', timeout: 5000,
    })
    expect(contents(join(root, 'config.yaml'))).toBe(config)
    expect(existsSync(join(root, 'restarted'))).toBe(false)
    expect(result.status).toBe(20)
    expect(result.stdout).toBe('old-secret\nnew-obfs\n')
  } finally { rmSync(root, { recursive: true, force: true }) }
})

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
  test(`VPS preflight installs missing OpenSSL with ${manager}`, () => {
    const { result, calls } = fixture({ manager, missing: true })
    expect(result.status).toBe(0)
    expect(result.stdout).toBe('openrc:amd64')
    expect(calls).toContain(args)
    expect(calls).toContain('openssl')
    expect(calls).not.toContain('upgrade')
  })
}

test('VPS preflight supports arm64 without installing existing tools', () => {
  const { result, calls } = fixture({ manager: 'apk', arch: 'aarch64' })
  expect(result.status).toBe(0)
  expect(result.stdout).toBe('openrc:arm64')
  expect(calls).toBe('')
})

test('VPS preflight rejects unsupported architecture before installation', () => {
  const { result, calls } = fixture({ manager: 'apk', missing: true, arch: 'riscv64' })
  expect(result.status).not.toBe(0)
  expect(result.stderr).toContain('不支持的 CPU 架构')
  expect(calls).toBe('')
})

test('VPS preflight rejects missing init before installation', () => {
  const { result, calls } = fixture({ manager: 'apk', missing: true, init: 'none' })
  expect(result.status).not.toBe(0)
  expect(result.stderr).toContain('systemd 或 OpenRC')
  expect(calls).toBe('')
})

// Execute a path-rewritten copy of provision.sh. Every external/network/service
// boundary is fake; only ordinary file operations happen, under this fixture.
function provisionFixture({ init = 'systemd', fail = '', running = true, enabled = true, withUnit = true, existing = true, legacy = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'miao-vps-provision-'))
  const bin = join(root, 'mock-bin')
  const state = join(root, 'state')
  mkdirSync(bin)
  mkdirSync(state)
  mkdirSync(join(root, 'etc/hysteria'), { recursive: true })
  mkdirSync(join(root, 'usr/local/bin'), { recursive: true })
  mkdirSync(join(root, init === 'systemd' ? 'etc/systemd/system' : 'etc/init.d'), { recursive: true })
  writeFileSync(join(root, 'etc/hysteria/config.yaml'), legacy ? legacyConfig : 'old-config\n')
  writeFileSync(join(root, 'etc/hysteria/server.crt'), 'old-cert\n')
  writeFileSync(join(root, 'etc/hysteria/server.key'), 'old-key\n')
  writeFileSync(join(root, 'usr/local/bin/hysteria'), 'old-binary\n')
  const unit = join(root, init === 'systemd' ? 'etc/systemd/system/hysteria-server.service' : 'etc/init.d/hysteria-server')
  if (withUnit) writeFileSync(unit, 'old-unit\n')
  if (running) writeFileSync(join(state, 'running'), '')
  if (enabled) writeFileSync(join(state, 'enabled'), '')
  if (!existing) {
    rmSync(join(root, 'etc/hysteria'), { recursive: true })
    rmSync(join(root, 'usr/local/bin/hysteria'))
    rmSync(unit, { force: true })
  }
  writeFileSync(join(state, 'fail'), fail)

  const command = (name, body) => writeFileSync(join(bin, name), `#!/bin/sh\nset -eu\n${body}\n`, { mode: 0o700 })
  command('curl', `out=''; while [ "$#" -gt 0 ]; do case "$1" in -o|-fsSLo) out="$2"; shift 2;; *hashes.txt) url=hash; shift;; *) shift;; esac; done
[ "\${url:-}" = hash ] && printf 'good  build/hysteria-linux-amd64\\n' > "$out" || printf 'new-binary\\n' > "$out"`)
  command('sha256sum', `printf 'good  %s\\n' "$1"`)
  command('openssl', `
[ "$(cat '${state}/fail')" = generate ] && exit 1
key=''; crt=''; while [ "$#" -gt 0 ]; do case "$1" in -keyout) key="$2"; shift 2;; -out) crt="$2"; shift 2;; *) shift;; esac; done
printf 'new-key\\n' > "$key"; printf 'new-cert\\n' > "$crt"`)
  command('sleep', ':')
  const serviceBody = init === 'systemd' ? `
cmd="$1"; shift || true
case "$cmd" in
 is-active) [ -f '${state}/running' ];; is-enabled) [ -f '${state}/enabled' ];;
 enable) [ "$(cat '${state}/fail')" != enable ] || exit 1; touch '${state}/enabled';; disable) rm -f '${state}/enabled'; [ -f '${unit}' ];;
 stop) rm -f '${state}/running'; [ -f '${unit}' ];;
 restart) n=$(cat '${state}/restarts' 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > '${state}/restarts'; [ "$(cat '${state}/fail')" != restore ] || exit 1; if [ "$(cat '${state}/fail')" = activate ] && [ ! -f '${state}/reloads' ] && [ "$n" -eq 1 ]; then exit 1; fi; touch '${state}/running';;
 daemon-reload) n=$(cat '${state}/reloads' 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > '${state}/reloads'; [ "$(cat '${state}/fail'):$n" != activate:1 ];;
esac` : `
cmd="$2"
case "$cmd" in
 status) [ -f '${state}/running' ];; stop) rm -f '${state}/running'; [ -f '${unit}' ];;
 restart) n=$(cat '${state}/restarts' 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > '${state}/restarts'; case "$(cat '${state}/fail'):$n" in activate:1|restore:*) exit 1;; esac; touch '${state}/running';;
esac`
  command(init === 'systemd' ? 'systemctl' : 'rc-service', serviceBody)
  command('rc-update', `
case "$1" in show) [ -f '${state}/enabled' ] && echo ' hysteria-server ';; add) [ "$(cat '${state}/fail')" != enable ] || exit 1; touch '${state}/enabled';; del) rm -f '${state}/enabled'; [ -f '${unit}' ];; esac`)

  const rewritten = provision
    .replaceAll('/etc/hysteria', join(root, 'etc/hysteria'))
    .replaceAll('/usr/local/bin', join(root, 'usr/local/bin'))
    .replaceAll('/etc/systemd/system/hysteria-server.service', join(root, 'etc/systemd/system/hysteria-server.service'))
    .replaceAll('/etc/init.d/hysteria-server', join(root, 'etc/init.d/hysteria-server'))
    .replaceAll('/tmp/miao-hysteria-backup.', `${root}/backup.`)
  let prefix = `set -eu
MIAO_INIT='${init}'; HYSTERIA_ARCH=amd64; SERVICE=hysteria-server; PASSWORD=p; OBFS_PASSWORD=o
miao_fail() { echo "MIAO_ERROR: $*" >&2; exit 1; }
miao_stop() { if [ "$MIAO_INIT" = systemd ]; then systemctl stop "$SERVICE"; else rc-service "$SERVICE" stop; fi; }
miao_disable() { if [ "$MIAO_INIT" = systemd ]; then systemctl disable "$SERVICE"; else rc-update del "$SERVICE" default; fi; }
miao_is_running() { if [ "$MIAO_INIT" = systemd ]; then systemctl is-active --quiet "$SERVICE"; else rc-service "$SERVICE" status >/dev/null 2>&1; fi; }
miao_is_enabled() { if [ "$MIAO_INIT" = systemd ]; then systemctl is-enabled --quiet "$SERVICE"; else rc-update show default | grep -q hysteria-server; fi; }
miao_reload_init() { [ "$MIAO_INIT" != systemd ] || systemctl daemon-reload; }
`
  if (legacy) {
    const result = spawnSync('/bin/sh', ['-s'], {
      input: `set -eu\nFALLBACK_OBFS_PASSWORD=new-obfs\nopenssl() { echo 'subject=CN = miao-hysteria'; }\nmiao_start_checked() { exit 99; }\n` + probe.replaceAll('/etc/hysteria', join(root, 'etc/hysteria')),
      encoding: 'utf8', timeout: 5000,
    })
    if (result.status !== 20 || result.stdout !== 'old-secret\nnew-obfs\n') {
      rmSync(root, { recursive: true, force: true })
      throw new Error(`unexpected migration probe: ${result.status}: ${result.stdout} ${result.stderr}`)
    }
    const [password, obfsPassword] = result.stdout.trim().split('\n')
    prefix += `PASSWORD='${password}'; OBFS_PASSWORD='${obfsPassword}'\n`
  }
  const result = spawnSync('/bin/sh', ['-s'], { input: prefix + rewritten, env: { PATH: `${bin}:/usr/bin:/bin`, TMPDIR: root }, encoding: 'utf8', timeout: 5000 })
  return { root, state, unit, result, cleanup: () => rmSync(root, { recursive: true, force: true }) }
}

function contents(path) { return readFileSync(path, 'utf8') }

for (const init of ['systemd', 'openrc']) {
  for (const fail of ['', 'activate']) test(`${init} legacy migration ${fail ? 'rolls back' : 'upgrades binary and config'}`, () => {
    const f = provisionFixture({ init, fail, legacy: true })
    try {
      expect(f.result.signal, f.result.stderr).toBeNull()
      const config = contents(join(f.root, 'etc/hysteria/config.yaml'))
      if (fail) {
        expect(f.result.status).not.toBe(0)
        expect(f.result.stderr).toContain('已恢复旧部署及服务状态')
        expect(config).toBe(legacyConfig)
        expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('old-binary\n')
        expect(contents(join(f.root, 'etc/hysteria/server.crt'))).toBe('old-cert\n')
        expect(contents(f.unit)).toBe('old-unit\n')
      } else {
        expect(f.result.status, f.result.stderr).toBe(0)
        expect(config).toContain('type: gecko')
        expect(config).toContain('password: old-secret')
        expect(config).toContain('password: new-obfs')
        expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('new-binary\n')
      }
      expect(existsSync(join(f.state, 'running'))).toBe(true)
      expect(existsSync(join(f.state, 'enabled'))).toBe(true)
    } finally { f.cleanup() }
  })
}

test('generation failure leaves the old deployment and service untouched', () => {
  const f = provisionFixture({ fail: 'generate' })
  try {
    expect(f.result.signal, f.result.stderr).toBeNull()
    expect(f.result.status).not.toBe(0)
    expect(contents(join(f.root, 'etc/hysteria/config.yaml'))).toBe('old-config\n')
    expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('old-binary\n')
    expect(contents(f.unit)).toBe('old-unit\n')
    expect(existsSync(join(f.state, 'running'))).toBe(true)
  } finally { f.cleanup() }
})

for (const init of ['systemd', 'openrc']) test(`${init} activation failure restores files and state`, () => {
  const f = provisionFixture({ init, fail: 'activate', running: true, enabled: true })
  try {
    expect(f.result.status).not.toBe(0)
    expect(f.result.stderr).toContain('已恢复旧部署及服务状态')
    expect(contents(join(f.root, 'etc/hysteria/config.yaml'))).toBe('old-config\n')
    expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('old-binary\n')
    expect(contents(f.unit)).toBe('old-unit\n')
    expect(existsSync(join(f.state, 'running'))).toBe(true)
    expect(existsSync(join(f.state, 'enabled'))).toBe(true)
  } finally { f.cleanup() }
})

test('rollback preserves an originally stopped/disabled partial deployment', () => {
  const f = provisionFixture({ fail: 'activate', running: false, enabled: false, withUnit: false })
  try {
    expect(f.result.status).not.toBe(0)
    expect(f.result.stderr).toContain('已恢复旧部署及服务状态')
    expect(contents(join(f.root, 'etc/hysteria/config.yaml'))).toBe('old-config\n')
    expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('old-binary\n')
    expect(existsSync(f.unit)).toBe(false)
    expect(existsSync(join(f.state, 'running'))).toBe(false)
    expect(existsSync(join(f.state, 'enabled'))).toBe(false)
  } finally { f.cleanup() }
})

test('failed first installation restores the absence of a deployment', () => {
  const f = provisionFixture({ fail: 'activate', running: false, enabled: false, existing: false })
  try {
    expect(f.result.status).not.toBe(0)
    expect(f.result.stderr).toContain('已恢复旧部署及服务状态')
    expect(existsSync(join(f.root, 'etc/hysteria'))).toBe(false)
    expect(existsSync(join(f.root, 'usr/local/bin/hysteria'))).toBe(false)
    expect(existsSync(f.unit)).toBe(false)
    expect(existsSync(join(f.state, 'running'))).toBe(false)
    expect(existsSync(join(f.state, 'enabled'))).toBe(false)
  } finally { f.cleanup() }
})

test('successful activation starts and enables the requested new deployment', () => {
  const f = provisionFixture({ running: false, enabled: false, withUnit: false })
  try {
    expect(f.result.status).toBe(0)
    expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('new-binary\n')
    expect(contents(join(f.root, 'etc/hysteria/server.key'))).toBe('new-key\n')
    expect(contents(f.unit)).toContain('ExecStart=')
    expect(existsSync(join(f.state, 'running'))).toBe(true)
    expect(existsSync(join(f.state, 'enabled'))).toBe(true)
  } finally { f.cleanup() }
})

test('rollback failure retains backup and reports its real path', () => {
  const f = provisionFixture({ fail: 'restore' })
  try {
    expect(f.result.status).not.toBe(0)
    expect(f.result.stderr).toContain('恢复旧部署失败；备份保留在')
    const backup = f.result.stderr.match(/备份保留在 ([^，]+)/)?.[1]
    expect(backup).toBeTruthy()
    expect(existsSync(backup)).toBe(true)
    expect(contents(join(backup, 'config/config.yaml'))).toBe('old-config\n')
    expect(contents(join(backup, 'binary'))).toBe('old-binary\n')
  } finally { f.cleanup() }
})

for (const init of ['systemd', 'openrc']) test(`${init} rollback still restarts the old service when restoring autostart fails`, () => {
  const f = provisionFixture({ init, fail: 'enable' })
  try {
    expect(f.result.signal, f.result.stderr).toBeNull()
    expect(f.result.status).not.toBe(0)
    expect(f.result.stderr).toContain('恢复旧部署失败；备份保留在')
    expect(contents(join(f.root, 'etc/hysteria/config.yaml'))).toBe('old-config\n')
    expect(contents(join(f.root, 'usr/local/bin/hysteria'))).toBe('old-binary\n')
    expect(contents(f.unit)).toBe('old-unit\n')
    expect(existsSync(join(f.state, 'running'))).toBe(true)
    expect(contents(join(f.state, 'restarts')).trim()).toBe('1')
    const backup = f.result.stderr.match(/备份保留在 ([^，]+)/)?.[1]
    expect(contents(join(backup, 'binary'))).toBe('old-binary\n')
  } finally { f.cleanup() }
})
