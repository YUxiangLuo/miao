import { afterEach, expect, test } from 'bun:test'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const head = 'a'.repeat(40)
const shellTest = test.skipIf(process.platform === 'win32')

function run(args, overrides = {}, { withPublicIndex = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'harbor-dev-test-'))
  roots.push(root)
  mkdirSync(join(root, 'scripts/lib'), { recursive: true })
  mkdirSync(join(root, 'bin'))
  if (withPublicIndex) {
    mkdirSync(join(root, 'public'), { recursive: true })
    writeFileSync(join(root, 'public/index.html'), '<html></html>\n')
  }
  writeFileSync(join(root, 'Cargo.toml'), 'original manifest\n')
  writeFileSync(join(root, 'Cargo.lock'), 'original lock\n')
  copyFileSync(new URL('../../dev.sh', import.meta.url), join(root, 'dev.sh'))
  copyFileSync(new URL('../lib/release.sh', import.meta.url), join(root, 'scripts/lib/release.sh'))
  writeFileSync(join(root, 'scripts/lib/kernel.sh'), 'printf "kernel %s\\n" "$*" >> "$LOG"\n')
  const log = join(root, 'calls')
  writeFileSync(log, '')
  const stub = `#!/bin/bash
tool=\${0##*/}
printf '%s %s\\n' "$tool" "$*" >> "$LOG"
case "$tool:$*" in
  'git:status '*) printf '%s' "\${DIRTY:-}"; exit "\${GIT_STATUS:-0}" ;;
  'git:rev-parse HEAD') printf '%s\\n' "$HEAD" ;;
  'git:rev-parse --verify '*) exit "\${LOCAL_TAG_EXISTS:-1}" ;;
  'git:rev-parse refs/tags/'*) printf '%s\\n' "\${LOCAL_TAG_COMMIT:-$HEAD}" ;;
  'git:remote get-url --push --all origin') printf '%s\\n' "\${PUSH_URL:-git@github.com:example/miao.git}" ;;
  'git:ls-remote '*) printf '%s' "\${REMOTE_TAG:-}" ;;
  'bun:scripts/lib/release-artifacts.mjs version') echo 1.2.3 ;;
  'bun:scripts/lib/release-artifacts.mjs version '*) echo 'updated manifest' > Cargo.toml ;;
  'cargo:check '*--offline*) echo 'updated lock' > Cargo.lock; exit "\${CARGO_STATUS:-0}" ;;
  'rustup:target list --installed') printf '%s\\n' x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-gnu ;;
esac
`
  for (const name of ['git', 'bun', 'cargo', 'rustup', 'zig', 'cargo-zigbuild', 'go', 'curl', 'shellcheck', 'jq', 'x86_64-w64-mingw32-gcc']) {
    writeFileSync(join(root, 'bin', name), stub, { mode: 0o755 })
  }
  const result = Bun.spawnSync(['bash', join(root, 'dev.sh'), ...args], {
    env: { ...process.env, PATH: `${join(root, 'bin')}:/usr/bin:/bin`, LOG: log, HEAD: head, ...overrides },
    stdout: 'pipe', stderr: 'pipe',
  })
  return {
    status: result.exitCode,
    calls: readFileSync(log, 'utf8').trim().split('\n').filter(Boolean),
    stdout: result.stdout.toString(),
    error: result.stderr.toString(),
    manifest: readFileSync(join(root, 'Cargo.toml'), 'utf8'),
    lock: readFileSync(join(root, 'Cargo.lock'), 'utf8'),
    root,
  }
}

shellTest('build rejects ambiguous or host-mismatched targets and Linux checks skip desktop workspace crate', () => {
  expect(run(['build', 'amd64', 'arm64']).status).toBe(1)
  expect(run(['build', 'desktop']).status).toBe(1)
  expect(run(['version', 'v1.2.3;bad']).status).toBe(1)
  const result = run(['check', 'core'])
  expect(result.status).toBe(0)
  expect(result.calls.every(call => call.startsWith('cargo '))).toBe(true)
  expect(result.calls.filter(call => call.includes('--no-default-features'))).toHaveLength(2)
  const all = run(['check', 'all'])
  expect(all.status).toBe(0)
  expect(all.calls.some(call => call.startsWith('cargo ') && call.includes('--workspace'))).toBe(false)
})

