import { afterEach, expect, test } from 'bun:test'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const roots = []
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }) })
const head = 'a'.repeat(40)

function run(args, overrides = {}) {
  const root = mkdtempSync(join(tmpdir(), 'miao-dev-test-'))
  roots.push(root)
  mkdirSync(join(root, 'scripts/lib'), { recursive: true })
  mkdirSync(join(root, 'bin'))
  copyFileSync(new URL('../dev.sh', import.meta.url), join(root, 'dev.sh'))
  copyFileSync(new URL('./lib/release.sh', import.meta.url), join(root, 'scripts/lib/release.sh'))
  writeFileSync(join(root, 'scripts/lib/kernel.sh'), 'printf "kernel %s\\n" "$*" >> "$LOG"\n')
  const log = join(root, 'calls')
  writeFileSync(log, '')
  const stub = `#!/bin/bash
tool=\${0##*/}
printf '%s %s\\n' "$tool" "$*" >> "$LOG"
case "$tool:$*" in
  'git:status '*) printf '%s' "\${DIRTY:-}" ;;
  'git:rev-parse HEAD') printf '%s\\n' "$HEAD" ;;
  'git:rev-parse --verify '*) exit 1 ;;
  'git:remote get-url origin') echo 'git@github.com:example/miao.git' ;;
  'git:ls-remote '*) printf '%s' "\${REMOTE_TAG:-}" ;;
  'bun:scripts/release-artifacts.mjs version') echo 1.2.3 ;;
  'bun:scripts/release-artifacts.mjs verify '*) exit "\${VERIFY_STATUS:-0}" ;;
  'rustup:target list --installed') printf '%s\\n' x86_64-unknown-linux-musl aarch64-unknown-linux-musl ;;
  'gh:release view '*"--json assets"*)
    [[ \${EXISTING_ASSETS+x} ]] || exit 1
    printf '%s\\n' "$EXISTING_ASSETS" ;;
  'gh:release upload '*) exit "\${UPLOAD_STATUS:-0}" ;;
esac
`
  for (const name of ['git', 'bun', 'cargo', 'gh', 'rustup', 'zig', 'cargo-zigbuild', 'go', 'curl']) {
    writeFileSync(join(root, 'bin', name), stub, { mode: 0o755 })
  }
  const result = Bun.spawnSync(['bash', join(root, 'dev.sh'), ...args], {
    env: { ...process.env, PATH: `${join(root, 'bin')}:/usr/bin:/bin`, LOG: log, HEAD: head, ...overrides },
    stdout: 'pipe', stderr: 'pipe',
  })
  return { status: result.exitCode, calls: readFileSync(log, 'utf8').trim().split('\n'), error: result.stderr.toString() }
}

test('build rejects ambiguous targets and core checks do not need Bun or resources', () => {
  expect(run(['build', 'amd64', 'arm64']).status).toBe(1)
  expect(run(['version', 'v1.2.3;bad']).status).toBe(1)
  const result = run(['check', 'core'])
  expect(result.status).toBe(0)
  expect(result.calls.every(call => call.startsWith('cargo '))).toBe(true)
  expect(result.calls.filter(call => call.includes('--no-default-features'))).toHaveLength(2)
})

test('release preparation builds both musl targets with one kernel build and no remote writes', () => {
  const result = run(['release'])
  expect(result.status).toBe(0)
  expect(result.calls.filter(call => call === 'kernel all')).toHaveLength(1)
  expect(result.calls.filter(call => call.startsWith('cargo zigbuild'))).toHaveLength(2)
  expect(result.calls.some(call => call.includes('mjs stage '))).toBe(true)
  expect(result.calls.some(call => /^(gh |git (push|tag) )/.test(call))).toBe(false)
})

test('publishing rejects dirty trees, unverified builds and mismatched remote tags before writes', () => {
  for (const env of [{ DIRTY: ' M changed' }, { VERIFY_STATUS: '1' }, { REMOTE_TAG: `${'b'.repeat(40)}\trefs/tags/v1.2.3` }]) {
    const result = run(['release', '--publish'], env)
    expect(result.status).toBe(1)
    expect(result.calls.some(call => /^(git (push|tag) |gh release (create|upload|delete-asset) )/.test(call))).toBe(false)
  }
})

test('publishing uploads exactly two binaries to origin and leaves source archives to GitHub', () => {
  const result = run(['release', '--publish', '--draft'])
  expect(result.status).toBe(0)
  const create = result.calls.find(call => call.startsWith('gh release create '))
  expect(create).toContain('--repo github.com/example/miao')
  expect(create).toContain('--verify-tag')
  expect(create).toContain('--draft')
  const files = create.split(' ').filter(arg => arg.includes('/dist/')).map(path => path.split('/').at(-1))
  expect(files).toEqual(['miao-rust-linux-amd64', 'miao-rust-linux-arm64'])
  expect(result.calls.some(call => call.startsWith('cargo '))).toBe(false)
})

test('republishing removes auxiliary assets only after both binary uploads succeed', () => {
  const env = { EXISTING_ASSETS: 'miao-rust-linux-amd64\nmiao-rust-linux-arm64\nchecksums.txt\nold metadata.json' }
  const result = run(['release', '--publish'], env)
  expect(result.status).toBe(0)
  const upload = result.calls.findIndex(call => call.startsWith('gh release upload '))
  const deletions = result.calls.filter(call => call.startsWith('gh release delete-asset '))
  expect(deletions).toHaveLength(2)
  expect(deletions.every(call => result.calls.indexOf(call) > upload)).toBe(true)
  expect(deletions.some(call => call.includes('miao-rust-linux-'))).toBe(false)
  const failed = run(['release', '--publish'], { ...env, UPLOAD_STATUS: '1' })
  expect(failed.status).toBe(1)
  expect(failed.calls.some(call => call.startsWith('gh release delete-asset '))).toBe(false)
})