shellTest('frontend, scripts, rust, and --skip-frontend checks run independently', () => {
  const frontend = run(['check', 'frontend'])
  expect(frontend.status).toBe(0)
  expect(frontend.calls.every(call => call.startsWith('bun ') && call.includes('frontend-rsbuild'))).toBe(true)

  const scripts = run(['check', 'scripts'])
  expect(scripts.status).toBe(0)
  expect(scripts.calls.some(call => call.startsWith('shellcheck '))).toBe(true)
  expect(scripts.calls).toContain('bun test scripts/tests')
  expect(scripts.calls.some(call => call.includes('frontend-rsbuild') || call.startsWith('cargo '))).toBe(false)

  expect(run(['check', 'rust', '--skip-frontend']).status).toBe(1)
  const rustSkipped = run(['check', 'rust', '--skip-frontend'], {}, { withPublicIndex: true })
  expect(rustSkipped.status).toBe(0)
  expect(rustSkipped.calls.some(call => call.includes('frontend-rsbuild'))).toBe(false)
  expect(rustSkipped.calls.some(call => call.startsWith('cargo clippy --locked'))).toBe(true)
})

shellTest('build all --skip-frontend builds both musl targets with one kernel build and reuses public/', () => {
  expect(run(['build', 'all', '--skip-frontend']).status).toBe(1)
  const result = run(['build', 'all', '--refresh-kernel', '--skip-frontend'], { CARGO_TARGET_DIR: '/unused-review-target' }, { withPublicIndex: true })
  expect(result.status).toBe(0)
  expect(result.calls.filter(call => call === 'kernel all')).toHaveLength(1)
  expect(result.calls.filter(call => call.startsWith('cargo zigbuild'))).toHaveLength(2)
  expect(result.calls.filter(call => call.startsWith('cargo zigbuild')).every(call => call.includes(`--target-dir ${result.root}/target`))).toBe(true)
  expect(result.calls.some(call => call.includes('frontend-rsbuild'))).toBe(false)
})

shellTest('kernel-only builds accept the default host target', () => {
  const defaultTarget = run(['kernel']).calls.find(call => call.startsWith('kernel '))
  const result = run(['kernel', '--kernel-only'])
  expect(result.status).toBe(0)
  expect(result.calls).toContain(`${defaultTarget} --kernel-only`)
  expect(run(['kernel', 'arm64', '--kernel-only']).calls).toContain('kernel arm64 --kernel-only')
})

shellTest('version updates restore both original files when Cargo fails', () => {
  const failed = run(['version', 'v1.2.4'], { CARGO_STATUS: '1' })
  expect(failed.status).toBe(1)
  expect(failed.manifest).toBe('original manifest\n')
  expect(failed.lock).toBe('original lock\n')
  const success = run(['version', 'v1.2.4'])
  expect(success.status).toBe(0)
  expect(success.manifest).toBe('updated manifest\n')
  expect(success.lock).toBe('updated lock\n')
})

shellTest('release without --push checks tree readiness without remote writes or local cross-builds', () => {
  const result = run(['release'])
  expect(result.status).toBe(0)
  expect(result.stdout).toContain('Ready to release v1.2.3')
  expect(result.calls.some(call => /^(git (push|tag) |cargo )/.test(call))).toBe(false)
})

shellTest('release --push rejects dirty trees, mismatched local/remote tags, and invalid origins before writes', () => {
  for (const env of [
    { DIRTY: ' M changed' },
    { GIT_STATUS: '1' },
    { LOCAL_TAG_EXISTS: '0', LOCAL_TAG_COMMIT: 'b'.repeat(40) },
    { REMOTE_TAG: `${'b'.repeat(40)}\trefs/tags/v1.2.3` },
    { PUSH_URL: 'git@github.com:a/one.git\ngit@github.com:b/two.git' },
    { PUSH_URL: 'https://gitlab.com/example/miao.git' },
  ]) {
    const result = run(['release', '--push'], env)
    expect(result.status).toBe(1)
    expect(result.calls.some(call => /^git (push|tag) /.test(call))).toBe(false)
  }
})

shellTest('release --push tags HEAD and pushes branch + tag to origin push URL to trigger GitHub Actions', () => {
  const pushUrl = 'https://github.com/release-owner/miao.git'
  const result = run(['release', '--push'], { PUSH_URL: pushUrl })
  expect(result.status).toBe(0)
  expect(result.calls.some(call => call.startsWith(`git ls-remote --tags ${pushUrl} `))).toBe(true)
  expect(result.calls).toContain(`git tag v1.2.3 ${head}`)
  expect(result.calls).toContain(`git push ${pushUrl} HEAD refs/tags/v1.2.3`)
})
